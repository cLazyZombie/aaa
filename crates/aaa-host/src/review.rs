//! 자기 thread의 후보 판에 묶인 일회 검수 원장. 실행 표지를 먼저 저장하고 결과 불명은 재전송하지 않는다.
use crate::{
    AgentConn, Host,
    account_pool::{Allocation, TaskReservation, fresh_windows},
    err,
    orch::ThreadRec,
};
use aaa_common::util;
use aaa_proto::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

const PREFIX: &str = "review-job:";
const MAX_ACTIVE: usize = 4;

#[cfg(test)]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
struct Job {
    start: ReviewStart,
    project: String,
    thread: String,
    machine: String,
    session: String,
    harness: String,
    model: String,
    effort: String,
    view: ReviewView,
    allocation: Option<Allocation>,
}

fn key(id: &str) -> String {
    format!("{PREFIX}{id}")
}
fn result_key(id: &str) -> String {
    format!("review-result:{id}")
}
fn internal(error: impl std::fmt::Display + 'static) -> ErrorBody {
    aaa_common::diagnostic::context(error, err(ErrorCode::Internal, "검수 원장 읽기·쓰기 실패"))
}
fn bad(message: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, message)
}
fn clean(text: &str) -> String {
    aaa_common::redact::text(text).chars().filter(|c| !c.is_control() || *c == ' ').take(512).collect()
}

pub(crate) fn pool_reservations(db: &Connection) -> anyhow::Result<Vec<TaskReservation>> {
    let mut statement = db.prepare(
        "SELECT value FROM meta WHERE key GLOB 'review-job:*' AND json_extract(value, '$.allocation.released') = 0",
    )?;
    let mut jobs = Vec::new();
    for row in statement.query_map([], |r| r.get::<_, String>(0))? {
        let job: Job = serde_json::from_str(&row?)?;
        if let (Some(account), Some(allocation)) = (job.view.account, job.allocation) {
            jobs.push(TaskReservation { account, allocation });
        }
    }
    Ok(jobs)
}

impl Host {
    fn review_load(&self, id: &str) -> Result<Option<Job>, ErrorBody> {
        self.db.meta(&key(id)).map_err(internal)?.map(|v| serde_json::from_str(&v).map_err(internal)).transpose()
    }

    fn review_save(&self, job: &Job, result: Option<&ReviewResult>) -> Result<(), ErrorBody> {
        let value = serde_json::to_string(job).map_err(internal)?;
        if let Some(result) = result {
            self.db
                .image_transaction(|tx| {
                    tx.execute(
                        "INSERT INTO meta(key,value) VALUES(?1,?2)",
                        [result_key(&job.start.id), serde_json::to_string(result)?],
                    )?;
                    tx.execute("UPDATE meta SET value=?2 WHERE key=?1", [key(&job.start.id), value])?;
                    Ok(())
                })
                .map_err(internal)?;
        } else {
            self.db.set_meta(&key(&job.start.id), &value).map_err(internal)?;
        }
        self.bump();
        self.accounts_bump();
        Ok(())
    }

    fn review_owner(&self, machine: &str, session: &str) -> Result<ThreadRec, ErrorBody> {
        let forbidden = || err(ErrorCode::Forbidden, "실행 중인 자기 worker thread에서 검수 작업을 요청한다");
        {
            let inner = self.inner.lock().unwrap();
            if !inner.panes.get(session).is_some_and(|p| p.machine_id == machine && p.info.state.is_running()) {
                return Err(forbidden());
            }
        }
        self.thread_of_pane(session)
            .filter(|r| r.role == Role::Worker && r.status == ThreadStatus::Open && r.machine_id == machine)
            .ok_or_else(forbidden)
    }

    fn review_paths(&self, rec: &ThreadRec) -> Result<(String, String), ErrorBody> {
        let (Some(repo), Some(cwd)) = (&rec.repo, &rec.cwd) else {
            return Err(bad("검수에는 등록한 저장소의 thread 작업 폴더가 필요하다"));
        };
        if !self
            .settings(&rec.project)
            .repos
            .iter()
            .any(|r| r.path == *repo && self.resolve_machine(r.machine.as_deref()).is_ok_and(|m| m == rec.machine_id))
        {
            return Err(err(ErrorCode::Forbidden, "그 머신의 프로젝트에 등록한 저장소만 검수한다"));
        }
        Ok((repo.clone(), cwd.clone()))
    }

    fn review_start(
        self: &Arc<Self>,
        rec: &ThreadRec,
        session: &str,
        start: ReviewStart,
        existing: Option<Job>,
    ) -> Result<ReviewResponse, ErrorBody> {
        let mut job = if let Some(mut existing) = existing {
            if existing.start != start {
                return Err(err(ErrorCode::Conflict, "같은 검수 ID에 다른 요청을 보냈다"));
            }
            if existing.view.state != ReviewState::Waiting {
                return Ok(ReviewResponse { job: existing.view, result: None });
            }
            existing.machine = rec.machine_id.clone();
            existing.session = session.into();
            existing
        } else {
            if !start.target.valid() || start.cwd.is_empty() || start.cwd.len() > 4096 || start.cwd.contains('\0') {
                return Err(bad("검수의 기준·후보·tree SHA와 작업 폴더를 확인한다"));
            }
            self.check_profile_allowed(&rec.project, &start.profile)?;
            let profile = self.configured_thread_profile(Some(&rec.project), &start.profile)?;
            if !matches!(profile.harness.as_str(), "codex" | "claude") || profile.model.trim().is_empty() {
                return Err(bad("모델을 명시한 Codex 또는 Claude profile을 지정한다"));
            }
            let view = ReviewView {
                id: start.id.clone(),
                profile: start.profile.clone(),
                target: start.target.clone(),
                state: ReviewState::Queued,
                account: None,
                reason: String::new(),
            };
            Job {
                start,
                project: rec.project.clone(),
                thread: rec.id.clone(),
                machine: rec.machine_id.clone(),
                session: session.into(),
                harness: profile.harness,
                model: profile.model,
                effort: profile.effort,
                view,
                allocation: None,
            }
        };
        self.review_paths(rec)?;
        let active = self.db.image_transaction(|tx| {
            let count = tx.query_row(
                "SELECT count(*) FROM meta WHERE key GLOB 'review-job:*' AND json_extract(value,'$.view.state') IN ('queued','running')",
                [],
                |row| row.get::<_, usize>(0),
            )?;
            Ok(count)
        }).map_err(internal)?;
        if active >= MAX_ACTIVE {
            return Err(err(ErrorCode::Unavailable, "동시 검수 한도(4건)에 도달했다"));
        }
        job.view.state = ReviewState::Queued;
        job.view.reason = "host 계정 배정 대기".into();
        self.review_save(&job, None)?;
        self.review_event(rec, &job, "review_queued");
        let host = self.clone();
        let id = job.start.id.clone();
        tokio::spawn(async move {
            aaa_common::diagnostic::ignored(host.review_run(&id).await, "검수 원장 실행 실패");
        });
        Ok(ReviewResponse { job: job.view, result: None })
    }

    pub(crate) async fn review_request(
        self: &Arc<Self>,
        machine: &str,
        session: &str,
        request: ReviewRequest,
    ) -> Result<ReviewResponse, ErrorBody> {
        let rec = self.review_owner(machine, session)?;
        let id = request.id().to_string();
        if !review_id_ok(&id) {
            return Err(bad("검수 작업 ID는 26자리 영문 대문자·숫자다"));
        }
        let _rename = if matches!(request, ReviewRequest::Start(_) | ReviewRequest::Cancel { .. }) {
            Some(self.account_rename_gate.read().await)
        } else {
            None
        };
        let mut changes = self.changed.subscribe();
        let lock = self.thread_op_lock("review", "ledger");
        let guard = lock.lock().await;
        let job = self.review_load(&id)?;
        if job.as_ref().is_some_and(|j| j.project != rec.project || j.thread != rec.id) {
            return Err(err(ErrorCode::NotFound, "자기 thread의 검수 작업이 없다"));
        }
        if let ReviewRequest::Start(start) = request {
            return self.review_start(&rec, session, start, job);
        }
        let mut job = job.ok_or_else(|| err(ErrorCode::NotFound, "검수 작업이 없다"))?;
        let result = match request {
            ReviewRequest::Status { wait, .. } => {
                drop(guard);
                if wait && job.view.state.active() {
                    let _ = tokio::time::timeout(Duration::from_secs(2), changes.changed()).await;
                    job = self.review_load(&id)?.ok_or_else(|| err(ErrorCode::NotFound, "검수 작업이 없다"))?;
                }
                None
            }
            ReviewRequest::Fetch { .. } if job.view.state == ReviewState::Done => {
                let result =
                    self.db.meta(&result_key(&id)).map_err(internal)?.ok_or_else(|| internal("검수 결과 없음"))?;
                Some(serde_json::from_str(&result).map_err(internal)?)
            }
            ReviewRequest::Cancel { .. } => {
                match job.view.state {
                    ReviewState::Waiting | ReviewState::Queued => {
                        job.view.state = ReviewState::Cancelled;
                        job.view.reason = "실행 전 취소".into();
                        self.review_save(&job, None)?;
                        self.review_event(&rec, &job, "review_finished");
                    }
                    ReviewState::Running => {
                        let conn = self.live_conn(&job.machine)?;
                        job.view.reason = "검수 실행기 종료 대기".into();
                        self.review_save(&job, None)?;
                        let id = id.clone();
                        tokio::spawn(async move {
                            aaa_common::diagnostic::ignored(
                                conn.request(AgentRequest::ReviewCancel { id }).await,
                                "검수 취소 전달 실패",
                            );
                        });
                    }
                    _ => {}
                }
                None
            }
            _ => None,
        };
        Ok(ReviewResponse { job: job.view, result })
    }

    fn review_event(&self, rec: &ThreadRec, job: &Job, kind: &str) {
        self.flow(rec, kind, None, format!("{} · {} · {}", job.start.id, job.start.profile, job.view.reason));
    }

    async fn review_prepare(self: &Arc<Self>, job: &mut Job) -> Result<(AgentConn, ReviewRun), ErrorBody> {
        let rec = self.review_owner(&job.machine, &job.session)?;
        if rec.project != job.project || rec.id != job.thread {
            return Err(err(ErrorCode::Forbidden, "검수 요청의 thread가 바뀌었다"));
        }
        let (repo, thread_cwd) = self.review_paths(&rec)?;
        let conn = self.live_conn(&job.machine)?;
        if !self.machine_feature(&job.machine, REVIEW_FEATURE) {
            return Err(bad("thread 머신 agent에 review.v1 지원이 필요하다"));
        }
        self.check_profile_allowed(&job.project, &job.start.profile)?;
        let profile = self.configured_thread_profile(Some(&job.project), &job.start.profile)?;
        if profile.harness != job.harness || profile.model != job.model || profile.effort != job.effort {
            return Err(err(ErrorCode::Conflict, "검수 profile 설정이 바뀌었다. 새 작업 ID로 요청한다"));
        }
        let size = AccountTaskSize::Small;
        let selection = self.account_selection_input(&job.project, &profile, None, size).await?;
        let default = if selection.is_none() {
            let name = self.default_account(&job.harness)?.ok_or_else(|| {
                err(ErrorCode::Unavailable, format!("host의 {} 계정 풀 또는 기본 계정이 필요하다", job.harness))
            })?;
            self.check_reserved_account(&name, &profile)?;
            let group = self
                .account_pool()?
                .members
                .into_iter()
                .find(|m| m.account == name)
                .map_or_else(|| name.clone(), |m| m.group);
            Some((name, group))
        } else {
            None
        };
        let (account, allocation, auth) = self
            .db
            .image_transaction(|tx| {
                let now = util::now_ms();
                let (account, mut allocation) = if let Some(input) = selection {
                    input.select(tx, now)?
                } else {
                    let (account, group) = default.ok_or_else(|| anyhow::anyhow!("검수 기본 계정이 없다"))?;
                    let generation = self.accounts.generation(&account).map_err(|e| anyhow::anyhow!(e.message))?;
                    (
                        account,
                        Allocation {
                            generation,
                            ended_at_ms: None,
                            released: false,
                            reserve_percent: None,
                            selection: AccountSelection {
                                size,
                                group,
                                unobserved: false,
                                reason: "host 기본 계정".into(),
                                selected_at_ms: now,
                            },
                        },
                    )
                };
                let credential = self.accounts.credential(&account).map_err(|e| anyhow::anyhow!(e.message))?;
                anyhow::ensure!(
                    credential.harness == job.harness
                        && credential.auth_generation.as_ref() == Some(&allocation.generation),
                    "검수 계정 인증 세대·harness가 바뀌었다"
                );
                let auth = if job.harness == "codex" {
                    crate::usage::review_codex_auth(&credential.secret.0)?
                } else {
                    ReviewAuth::Claude { token: credential.secret }
                };
                // 이미지와 같은 small 5%p를 실행 표지와 같은 transaction에 보존한다.
                allocation.reserve_percent = Some(size.percent());
                let mut running = job.clone();
                running.view.account = Some(account.clone());
                running.view.state = ReviewState::Running;
                running.view.reason = "thread 머신에서 검수 중".into();
                running.allocation = Some(allocation.clone());
                tx.execute(
                    "UPDATE meta SET value=?2 WHERE key=?1",
                    [key(&job.start.id), serde_json::to_string(&running)?],
                )?;
                Ok((account, allocation, auth))
            })
            .map_err(|e| {
                let message = clean(&e.to_string());
                err(
                    if message.starts_with("start_wait:") { ErrorCode::Unavailable } else { ErrorCode::BadRequest },
                    message,
                )
            })?;
        job.view.account = Some(account);
        job.view.state = ReviewState::Running;
        job.view.reason = "thread 머신에서 검수 중".into();
        job.allocation = Some(allocation);
        self.bump();
        self.accounts_bump();
        Ok((
            conn,
            ReviewRun {
                id: job.start.id.clone(),
                repo,
                thread_cwd,
                requested_cwd: job.start.cwd.clone(),
                target: job.start.target.clone(),
                model: job.model.clone(),
                effort: job.effort.clone(),
                auth,
            },
        ))
    }

    async fn review_run(self: &Arc<Self>, id: &str) -> Result<(), ErrorBody> {
        let rename = self.account_rename_gate.read().await;
        let lock = self.thread_op_lock("review", "ledger");
        let guard = lock.lock().await;
        let Some(mut job) = self.review_load(id)?.filter(|j| j.view.state == ReviewState::Queued) else {
            return Ok(());
        };
        let prepared = Box::pin(self.review_prepare(&mut job)).await;
        let rec = self.load(&job.project, &job.thread).ok();
        let (conn, run) = match prepared {
            Ok(p) => p,
            Err(e) => {
                job.view.state = if matches!(e.code, ErrorCode::Unavailable | ErrorCode::AgentDisconnected) {
                    ReviewState::Waiting
                } else {
                    ReviewState::Failed
                };
                job.view.reason = clean(&e.message);
                self.review_save(&job, None)?;
                if let Some(rec) = rec {
                    self.review_event(&rec, &job, "review_finished");
                }
                return Ok(());
            }
        };
        if let Some(rec) = &rec {
            self.review_event(rec, &job, "review_running");
        }
        drop(guard);
        drop(rename);
        let answer = conn
            .request_timeout(AgentRequest::ReviewRun(Box::new(run)), Duration::from_secs(REVIEW_RUN_SECS + 60))
            .await;
        let _rename = self.account_rename_gate.read().await;
        let _guard = lock.lock().await;
        let mut job = self.review_load(id)?.ok_or_else(|| internal("실행하던 검수 작업이 없다"))?;
        let (state, reason, used, result) = match answer {
            Ok(AgentResponse::ReviewFinished(ReviewExecution::Done(result))) if result.valid() => {
                (ReviewState::Done, format!("{} · 지적 {}건", result.model, result.findings.len()), true, Some(result))
            }
            Ok(AgentResponse::ReviewFinished(ReviewExecution::Done(_))) => {
                (ReviewState::Failed, "검수 결과 형식 오류".into(), true, None)
            }
            Ok(AgentResponse::ReviewFinished(ReviewExecution::Failed { reason, used })) => {
                (ReviewState::Failed, reason, used, None)
            }
            Ok(AgentResponse::ReviewFinished(ReviewExecution::Cancelled { used })) => {
                (ReviewState::Cancelled, "검수 실행 종료 확인".into(), used, None)
            }
            Ok(AgentResponse::ReviewFinished(ReviewExecution::Unknown { reason })) => {
                (ReviewState::Unknown, reason, true, None)
            }
            other => {
                let cause = other.err().map(|e| e.message).unwrap_or_else(|| "실행기 응답 형식 오류".into());
                (ReviewState::Unknown, format!("검수 결과 불명 · {cause}"), true, None)
            }
        };
        job.view.state = state;
        job.view.reason = clean(&reason);
        if let Some(allocation) = &mut job.allocation
            && state != ReviewState::Unknown
        {
            allocation.ended_at_ms = Some(util::now_ms());
            allocation.released = !used;
        }
        self.review_save(&job, result.as_ref())?;
        if let Some(rec) = rec {
            self.review_event(&rec, &job, "review_finished");
        }
        Ok(())
    }

    pub(crate) fn review_recover(&self) -> anyhow::Result<()> {
        for id in self.db.meta_suffixes(PREFIX)? {
            let Some(mut job) = self.review_load(&id).map_err(|e| anyhow::anyhow!(e.message))? else { continue };
            job.view.state = match job.view.state {
                ReviewState::Queued => ReviewState::Waiting,
                ReviewState::Running => ReviewState::Unknown,
                _ => continue,
            };
            job.view.reason = "host 재시작 · 검수 자동 반복 없음".into();
            self.review_save(&job, None).map_err(|e| anyhow::anyhow!(e.message))?;
            if let Ok(rec) = self.load(&job.project, &job.thread) {
                self.review_event(&rec, &job, "review_finished");
            }
        }
        Ok(())
    }

    pub(crate) fn review_reconcile(&self, report: &AccountQuotaReport) -> Result<(), ErrorBody> {
        for id in self.db.meta_suffixes(PREFIX).map_err(internal)? {
            let Some(job) = self.review_load(&id)? else { continue };
            let Some(allocation) = job.allocation.filter(|a| !a.released) else { continue };
            let Some(ended) = allocation.ended_at_ms else { continue };
            if report.accounts.iter().find(|q| Some(&q.account) == job.view.account.as_ref()).is_some_and(|q| {
                fresh_windows(q, Some(&job.model), util::now_ms())
                    .is_ok_and(|ws| ws.iter().all(|w| w.window.observed_at_ms > ended))
            }) {
                self.db
                    .update_meta(&key(&id), |text| {
                        let mut current: Job =
                            serde_json::from_str(&text.ok_or_else(|| anyhow::anyhow!("검수 작업이 없다"))?)?;
                        if let Some(a) = &mut current.allocation
                            && a.generation == allocation.generation
                            && a.ended_at_ms == Some(ended)
                        {
                            a.released = true;
                        }
                        Ok(serde_json::to_string(&current)?)
                    })
                    .map_err(internal)?;
                self.accounts_bump();
            }
        }
        Ok(())
    }
}
