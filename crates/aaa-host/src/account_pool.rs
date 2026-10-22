//! host 공통 허용 풀(A3). 선택·예상량 예약·시작 영수증은 같은 SQLite transaction이다.
use crate::{
    Host, err,
    orch::{StartArgs, ThreadRec, internal},
};
use aaa_common::util;
use aaa_proto::*;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// 전역 key는 project_meta의 `<종류>:<프로젝트>` 이름 공간과 겹치지 않는다.
const CONFIG: &str = "account-pool-v1";
const JOB: &str = "account-job:";

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PoolRequest {
    pub id: String,
    pub size: AccountTaskSize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Allocation {
    pub selection: AccountSelection,
    pub generation: String,
    pub ended_at_ms: Option<u64>,
    pub released: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Job {
    view: AccountStartView,
    args: StartArgs,
    actor: String,
    picked: Option<TaskView>,
    profile_revision: String,
}

pub(crate) struct Candidate {
    member: AccountPoolMember,
    generation: String,
    quota: Option<AccountQuota>,
}

pub(crate) struct SelectionInput {
    pool: AccountPoolView,
    project: String,
    model: Option<String>,
    auto: bool,
    size: AccountTaskSize,
    candidates: Vec<Candidate>,
    excluded: Vec<String>,
}

fn bad(s: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, s)
}
fn job_key(project: &str, id: &str) -> String {
    format!("{JOB}{project}:{id}")
}
pub(crate) fn receipt(project: &str, id: &str) -> String {
    format!("account-receipt:{project}:{id}")
}
fn decode_pool(text: Option<String>) -> anyhow::Result<AccountPoolView> {
    text.map(|s| serde_json::from_str(&s)).transpose().map(|p| p.unwrap_or_default()).map_err(Into::into)
}

/// 프로젝트 삭제·이름 변경과 참여 설정을 같은 transaction에서 옮긴다.
pub(crate) fn project_transition(tx: &Connection, from: &str, to: Option<&str>) -> anyhow::Result<()> {
    let text = tx.query_row("SELECT value FROM meta WHERE key=?1", [CONFIG], |r| r.get(0)).optional()?;
    let mut pool = decode_pool(text)?;
    if pool.projects.iter().any(|p| p == from) {
        pool.projects.retain(|p| p != from);
        if let Some(to) = to {
            pool.projects.push(to.into());
        }
        pool.projects.sort();
        pool.projects.dedup();
        pool.revision = util::new_id();
        tx.execute("UPDATE meta SET value=?2 WHERE key=?1", [CONFIG, &serde_json::to_string(&pool)?])?;
    }
    Ok(())
}

pub(crate) fn rename_job(text: &str, project: &str) -> anyhow::Result<String> {
    let mut job: Job = serde_json::from_str(text)?;
    job.args.project = project.into();
    job.view.project = project.into();
    Ok(serde_json::to_string(&job)?)
}

impl Host {
    pub(crate) fn account_pool(&self) -> Result<AccountPoolView, ErrorBody> {
        decode_pool(self.db.meta(CONFIG).map_err(internal)?).map_err(internal)
    }

    pub(crate) fn pool_project_enabled(&self, project: &str) -> Result<bool, ErrorBody> {
        Ok(self.account_pool()?.projects.iter().any(|p| p == project))
    }

    fn change_pool(
        &self,
        revision: &str,
        change: impl FnOnce(&mut AccountPoolView) -> anyhow::Result<()>,
    ) -> Result<AccountPoolView, ErrorBody> {
        let mut changed = None;
        self.db
            .update_meta(CONFIG, |old| {
                let mut pool = decode_pool(old)?;
                anyhow::ensure!(pool.revision == revision, "풀 설정이 바뀌었다. 다시 읽은 뒤 저장한다");
                change(&mut pool)?;
                pool.members.sort_by(|a, b| a.account.cmp(&b.account));
                pool.projects.sort();
                pool.projects.dedup();
                pool.revision = util::new_id();
                changed = Some(pool.clone());
                Ok(serde_json::to_string(&pool)?)
            })
            .map_err(|e| err(ErrorCode::Conflict, e.to_string()))?;
        self.accounts_bump();
        self.tick_wake.notify_one();
        changed.ok_or_else(|| internal("풀 저장 결과가 없다"))
    }

    pub(crate) fn pool_member_set(
        &self,
        revision: &str,
        account: &str,
        member: Option<AccountPoolMember>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if !crate::accounts::valid_name(account) {
            return Err(bad("계정 별칭을 확인한다"));
        }
        if let Some(m) = &member {
            if m.account != account
                || !crate::accounts::valid_name(&m.group)
                || m.models.len() > 64
                || m.models
                    .iter()
                    .any(|s| s.is_empty() || s.len() > 128 || s.chars().any(|c| c.is_whitespace() || c.is_control()))
            {
                return Err(bad("풀의 계정·한도 그룹·모델 목록을 확인한다"));
            }
            let credential = self.accounts.credential(account)?;
            for other in self.account_pool()?.members {
                if other.account != account
                    && other.group == m.group
                    && self.accounts.credential(&other.account)?.harness != credential.harness
                {
                    return Err(bad("같은 한도 그룹에는 같은 harness 계정만 넣는다"));
                }
            }
        }
        let pool = self.change_pool(revision, |pool| {
            pool.members.retain(|m| m.account != account);
            if let Some(mut m) = member {
                m.models.sort();
                m.models.dedup();
                pool.members.push(m);
            }
            anyhow::ensure!(pool.members.len() <= 256, "풀은 계정 256개까지다");
            Ok(())
        })?;
        self.db.audit(actor, "account_pool_member", account);
        Ok(FrontResponse::AccountPool { pool })
    }

    pub(crate) fn pool_project_set(
        &self,
        revision: &str,
        project: &str,
        enabled: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        let pool = self.change_pool(revision, |pool| {
            pool.projects.retain(|p| p != project);
            if enabled {
                pool.projects.push(project.into());
            }
            Ok(())
        })?;
        self.db.audit(actor, "account_pool_project", &format!("{project} {enabled}"));
        Ok(FrontResponse::AccountPool { pool })
    }

    pub(crate) fn pool_forget(&self, account: &str) -> Result<(), ErrorBody> {
        let pool = self.account_pool()?;
        if pool.members.iter().any(|m| m.account == account) {
            self.pool_member_set(&pool.revision, account, None, "host")?;
        }
        Ok(())
    }

    pub(crate) async fn account_selection_input(
        self: &Arc<Self>,
        project: &str,
        profile: &ResolvedProfile,
        account: Option<&str>,
        size: AccountTaskSize,
    ) -> Result<Option<SelectionInput>, ErrorBody> {
        let pool = self.account_pool()?;
        let auto = account.is_none();
        if auto && !pool.projects.iter().any(|p| p == project) {
            return Err(bad("이 프로젝트는 host 공통 계정 풀에 참여하지 않는다"));
        }
        if !auto && !pool.members.iter().any(|m| Some(m.account.as_str()) == account) {
            return Ok(None);
        }
        let report = if auto { self.quota_report(true).await? } else { AccountQuotaReport::default() };
        if auto {
            self.pool_reconcile(&report)?;
        }
        let mut input = SelectionInput {
            pool: pool.clone(),
            project: project.into(),
            model: (!profile.model.is_empty()).then(|| profile.model.clone()),
            auto,
            size,
            candidates: vec![],
            excluded: vec![],
        };
        for member in pool.members {
            if !auto && Some(member.account.as_str()) != account {
                continue;
            }
            let name = member.account.clone();
            let credential = match self.accounts.credential(&name) {
                Ok(c) => c,
                Err(_) => {
                    input.excluded.push(format!("{name}: 인증 잠김·없음"));
                    continue;
                }
            };
            if credential.harness != profile.harness {
                continue;
            }
            if auto && !member.models.is_empty() && !member.models.contains(&profile.model) {
                input.excluded.push(format!("{name}: 허용 모델이 아님"));
                continue;
            }
            if auto && crate::account_meta::health(&self.account_meta(&name), util::now_ms()).broken() {
                input.excluded.push(format!("{name}: 인증 끊김"));
                continue;
            }
            let generation = credential.auth_generation.ok_or_else(|| bad("계정 인증 세대를 확인하지 못했다"))?;
            if auto && self.quota_generation(&name).await.as_deref() != Some(&generation) {
                input.excluded.push(format!("{name}: 인증 세대 변경 · 새 관측 대기"));
                continue;
            }
            input.candidates.push(Candidate {
                member,
                generation,
                quota: report.accounts.iter().find(|a| a.account == name).cloned(),
            });
        }
        Ok(Some(input))
    }

    /// 종료 사건을 확인한 예약만 종료로 기록한다. 단절·결과 불명은 유지한다.
    fn pool_reconcile(&self, report: &AccountQuotaReport) -> Result<(), ErrorBody> {
        let pool = self.account_pool()?;
        for text in self.db.threads(None).map_err(internal)? {
            let r: ThreadRec = serde_json::from_str(&text).map_err(internal)?;
            let mut allocation = match &r.account_allocation {
                Some(a) if !a.released => a.clone(),
                Some(_) => continue,
                None => {
                    let Some(member) = pool.members.iter().find(|m| r.account.as_deref() == Some(&m.account)) else {
                        continue;
                    };
                    if !matches!(r.status, ThreadStatus::Open | ThreadStatus::Starting) {
                        continue;
                    }
                    let generation = self.accounts.generation(&member.account)?;
                    Allocation {
                        generation,
                        ended_at_ms: None,
                        released: false,
                        selection: AccountSelection {
                            size: AccountTaskSize::Medium,
                            group: member.group.clone(),
                            unobserved: false,
                            reason: "기존 계정 작업 · 공유 한도 예약".into(),
                            selected_at_ms: r.created_at_ms,
                        },
                    }
                }
            };
            if r.status == ThreadStatus::Failed && !r.launch_sent {
                allocation.released = true;
            } else {
                let ended = r.pane.as_ref().is_some_and(|p| {
                    self.inner.lock().unwrap().panes.get(p).is_some_and(|p| {
                        matches!(p.info.state, SessionState::Exited { .. } | SessionState::Interrupted)
                    })
                }) || (r.status == ThreadStatus::Resolved && r.resolve_pending.is_none())
                    || r.status == ThreadStatus::Parked;
                if ended && allocation.ended_at_ms.is_none() {
                    allocation.ended_at_ms = Some(util::now_ms());
                }
                if let Some(ended) = allocation.ended_at_ms
                    && let Some(q) = report.accounts.iter().find(|q| r.account.as_deref() == Some(&q.account))
                    && fresh_windows(q, None, util::now_ms())
                        .is_ok_and(|ws| ws.iter().all(|w| w.window.observed_at_ms > ended))
                {
                    allocation.released = true;
                }
            }
            if serde_json::to_value(&allocation).map_err(internal)?
                != serde_json::to_value(&r.account_allocation).map_err(internal)?
            {
                self.update_rec(&r.project, &r.id, |r| {
                    if r.account_allocation.is_none() {
                        r.account_allocation = Some(allocation.clone());
                    } else if let Some(a) = &mut r.account_allocation
                        && a.selection.selected_at_ms == allocation.selection.selected_at_ms
                    {
                        a.ended_at_ms = allocation.ended_at_ms;
                        a.released = allocation.released;
                    }
                })?;
            }
        }
        Ok(())
    }

    fn pool_job(&self, project: &str, id: &str) -> Result<Job, ErrorBody> {
        self.db
            .meta(&job_key(project, id))
            .map_err(internal)?
            .ok_or_else(|| err(ErrorCode::NotFound, "계정 배정 대기가 없다"))
            .and_then(|s| serde_json::from_str(&s).map_err(internal))
    }

    fn profile_revision(&self, a: &StartArgs) -> Result<String, ErrorBody> {
        let p = self.resolved_profile(Some(&a.project), &self.resolve_machine(a.machine.as_deref())?, &a.profile)?;
        // env·인증 원문은 hash의 입력에도 넣지 않는다.
        Ok(crate::tasks::sha256(&serde_json::to_string(&(&p.harness, &p.model, &p.effort, &p.args)).map_err(internal)?))
    }

    pub(crate) async fn pool_submit(
        self: &Arc<Self>,
        mut args: StartArgs,
        actor: &str,
        picked: Option<TaskView>,
        background: bool,
    ) -> Result<FrontResponse, ErrorBody> {
        let request = args.pool.clone().ok_or_else(|| internal("풀 시작 ID가 없다"))?;
        if !crate::accounts::valid_name(&request.id) {
            return Err(bad("시작 ID는 영문·숫자·-·_ 64자 이하다"));
        }
        self.check_active(&args.project)?;
        if !self.pool_project_enabled(&args.project)? {
            return Err(bad("이 프로젝트는 host 공통 계정 풀에 참여하지 않는다"));
        }
        if args.profile.is_empty() {
            args.profile = self.default_thread_profile(&args.project);
        }
        args.machine = Some(self.resolve_machine(args.machine.as_deref())?);
        let key = job_key(&args.project, &request.id);
        let job = Job {
            view: AccountStartView {
                id: request.id.clone(),
                project: args.project.clone(),
                title: args.title.clone(),
                size: request.size,
                state: AccountStartState::Queued,
                thread: None,
                reason: "계정 배정 대기".into(),
                created_at_ms: util::now_ms(),
            },
            profile_revision: self.profile_revision(&args)?,
            args,
            actor: actor.into(),
            picked,
        };
        self.db
            .update_meta(&key, |old| {
                if let Some(old) = old {
                    let prior: Job = serde_json::from_str(&old)?;
                    anyhow::ensure!(
                        prior.actor == actor
                            && serde_json::to_value(&prior.args)? == serde_json::to_value(&job.args)?
                            && serde_json::to_value(&prior.picked)? == serde_json::to_value(&job.picked)?,
                        "같은 시작 ID에 다른 내용이 있다"
                    );
                    Ok(old)
                } else {
                    Ok(serde_json::to_string(&job)?)
                }
            })
            .map_err(|e| err(ErrorCode::Conflict, e.to_string()))?;
        self.pool_run(&job.args.project, &request.id, background).await
    }

    async fn pool_run(self: &Arc<Self>, project: &str, id: &str, background: bool) -> Result<FrontResponse, ErrorBody> {
        let _guard = match self.claim_thread(project, &format!("account-{id}")) {
            Ok(g) => g,
            Err(_) => return Ok(FrontResponse::AccountStart { start: self.pool_job(project, id)?.view }),
        };
        let mut job = self.pool_job(project, id)?;
        let previous = job.view.clone();
        if job.view.state == AccountStartState::Cancelled || job.view.state == AccountStartState::Failed {
            return Ok(FrontResponse::AccountStart { start: job.view });
        }
        if let Some(tid) = self.db.meta(&receipt(project, id)).map_err(internal)? {
            job.view.state = AccountStartState::Started;
            job.view.thread = Some(tid.clone());
            job.view.reason = "같은 시작 영수증".into();
            self.db
                .set_meta(&job_key(project, id), &serde_json::to_string(&job).map_err(internal)?)
                .map_err(internal)?;
            let mut thread = self.thread_show(project, &tid)?;
            if let Some(t) = &job.picked
                && !self
                    .task_board(project)?
                    .tasks
                    .iter()
                    .any(|now| now.id == t.id && now.thread.as_deref() == Some(&tid))
            {
                thread.warnings.extend(self.link_started(project, t, &tid, &job.actor).await);
            }
            return Ok(FrontResponse::Thread(Box::new(thread)));
        }
        let checked = (|| {
            if self.profile_revision(&job.args)? != job.profile_revision {
                return Err(err(ErrorCode::Conflict, "대기 중 모델·effort 설정이 바뀌었다. 취소 뒤 새로 시작한다"));
            }
            if let Some(t) = &job.picked {
                let board = self.task_board(project)?;
                if !board.tasks.iter().any(|now| now.id == t.id && now.block_sha256 == t.block_sha256) {
                    return Err(err(ErrorCode::Conflict, "대기 중 task 내용이 바뀌었다. 취소 뒤 새로 시작한다"));
                }
            }
            Ok(())
        })();
        let result = match checked {
            Err(e) => Err(e),
            Ok(()) => self.thread_start_mode(job.args.clone(), &job.actor, background).await,
        };
        let response = match result {
            Ok(mut thread) => {
                job.view.state = AccountStartState::Started;
                job.view.thread = Some(thread.id.clone());
                job.view.reason = "계정 배정 완료".into();
                if let Some(t) = &job.picked {
                    thread.warnings.extend(self.link_started(project, t, &thread.id, &job.actor).await);
                }
                FrontResponse::Thread(Box::new(thread))
            }
            Err(e) => {
                if let Some(tid) = self.db.meta(&receipt(project, id)).map_err(internal)? {
                    job.view.thread = Some(tid);
                    job.view.state = AccountStartState::Failed;
                } else if e.message.starts_with("start_wait:") || matches!(e.code, ErrorCode::AgentDisconnected) {
                    job.view.state = AccountStartState::Queued;
                } else {
                    job.view.state = AccountStartState::Failed;
                }
                job.view.reason = e.message.strip_prefix("start_wait: ").unwrap_or(&e.message).to_string();
                FrontResponse::AccountStart { start: job.view.clone() }
            }
        };
        self.db.set_meta(&job_key(project, id), &serde_json::to_string(&job).map_err(internal)?).map_err(internal)?;
        if job.view != previous {
            self.accounts_bump();
        }
        Ok(response)
    }

    pub(crate) fn pool_starts(&self, project: &str) -> Result<Vec<AccountStartView>, ErrorBody> {
        self.project_dir(project)?;
        self.db
            .meta_suffixes(&format!("{JOB}{project}:"))
            .map_err(internal)?
            .into_iter()
            .map(|id| {
                let mut v = self.pool_job(project, &id)?.view;
                if let Some(tid) = self.db.meta(&receipt(project, &id)).map_err(internal)? {
                    let rec = self.load(project, &tid)?;
                    v.thread = Some(tid);
                    v.state = if rec.status == ThreadStatus::Failed {
                        AccountStartState::Failed
                    } else {
                        AccountStartState::Started
                    };
                    v.reason = rec.error.unwrap_or_else(|| "계정 배정 완료".into());
                }
                Ok(v)
            })
            .collect()
    }

    pub(crate) fn pool_cancel(&self, project: &str, id: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let _guard = self.claim_thread(project, &format!("account-{id}"))?;
        if self.db.meta(&receipt(project, id)).map_err(internal)?.is_some() {
            return Err(err(ErrorCode::Conflict, "이미 시작한 thread는 thread에서 끝낸다"));
        }
        let mut job = self.pool_job(project, id)?;
        job.view.state = AccountStartState::Cancelled;
        job.view.reason = "배정 대기 취소".into();
        self.db.set_meta(&job_key(project, id), &serde_json::to_string(&job).map_err(internal)?).map_err(internal)?;
        self.db.audit(actor, "account_start_cancel", &format!("{project}/{id}"));
        self.accounts_bump();
        Ok(FrontResponse::AccountStart { start: job.view })
    }

    pub(crate) async fn pool_pass(self: &Arc<Self>, project: &str) -> Result<(), ErrorBody> {
        for id in self.db.meta_suffixes(&format!("{JOB}{project}:")).map_err(internal)? {
            if self.pool_job(project, &id)?.view.state == AccountStartState::Queued {
                self.pool_run(project, &id, true).await?;
            }
        }
        Ok(())
    }
}

fn fresh_windows<'a>(
    q: &'a AccountQuota,
    model: Option<&str>,
    now: u64,
) -> Result<Vec<&'a AccountQuotaWindow>, &'static str> {
    let windows: Vec<_> =
        q.windows.iter().filter(|w| w.window.scope.as_deref().is_none_or(|s| model.is_none_or(|m| s == m))).collect();
    if q.problem.is_some() || windows.is_empty() {
        return Err("사용량 모름·조회 실패");
    }
    if windows.iter().any(|w| {
        w.problem.is_some()
            || w.freshness != QuotaFreshness::Fresh
            || aaa_common::quota::freshness(&w.window, now) != QuotaFreshness::Fresh
            || w.window.resets_at_ms.is_none()
            || w.window.duration_mins.is_none()
    }) {
        return Err("오래된 관측·리셋 뒤 미확인·창 정보 부족");
    }
    if ![300, 10080].iter().all(|d| windows.iter().any(|w| w.window.duration_mins == Some(*d))) {
        return Err("필수 5시간·주간 관측 누락");
    }
    Ok(windows)
}

impl SelectionInput {
    pub(crate) fn select(&self, tx: &Connection, now: u64) -> anyhow::Result<(String, Allocation)> {
        let current =
            decode_pool(tx.query_row("SELECT value FROM meta WHERE key=?1", [CONFIG], |r| r.get(0)).optional()?)?;
        anyhow::ensure!(current.revision == self.pool.revision, "start_wait: 계정 풀 설정 변경 · 다시 판정 대기");
        if self.auto {
            anyhow::ensure!(current.projects.contains(&self.project), "start_wait: 프로젝트의 풀 참여가 꺼짐");
        }
        let mut st = tx.prepare("SELECT data FROM threads")?;
        let records = st
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|s| serde_json::from_str::<ThreadRec>(&s))
            .collect::<serde_json::Result<Vec<_>>>()?;
        let mut rejected = self.excluded.clone();
        let mut known = Vec::new();
        let mut unknown = Vec::new();
        for c in &self.candidates {
            let mine: Vec<_> = records
                .iter()
                .filter(|r| {
                    if let Some(a) = &r.account_allocation {
                        !a.released
                            && (a.selection.group == c.member.group
                                || r.account.as_ref().is_some_and(|account| {
                                    self.pool.members.iter().any(|m| &m.account == account && m.group == c.member.group)
                                }))
                    } else {
                        r.account.as_ref().is_some_and(|account| {
                            self.pool.members.iter().any(|m| &m.account == account && m.group == c.member.group)
                        }) && matches!(r.status, ThreadStatus::Open | ThreadStatus::Starting)
                    }
                })
                .collect();
            let reserved: f64 = mine
                .iter()
                .map(|r| f64::from(r.account_allocation.as_ref().map_or(15, |a| a.selection.size.percent())))
                .sum();
            let last = records
                .iter()
                .filter(|r| r.account.as_deref() == Some(&c.member.account))
                .filter_map(|r| r.account_allocation.as_ref().map(|a| a.selection.selected_at_ms))
                .max()
                .unwrap_or(0);
            if !self.auto {
                return Ok(self.allocate(c, false, now, "명시한 계정 · 공유 한도 예약"));
            }
            let quota = c.quota.as_ref();
            if let Some(q) = quota {
                match fresh_windows(q, self.model.as_deref(), now) {
                    Ok(mut windows) => {
                        // 같은 한도라고 연결한 다른 별칭의 숫자도 제약이다. 별칭을 늘려 용량을 더하지 않는다.
                        for other in &self.candidates {
                            if other.member.account != c.member.account
                                && other.member.group == c.member.group
                                && let Some(q) = &other.quota
                                && let Ok(shared) = fresh_windows(q, self.model.as_deref(), now)
                            {
                                windows.extend(shared);
                            }
                        }
                        let headroom = windows
                            .iter()
                            .map(|w| 100.0 - w.window.used_percent - reserved - 10.0 - f64::from(self.size.percent()))
                            .fold(f64::INFINITY, f64::min);
                        if headroom >= 0.0 {
                            let reset = windows.iter().filter_map(|w| w.window.resets_at_ms).min().unwrap_or(u64::MAX);
                            known.push((c, headroom, reset, mine.len(), last));
                        } else {
                            rejected.push(format!("{}: 예약·안전 여유를 빼면 잔량 부족", c.member.account));
                        }
                        continue;
                    }
                    Err(reason) if !q.windows.is_empty() || q.problem != Some(QuotaProblem::Unknown) => {
                        rejected.push(format!("{}: {reason}", c.member.account));
                        continue;
                    }
                    Err(_) => {}
                }
            }
            // 미관측은 한 번의 첫 실행만 허용한다. 공유 그룹의 알려진 제한을 우회하지 않는다.
            let bootstrapped = records.iter().any(|r| {
                r.account.as_deref() == Some(&c.member.account)
                    && r.account_allocation
                        .as_ref()
                        .is_some_and(|a| a.generation == c.generation && a.selection.unobserved)
                    && (r.launch_sent || r.status != ThreadStatus::Failed)
            });
            let observed_group = self.candidates.iter().any(|other| {
                other.member.group == c.member.group
                    && other
                        .quota
                        .as_ref()
                        .is_some_and(|q| !q.windows.is_empty() || q.problem != Some(QuotaProblem::Unknown))
            });
            if bootstrapped || !mine.is_empty() || observed_group {
                rejected.push(format!("{}: 첫 실행 관측 대기·공유 한도 미확인", c.member.account));
            } else {
                unknown.push((c, last));
            }
        }
        if !known.is_empty() {
            let best = known.iter().map(|v| v.1).fold(f64::NEG_INFINITY, f64::max);
            known.retain(|v| v.1 >= best - 5.0);
            known.sort_by(|a, b| (a.2, a.3, a.4, &a.0.member.account).cmp(&(b.2, b.3, b.4, &b.0.member.account)));
            let (candidate, headroom, _, _, _) = known[0];
            return Ok(self.allocate(
                candidate,
                false,
                now,
                &format!("최신 잔량 · 예약·안전 여유 뒤 최소 {headroom:.1}%p"),
            ));
        }
        unknown.sort_by(|a, b| (a.1, &a.0.member.account).cmp(&(b.1, &b.0.member.account)));
        if let Some((c, _)) = unknown.first() {
            return Ok(self.allocate(c, true, now, "사용량 미관측 · 자동 첫 실행"));
        }
        if rejected.is_empty() {
            rejected.push("이 harness·모델에 허용한 계정이 없음".into());
        }
        anyhow::bail!("start_wait: 계정 배정 대기 · {}", rejected.join("; "))
    }

    fn allocate(&self, c: &Candidate, unobserved: bool, now: u64, reason: &str) -> (String, Allocation) {
        (
            c.member.account.clone(),
            Allocation {
                generation: c.generation.clone(),
                ended_at_ms: None,
                released: false,
                selection: AccountSelection {
                    size: self.size,
                    group: c.member.group.clone(),
                    unobserved,
                    reason: reason.into(),
                    selected_at_ms: now,
                },
            },
        )
    }
}
