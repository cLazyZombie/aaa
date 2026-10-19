//! 감시 source의 실행 상태·따라잡기·outage·내구 전달. 판정은 각 source가 맡는다.
use crate::{
    Host, err,
    inbox::{Outage, OutageEvent},
};
use aaa_common::util;
use aaa_proto::{ErrorBody, ErrorCode, InboxItem};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Memory {
    last: HashMap<String, Instant>,
    running: HashSet<String>,
    outages: HashMap<String, Outage>,
    seen: HashSet<String>,
}
/// source 필드는 L0의 최상위 JSON 모양을 유지한다. 저장 key도 source가 정한다.
#[derive(Default, Serialize)]
pub(crate) struct State<S: Default> {
    pub last_poll_ms: u64,
    pub outbox: Vec<Notice>,
    #[serde(flatten)]
    pub source: S,
}
// serde flatten의 중간 값은 JSON 숫자 map key를 문자열로 남긴다. source는
// serde_json으로 직접 읽어 L0의 BTreeMap<u64, _> 문서를 그대로 받아들인다.
impl<'de, S: DeserializeOwned + Default> Deserialize<'de> for State<S> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut fields = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;
        let last_poll_ms = fields
            .remove("last_poll_ms")
            .map(serde_json::from_value)
            .transpose()
            .map_err(D::Error::custom)?
            .unwrap_or_default();
        let outbox = fields
            .remove("outbox")
            .map(serde_json::from_value)
            .transpose()
            .map_err(D::Error::custom)?
            .unwrap_or_default();
        let source = serde_json::from_value(serde_json::Value::Object(fields)).map_err(D::Error::custom)?;
        Ok(Self { last_poll_ms, outbox, source })
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Notice {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub routine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    pub kind: String,
    pub sha: String,
    pub owners: Vec<String>,
    pub text: String,
    pub mission: Option<String>,
    pub wake: bool,
    pub record_only: bool,
    pub append: Option<String>,
    pub fixed: Option<String>,
    pub at: u64,
}

pub(crate) struct Delivery {
    pub source: &'static str,
    pub label: &'static str,
    pub alert_kinds: &'static [&'static str],
    pub recovered_kind: &'static str,
    pub passed_after_recovery: Option<&'static str>,
    pub missing_mission: &'static str,
}
fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}
impl Host {
    pub(crate) fn watch_next_due(
        &self,
        source: &str,
        every: Duration,
        active: impl Fn(&str) -> bool,
    ) -> Option<Instant> {
        let prefix = format!("{source}:");
        let candidates: Vec<_> = {
            let m = self.watch.lock().unwrap();
            m.last.keys().filter_map(|key| key.strip_prefix(&prefix).map(|p| (key.clone(), p.to_string()))).collect()
        };
        // 정의 파일을 읽는 callback은 runtime 잠금 밖에서 실행한다.
        let inactive: HashSet<_> = candidates.into_iter().filter_map(|(key, p)| (!active(&p)).then_some(key)).collect();
        let mut m = self.watch.lock().unwrap();
        // 꺼진 정의·삭제한 프로젝트의 지난 deadline이 ticker를 계속 깨우지 않게 한다.
        m.last.retain(|key, _| !inactive.contains(key));
        m.last
            .iter()
            .filter(|(p, _)| p.starts_with(&prefix) && !m.running.contains(*p))
            .map(|(_, at)| *at + every)
            .min()
    }
    pub(crate) fn watch_claim(&self, source: &str, project: &str, every: Duration) -> bool {
        let key = format!("{source}:{project}");
        let mut m = self.watch.lock().unwrap();
        if m.running.contains(&key) || m.last.get(&key).is_some_and(|at| at.elapsed() < every) {
            return false;
        }
        m.running.insert(key);
        true
    }
    pub(crate) fn watch_finished(&self, source: &str, project: &str) {
        let key = format!("{source}:{project}");
        let mut m = self.watch.lock().unwrap();
        m.running.remove(&key);
        m.last.insert(key, Instant::now());
    }
    pub(crate) fn watch_catchup(&self, key: &str, last: u64, now: u64, every: Duration) -> bool {
        let first = !self.watch.lock().unwrap().seen.contains(key);
        last > 0 && (first || now.saturating_sub(last) > every.as_millis() as u64 * 2)
    }
    pub(crate) fn watch_polled(&self, key: &str) {
        self.watch.lock().unwrap().seen.insert(key.into());
    }
    pub(crate) fn watch_state<S: DeserializeOwned + Default>(&self, key: &str) -> Result<State<S>, ErrorBody> {
        match self.db.meta(key).map_err(internal)? {
            Some(s) => serde_json::from_str(&s).map_err(internal),
            None => Ok(State::default()),
        }
    }
    pub(crate) fn watch_store<S: Serialize + Default>(&self, key: &str, state: &State<S>) -> Result<(), ErrorBody> {
        self.db.set_meta(key, &serde_json::to_string(state).map_err(internal)?).map_err(internal)
    }
    pub(crate) fn watch_outage(&self, p: &str, subject: &str, error: Option<&str>, policy: &Delivery) {
        let key = format!("{p}/{subject}");
        let reported = self.outage_open(p, subject, "failing");
        let event = self.watch.lock().unwrap().outages.entry(key).or_insert_with(|| Outage::resumed(reported)).record(
            error.is_none(),
            util::now_ms(),
            self.cfg.projects.outage_secs.saturating_mul(1000),
        );
        match event {
            Some(OutageEvent::Down) => {
                if let Some(name) = subject.strip_prefix(&format!("{}:", policy.source)) {
                    self.record_routine(
                        p,
                        name,
                        &aaa_common::util::new_id(),
                        "failed",
                        &format!("조회 장애: {}", aaa_common::pr::sanitize(error.unwrap_or(""))),
                        None,
                    );
                }
                self.inbox_once(
                    p,
                    "outage",
                    subject,
                    "failing",
                    &format!(
                        "{} 조회가 계속 실패한다: {}",
                        policy.label,
                        aaa_common::pr::sanitize(error.unwrap_or(""))
                    ),
                );
                self.flow_project(p, &format!("{}_query_failing", policy.source), None, subject);
            }
            Some(OutageEvent::Recovered) => {
                if let Some(name) = subject.strip_prefix(&format!("{}:", policy.source)) {
                    self.record_routine(p, name, &aaa_common::util::new_id(), "done", "조회 복구", None);
                }
                self.inbox_finish_where(p, |i| i.item_kind == "outage" && i.subject == subject && i.event == "failing");
                self.flow_project(p, &format!("{}_query_recovered", policy.source), None, subject);
            }
            None => {}
        }
    }
    pub(crate) fn watch_drain<S: Serialize + Default>(
        &self,
        p: &str,
        key: &str,
        s: &mut State<S>,
        policy: &Delivery,
        publish: impl Fn(&Notice) -> Result<(), ErrorBody>,
        recovered: impl Fn(&Notice) -> Result<(), ErrorBody>,
    ) -> Result<(), ErrorBody> {
        let mut waiting = Vec::new();
        while let Some(n) = s.outbox.first().cloned() {
            let red = policy.alert_kinds.contains(&n.kind.as_str());
            if red && !n.record_only {
                let existing = n
                    .append
                    .as_ref()
                    .and_then(|id| self.db.inbox_list(Some(p), true).ok()?.into_iter().find(|i| &i.id == id));
                if let Some(mut item) = existing {
                    if !item.body.contains(&n.id) {
                        item.body.push_str(&format!("\n\n{}\n{}", n.id, n.text));
                        // 항목은 반복 로그가 아니라 연속 실패 기록을 가진다. 전체는 최근 256KiB까지.
                        if item.body.len() > 256 * 1024 {
                            item.body = format!("{}\n{}", n.id, n.text);
                        }
                        self.db.inbox_put(&item).map_err(internal)?;
                        self.inbox_publish(&item);
                    }
                } else {
                    let item = InboxItem {
                        id: n.id.clone(),
                        project: p.into(),
                        item_kind: policy.source.into(),
                        subject: n.owners.first().cloned().unwrap_or_else(|| "주인 불명".into()),
                        event: format!("{} {}", policy.label, n.kind),
                        summary: format!("{} {} {}", policy.label, n.kind, n.sha),
                        body: format!("{}\n{}", n.id, n.text),
                        done: false,
                        seen: false,
                        created_at_ms: n.at,
                        done_at_ms: None,
                    };
                    if self.db.inbox_insert(&item).map_err(internal)? {
                        self.inbox_publish(&item);
                    }
                }
            }
            if let Some(id) = &n.fixed {
                if let Some(mut item) =
                    self.db.inbox_list(Some(p), false).map_err(internal)?.into_iter().find(|i| &i.id == id)
                {
                    item.event = format!("{} {}", policy.label, policy.recovered_kind);
                    item.summary = format!("{} {} {}", policy.label, policy.recovered_kind, n.sha);
                    self.db.inbox_put(&item).map_err(internal)?;
                    self.inbox_publish(&item);
                }
                self.inbox_finish_where(p, |i| &i.id == id);
            }
            if n.kind == policy.recovered_kind {
                recovered(&n)?;
            }
            publish(&n)?;
            if n.kind == policy.recovered_kind
                && let Some(kind) = policy.passed_after_recovery
            {
                let mut passed = n.clone();
                passed.kind = kind.into();
                passed.id.push_str(&format!("-{kind}"));
                publish(&passed)?;
            }
            if n.wake
                && let Some(m) = &n.mission
                && !self.mission_target_exists(p, m)
            {
                self.inbox_once(
                    p,
                    "config-error",
                    m,
                    &format!("{} mission missing", policy.label),
                    policy.missing_mission,
                );
                waiting.push(n.clone());
            } else if n.wake
                && let Some(m) = &n.mission
            {
                self.mission_data_input(
                    p,
                    m,
                    &format!("{}_input:{p}:{}", policy.source, n.id),
                    &n.id,
                    n.text.clone(),
                    policy.source,
                )?;
            }
            if !n.routine.is_empty() {
                let (status, detail) = if waiting.iter().any(|w| w.id == n.id) {
                    ("waiting", "대상 임무 없음".to_string())
                } else if let Some(reason) = &n.skip_reason {
                    ("skipped", reason.clone())
                } else {
                    ("done", format!("{} {}{}", policy.label, n.kind, if n.wake { " · 임무 입력" } else { "" }))
                };
                self.record_routine(p, &n.routine, &n.id, status, &detail, None);
            }
            s.outbox.remove(0);
            let rest = std::mem::take(&mut s.outbox);
            s.outbox = waiting.clone();
            s.outbox.extend(rest.clone());
            self.watch_store(key, s)?;
            s.outbox = rest;
        }
        s.outbox = waiting;
        self.watch_store(key, s)?;
        Ok(())
    }
}
