//! rollout: SHA 고정·모든 대상 사전 검사·host 먼저·같은 ID 재개. 사람/조정자 승인 출처를 보존한다.
//! 참고: herdr handoff 세 분류, web-ui autoChain/current/failed, t3code pending 복구.
use crate::{Host, err};
use aaa_proto::*;
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc, time::Duration};

mod card;
mod recovery;
#[cfg(test)]
mod web_tests;
const STEP_LIMIT_MS: u64 = 120_000;
const RECOVERY_LIMIT_MS: u64 = 3 * STEP_LIMIT_MS;
pub(crate) fn file_sha(path: &Path) -> anyhow::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}
pub(crate) fn running_sha() -> anyhow::Result<String> {
    // 한 프로세스의 실행 이미지는 바뀌지 않는다. unit host 여러 개도 같은 판정을 쓴다.
    static SHA: std::sync::OnceLock<Result<String, String>> = std::sync::OnceLock::new();
    SHA.get_or_init(|| {
        std::env::current_exe().map_err(|e| e.to_string()).and_then(|p| file_sha(&p).map_err(|e| e.to_string()))
    })
    .clone()
    .map_err(anyhow::Error::msg)
}
fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}
fn person(actor: &str) -> bool {
    actor.strip_prefix("person:").and_then(|s| s.parse::<u32>().ok()).is_some_and(|pid| pid > 0)
}
fn valid_id(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}
fn steps(machines: &[String], transfer: bool) -> Vec<RolloutStep> {
    let mut order = Vec::new();
    if transfer {
        order.extend(machines.iter().map(|m| (m, RolloutPhase::Transfer)));
    }
    order.extend(machines.iter().map(|m| (m, RolloutPhase::Preflight)));
    order.extend([
        (&machines[0], RolloutPhase::HostUpdate),
        (&machines[0], RolloutPhase::HostRestart),
        (&machines[0], RolloutPhase::AgentRestart),
    ]);
    for m in &machines[1..] {
        order.extend([(m, RolloutPhase::AgentUpdate), (m, RolloutPhase::AgentRestart)]);
    }
    order
        .into_iter()
        .map(|(m, phase)| RolloutStep {
            machine: m.clone(),
            phase,
            job_id: aaa_common::util::new_id(),
            completed: false,
            result: None,
            host_instance: None,
            started_ms: 0,
            transferred_bytes: 0,
            total_bytes: 0,
            baseline: None,
            guard: None,
            guard_ready: false,
        })
        .collect()
}
fn action(spec: &RolloutSpec, id: &str, phase: RolloutPhase) -> DeployAction {
    match phase {
        RolloutPhase::Transfer => unreachable!("전송은 배포 작업을 실행하지 않는다"),
        RolloutPhase::Preflight | RolloutPhase::HostUpdate | RolloutPhase::AgentUpdate
            if spec.source_machine.is_some() =>
        {
            DeployAction::UpdateStaged { rollout: id.into(), dry_run: phase == RolloutPhase::Preflight }
        }
        RolloutPhase::Preflight | RolloutPhase::HostUpdate | RolloutPhase::AgentUpdate => DeployAction::CheckedUpdate {
            from: spec.from.clone(),
            sha256: spec.sha256.clone(),
            dry_run: phase == RolloutPhase::Preflight,
        },
        RolloutPhase::HostRestart => DeployAction::Restart { service: DeployService::Host },
        RolloutPhase::AgentRestart => DeployAction::Restart { service: DeployService::Agent },
    }
}
/// 완료 영수증은 요청·소속·ID와 일치해야 한다. 재시작은 설치 파일과 실행 이미지를 구별한다.
fn assess(
    r: &RolloutView,
    step: &RolloutStep,
    response: AgentResponse,
    host_instance: &str,
    host_sha: &str,
) -> Result<Option<String>, ErrorBody> {
    let AgentResponse::Deployment { job, running_software, running_sha256, running_instance, .. } = response else {
        return Err(internal("배포 응답이 아니다"));
    };
    if job.id != step.job_id || job.project != r.project || job.action != action(&r.spec, &r.id, step.phase) {
        return Err(internal("배포 영수증이 기록한 요청과 다르다"));
    }
    match job.state {
        DeployState::Queued | DeployState::Running => return Ok(None),
        DeployState::Failed | DeployState::Interrupted => {
            return Err(err(ErrorCode::BadRequest, job.result.unwrap_or_else(|| "배포 실패".into())));
        }
        DeployState::Completed => {}
    }
    match step.phase {
        RolloutPhase::Transfer => Err(internal("전송 단계에 배포 영수증이 왔다")),
        RolloutPhase::Preflight | RolloutPhase::HostUpdate | RolloutPhase::AgentUpdate => {
            let value: serde_json::Value =
                serde_json::from_str(job.result.as_deref().unwrap_or("")).map_err(internal)?;
            if value["sha256"] != r.spec.sha256 {
                return Err(err(ErrorCode::BadRequest, "후보/설치 SHA-256 불일치"));
            }
            let version = value["version"].as_str().ok_or_else(|| internal("후보 판이 없다"))?;
            if r.spec.commit.as_ref().is_some_and(|c| !version.ends_with(&format!("+{}", &c[..12]))) {
                return Err(err(ErrorCode::BadRequest, "후보 판의 커밋 표시가 출처와 다르다"));
            }
            if r.version.as_ref().is_some_and(|v| v != version) {
                return Err(err(ErrorCode::BadRequest, "후보 판 불일치"));
            }
            Ok(Some(version.into()))
        }
        RolloutPhase::HostRestart => {
            if step.host_instance.as_deref() == Some(host_instance) {
                return Ok(None);
            }
            if host_sha != r.spec.sha256
                || aaa_common::util::software() != format!("aaa {}", r.version.as_deref().unwrap_or(""))
            {
                return Err(err(ErrorCode::BadRequest, "옛 host가 돌아왔다: 실행 판/SHA-256 불일치"));
            }
            Ok(Some("새 host 실행 판/SHA-256 확인".into()))
        }
        RolloutPhase::AgentRestart => {
            if running_instance.is_empty() || running_instance == job.instance {
                return Ok(None);
            }
            if running_sha256 != r.spec.sha256
                || running_software != format!("aaa {}", r.version.as_deref().unwrap_or(""))
            {
                return Err(err(ErrorCode::BadRequest, "옛 agent가 돌아왔다: 실행 판/SHA-256 불일치"));
            }
            Ok(Some("새 agent 실행 판/SHA-256 확인".into()))
        }
    }
}
impl Host {
    fn load_rollout(&self, id: &str) -> Result<RolloutView, ErrorBody> {
        if !valid_id(id) {
            return Err(err(ErrorCode::BadRequest, "rollout ID 형식이 아니다"));
        }
        let raw = self
            .db
            .meta(&format!("rollout:{id}"))
            .map_err(internal)?
            .ok_or_else(|| err(ErrorCode::NotFound, "rollout 기록이 없다"))?;
        serde_json::from_str(&raw).map_err(internal)
    }
    fn save_rollout(&self, r: &mut RolloutView) -> Result<(), ErrorBody> {
        r.updated_ms = aaa_common::util::now_ms();
        let completion = card::completion_item(r);
        let announced = self
            .db
            .save_rollout_record(&r.id, &serde_json::to_string(r).map_err(internal)?, completion.as_ref())
            .map_err(internal)?;
        self.flow_project(&r.project, "rollout_changed", None, r.id.clone());
        self.rollout_changed.send_modify(|v| *v += 1);
        if announced && let Some(item) = completion {
            self.announce_rollout_finished(r, &item);
        }
        Ok(())
    }
    /// 사람 터미널·웹 조작 기기의 승인은 그 rollout 끝까지 남는다. coordinator 시작만 매번 safety를 다시 읽는다
    fn rollout_permitted(&self, r: &RolloutView) -> bool {
        person(&r.actor) || crate::deploy::device(&r.actor) || self.safety(&r.project).coordinator_deploy
    }
    /// 자동화 제안의 기준은 최근 성공한 커밋 기록이다. 실패한 새 rollout은 이 기준을 지우지 않는다.
    pub(crate) fn rollout_deployed_commit(&self, project: &str) -> Result<Option<(String, String)>, ErrorBody> {
        let mut records = Vec::new();
        for id in self.db.meta_suffixes("rollout:").map_err(internal)?.into_iter().filter(|id| valid_id(id)) {
            let r = self.load_rollout(&id)?;
            if r.project == project && r.state == RolloutState::Done && r.spec.commit.is_some() {
                records.push(r);
            }
        }
        Ok(records
            .into_iter()
            .max_by_key(|r| (r.updated_ms, r.id.clone()))
            .and_then(|r| r.spec.commit.map(|s| (r.id, s.to_ascii_lowercase()))))
    }
    pub(crate) fn rollout_busy(&self) -> Result<bool, ErrorBody> {
        self.db
            .meta("rollout:active")
            .map_err(internal)?
            .map(|id| self.load_rollout(&id).map(|r| recovery::active(r.state)))
            .transpose()
            .map(|r| r.unwrap_or(false))
    }
    /// 대상 한 머신이 rollout을 받을 수 있는가(연결·기능). 시작과 웹 점검이 같이 쓴다
    fn target_problem(&self, mid: &str, staged: bool) -> Option<ErrorBody> {
        if let Err(e) = self.live_conn(mid) {
            return Some(e);
        }
        if !self.machine_feature(mid, "rollout.v1") {
            return Some(err(ErrorCode::UnsupportedVersion, "먼저 rollout.v1을 아는 host·agent를 반영한다"));
        }
        if staged && !self.machine_feature(mid, "deploy_stage.v1") {
            return Some(err(ErrorCode::UnsupportedVersion, "전송 대상에 deploy_stage.v1이 필요하다"));
        }
        if !self.machine_feature(mid, "rollout_recovery.v1") {
            return Some(err(ErrorCode::UnsupportedVersion, "먼저 독립 복구 감시를 아는 host·agent를 반영한다"));
        }
        None
    }
    /// host 머신을 맨 앞에, 나머지는 받은 순서(비었으면 등록된 전부를 ID 순)로
    fn rollout_order(&self, requested: &[String]) -> Result<Vec<String>, ErrorBody> {
        let local = self.resolve_machine(None)?;
        let mut machines = vec![local];
        let requested = if requested.is_empty() {
            let i = self.inner.lock().unwrap();
            let mut ids: Vec<_> = i.machines.keys().cloned().collect();
            ids.sort();
            ids
        } else {
            requested.iter().map(|m| self.resolve_machine(Some(m))).collect::<Result<Vec<_>, _>>()?
        };
        for mid in requested {
            if !machines.contains(&mid) {
                machines.push(mid);
            }
        }
        Ok(machines)
    }
    pub(crate) async fn rollout_start(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        id: String,
        mut spec: RolloutSpec,
    ) -> Result<FrontResponse, ErrorBody> {
        // 웹 조작 기기는 빌드 머신에 이미 있는 후보만 고른다(관리 GUI W6a, 답 2A). 응답은 표시용 요약이다
        let web = crate::deploy::web_controller(role, actor);
        if web && spec.source_machine.is_none() {
            return Err(err(ErrorCode::BadRequest, "웹은 빌드 머신의 후보(머신:폴더/aaa)로만 배포한다"));
        }
        let project = if person(actor) || web { self.instance_project()? } else { self.deployment_authorize(actor)? };
        if !valid_id(&id)
            || spec.sha256.len() != 64
            || !spec.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || !(Path::new(&spec.from).is_absolute() || (spec.source_machine.is_some() && spec.from.starts_with("~/")))
            || spec.commit.as_ref().is_some_and(|c| c.len() != 40 || !c.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(err(ErrorCode::BadRequest, "ID·절대 경로·SHA-256·커밋 형식을 확인한다"));
        }
        spec.sha256.make_ascii_lowercase();
        if let Some(source) = &spec.source_machine {
            let source = self.resolve_machine(Some(source))?;
            self.live_conn(&source)?;
            if !self.machine_feature(&source, "deploy_stage.v1") {
                return Err(err(ErrorCode::UnsupportedVersion, "빌드 머신에 deploy_stage.v1이 필요하다"));
            }
            spec.source_machine = Some(source);
        }
        let _gate = self.deployment_gate.lock().unwrap();
        // 같은 ID의 중복 시작은 기존 요청과 같을 때만 조회로 취급한다.
        if let Ok(old) = self.load_rollout(&id) {
            // all은 첫 시작 때 고정한 대상을 유지한다. 명시한 이름은 기존 머신 ID로 정규화한다.
            if spec.machines.is_empty() {
                spec.machines = old.spec.machines.clone();
            } else {
                let mut normalized = vec![old.spec.machines[0].clone()];
                for m in &spec.machines {
                    let mid = self.resolve_machine(Some(m))?;
                    if !normalized.contains(&mid) {
                        normalized.push(mid);
                    }
                }
                spec.machines = normalized;
            }
            if old.project != project || old.spec != spec {
                return Err(err(ErrorCode::BadRequest, "같은 rollout ID를 다른 요청에 쓸 수 없다"));
            }
            return Ok(if web {
                FrontResponse::RolloutCards { cards: vec![self.card_of(old)] }
            } else {
                FrontResponse::Rollout { rollout: old }
            });
        }
        if self.rollout_busy()? {
            return Err(err(ErrorCode::BadRequest, "진행 중인 rollout이 있다"));
        }
        let machines = self.rollout_order(&spec.machines)?;
        for mid in &machines {
            if let Some(e) = self.target_problem(mid, spec.source_machine.is_some()) {
                return Err(e);
            }
        }
        spec.machines = machines;
        let now = aaa_common::util::now_ms();
        let mut r = RolloutView {
            id,
            project,
            actor: actor.into(),
            steps: steps(&spec.machines, spec.source_machine.is_some()),
            spec,
            state: RolloutState::Running,
            cursor: 0,
            version: None,
            error: None,
            finish: None,
            recovery_enabled: true,
            recovery: None,
            recovery_started_ms: 0,
            created_ms: now,
            updated_ms: now,
        };
        // pointer가 먼저 남아도 복구 가능한 정본을 한 transaction으로 저장한다.
        self.db.start_rollout(&r.id, &serde_json::to_string(&r).map_err(internal)?).map_err(internal)?;
        self.save_rollout(&mut r)?;
        self.resume_rollout();
        Ok(if web {
            FrontResponse::RolloutCards { cards: vec![self.card_of(r)] }
        } else {
            FrontResponse::Rollout { rollout: r }
        })
    }
    pub(crate) fn rollout_status(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        id: String,
    ) -> Result<FrontResponse, ErrorBody> {
        let web = crate::deploy::web_controller(role, actor);
        let project = if person(actor) || web {
            self.instance_project()?
        } else {
            self.coordinator_project_of_actor(actor)
                .ok_or_else(|| err(ErrorCode::Forbidden, "rollout은 사람 터미널 또는 coordinator 세션에서 조회한다"))?
        };
        let r = self.load_rollout(&id)?;
        if r.project != project {
            return Err(err(ErrorCode::Forbidden, "다른 프로젝트의 rollout이다"));
        }
        self.resume_rollout();
        Ok(if web {
            FrontResponse::RolloutCards { cards: vec![self.card_of(r)] }
        } else {
            FrontResponse::Rollout { rollout: r }
        })
    }
    /// 웹 「배포」의 시작 전 점검(관리 GUI W6a): 빌드 머신 agent가 후보 provenance·SHA를 대조하고 dry-run으로 self-check한다.
    /// 대상은 시작과 같은 순서·검사를 보인다. 아무것도 저장하지 않는다
    pub(crate) async fn rollout_plan(
        &self,
        role: DeviceRole,
        actor: &str,
        source: String,
        path: String,
        machines: Vec<String>,
    ) -> Result<FrontResponse, ErrorBody> {
        // 점검은 빌드 머신에서 후보를 실행한다. 세션 안 agent·coordinator가 다른 머신에서 실행 파일을 띄우는 길이 되지 않게 한다
        if !(crate::deploy::web_controller(role, actor) || person(actor)) {
            return Err(err(ErrorCode::Forbidden, "배포 점검은 웹 조작 기기나 사람의 aaa 밖 터미널에서 한다"));
        }
        self.instance_project()?;
        // 진행 중에는 시작하지 못하고, 빌드 머신의 dry-run이 그 rollout 단계와 겹친다
        if self.rollout_busy()? {
            return Err(err(ErrorCode::BadRequest, "진행 중인 rollout이 있다. 끝난 뒤 다시 점검한다"));
        }
        if !(Path::new(&path).is_absolute() || path.starts_with("~/")) {
            return Err(err(ErrorCode::BadRequest, "후보는 빌드 머신의 절대 경로나 ~/로 시작하는 경로다"));
        }
        let src = self.resolve_machine(Some(&source))?;
        let conn = self.live_conn(&src)?;
        if !self.machine_feature(&src, "deploy_candidate.v1") || !self.machine_feature(&src, "deploy_stage.v1") {
            return Err(err(
                ErrorCode::UnsupportedVersion,
                "빌드 머신 agent가 후보 읽기(deploy_candidate.v1)를 모른다. 먼저 새 판 agent를 반영한다",
            ));
        }
        // self-check는 후보를 두 번 실행한다(`--version`·`config check`). agent 실행기의 한도가 먼저 끝낸다
        let response = conn
            .request_timeout(AgentRequest::DeployCandidate { path: path.clone() }, Duration::from_millis(STEP_LIMIT_MS))
            .await?;
        let AgentResponse::DeployCandidate { sha256, commit, version, total_bytes, built_at } = response else {
            return Err(internal("후보 읽기 응답이 아니다"));
        };
        let candidate = DeployCandidateView {
            machine: self.machine_name(&src),
            path,
            sha256,
            commit,
            version,
            total_bytes,
            built_at,
        };
        let local = self.resolve_machine(None)?;
        let targets = self
            .rollout_order(&machines)?
            .into_iter()
            .map(|mid| RolloutTargetView {
                machine: self.machine_name(&mid),
                host: mid == local,
                problem: self.target_problem(&mid, true).map(|e| e.message),
            })
            .collect();
        Ok(FrontResponse::RolloutPlan { plan: RolloutPlanView { candidate, targets, busy: self.rollout_busy()? } })
    }
    /// 이 프로젝트의 최근 rollout 요약(새것부터)
    pub(crate) fn rollouts(&self) -> Result<FrontResponse, ErrorBody> {
        let project = self.instance_project()?;
        let mut records = Vec::new();
        for id in self.db.meta_suffixes("rollout:").map_err(internal)?.into_iter().filter(|id| valid_id(id)) {
            let r = self.load_rollout(&id)?;
            if r.project == project {
                records.push(r);
            }
        }
        records.sort_by(|a, b| (b.created_ms, &b.id).cmp(&(a.created_ms, &a.id)));
        records.truncate(crate::deploy::RECENT);
        Ok(FrontResponse::RolloutCards { cards: records.into_iter().map(|r| self.card_of(r)).collect() })
    }
    pub(crate) fn resume_rollout(self: &Arc<Self>) {
        let h = self.clone();
        tokio::spawn(async move {
            let _guard = h.rollout_gate.lock().await;
            if let Err(e) = h.drive_rollout().await {
                tracing::error!("rollout 기록/진행 실패: {}", e.message);
            }
        });
    }
    async fn drive_rollout(&self) -> Result<(), ErrorBody> {
        let Some(id) = self.db.meta("rollout:active").map_err(internal)? else {
            return Ok(());
        };
        let mut r = self.load_rollout(&id)?;
        let mut changes = self.changed.subscribe();
        while recovery::active(r.state) {
            let notified = self.rollout_wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let mut waiting_for_commit = false;
            if r.state == RolloutState::Running {
                match self.settle_rollout_guards(&mut r).await {
                    Ok(ready) => waiting_for_commit = !ready,
                    Err(e) if matches!(e.code, ErrorCode::AgentDisconnected | ErrorCode::Timeout) => {
                        waiting_for_commit = true
                    }
                    Err(e) => {
                        let cursor = (r.cursor as usize).min(r.steps.len().saturating_sub(1));
                        self.stop_or_recover(&mut r, cursor, e.message).await?;
                    }
                }
            }
            if r.state == RolloutState::Recovering {
                self.poll_rollout_recovery(&mut r).await?;
            } else if !waiting_for_commit && r.state == RolloutState::Running {
                let cursor = r.cursor as usize;
                if cursor == r.steps.len() {
                    let response = self.project_files(true, "rollout:finish");
                    let failed = matches!(&response,FrontResponse::ProjectFiles {checks} if checks.iter().any(|c|c.error.is_some()));
                    r.finish = Some(serde_json::to_string(&response).map_err(internal)?);
                    r.state = if failed { RolloutState::Stopped } else { RolloutState::Done };
                    if failed {
                        r.error = Some("coordinator 안내 갱신 실패".into());
                    }
                    self.save_rollout(&mut r)?;
                    break;
                }
                if !self.rollout_permitted(&r) {
                    self.stop_or_recover(&mut r, cursor, "coordinator 배포 허용이 철회됐다".into()).await?;
                } else {
                    if r.steps[cursor].started_ms == 0 {
                        r.steps[cursor].started_ms = aaa_common::util::now_ms();
                        r.steps[cursor].host_instance = Some(self.rollout_instance.clone());
                        self.save_rollout(&mut r)?;
                    }
                    let step = r.steps[cursor].clone();
                    if aaa_common::util::now_ms().saturating_sub(step.started_ms) >= STEP_LIMIT_MS {
                        self.stop_or_recover(
                            &mut r,
                            cursor,
                            format!("무응답/옛 판: {} {:?} 단계 한도 초과", step.machine, step.phase),
                        )
                        .await?;
                    } else {
                        let result = match self.prepare_rollout_guard(&mut r, cursor).await {
                            Ok(()) => {
                                if step.phase == RolloutPhase::Transfer {
                                    self.transfer_candidate(&mut r, cursor).await
                                } else {
                                    self.rollout_step(&mut r, &step).await
                                }
                            }
                            Err(e) => Err(e),
                        };
                        match result {
                            Ok(Some(result)) => {
                                if matches!(step.phase, RolloutPhase::Preflight | RolloutPhase::Transfer)
                                    && r.version.is_none()
                                {
                                    r.version = Some(result.clone());
                                }
                                r.steps[cursor].completed = true;
                                r.steps[cursor].result = Some(result);
                                r.cursor += 1;
                                self.save_rollout(&mut r)?;
                                continue;
                            }
                            Err(e) if !matches!(e.code, ErrorCode::AgentDisconnected | ErrorCode::Timeout) => {
                                self.stop_or_recover(&mut r, cursor, e.message).await?
                            }
                            _ => {}
                        }
                    }
                }
            }
            if !recovery::active(r.state) {
                break;
            }
            let now = aaa_common::util::now_ms();
            let deadline = if r.state == RolloutState::Recovering {
                r.recovery_started_ms.saturating_add(RECOVERY_LIMIT_MS)
            } else if let Some(step) = r.steps.get(r.cursor as usize).filter(|s| s.started_ms != 0) {
                step.started_ms.saturating_add(STEP_LIMIT_MS)
            } else {
                r.steps
                    .iter()
                    .filter(|s| s.completed && s.guard.is_some())
                    .map(|s| s.started_ms.saturating_add(STEP_LIMIT_MS))
                    .max()
                    .unwrap_or(now + STEP_LIMIT_MS)
            };
            let remaining = deadline.saturating_sub(now);
            if remaining == 0 {
                if r.state == RolloutState::Recovering {
                    r.state = RolloutState::RecoveryFailed;
                    r.error = Some("복구 영수증/옛 실행 신원 확인 한도 초과: 사람이 확인한다".into());
                    self.save_rollout(&mut r)?;
                    break;
                }
                let cursor = (r.cursor as usize).min(r.steps.len().saturating_sub(1));
                self.stop_or_recover(&mut r, cursor, "재개/완료 확인 한도 초과".into()).await?;
                continue;
            }
            tokio::select! { _=&mut notified=>{}, _=changes.changed()=>{}, _=tokio::time::sleep(Duration::from_millis(remaining))=>{} }
        }
        if r.spec.source_machine.is_some() {
            for mid in &r.spec.machines {
                if let Ok(conn) = self.live_conn(mid)
                    && let Err(e) = conn
                        .request_timeout(
                            AgentRequest::DeployStageClean {
                                rollout: r.id.clone(),
                                keep: r.state == RolloutState::Done,
                            },
                            Duration::from_secs(2),
                        )
                        .await
                {
                    tracing::warn!("rollout 후보 정리 실패: {}", e.message);
                }
            }
        }
        Ok(())
    }
    async fn rollout_step(&self, r: &mut RolloutView, step: &RolloutStep) -> Result<Option<String>, ErrorBody> {
        let conn = self.live_conn(&step.machine)?;
        if !self.machine_feature(&step.machine, "rollout.v1") {
            return Err(err(ErrorCode::UnsupportedVersion, "agent rollout 기능이 사라졌다"));
        }
        let response = match conn.request(AgentRequest::DeployStatus { id: step.job_id.clone() }).await {
            // 보냈는지 모르는 경우도 전송 전에 저장한 같은 ID만 쓴다.
            Err(e) if e.code == ErrorCode::NotFound => {
                conn.request(AgentRequest::DeployStart {
                    id: step.job_id.clone(),
                    project: r.project.clone(),
                    action: action(&r.spec, &r.id, step.phase),
                })
                .await?
            }
            result => result?,
        };
        if r.recovery_enabled
            && step.phase == RolloutPhase::Preflight
            && let AgentResponse::Deployment {
                job,
                running_software,
                running_sha256,
                running_instance,
                running_started_ms,
            } = &response
            && job.state == DeployState::Completed
        {
            if running_sha256.len() != 64
                || running_instance.is_empty()
                || *running_started_ms == 0
                || !running_software.starts_with("aaa ")
            {
                return Err(internal("배포 전 실행 신원이 없다"));
            }
            let cursor = r.cursor as usize;
            r.steps[cursor].baseline = Some(RuntimeIdentity {
                sha256: running_sha256.clone(),
                software: running_software.clone(),
                instance: running_instance.clone(),
                started_ms: *running_started_ms,
            });
        }
        assess(r, step, response, &self.rollout_instance, &self.running_sha256)
    }
    async fn transfer_candidate(&self, r: &mut RolloutView, cursor: usize) -> Result<Option<String>, ErrorBody> {
        let source = r.spec.source_machine.as_ref().ok_or_else(|| internal("빌드 머신 기록이 없다"))?.clone();
        let target = r.steps[cursor].machine.clone();
        // 새 host의 DB 머신에는 hello 전 기능 목록이 없다. 미연결을 기능 소멸로 확정하지 않는다.
        self.live_conn(&source)?;
        let dest = self.live_conn(&target)?;
        for mid in [&source, &target] {
            if !self.machine_feature(mid, "deploy_stage.v1") {
                return Err(err(ErrorCode::UnsupportedVersion, "agent 전송 기능이 사라졌다"));
            }
        }
        let initial = dest.request(AgentRequest::DeployStageStatus { rollout: r.id.clone() }).await;
        let mut offset = match initial {
            Ok(AgentResponse::DeployStage { stage }) => {
                let version = stage_receipt(r, &stage)?;
                r.steps[cursor].transferred_bytes = stage.offset;
                r.steps[cursor].total_bytes = stage.total;
                if version.is_some() {
                    return Ok(version);
                }
                stage.offset
            }
            Err(e) if e.code == ErrorCode::NotFound => 0,
            Err(e) => return Err(e),
            _ => return Err(internal("전송 상태 응답이 아니다")),
        };
        loop {
            if !self.rollout_permitted(r) {
                return Err(err(ErrorCode::Forbidden, "coordinator 배포 허용이 철회됐다"));
            }
            let left =
                r.steps[cursor].started_ms.saturating_add(STEP_LIMIT_MS).saturating_sub(aaa_common::util::now_ms());
            if left == 0 {
                return Err(err(ErrorCode::BadRequest, "전송 단계 한도 초과"));
            }
            let response = self
                .live_conn(&source)?
                .request(AgentRequest::DeployFetch {
                    path: r.spec.from.clone(),
                    offset,
                    len: DEPLOY_CHUNK as u32,
                    sha256: r.spec.sha256.clone(),
                })
                .await?;
            let AgentResponse::DeployChunk { offset: got, data, eof, total } = response else {
                return Err(internal("후보 조각 응답이 아니다"));
            };
            let next = offset.checked_add(data.0.len() as u64).ok_or_else(|| internal("전송 크기 넘침"))?;
            if got != offset
                || data.0.len() > DEPLOY_CHUNK
                || total == 0
                || total > DEPLOY_MAX_BYTES
                || next > total
                || eof != (next == total)
                || (data.0.is_empty() && !eof)
                || (r.steps[cursor].total_bytes != 0 && r.steps[cursor].total_bytes != total)
            {
                return Err(err(ErrorCode::BadRequest, "후보 전송 크기/offset 불일치"));
            }
            let left =
                r.steps[cursor].started_ms.saturating_add(STEP_LIMIT_MS).saturating_sub(aaa_common::util::now_ms());
            if left == 0 {
                return Err(err(ErrorCode::BadRequest, "전송 단계 한도 초과"));
            }
            let reply = dest
                .request_timeout(
                    AgentRequest::DeployStage {
                        rollout: r.id.clone(),
                        project: r.project.clone(),
                        offset,
                        data,
                        eof,
                        sha256: r.spec.sha256.clone(),
                        total,
                    },
                    Duration::from_millis(left),
                )
                .await?;
            let AgentResponse::DeployStage { stage } = reply else {
                return Err(internal("전송 영수증이 아니다"));
            };
            let version = stage_receipt(r, &stage)?;
            if stage.offset != next || stage.total != total || stage.complete != eof {
                return Err(err(ErrorCode::BadRequest, "조각 영수증 offset/EOF 불일치"));
            }
            r.steps[cursor].transferred_bytes = next;
            r.steps[cursor].total_bytes = total;
            self.save_rollout(r)?;
            if version.is_some() {
                return Ok(version);
            }
            offset = next;
        }
    }
}
fn stage_receipt(r: &RolloutView, stage: &DeployStageView) -> Result<Option<String>, ErrorBody> {
    if stage.rollout != r.id
        || stage.project != r.project
        || stage.sha256 != r.spec.sha256
        || stage.total == 0
        || stage.total > DEPLOY_MAX_BYTES
        || stage.offset > stage.total
    {
        return Err(err(ErrorCode::BadRequest, "전송 영수증 소속/SHA-256/크기 불일치"));
    }
    if !stage.complete {
        return Ok(None);
    }
    let version = stage.version.as_ref().ok_or_else(|| internal("전송 self-check 판이 없다"))?;
    if stage.offset != stage.total
        || r.version.as_ref().is_some_and(|v| v != version)
        || r.spec.commit.as_ref().is_some_and(|c| !version.ends_with(&format!("+{}", &c[..12])))
    {
        return Err(err(ErrorCode::BadRequest, "전송 후보 판/크기 불일치"));
    }
    Ok(Some(version.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn record() -> RolloutView {
        RolloutView {
            id: aaa_common::util::new_id(),
            project: "demo".into(),
            actor: "local:test@coordinator".into(),
            spec: RolloutSpec {
                from: "/candidate/aaa".into(),
                source_machine: None,
                sha256: "a".repeat(64),
                commit: None,
                machines: vec!["host".into(), "remote".into()],
            },
            state: RolloutState::Running,
            steps: steps(&["host".into(), "remote".into()], false),
            cursor: 0,
            version: Some(env!("CARGO_PKG_VERSION").into()),
            error: None,
            finish: None,
            recovery_enabled: false,
            recovery: None,
            recovery_started_ms: 0,
            created_ms: 0,
            updated_ms: 0,
        }
    }
    fn response(r: &RolloutView, step: &RolloutStep, state: DeployState, sha: &str, instance: &str) -> AgentResponse {
        AgentResponse::Deployment {
            job: DeployJob {
                id: step.job_id.clone(),
                project: r.project.clone(),
                action: action(&r.spec, &r.id, step.phase),
                state,
                instance: "old-agent".into(),
                executor_pid: 1,
                expected_sha256: r.spec.sha256.clone(),
                result: Some(serde_json::json!({"sha256":sha,"version":env!("CARGO_PKG_VERSION")}).to_string()),
                created_ms: 0,
                updated_ms: 0,
            },
            running_software: aaa_common::util::software(),
            running_sha256: sha.into(),
            running_instance: instance.into(),
            running_started_ms: 1,
        }
    }
    #[test]
    fn transition_receipts_require_success_hash_and_new_process() {
        let r = record();
        let order: Vec<_> = r.steps.iter().map(|s| (s.machine.as_str(), s.phase)).collect();
        assert_eq!(
            order,
            vec![
                ("host", RolloutPhase::Preflight),
                ("remote", RolloutPhase::Preflight),
                ("host", RolloutPhase::HostUpdate),
                ("host", RolloutPhase::HostRestart),
                ("host", RolloutPhase::AgentRestart),
                ("remote", RolloutPhase::AgentUpdate),
                ("remote", RolloutPhase::AgentRestart)
            ]
        );
        for step in &r.steps {
            let mut step = step.clone();
            step.host_instance = Some("old-host".into());
            let check = |state, sha: &str, agent: &str, host: &str| {
                assess(&r, &step, response(&r, &step, state, sha, agent), host, sha)
            };
            for state in [DeployState::Queued, DeployState::Running] {
                assert_eq!(check(state, &r.spec.sha256, "new-agent", "new-host").unwrap(), None);
            }
            for state in [DeployState::Failed, DeployState::Interrupted] {
                assert!(check(state, &r.spec.sha256, "new-agent", "new-host").is_err(), "{:?} {state:?}", step.phase);
            }
            assert!(check(DeployState::Completed, &r.spec.sha256, "new-agent", "new-host").unwrap().is_some());
            assert!(
                check(DeployState::Completed, &"b".repeat(64), "new-agent", "new-host").is_err(),
                "{:?} SHA 검증",
                step.phase
            );
            if step.phase == RolloutPhase::HostRestart {
                assert_eq!(check(DeployState::Completed, &r.spec.sha256, "new-agent", "old-host").unwrap(), None);
            }
            if step.phase == RolloutPhase::AgentRestart {
                assert_eq!(check(DeployState::Completed, &r.spec.sha256, "old-agent", "new-host").unwrap(), None);
            }
            let mut wrong = response(&r, &step, DeployState::Completed, &r.spec.sha256, "new-agent");
            if let AgentResponse::Deployment { job, .. } = &mut wrong {
                job.id = aaa_common::util::new_id();
            }
            assert!(assess(&r, &step, wrong, "new-host", &r.spec.sha256).is_err());
        }
    }
    #[test]
    fn staged_rollouts_transfer_every_target_before_preflight_and_validate_receipts() {
        let mut r = record();
        r.spec.source_machine = Some("builder".into());
        r.steps = steps(&r.spec.machines, true);
        assert_eq!(
            r.steps.iter().take(4).map(|s| s.phase).collect::<Vec<_>>(),
            vec![RolloutPhase::Transfer, RolloutPhase::Transfer, RolloutPhase::Preflight, RolloutPhase::Preflight]
        );
        let mut receipt = DeployStageView {
            rollout: r.id.clone(),
            project: r.project.clone(),
            sha256: r.spec.sha256.clone(),
            total: 10,
            offset: 3,
            complete: false,
            version: None,
        };
        assert_eq!(stage_receipt(&r, &receipt).unwrap(), None);
        receipt.complete = true;
        receipt.version = r.version.clone();
        assert!(stage_receipt(&r, &receipt).is_err());
        receipt.offset = 10;
        assert_eq!(stage_receipt(&r, &receipt).unwrap(), r.version);
        receipt.project = "other".into();
        assert!(stage_receipt(&r, &receipt).is_err());
        receipt.project = r.project.clone();
        receipt.sha256 = "b".repeat(64);
        assert!(stage_receipt(&r, &receipt).is_err());
        assert_eq!(
            action(&r.spec, &r.id, RolloutPhase::HostUpdate),
            DeployAction::UpdateStaged { rollout: r.id.clone(), dry_run: false }
        );
    }
}
