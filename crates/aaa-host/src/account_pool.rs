//! harness별 계정 자동 배정. 선택·작업 수·시작 영수증은 같은 SQLite transaction에서 확정한다.
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
const CONFIG: &str = "account-pool-v2";
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
    /// 구판 예상 소비량. 이력 읽기용이며 배정 차단에 쓰지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve_percent: Option<u8>,
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
    model: Option<String>,
    auto: bool,
    size: AccountTaskSize,
    candidates: Vec<Candidate>,
    excluded: Vec<String>,
    required_account: Option<String>,
    pub(crate) minimum: [Option<f64>; 2],
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

// v1의 참여 프로젝트는 이전 자료로만 보존한다. 새 정책은 host 전체에 적용한다.
pub(crate) fn project_transition(_tx: &Connection, _from: &str, _to: Option<&str>) -> anyhow::Result<()> {
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

    /// v1 원문은 그대로 두고 별도 key에 한 번만 이전한다. 꺼 둔 옛 계정은 켜지 않는다.
    pub(crate) fn pool_migrate(&self) -> Result<(), ErrorBody> {
        let legacy = self.db.meta("account-pool-v1").map_err(internal)?;
        let accounts = self.accounts.list()?;
        self.db
            .update_meta(CONFIG, |old| {
                if let Some(old) = old {
                    return Ok(old);
                }
                let mut pool = decode_pool(legacy.clone())?;
                if legacy.is_none() {
                    pool.members = accounts
                        .iter()
                        .map(|a| AccountPoolMember { account: a.name.clone(), group: a.name.clone(), models: vec![] })
                        .collect();
                }
                for member in &mut pool.members {
                    member.group = member.account.clone();
                    member.models.clear();
                }
                pool.projects.clear();
                Ok(serde_json::to_string(&pool)?)
            })
            .map_err(internal)
    }

    /// 새 로그인만 기본으로 참여시킨다. 다시 로그인해도 사람이 끈 설정을 되살리지 않는다.
    pub(crate) fn register_account(&self, name: &str, harness: &str, secret: &str) -> Result<AccountView, ErrorBody> {
        let new = !self.accounts.list()?.iter().any(|a| a.name == name);
        let view = self.accounts.import(name, harness, secret)?;
        if new {
            self.db
                .update_meta(CONFIG, |old| {
                    let mut pool = decode_pool(old)?;
                    if !pool.members.iter().any(|m| m.account == name) {
                        pool.members.push(AccountPoolMember {
                            account: name.into(),
                            group: name.into(),
                            models: vec![],
                        });
                        pool.members.sort_by(|a, b| a.account.cmp(&b.account));
                        pool.revision = util::new_id();
                    }
                    Ok(serde_json::to_string(&pool)?)
                })
                .map_err(internal)?;
            self.accounts_bump();
            self.tick_wake.notify_one();
        }
        Ok(view)
    }

    pub(crate) fn pool_project_enabled(&self, _project: &str) -> Result<bool, ErrorBody> {
        Ok(!self.account_pool()?.members.is_empty())
    }

    pub(crate) fn pool_harness_enabled(&self, harness: &str) -> Result<bool, ErrorBody> {
        let accounts = self.accounts.list()?;
        Ok(self
            .account_pool()?
            .members
            .iter()
            .any(|m| accounts.iter().any(|a| a.name == m.account && a.harness == harness)))
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
            if m.account != account {
                return Err(bad("자동 배정 계정 이름을 확인한다"));
            }
            self.accounts.credential(account)?;
        }
        let pool = self.change_pool(revision, |pool| {
            pool.members.retain(|m| m.account != account);
            if let Some(mut m) = member {
                // 옛 client의 그룹·모델 필드는 받아도 배정 조건으로 쓰지 않는다.
                m.group = m.account.clone();
                m.models.clear();
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
        let _ = (revision, project, enabled, actor);
        Err(bad("프로젝트 참여 설정은 없어졌다. account pool add/remove로 계정별 자동 배정을 켜고 끈다"))
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
        _project: &str,
        profile: &ResolvedProfile,
        account: Option<&str>,
        size: AccountTaskSize,
    ) -> Result<Option<SelectionInput>, ErrorBody> {
        let pool = self.account_pool()?;
        let auto = account.is_none();
        if auto && !self.pool_harness_enabled(&profile.harness)? {
            return Ok(None);
        }
        if !auto && !pool.members.iter().any(|m| Some(m.account.as_str()) == account) {
            return Ok(None);
        }
        // 사용량 조회·RPC 잠금은 기동의 선행 조건이 아니다. 완료된 같은 인증 세대의 캐시만 읽는다.
        let report = AccountQuotaReport {
            accounts: if auto {
                pool.members.iter().filter_map(|m| match self.cached_account_quota(&m.account, &profile.harness) {
                Ok(q) => Some(q),
                Err(e) => { tracing::debug!(account = %m.account, code = ?e.code, "배정용 사용량 캐시를 읽지 못했다: {}", e.message); None }
            }).collect()
            } else {
                vec![]
            },
        };
        if auto {
            self.pool_reconcile(&report)?;
        }
        let mut input = SelectionInput {
            pool: pool.clone(),
            model: (!profile.model.is_empty()).then(|| profile.model.clone()),
            auto,
            size,
            candidates: vec![],
            excluded: vec![],
            required_account: None,
            minimum: [None, None],
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
            if auto && crate::account_meta::health(&self.account_meta(&name), util::now_ms()).broken() {
                input.excluded.push(format!("{name}: 인증 끊김"));
                continue;
            }
            let generation = credential.auth_generation.ok_or_else(|| bad("계정 인증 세대를 확인하지 못했다"))?;
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
        self.image_reconcile(report)?;
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
                    let generation = match self.accounts.generation(&member.account) {
                        Ok(generation) => generation,
                        Err(e) => {
                            tracing::warn!(account = %member.account, code = ?e.code, "기존 배정의 인증을 읽지 못해 해당 계정을 건너뛴다: {}", e.message);
                            continue;
                        }
                    };
                    Allocation {
                        reserve_percent: None,
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
                    let mut requested = job.args.clone();
                    // 이 내부 칸은 첫 배정 pass가 채운다. 같은 직접 시작 ID의 재전송은 원래 할 일을 재사용한다.
                    requested.start_task = prior.args.start_task.clone();
                    anyhow::ensure!(
                        prior.actor == actor
                            && serde_json::to_value(&prior.args)? == serde_json::to_value(&requested)?
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
        let _account_read = self.account_rename_gate.read().await;
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
        let checked = async {
            if job.args.start_task.is_none() {
                // 옛 대기의 원본 할 일과 직접 시작의 새 할 일을 한 번만 기록한다.
                // 계정이 없어 Queued로 남아도 다음 pass·재시작이 같은 할 일을 이어 쓴다.
                job.args.start_task = Some(match &job.picked {
                    Some(t) => t.clone(),
                    None => self.create_start_task(&job.args, &job.actor).await?,
                });
                self.db
                    .set_meta(&job_key(project, id), &serde_json::to_string(&job).map_err(internal)?)
                    .map_err(internal)?;
            }
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
        }
        .await;
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

    pub(crate) async fn pool_cancel(&self, project: &str, id: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        // 배정 판정이 진행 중이어도 사용자 취소를 거부하지 않는다. 같은 잠금 아래에서
        // 최신 영수증을 확인하고, pool_run의 마지막 저장 뒤에 취소를 기록한다.
        let _guard = self.thread_op_lock(project, &format!("account-{id}")).lock_owned().await;
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
            if self.pool_job(project, &id)?.view.state == AccountStartState::Queued
                && let FrontResponse::AccountStart { start } = self.pool_run(project, &id, true).await?
                && start.state == AccountStartState::Failed
                && start.thread.is_none()
            {
                // 대기 뒤 실패는 요청한 화면이 닫힌 뒤라 inbox로 한 번 알린다. thread가 생긴 실패는 thread 사건이 알린다.
                let task = self.pool_job(project, &id)?.args.start_task;
                let subject = task.as_ref().and_then(|t| t.short_id.clone()).unwrap_or_else(|| id.clone());
                let title = task.map_or(start.title, |t| t.title);
                self.inbox_once(
                    project,
                    "task-start",
                    &subject,
                    "account start failed",
                    &format!("{title}: 계정 배정 대기 뒤 시작하지 못했다: {}", start.reason),
                );
            }
        }
        Ok(())
    }
}

pub(crate) fn fresh_windows<'a>(
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
            || w.conflict
            || w.freshness != QuotaFreshness::Fresh
            || aaa_common::quota::freshness(&w.window, now) != QuotaFreshness::Fresh
            || w.window.resets_at_ms.is_none()
            || w.window.duration_mins.is_none()
    }) {
        return Err("오래된 관측·리셋 뒤 미확인·창 정보 부족");
    }
    Ok(windows)
}

impl SelectionInput {
    pub(crate) fn only(&mut self, account: &str) {
        self.required_account = Some(account.into());
    }
    pub(crate) fn select(&self, tx: &Connection, now: u64) -> anyhow::Result<(String, Allocation)> {
        self.select_with_percent(tx, now, self.size.percent())
    }

    pub(crate) fn select_with_percent(
        &self,
        tx: &Connection,
        now: u64,
        percent: u8,
    ) -> anyhow::Result<(String, Allocation)> {
        let current =
            decode_pool(tx.query_row("SELECT value FROM meta WHERE key=?1", [CONFIG], |r| r.get(0)).optional()?)?;
        anyhow::ensure!(current.revision == self.pool.revision, "start_wait: 계정 풀 설정 변경 · 다시 판정 대기");
        let mut st = tx.prepare("SELECT data FROM threads")?;
        let records = st
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|s| serde_json::from_str::<ThreadRec>(&s))
            .collect::<serde_json::Result<Vec<_>>>()?;
        let mut rejected = self.excluded.clone();
        let images = crate::image::pool_reservations(tx)?;
        let mut candidates = Vec::new();
        for c in &self.candidates {
            if self.required_account.as_ref().is_some_and(|a| a != &c.member.account) {
                continue;
            }
            if !self.auto {
                return Ok(self.allocate(c, false, now, percent, "명시한 계정"));
            }
            let quota = c.quota.as_ref();
            if quota.is_some_and(|q| matches!(q.problem, Some(QuotaProblem::Expired | QuotaProblem::Locked))) {
                rejected.push(format!("{}: 인증 끊김", c.member.account));
                continue;
            }
            if quota.is_some_and(|q| known_limit(q, self.model.as_deref(), now)) {
                rejected.push(format!("{}: 사용 한도 도달", c.member.account));
                continue;
            }
            if quota.is_some_and(|q| below_usage_threshold(q, self.minimum, now)) {
                rejected.push(format!("{}: 자동화 사용량 문턱", c.member.account));
                continue;
            }
            // 옛 그룹·예상 %는 작업 이력으로만 남긴다. 종료된 작업의 관측 지연은 새 시작을 막지 않는다.
            let active = records
                .iter()
                .filter(|r| {
                    r.account.as_deref() == Some(&c.member.account)
                        && (matches!(r.status, ThreadStatus::Open | ThreadStatus::Starting)
                            || (r.status == ThreadStatus::Failed
                                && r.launch_sent
                                && r.account_allocation
                                    .as_ref()
                                    .is_some_and(|a| !a.released && a.ended_at_ms.is_none())))
                })
                .count()
                + images
                    .iter()
                    .filter(|r| {
                        r.account == c.member.account && !r.allocation.released && r.allocation.ended_at_ms.is_none()
                    })
                    .count();
            let last = records
                .iter()
                .filter(|r| r.account.as_deref() == Some(&c.member.account))
                .map(|r| r.account_allocation.as_ref().map_or(r.created_at_ms, |a| a.selection.selected_at_ms))
                .chain(
                    images
                        .iter()
                        .filter(|r| r.account == c.member.account)
                        .map(|r| r.allocation.selection.selected_at_ms),
                )
                .max()
                .unwrap_or(0);
            let unobserved = quota.is_none_or(|q| fresh_windows(q, self.model.as_deref(), now).is_err());
            candidates.push((c, active, last, unobserved));
        }
        candidates.sort_by(|a, b| (a.1, a.2, &a.0.member.account).cmp(&(b.1, b.2, &b.0.member.account)));
        if let Some((c, active, _, unobserved)) = candidates.first() {
            let reason = if *unobserved {
                "사용량 판단 불가 · 그대로 시작"
            } else {
                "진행 중 작업·최근 배정 순서"
            };
            return Ok(self.allocate(c, *unobserved, now, percent, &format!("{reason} · 진행 중 {active}개")));
        }
        if rejected.is_empty() {
            rejected.push("이 harness에서 사용할 수 있는 자동 배정 계정이 없음".into());
        }
        anyhow::bail!("start_wait: 계정 배정 대기 · {}", rejected.join("; "))
    }

    fn allocate(&self, c: &Candidate, unobserved: bool, now: u64, percent: u8, reason: &str) -> (String, Allocation) {
        (
            c.member.account.clone(),
            Allocation {
                reserve_percent: (percent != self.size.percent()).then_some(percent),
                generation: c.generation.clone(),
                ended_at_ms: None,
                released: false,
                selection: AccountSelection {
                    size: self.size,
                    group: c.member.account.clone(),
                    unobserved,
                    reason: reason.into(),
                    selected_at_ms: now,
                },
            },
        )
    }
}

/// 확실하게 관측한 현재 한도만 제외한다. 일부 창이 없어도 알려진 고갈은 존중한다.
fn known_limit(q: &AccountQuota, model: Option<&str>, now: u64) -> bool {
    if q.problem == Some(QuotaProblem::RateLimited) && q.retry_at_ms.is_some_and(|t| t > now) {
        return true;
    }
    q.problem.is_none()
        && q.windows.iter().any(|w| {
            w.window.scope.as_deref().is_none_or(|s| model == Some(s))
                && w.problem.is_none()
                && !w.conflict
                && w.freshness == QuotaFreshness::Fresh
                && aaa_common::quota::freshness(&w.window, now) == QuotaFreshness::Fresh
                && w.window.resets_at_ms.is_some_and(|t| t > now)
                && w.window.used_percent >= 100.0
        })
}

/// 자동화에 정한 잔량 문턱은 실제 들어온 최신 공용 창에만 적용한다.
pub(crate) fn below_usage_threshold(q: &AccountQuota, minimum: [Option<f64>; 2], now: u64) -> bool {
    q.problem.is_none()
        && q.windows.iter().any(|w| {
            let threshold = match w.window.duration_mins {
                Some(300) => minimum[0],
                Some(10080) => minimum[1],
                _ => None,
            };
            threshold.is_some_and(|threshold| {
                w.window.scope.is_none()
                    && w.problem.is_none()
                    && !w.conflict
                    && w.freshness == QuotaFreshness::Fresh
                    && aaa_common::quota::freshness(&w.window, now) == QuotaFreshness::Fresh
                    && 100.0 - w.window.used_percent < threshold
            })
        })
}
