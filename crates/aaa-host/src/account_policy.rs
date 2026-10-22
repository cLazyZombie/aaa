//! 기본 인증은 새 작업의 시작 시점에만 선택한다. 실행 참조에는 비밀 없이 선택을 고정한다.
use std::sync::Arc;

use aaa_proto::{AccountDefault, ErrorBody, ErrorCode, SpawnSpec};
use serde::{Deserialize, Serialize};

use crate::Host;

fn fail() -> ErrorBody {
    ErrorBody::new(ErrorCode::BadRequest, "기본 계정·실행 인증 기록을 확인한다. 로컬 인증으로 바꾸지 않았다")
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PaneAccount {
    pub run: String,
    pub account: String,
    pub harness: String,
    pub machine: String,
    pub pane: Option<String>,
    pub profile: Option<String>,
}

impl Host {
    pub(crate) fn default_account(&self, harness: &str) -> Result<Option<String>, ErrorBody> {
        let name = self.db.meta(&format!("default-account:{harness}")).map_err(|_| fail())?;
        if name.as_ref().is_some_and(|n| !crate::accounts::valid_name(n)) {
            return Err(fail());
        }
        Ok(name)
    }

    pub(crate) fn account_defaults(&self) -> Result<Vec<AccountDefault>, ErrorBody> {
        let mut out = vec![];
        for harness in ["claude", "codex"] {
            if let Some(account) = self.default_account(harness)? {
                out.push(AccountDefault { harness: harness.into(), account });
            }
        }
        Ok(out)
    }

    pub(crate) fn set_default_account(&self, harness: &str, name: Option<&str>) -> Result<(), ErrorBody> {
        if !matches!(harness, "claude" | "codex") {
            return Err(fail());
        }
        let key = format!("default-account:{harness}");
        match name {
            Some(name) => {
                let c = self.accounts.credential(name)?;
                if c.harness != harness {
                    return Err(fail());
                }
                self.db.set_meta(&key, name).map_err(|_| fail())
            }
            None => self.db.del_meta(&key).map_err(|_| fail()),
        }
    }

    pub(crate) fn account_run(&self, run: &str) -> Result<Option<PaneAccount>, ErrorBody> {
        self.db
            .meta(&format!("account-run:{run}"))
            .map_err(|_| fail())?
            .map(|s| serde_json::from_str(&s).map_err(|_| fail()))
            .transpose()
    }

    /// 보류는 닫힌 pane 참조를 thread에서 지우지만 실행 인증 기록은 남긴다. 이미 보류한 thread를
    /// 이어 할 때만 이 참조를 기대값으로 회수한다. 살아 있는/준비 중인 실행을 덮어쓰지 않는다.
    pub(crate) fn parked_account_pane(&self, run: &str, account: &str, harness: &str) -> Result<String, ErrorBody> {
        let binding = self.account_run(run)?.ok_or_else(fail)?;
        if binding.run != run || binding.account != account || binding.harness != harness {
            return Err(fail());
        }
        let pane = binding.pane.ok_or_else(fail)?;
        if self.pane_view_of(&pane, aaa_proto::DeviceRole::Local).is_some() {
            return Err(fail());
        }
        Ok(pane)
    }

    pub(crate) fn pane_account(&self, pane: &str) -> Result<Option<PaneAccount>, ErrorBody> {
        let Some(run) = self.db.meta(&format!("account-pane:{pane}")).map_err(|_| fail())? else {
            return Ok(None);
        };
        let record = self.account_run(&run)?.ok_or_else(fail)?;
        if record.pane.as_deref() != Some(pane) {
            return Err(fail());
        }
        Ok(Some(record))
    }

    pub(crate) fn save_account_run(&self, record: &PaneAccount) -> Result<(), ErrorBody> {
        let text = serde_json::to_string(record).map_err(|_| fail())?;
        self.db
            .update_meta_checked(&format!("account-run:{}", record.run), &[], |_| {
                let extra = record
                    .pane
                    .as_ref()
                    .map(|p| vec![(format!("account-pane:{p}"), record.run.clone())])
                    .unwrap_or_default();
                Ok((text, extra))
            })
            .map_err(|_| fail())?;
        Ok(())
    }

    /// 이전 pane 검사와 시작 중 표지를 같은 transaction에서 확정한다.
    pub(crate) fn claim_account_run(
        &self,
        record: &mut PaneAccount,
        expected_pane: Option<&str>,
    ) -> Result<Option<PaneAccount>, ErrorBody> {
        let mut previous = None;
        self.db
            .update_meta_checked(&format!("account-run:{}", record.run), &[], |old| {
                let old = old.map(|s| serde_json::from_str::<PaneAccount>(&s)).transpose()?;
                if old.as_ref().is_some_and(|r| r.pane.is_none() || r.pane.as_deref() != expected_pane) {
                    anyhow::bail!("실행 인증 기록이 이미 바뀌었다");
                }
                if record.profile.is_none() {
                    record.profile = old.as_ref().and_then(|r| r.profile.clone());
                }
                let text = serde_json::to_string(record)?;
                previous = old;
                Ok((text, vec![]))
            })
            .map_err(|_| fail())?;
        Ok(previous)
    }

    /// coordinator의 pane 재개·복원도 처음 선택한 인증과 같은 기록 홈을 쓴다.
    pub(crate) async fn respawn_with_account(
        self: &Arc<Self>,
        id: &str,
        mid: &str,
        mut spec: SpawnSpec,
        actor: &str,
    ) -> Result<String, ErrorBody> {
        let Some(binding) = self.pane_account(id)? else {
            return self.spawn_on(mid, spec, actor).await;
        };
        if binding.machine != mid {
            return Err(fail());
        }
        if let Some(profile) = &binding.profile {
            let (project, _) = aaa_proto::account_run_parts(&binding.run).ok_or_else(fail)?;
            let resolved = self.resolved_coordinator_profile(project, mid, profile)?;
            if resolved.harness != binding.harness {
                return Err(fail());
            }
            // argv는 저장한 대화/재개 계약, env는 host 정본과 머신 덮어쓰기로 다시 푼다.
            spec.env.extend(resolved.env.iter().map(|(k, v)| (k.clone(), v.clone())));
            spec.resolved_profile = Some(resolved);
        }
        let mut credential = self.accounts.credential(&binding.account)?;
        if credential.harness != binding.harness {
            return Err(fail());
        }
        credential.run = binding.run;
        self.spawn_account_on(mid, spec, credential, actor, Some(id)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentConn, MachineRt};
    use aaa_common::{
        conn::{FrameRx, spawn_writer},
        util,
    };
    use aaa_proto::{
        AccountCredential, AgentRequest, AgentResponse, LinkMsg, MAX_FRAME, Outcome, SecretText, SessionInfo,
        SessionState,
    };

    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[tokio::test]
    async fn preparing_account_resume_rejects_duplicates_before_agent_spawn() {
        let root = Temp(std::env::temp_dir().join(format!("aaa-account-resume-{}", util::new_id())));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.0.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.0.join("host")).unwrap();
        let (writer, reader) = tokio::io::duplex(65536);
        let conn = AgentConn::new(spawn_writer(writer, MAX_FRAME), 1);
        h.inner.lock().unwrap().machines.insert(
            "m".into(),
            MachineRt {
                name: "mac-a".into(),
                local: true,
                os: None,
                boot_id: None,
                dev_mode: true,
                conn: Some(conn.clone()),
                last_seen_ms: None,
                health: None,
                health_sent: None,
                features: vec!["account_auth.v2".into()],
                identity: None,
                agent_pid: None,
                colors: None,
                build: Default::default(),
            },
        );
        let binding = PaneAccount {
            run: format!("demo/c-{}", util::new_id()),
            account: "work".into(),
            harness: "codex".into(),
            machine: "m".into(),
            pane: Some("old".into()),
            profile: None,
        };
        h.save_account_run(&binding).unwrap();
        // thread가 닫힌 pane을 지운 보류 기록만 회수한다. 잘못된 신원·빠진 기록은 local로 우회하지 않는다.
        assert_eq!(h.parked_account_pane(&binding.run, "work", "codex").unwrap(), "old");
        assert!(h.parked_account_pane(&binding.run, "other", "codex").is_err());
        assert!(h.parked_account_pane(&binding.run, "work", "claude").is_err());
        assert!(h.parked_account_pane("demo/t-9999", "work", "codex").is_err());
        let credential = AccountCredential {
            run: binding.run.clone(),
            account: binding.account.clone(),
            harness: binding.harness.clone(),
            version: 1,
            secret: SecretText("fixture-only".into()),
            trust_cwd: None,
            auth_generation: None,
        };
        let spec = SpawnSpec {
            name: "coordinator".into(),
            argv: vec!["codex".into()],
            cwd: None,
            env: vec![],
            cols: 80,
            rows: 24,
            role: None,
            harness: Some("codex".into()),
            profile: None,
            resolved_profile: None,
        };
        let first = h.spawn_account_on("m", spec.clone(), credential.clone(), "test", Some("old"));
        tokio::pin!(first);
        let mut rx = FrameRx::new(reader, MAX_FRAME);
        // 실제 SpawnAccount가 전송된 사건으로 준비 구간을 잡는다. CLI version 제한시간을 쓰지 않는다.
        let request = tokio::select! {
            result = &mut first => panic!("agent 응답 전에 끝난 재개: {result:?}"),
            request = rx.control::<LinkMsg>() => request.unwrap(),
        };
        let LinkMsg::Req { id, body: AgentRequest::SpawnAccount { .. }, .. } = request else {
            panic!("계정 pane 기동 요청이어야 한다");
        };
        assert!(h.account_run(&binding.run).unwrap().unwrap().pane.is_none());
        assert!(h.parked_account_pane(&binding.run, "work", "codex").is_err(), "준비 중 실행은 회수하지 않는다");
        // 잠금이 없으면 두 번째 요청을 읽는 분기로 바로 실패한다. 거부가 timeout에 기대지 않는다.
        let duplicate = tokio::select! {
            result = h.spawn_account_on("m", spec.clone(), credential.clone(), "test", Some("old")) => result,
            request = rx.control::<LinkMsg>() => panic!("같은 인증 홈의 중복 기동: {request:?}"),
        };
        assert_eq!(duplicate.unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!(conn.pending.lock().unwrap().len(), 1);
        let info = SessionInfo {
            id: "new".into(),
            name: spec.name.clone(),
            argv: spec.argv.clone(),
            cwd: None,
            pid: None,
            started_at_ms: 0,
            boot_id: "boot".into(),
            cols: 80,
            rows: 24,
            state: SessionState::Running,
            role: None,
            label: None,
            harness: spec.harness.clone(),
        };
        conn.pending.lock().unwrap().remove(&id).unwrap().send(Outcome::Ok(AgentResponse::Session(info))).unwrap();
        assert_eq!(first.await.unwrap(), "new");
        assert_eq!(h.account_run(&binding.run).unwrap().unwrap().pane.as_deref(), Some("new"));
        assert!(h.parked_account_pane(&binding.run, "work", "codex").is_err(), "남은 pane은 회수하지 않는다");
        assert!(
            h.spawn_account_on("m", spec, credential, "test", Some("old")).await.is_err(),
            "완료 뒤 옛 pane의 재개도 거부한다"
        );
        assert!(conn.pending.lock().unwrap().is_empty());
    }
}
