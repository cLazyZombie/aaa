//! 실패 정책과 독립 guard의 내구 영수증. 다른 머신의 완료를 되돌리지 않는다.
use super::*;
use aaa_proto::{DeployGuardControl as Control, DeployGuardPhase as Phase};

pub(super) fn active(state: RolloutState) -> bool {
    matches!(state, RolloutState::Running | RolloutState::Recovering)
}
pub(super) fn guard_spec(r: &RolloutView, cursor: usize) -> Option<DeployGuardSpec> {
    let step = r.steps.get(cursor)?;
    let phase = match step.phase {
        RolloutPhase::HostUpdate | RolloutPhase::HostRestart => RolloutPhase::HostRestart,
        RolloutPhase::AgentUpdate | RolloutPhase::AgentRestart => RolloutPhase::AgentRestart,
        _ => return None,
    };
    r.steps.iter().find(|s| s.machine == step.machine && s.phase == phase)?.guard.clone()
}
impl Host {
    async fn guard_control(
        &self,
        machine: &str,
        spec: &DeployGuardSpec,
        control: Control,
    ) -> Result<DeployGuardView, ErrorBody> {
        let response =
            self.live_conn(machine)?.request(AgentRequest::DeployGuardControl { id: spec.id.clone(), control }).await?;
        self.guard_receipt(spec, response)
    }
    pub(super) fn guard_receipt(
        &self,
        spec: &DeployGuardSpec,
        response: AgentResponse,
    ) -> Result<DeployGuardView, ErrorBody> {
        let AgentResponse::DeployGuard { guard } = response else {
            return Err(internal("감시 영수증이 아니다"));
        };
        if &guard.spec != spec || guard.worker_pid == 0 {
            return Err(internal("감시 영수증의 작업/소속/실행기가 다르다"));
        }
        Ok(*guard)
    }
    pub(super) async fn prepare_rollout_guard(&self, r: &mut RolloutView, cursor: usize) -> Result<(), ErrorBody> {
        if !r.recovery_enabled {
            return Ok(());
        }
        let had = r.steps.iter().filter(|s| s.guard.is_some()).count();
        let Some(restart) = self.set_guard_spec(r, cursor)? else {
            return Ok(());
        };
        if r.steps.iter().filter(|s| s.guard.is_some()).count() != had {
            self.save_rollout(r)?;
        }
        let machine = r.steps[restart].machine.clone();
        let spec = r.steps[restart].guard.as_ref().unwrap();
        let response = self.live_conn(&machine)?.request(AgentRequest::DeployGuardArm { spec: spec.clone() }).await?;
        let view = self.guard_receipt(spec, response)?;
        if matches!(view.phase, Phase::Failed | Phase::Cancelled) {
            return Err(err(ErrorCode::BadRequest, "독립 복구 감시가 준비되지 않았다"));
        }
        if !r.steps[restart].guard_ready {
            r.steps[restart].guard_ready = true;
            self.save_rollout(r)?;
        }
        Ok(())
    }
    /// 이 단계가 세울 감시의 재시작 단계 번호. 아직 감시 정본이 없으면 만든다(저장은 부르는 쪽이 한다)
    pub(super) fn set_guard_spec(&self, r: &mut RolloutView, cursor: usize) -> Result<Option<usize>, ErrorBody> {
        let step = &r.steps[cursor];
        let service = match (r.spec.restart, step.phase) {
            // 재시작만 하는 rollout: 그 단계 자체를 감시한다(교체가 없어 설치 작업·공유 실행 파일 복구가 없다)
            (Some(service), RolloutPhase::HostRestart | RolloutPhase::AgentRestart) => service,
            (Some(_), _) => return Ok(None),
            (None, RolloutPhase::HostUpdate) => DeployService::Host,
            (None, RolloutPhase::AgentUpdate) => DeployService::Agent,
            (None, RolloutPhase::AgentRestart) if step.machine == r.spec.machines[0] => DeployService::Agent,
            _ => return Ok(None),
        };
        let machine = step.machine.clone();
        let phase = if service == DeployService::Host { RolloutPhase::HostRestart } else { RolloutPhase::AgentRestart };
        let restart = r
            .steps
            .iter()
            .position(|s| s.machine == machine && s.phase == phase)
            .ok_or_else(|| internal("감시할 재시작 단계가 없다"))?;
        if r.steps[restart].guard.is_none() {
            let before = if r.spec.restart.is_some() { phase } else { RolloutPhase::Preflight };
            let mut baseline = r
                .steps
                .iter()
                .find(|s| s.machine == machine && s.phase == before)
                .and_then(|s| s.baseline.clone())
                .ok_or_else(|| internal("배포 전 실행 신원이 없다"))?;
            // 공유 실행 파일을 바꾼 host 머신만 복구 때 host·agent를 함께 다시 띄운다. 재시작만 하면 바꾼 파일이 없다
            let host_machine = r.spec.restart.is_none() && machine == r.spec.machines[0];
            let old_host_instance = if service == DeployService::Host {
                self.rollout_instance.clone()
            } else {
                r.steps
                    .iter()
                    .find(|s| s.phase == RolloutPhase::HostRestart)
                    .and_then(|s| s.guard.as_ref())
                    .map(|g| g.old_host_instance.clone())
                    .unwrap_or_default()
            };
            if service == DeployService::Host {
                if baseline.sha256 != self.running_sha256 || baseline.software != aaa_common::util::software() {
                    return Err(err(ErrorCode::BadRequest, "host와 로컬 agent의 배포 전 실행 판/SHA가 다르다"));
                }
                baseline.instance = self.rollout_instance.clone();
                baseline.started_ms = self.rollout_started_ms;
            }
            r.steps[restart].guard = Some(DeployGuardSpec {
                id: r.steps[restart].job_id.clone(),
                rollout: r.id.clone(),
                project: r.project.clone(),
                service,
                host_machine,
                baseline,
                expected_sha256: r.spec.sha256.clone(),
                expected_software: format!(
                    "aaa {}",
                    r.version.as_deref().ok_or_else(|| internal("검증한 후보 판이 없다"))?
                ),
                old_host_instance,
                install_job: r
                    .steps
                    .iter()
                    .find(|s| {
                        s.machine == machine
                            && s.phase
                                == if service == DeployService::Host {
                                    RolloutPhase::HostUpdate
                                } else {
                                    RolloutPhase::AgentUpdate
                                }
                    })
                    .map(|s| s.job_id.clone()),
                rollback_job: aaa_common::util::new_id(),
                restart_job: aaa_common::util::new_id(),
                host_restart_job: aaa_common::util::new_id(),
            });
        }
        Ok(Some(restart))
    }
    fn begin_recovery(&self, r: &mut RolloutView, view: DeployGuardView, reason: String) -> Result<(), ErrorBody> {
        let state_changed = r.state != RolloutState::Recovering;
        if r.recovery_started_ms == 0 {
            r.recovery_started_ms = aaa_common::util::now_ms();
        }
        r.state = RolloutState::Recovering;
        if r.error.is_none() {
            r.error = Some(reason);
        }
        if state_changed || r.recovery.as_deref() != Some(&view) {
            r.recovery = Some(Box::new(view));
            self.save_rollout(r)?;
        }
        Ok(())
    }
    pub(super) async fn stop_or_recover(
        &self,
        r: &mut RolloutView,
        cursor: usize,
        reason: String,
    ) -> Result<(), ErrorBody> {
        let Some(spec) = guard_spec(r, cursor).filter(|_| r.recovery_enabled) else {
            r.state = RolloutState::Stopped;
            r.error = Some(reason);
            return self.save_rollout(r);
        };
        let machine = r.steps[cursor].machine.clone();
        if matches!(r.steps[cursor].phase, RolloutPhase::HostUpdate | RolloutPhase::AgentUpdate)
            && !r.steps.iter().any(|s| s.job_id == spec.id && s.guard_ready)
        {
            r.state = RolloutState::Stopped;
            r.error = Some(reason);
            return self.save_rollout(r);
        }
        let view = match self.guard_control(&machine, &spec, Control::Status).await {
            Ok(view) => view,
            Err(e) if matches!(e.code, ErrorCode::AgentDisconnected | ErrorCode::Timeout) => DeployGuardView {
                spec: spec.clone(),
                phase: Phase::AwaitingNew,
                worker_pid: 0,
                deadline_ms: r.steps[cursor].started_ms + STEP_LIMIT_MS,
                install_started: false,
                activated: false,
                restore_started: false,
                restart_index: 0,
                restart_started: false,
                host_restart_ms: 0,
                agent_restart_ms: 0,
                observed: vec![],
                installed_sha256: String::new(),
                error: None,
                pending_web: None,
            },
            Err(e) => {
                r.state = RolloutState::RecoveryFailed;
                r.error = Some(format!("{reason}; 감시 상태 조회 실패: {}", e.message));
                return self.save_rollout(r);
            }
        };
        if view.phase == Phase::Armed && !view.install_started && view.installed_sha256 == spec.baseline.sha256 {
            let cancelled = match self.guard_control(&machine, &spec, Control::Cancel).await {
                Ok(cancelled) => cancelled,
                // 취소가 적용됐는지 모르면 같은 감시 영수증을 계속 기다린다.
                Err(e) if matches!(e.code, ErrorCode::AgentDisconnected | ErrorCode::Timeout) => {
                    return self.begin_recovery(r, view, reason);
                }
                Err(e) => {
                    r.state = RolloutState::RecoveryFailed;
                    r.error = Some(format!("{reason}; 감시 취소 요청 실패: {}", e.message));
                    return self.save_rollout(r);
                }
            };
            if cancelled.phase == Phase::Cancelled {
                r.state = RolloutState::Stopped;
                r.error = Some(reason);
                return self.save_rollout(r);
            }
            return self.begin_recovery(r, cancelled, reason);
        }
        if matches!(view.phase, Phase::Armed | Phase::AwaitingNew | Phase::NewReady) && view.worker_pid != 0 {
            match self.guard_control(&machine, &spec, Control::Recover { reason: reason.clone() }).await {
                Ok(view) => return self.begin_recovery(r, view, reason),
                Err(e) if matches!(e.code, ErrorCode::AgentDisconnected | ErrorCode::Timeout) => {}
                Err(e) => {
                    r.state = RolloutState::RecoveryFailed;
                    r.error = Some(format!("{reason}; 감시 복구 요청 실패: {}", e.message));
                    return self.save_rollout(r);
                }
            }
        }
        self.begin_recovery(r, view, reason)
    }
    fn use_recovery_receipt(&self, r: &mut RolloutView, view: DeployGuardView) -> Result<bool, ErrorBody> {
        let phase = view.phase;
        let changed = r.recovery.as_deref() != Some(&view);
        r.recovery = Some(Box::new(view));
        match phase {
            Phase::Restored => r.state = RolloutState::StoppedRolledBack,
            Phase::Failed => r.state = RolloutState::RecoveryFailed,
            Phase::Cancelled => r.state = RolloutState::Stopped,
            _ => {}
        }
        if changed || !active(r.state) {
            self.save_rollout(r)?;
        }
        Ok(!active(r.state))
    }
    pub(super) async fn poll_rollout_recovery(&self, r: &mut RolloutView) -> Result<(), ErrorBody> {
        let spec = r.recovery.as_ref().ok_or_else(|| internal("복구 감시 기록이 없다"))?.spec.clone();
        let machine = r
            .steps
            .iter()
            .find(|s| s.job_id == spec.id)
            .ok_or_else(|| internal("복구 대상 단계가 없다"))?
            .machine
            .clone();
        match self.guard_control(&machine, &spec, Control::Status).await {
            Ok(view) => {
                if matches!(view.phase, Phase::Armed | Phase::AwaitingNew | Phase::NewReady) {
                    match self
                        .guard_control(
                            &machine,
                            &spec,
                            Control::Recover {
                                reason: r.error.clone().unwrap_or_else(|| "배포 실패 복구".into())
                            },
                        )
                        .await
                    {
                        Ok(result) => {
                            self.use_recovery_receipt(r, result)?;
                        }
                        // 진행기를 끝내지 않고 감시 사건/기존 복구 한도를 기다린다.
                        Err(e) if matches!(e.code, ErrorCode::AgentDisconnected | ErrorCode::Timeout) => {}
                        Err(e) => {
                            r.state = RolloutState::RecoveryFailed;
                            r.error = Some(format!("감시 복구 요청 실패: {}", e.message));
                            self.save_rollout(r)?;
                        }
                    }
                } else {
                    self.use_recovery_receipt(r, view)?;
                }
            }
            Err(e) if matches!(e.code, ErrorCode::AgentDisconnected | ErrorCode::Timeout) => {}
            Err(e) => {
                r.state = RolloutState::RecoveryFailed;
                r.error = Some(format!("복구 상태를 읽지 못했다: {}", e.message));
                self.save_rollout(r)?;
            }
        }
        Ok(())
    }
    /// 완료를 DB에 먼저 남긴 뒤 감시를 해제한다. 답 유실/host 재시작이면 같은 신원을 다시 대조한다.
    pub(super) async fn settle_rollout_guards(&self, r: &mut RolloutView) -> Result<bool, ErrorBody> {
        for index in 0..r.steps.len() {
            if !r.steps[index].completed {
                continue;
            }
            let Some(spec) = r.steps[index].guard.clone() else {
                continue;
            };
            let step = r.steps[index].clone();
            let view = self.guard_control(&step.machine, &spec, Control::Status).await?;
            match view.phase {
                Phase::VerifiedNew => {}
                Phase::NewReady => {
                    let response = self
                        .live_conn(&step.machine)?
                        .request(AgentRequest::DeployStatus { id: step.job_id.clone() })
                        .await?;
                    match assess(r, &step, response, &self.rollout_instance, &self.running_sha256) {
                        Ok(Some(_)) => {
                            let committed = self.guard_control(&step.machine, &spec, Control::Commit).await?;
                            if committed.phase != Phase::VerifiedNew {
                                r.steps[index].completed = false;
                                r.cursor = index as u32;
                                self.begin_recovery(r, committed, "감시 해제 전에 복구로 전환됐다".into())?;
                                return Ok(false);
                            }
                        }
                        Ok(None) => return Ok(false),
                        Err(e) => {
                            r.steps[index].completed = false;
                            r.cursor = index as u32;
                            self.stop_or_recover(r, index, e.message).await?;
                            return Ok(false);
                        }
                    }
                }
                Phase::Restoring | Phase::Restarting | Phase::AwaitingOld | Phase::Restored | Phase::Failed => {
                    r.steps[index].completed = false;
                    r.cursor = index as u32;
                    self.begin_recovery(r, view, "완료 기록 뒤 실행 신원을 확인하는 중 복구로 전환됐다".into())?;
                    return Ok(false);
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lab() -> (Arc<Host>, aaa_config::HostConfig, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("aaa-rollout-recovery-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        let h = Host::open(cfg.clone(), root.join("host")).unwrap();
        (h, cfg, root)
    }
    fn recovery_record() -> (RolloutView, DeployGuardView) {
        let mut r = crate::rollout::tests::record();
        r.cursor = 6;
        for step in &mut r.steps[..6] {
            step.completed = true;
        }
        r.recovery_enabled = true;
        let spec = DeployGuardSpec {
            id: r.steps[6].job_id.clone(),
            rollout: r.id.clone(),
            project: r.project.clone(),
            service: DeployService::Agent,
            host_machine: false,
            baseline: RuntimeIdentity {
                sha256: "b".repeat(64),
                software: "aaa 0.1.2".into(),
                instance: "old".into(),
                started_ms: 1,
                web: None,
            },
            expected_sha256: r.spec.sha256.clone(),
            expected_software: "aaa 0.1.2".into(),
            old_host_instance: "host".into(),
            install_job: Some(r.steps[5].job_id.clone()),
            rollback_job: aaa_common::util::new_id(),
            restart_job: aaa_common::util::new_id(),
            host_restart_job: aaa_common::util::new_id(),
        };
        r.steps[6].guard = Some(spec.clone());
        let view = DeployGuardView {
            spec,
            phase: Phase::Restoring,
            worker_pid: 1,
            deadline_ms: 1,
            install_started: true,
            activated: true,
            restore_started: true,
            restart_index: 0,
            restart_started: false,
            host_restart_ms: 0,
            agent_restart_ms: 0,
            observed: vec![],
            installed_sha256: r.spec.sha256.clone(),
            error: Some("failed".into()),
            pending_web: None,
        };
        (r, view)
    }
    #[tokio::test]
    async fn recovery_receipts_keep_the_failed_cursor_and_durable_busy_state() {
        let (h, cfg, root) = lab();
        let (mut r, mut view) = recovery_record();
        h.db.start_rollout(&r.id, &serde_json::to_string(&r).unwrap()).unwrap();
        h.begin_recovery(&mut r, view.clone(), "failed".into()).unwrap();
        assert!(h.rollout_busy().unwrap());
        assert!(!h.use_recovery_receipt(&mut r, view.clone()).unwrap());
        assert_eq!(h.load_rollout(&r.id).unwrap().state, RolloutState::Recovering);
        view.phase = Phase::Restored;
        assert!(h.use_recovery_receipt(&mut r, view.clone()).unwrap());
        assert_eq!(r.state, RolloutState::StoppedRolledBack);
        assert_eq!(r.cursor, 6, "실패한 머신 뒤로 진행하면 안 된다");
        assert!(!r.steps[6].completed && r.steps[..6].iter().all(|s| s.completed));
        assert!(!h.rollout_busy().unwrap());
        drop(h);
        let reopened = Host::open(cfg, root.join("host")).unwrap();
        let saved = reopened.load_rollout(&r.id).unwrap();
        assert_eq!(saved, r);
        assert_eq!(saved.recovery.as_deref(), Some(&view));
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn recovery_driver_survives_lost_recover_without_a_status_caller() {
        lost_control(false).await;
    }
    #[tokio::test]
    async fn recovery_driver_survives_lost_cancel_without_a_status_caller() {
        lost_control(true).await;
    }
    async fn lost_control(cancel: bool) {
        use aaa_common::conn::{FrameRx, spawn_writer};
        use aaa_proto::{LinkMsg, Outcome};
        for code in [ErrorCode::Timeout, ErrorCode::AgentDisconnected, ErrorCode::BadRequest] {
            tokio::time::timeout(Duration::from_secs(30), async {
                let (h, _, root) = lab();
                let (mut r, mut view) = recovery_record();
                if cancel {
                    r.cursor = 5;
                    r.steps[5].completed = false;
                    r.steps[6].guard_ready = true;
                    view.phase = Phase::Armed;
                    view.install_started = false;
                    view.installed_sha256 = view.spec.baseline.sha256.clone();
                } else {
                    r.state = RolloutState::Recovering;
                    r.recovery_started_ms = aaa_common::util::now_ms();
                    view.phase = Phase::AwaitingNew;
                    r.recovery = Some(Box::new(view.clone()));
                }
                r.error = Some("original failure".into());
                h.db.start_rollout(&r.id, &serde_json::to_string(&r).unwrap()).unwrap();
                let (writer, reader) = tokio::io::duplex(65536);
                let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
                h.agent_connected(
                    &aaa_proto::AgentHello {
                        proto: aaa_proto::AGENT_LINK,
                        machine_id: "remote".into(),
                        machine_name: "mac-b".into(),
                        boot_id: "boot".into(),
                        os: "test".into(),
                        arch: "test".into(),
                        software: "test".into(),
                        features: vec!["rollout_recovery.v1".into()],
                        dev_mode: true,
                        selfcheck: None,
                        sessions: vec![],
                        identity: None,
                        harness: vec![],
                    },
                    false,
                    conn.clone(),
                );
                let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
                // 진행기는 하나뿐이며 조회 호출이 실패한 진행기를 몰래 재개하면 안 된다.
                let driver_host = h.clone();
                let mut driver = tokio::spawn(async move { driver_host.drive_rollout().await });
                let LinkMsg::Req { id, body: AgentRequest::DeployGuardControl { id: guard, control }, .. } =
                    rx.control::<LinkMsg>().await.unwrap()
                else {
                    panic!("expected guard status")
                };
                assert_eq!(guard, view.spec.id);
                assert_eq!(control, Control::Status);
                conn.pending
                    .lock()
                    .unwrap()
                    .remove(&id)
                    .unwrap()
                    .send(Outcome::Ok(AgentResponse::DeployGuard { guard: Box::new(view.clone()) }))
                    .unwrap();
                let LinkMsg::Req { id, body: AgentRequest::DeployGuardControl { id: guard, control }, .. } =
                    rx.control::<LinkMsg>().await.unwrap()
                else {
                    panic!("expected recovery control")
                };
                assert_eq!(guard, view.spec.id);
                assert!(if cancel { control == Control::Cancel } else { matches!(control, Control::Recover { .. }) });
                conn.pending
                    .lock()
                    .unwrap()
                    .remove(&id)
                    .unwrap()
                    .send(Outcome::Err(ErrorBody::new(code, "control response lost")))
                    .unwrap();
                // DeployGuardChanged와 같은 알림만 보내고 조회/재개 호출은 하지 않는다.
                h.rollout_wake.notify_one();
                if code != ErrorCode::BadRequest {
                    let next = tokio::select! {
                        result = &mut driver => panic!("driver exited on {code:?}, cancel={cancel}: {result:?}"),
                        request = rx.control::<LinkMsg>() => request.unwrap(),
                    };
                    let LinkMsg::Req { id, body: AgentRequest::DeployGuardControl { id: guard, control }, .. } = next
                    else {
                        panic!("expected recovery status after guard event")
                    };
                    assert_eq!(guard, view.spec.id);
                    assert_eq!(control, Control::Status);
                    view.phase = Phase::Restored;
                    view.installed_sha256 = view.spec.baseline.sha256.clone();
                    conn.pending
                        .lock()
                        .unwrap()
                        .remove(&id)
                        .unwrap()
                        .send(Outcome::Ok(AgentResponse::DeployGuard { guard: Box::new(view.clone()) }))
                        .unwrap();
                }
                driver.await.unwrap().unwrap();
                let saved = h.load_rollout(&r.id).unwrap();
                assert_eq!(
                    saved.state,
                    if code == ErrorCode::BadRequest {
                        RolloutState::RecoveryFailed
                    } else {
                        RolloutState::StoppedRolledBack
                    }
                );
                assert_eq!(saved.cursor, r.cursor, "failed machine must not advance");
                if code != ErrorCode::BadRequest {
                    assert_eq!(saved.recovery.as_deref(), Some(&view), "fixed recovery IDs must survive");
                }
                assert!(!h.rollout_busy().unwrap());
                drop(h);
                std::fs::remove_dir_all(root).unwrap();
            })
            .await
            .unwrap();
        }
    }
}
