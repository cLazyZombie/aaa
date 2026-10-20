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
        // 인증 thread는 저장한 launch·작업 identity·감시를 함께 이어야 한다.
        if let Some(rec) =
            self.thread_of_pane(id).filter(|r| r.account.is_some() && r.kind != aaa_proto::ThreadKind::Adopted)
        {
            let resumed = self.thread_restart(&rec.project, &rec.id, true, None, actor).await?;
            return resumed.pane.ok_or_else(fail);
        }
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
