//! 설정 「모델」(t-1509, 참고 t3 code 「Models」의 켜기 스위치·「Disable all」): provider(harness)별 모델 목록과 켜짐.
//!
//! - 줄은 머신 agent가 알린 목록(model_catalog.v1, Codex), Claude 별칭 표, 끈 모델, profile이 쓰는 model을 합친다. 끈 모델과
//!   profile model은 목록에서 사라져도 다시 켜거나 볼 수 있게 남긴다.
//! - agent 목록은 머신마다 마지막 답을 host 메모리에 둔다: 끊긴 머신의 모델도 그대로 보인다.
//! - 고치기는 host.toml만 쓴다(판 대조로 동시 편집은 conflict). 끄는 모델을 profile이 쓰면 끄기는 하고 경고한다(실행은 그대로).
use std::collections::BTreeMap;
use std::time::Duration;

use aaa_config::models::{self as cfg_models, CLAUDE_ALIASES, HARNESSES};
use aaa_proto::{
    AgentRequest, AgentResponse, CatalogModel, ErrorBody, ModelAdminView, ModelRowView, ProviderModelsView,
};

use super::{conflict, local_defines, profiles_in, save_error};
use crate::Host;
use crate::file_rev::{self, Edit};
use crate::orch::bad;

/// 머신 하나가 목록을 답하기를 기다리는 시간(파일 하나 읽기)
const CATALOG_WAIT: Duration = Duration::from_secs(5);
const FEATURE: &str = "model_catalog.v1";

/// 줄 모으기: 끈 목록·profile이 쓰는 model·알린 목록으로 harness별 줄을 만든다(처음 본 차례대로)
fn rows(
    harness: &str,
    known: &[(String, String)],
    disabled: &[String],
    profiles: &BTreeMap<String, aaa_config::Profile>,
) -> Vec<ModelRowView> {
    let mut out: Vec<ModelRowView> = Vec::new();
    let mut push = |id: &str, name: &str| {
        if !out.iter().any(|r| r.id == id) {
            out.push(ModelRowView { id: id.into(), name: name.into(), enabled: true, profiles: Vec::new() });
        }
    };
    for (id, name) in known {
        push(id, name);
    }
    for id in disabled {
        push(id, id);
    }
    // Claude 계열 ID(`claude-opus-5-5[1m]`)는 그 별칭 줄이 켜고 끈다: 따로 줄을 두지 않는다
    for p in profiles.values().filter(|p| p.harness == harness && !p.model.is_empty()) {
        if harness != "claude" || cfg_models::claude_family(&p.model).is_none() {
            push(&p.model, &p.model);
        }
    }
    for r in &mut out {
        r.enabled = cfg_models::blocked(disabled, harness, &r.id).is_none();
        r.profiles = profiles
            .iter()
            .filter(|(_, p)| {
                p.harness == harness && cfg_models::blocked(std::slice::from_ref(&r.id), harness, &p.model).is_some()
            })
            .map(|(n, _)| n.clone())
            .collect();
    }
    out
}

impl Host {
    /// 연결된 머신에 목록을 묻고 메모리 목록을 고친다. 답하지 않은 머신은 경고 한 줄이다
    async fn refresh_catalogs(&self) -> Vec<String> {
        let machines: Vec<String> = self
            .machine_views()
            .into_iter()
            .filter(|m| self.conn_of(&m.id).is_some() && self.machine_feature(&m.id, FEATURE))
            .map(|m| m.id)
            .collect();
        let asks = machines.into_iter().map(|mid| async move {
            let res = match self.live_conn(&mid) {
                Ok(conn) => conn.request_timeout(AgentRequest::ModelCatalog, CATALOG_WAIT).await,
                Err(e) => Err(e),
            };
            (mid, res)
        });
        let mut warnings = Vec::new();
        for (mid, res) in futures_util::future::join_all(asks).await {
            let name = self.machine_name(&mid);
            match res {
                Ok(AgentResponse::ModelCatalog { models, problems }) => {
                    warnings.extend(problems.into_iter().map(|p| format!("{name}: {p}")));
                    self.model_catalogs.lock().unwrap().insert(mid, models);
                }
                Ok(other) => warnings.push(format!("{name}: 모델 목록 대신 다른 답이 왔다({other:?})")),
                Err(e) => warnings.push(format!("{name}: 모델 목록을 받지 못했다({})", e.message)),
            }
        }
        warnings
    }

    pub(crate) async fn model_admin(&self) -> Result<ModelAdminView, ErrorBody> {
        let mut warnings = self.refresh_catalogs().await;
        let dir = self.settings_dir()?;
        let rev = self.host_toml_rev(dir)?;
        let config = self.web_config()?;
        let catalogs: Vec<CatalogModel> = self.model_catalogs.lock().unwrap().values().flatten().cloned().collect();
        let providers = HARNESSES
            .iter()
            .map(|&h| {
                let mut known: Vec<(String, String)> = match h {
                    "claude" => CLAUDE_ALIASES.iter().map(|(a, n)| (a.to_string(), n.to_string())).collect(),
                    _ => Vec::new(),
                };
                known.extend(catalogs.iter().filter(|m| m.harness == h).map(|m| (m.id.clone(), m.name.clone())));
                if local_defines(dir, "models", h) {
                    warnings.push(format!("host.local.toml의 [models.{h}]가 있다: 웹은 host.toml만 고친다"));
                }
                ProviderModelsView {
                    harness: h.to_string(),
                    models: rows(h, &known, cfg_models::disabled_of(&config, h), &config.profiles),
                }
            })
            .collect();
        Ok(ModelAdminView { providers, warnings, rev: rev.sha256, notes: Vec::new() })
    }

    pub(crate) async fn model_set(
        &self,
        harness: &str,
        models: &[String],
        enabled: bool,
        expected: &str,
        actor: &str,
    ) -> Result<ModelAdminView, ErrorBody> {
        cfg_models::validate_harness(harness).map_err(bad)?;
        if models.is_empty() {
            return Err(bad("켜거나 끌 모델이 없다"));
        }
        let path = self.settings_dir()?.join("host.toml");
        let lock = self.file_locks.of(&path);
        let saved = {
            let _guard = lock.lock().unwrap();
            file_rev::save(&path, |rev| {
                if rev.sha256 != expected {
                    return Err(conflict());
                }
                profiles_in(&rev.text)?;
                let (text, changed) = cfg_models::set_in(&rev.text, harness, models, enabled).map_err(bad)?;
                Ok(if changed.is_empty() { Edit::Keep(changed) } else { Edit::Write(text, changed) })
            })
            .map_err(save_error)?
        };
        let changed = saved.out;
        if !changed.is_empty() {
            self.db.audit(
                actor,
                "model_set",
                &format!("{harness} {} {}", if enabled { "on" } else { "off" }, changed.join(",")),
            );
        }
        let mut view = self.model_admin().await?;
        let word = if enabled { "켰다" } else { "껐다" };
        view.notes.push(if changed.is_empty() {
            "바뀐 것이 없다".into()
        } else {
            format!("{harness} 모델 {}개를 {word}", changed.len())
        });
        if !enabled {
            let users: Vec<String> = view
                .providers
                .iter()
                .filter(|p| p.harness == harness)
                .flat_map(|p| p.models.iter())
                .filter(|r| changed.contains(&r.id))
                .flat_map(|r| r.profiles.iter().cloned())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            if !users.is_empty() {
                view.warnings.push(format!("끈 모델을 쓰는 profile이 있다(그대로 실행한다): {}", users.join(", ")));
            }
        }
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(harness: &str, model: &str) -> aaa_config::Profile {
        aaa_config::Profile { harness: harness.into(), model: model.into(), ..Default::default() }
    }

    #[test]
    fn rows_join_catalog_disabled_and_profile_models() {
        let known = vec![("gpt-6-sol".to_string(), "GPT-6-Sol".to_string())];
        let profiles: BTreeMap<_, _> = [
            ("review".to_string(), profile("codex", "gpt-old")),
            ("fast".to_string(), profile("codex", "gpt-6-sol")),
            ("c".to_string(), profile("claude", "gpt-6-sol")),
        ]
        .into();
        let got = rows("codex", &known, &["gpt-gone".to_string(), "gpt-old".to_string()], &profiles);
        let flat: Vec<_> =
            got.iter().map(|r| (r.id.as_str(), r.name.as_str(), r.enabled, r.profiles.clone())).collect();
        assert_eq!(
            flat,
            vec![
                ("gpt-6-sol", "GPT-6-Sol", true, vec!["fast".to_string()]),
                ("gpt-gone", "gpt-gone", false, vec![]),
                ("gpt-old", "gpt-old", false, vec!["review".to_string()]),
            ]
        );
    }

    #[test]
    fn claude_alias_row_lists_family_profiles() {
        let known = vec![("opus".to_string(), "Opus".to_string())];
        let profiles: BTreeMap<_, _> = [("deep".to_string(), profile("claude", "claude-opus-5-5[1m]"))].into();
        let got = rows("claude", &known, &["opus".to_string()], &profiles);
        assert_eq!(got[0].profiles, vec!["deep".to_string()]);
        assert!(!got[0].enabled);
        assert_eq!(got.len(), 1, "계열 ID는 별칭 줄에 묶인다");
    }
}
