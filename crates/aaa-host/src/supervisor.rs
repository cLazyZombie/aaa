//! host 내부 핵심 작업 감시. panic 내용과 요청 payload는 이 경계에서 읽거나 저장하지 않는다.
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::FutureExt;

use crate::Host;

const RESTARTS: u32 = 3;
const STABLE: Duration = Duration::from_secs(30);
const TIMEOUT_WINDOW: Duration = Duration::from_secs(30);
const TIMEOUT_LIMIT: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub(crate) enum Task {
    LocalLink,
    RemoteLink,
    AgentLink,
    Ticker,
    Front,
    Web,
    Responses,
}

impl Task {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::LocalLink => "agent-local-listener",
            Self::RemoteLink => "agent-tls-listener",
            Self::AgentLink => "agent-link",
            Self::Ticker => "ticker",
            Self::Front => "front-server",
            Self::Web => "web-server",
            Self::Responses => "agent-responses",
        }
    }
}

struct Incident {
    since_ms: u64,
    count: u32,
    restarts: u32,
    healthy_since: Option<Instant>,
    stopped: bool,
    failed_at: Instant,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Transition {
    pub id: u64,
    pub task: Task,
    pub recovered: bool,
    since_ms: u64,
    count: u32,
}
impl Transition {
    pub(crate) fn summary(&self) -> String {
        if self.recovered {
            format!(
                "host 회복 · {} · {}부터 이상 {}회 · 정상 진행 뒤 30초 안정",
                self.task.name(),
                aaa_common::util::utc_text(self.since_ms),
                self.count
            )
        } else {
            format!(
                "host 이상 · {} · {}부터 · {}회 · {}",
                self.task.name(),
                aaa_common::util::utc_text(self.since_ms),
                self.count,
                if self.task == Task::Responses {
                    "응답 시간 초과가 몰림 · 응답 경로 점검 필요"
                } else {
                    "자동 재시작 최대 3회, 상한 뒤 host 재시작 필요"
                }
            )
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredIncident {
    task: Task,
    since_ms: u64,
    count: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredHealth {
    incidents: Vec<StoredIncident>,
    events: VecDeque<Transition>,
    next_event: u64,
}

#[derive(Default)]
pub(crate) struct Health {
    incidents: BTreeMap<Task, Incident>,
    timeouts: VecDeque<(Instant, u64)>,
    events: VecDeque<Transition>,
    next_event: u64,
}

impl Health {
    pub(crate) fn load(db: &crate::db::Db) -> Self {
        let mut health = Self::default();
        if let Ok(Some(raw)) = db.meta("host_health_v1") {
            match serde_json::from_str::<StoredHealth>(&raw) {
                Ok(stored) => {
                    health.events = stored.events;
                    health.next_event = stored.next_event;
                    for i in stored.incidents {
                        health.incidents.insert(
                            i.task,
                            Incident {
                                since_ms: i.since_ms,
                                count: i.count,
                                restarts: 0,
                                healthy_since: None,
                                stopped: false,
                                failed_at: Instant::now(),
                            },
                        );
                    }
                }
                Err(_) => tracing::error!("저장한 host 이상 상태를 읽지 못했다"),
            }
        }
        health
    }

    fn snapshot_without(&self, ack: Option<u64>) -> String {
        let incidents = self
            .incidents
            .iter()
            .map(|(task, i)| StoredIncident { task: *task, since_ms: i.since_ms, count: i.count })
            .collect();
        let events = self.events.iter().filter(|e| Some(e.id) != ack).cloned().collect();
        serde_json::to_string(&StoredHealth { incidents, events, next_event: self.next_event })
            .expect("안전한 host 상태")
    }

    fn persist(&self, db: &crate::db::Db) -> bool {
        if db.set_meta("host_health_v1", &self.snapshot_without(None)).is_err() {
            tracing::error!("host 이상 상태 저장 실패");
            return false;
        }
        true
    }

    fn fail(&mut self, task: Task, now: Instant, now_ms: u64, count: u32) -> u32 {
        let first = !self.incidents.contains_key(&task);
        let i = self.incidents.entry(task).or_insert(Incident {
            since_ms: now_ms,
            count: 0,
            restarts: 0,
            healthy_since: None,
            stopped: false,
            failed_at: now,
        });
        i.count = i.count.saturating_add(count);
        i.healthy_since = None;
        i.failed_at = now;
        if task != Task::Responses {
            i.restarts = i.restarts.saturating_add(1);
        }
        i.stopped = i.restarts > RESTARTS;
        // payload·JoinError의 Display/Debug는 쓰지 않는다.
        tracing::error!(
            task = task.name(),
            count = i.count,
            since_ms = i.since_ms,
            stopped = i.stopped,
            "host 핵심 작업 이상"
        );
        if first {
            self.next_event += 1;
            self.events.push_back(Transition {
                id: self.next_event,
                task,
                recovered: false,
                since_ms: i.since_ms,
                count: i.count,
            });
        }
        i.restarts
    }

    fn progress(&mut self, task: Task, now: Instant) {
        if let Some(i) = self.incidents.get_mut(&task)
            && !i.stopped
        {
            i.healthy_since.get_or_insert(now);
        }
    }

    fn timeout(&mut self, now: Instant, now_ms: u64) {
        while self.timeouts.front().is_some_and(|t| now.saturating_duration_since(t.0) > TIMEOUT_WINDOW) {
            self.timeouts.pop_front();
        }
        self.timeouts.push_back((now, now_ms));
        // 창 전체를 저장할 필요 없이 임계 건수만 있으면 된다.
        while self.timeouts.len() > TIMEOUT_LIMIT {
            self.timeouts.pop_front();
        }
        if self.incidents.contains_key(&Task::Responses) || self.timeouts.len() >= TIMEOUT_LIMIT {
            let count = if self.incidents.contains_key(&Task::Responses) { 1 } else { TIMEOUT_LIMIT as u32 };
            let since_ms = self.timeouts.front().map_or(now_ms, |t| t.1);
            self.fail(Task::Responses, now, since_ms, count);
        }
    }

    fn pending(&mut self, now: Instant) -> Vec<Transition> {
        let recovered: Vec<_> = self
            .incidents
            .iter()
            .filter_map(|(task, i)| {
                i.healthy_since.filter(|t| now.saturating_duration_since(*t) >= STABLE).map(|_| *task)
            })
            .collect();
        for task in recovered {
            let i = self.incidents.remove(&task).unwrap();
            if task == Task::Responses {
                self.timeouts.clear();
            }
            self.next_event += 1;
            self.events.push_back(Transition {
                id: self.next_event,
                task,
                recovered: true,
                since_ms: i.since_ms,
                count: i.count,
            });
        }
        self.events.iter().cloned().collect()
    }
}

impl Host {
    pub(crate) fn task_allowed(&self, task: Task) -> bool {
        !self.health.lock().unwrap().incidents.get(&task).is_some_and(|i| i.stopped)
    }

    pub(crate) fn task_retry_delay(&self, task: Task) -> Duration {
        self.health.lock().unwrap().incidents.get(&task).map_or(Duration::ZERO, |i| {
            (i.failed_at + Duration::from_secs(1 << i.restarts.saturating_sub(1).min(2)))
                .saturating_duration_since(Instant::now())
        })
    }

    pub(crate) fn task_failed(&self, task: Task) -> u32 {
        let mut health = self.health.lock().unwrap();
        let restarts = health.fail(task, Instant::now(), aaa_common::util::now_ms(), 1);
        health.persist(&self.db);
        drop(health);
        self.health_wake.notify_one();
        restarts
    }

    pub(crate) fn task_progress(&self, task: Task) {
        self.health.lock().unwrap().progress(task, Instant::now());
    }

    pub(crate) fn agent_request_finished(&self, timed_out: bool, responded: bool) {
        let mut health = self.health.lock().unwrap();
        if timed_out {
            health.timeout(Instant::now(), aaa_common::util::now_ms());
            if health.incidents.contains_key(&Task::Responses) {
                health.persist(&self.db);
            }
        } else if responded {
            health.progress(Task::Responses, Instant::now());
        }
        drop(health);
        self.health_wake.notify_one();
    }

    /// listener는 factory 밖 Arc로 보유한다. 주소 재선택·bind 경쟁 없이 같은 입구로 다시 실행한다.
    pub(crate) fn supervise<F, Fut>(self: &Arc<Self>, task: Task, factory: F) -> tokio::task::JoinHandle<()>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send,
    {
        let host = self.clone();
        tokio::spawn(async move {
            loop {
                // factory 생성 단계의 panic과 정상 조기 종료도 감지한다.
                let _ = AssertUnwindSafe(async { factory().await }).catch_unwind().await;
                let failures = host.task_failed(task);
                if failures > RESTARTS {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1 << (failures - 1))).await;
            }
        })
    }

    /// inbox와 ACK 뒤 snapshot을 기존 DB transaction으로 함께 확정한다.
    pub(crate) fn commit_host_health(&self, id: u64, item: &aaa_proto::InboxItem) -> anyhow::Result<bool> {
        let mut health = self.health.lock().unwrap();
        let snapshot = health.snapshot_without(Some(id));
        let inserted = self.db.inbox_insert_marked(item, "host_health_v1", &snapshot, false)?;
        health.events.retain(|event| event.id != id);
        Ok(inserted)
    }

    async fn health_pass(&self, now: Instant) {
        let events = {
            let mut health = self.health.lock().unwrap();
            let events = health.pending(now);
            if !events.is_empty() && !health.persist(&self.db) {
                return;
            }
            events
        };
        for event in events {
            self.publish_host_health(event).await;
        }
    }

    /// ticker와 독립: ticker가 중단돼도 inbox·앱 알림·외부 알림은 이 task가 전달한다.
    pub(crate) fn start_supervisor(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let host = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = host.health_wake.notified() => {},
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                }
                host.health_pass(Instant::now()).await;
            }
        })
    }
}

#[cfg(test)]
mod tests;
