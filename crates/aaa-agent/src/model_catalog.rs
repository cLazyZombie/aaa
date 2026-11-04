//! 이 머신 harness가 아는 모델 목록(model_catalog.v1, t-1509): host 설정 「모델」이 켜고 끌 줄을 모은다.
//!
//! - Codex: `$CODEX_HOME|~/.codex/models_cache.json`의 `models[]` 가운데 `visibility = "list"`(`/model`에 보이는 것)만
//!   slug·보이는 이름으로 싣는다. 파일의 계정 정보(`identity` 등)는 읽지도 싣지도 않는다. Codex를 실행하지 않는다.
//! - Claude는 머신에 목록 파일이 없어 host가 별칭 표를 둔다([`aaa_config::models::CLAUDE_ALIASES`]).
//! - 파일이 없거나 형식이 다르면 빈 목록과 까닭 한 줄이다(오류가 아니다).

use std::path::Path;

use aaa_proto::{AgentResponse, CatalogModel, ErrorBody};

#[derive(serde::Deserialize)]
struct Cache {
    #[serde(default)]
    models: Vec<Entry>,
}

#[derive(serde::Deserialize)]
struct Entry {
    slug: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    visibility: String,
}

/// models_cache.json 글 → 보이는 모델
fn codex_models(text: &str) -> Result<Vec<CatalogModel>, serde_json::Error> {
    let cache: Cache = serde_json::from_str(text)?;
    Ok(cache
        .models
        .into_iter()
        .filter(|m| m.visibility == "list" && aaa_config::profile_edit::is_model_name(&m.slug))
        .map(|m| CatalogModel {
            harness: "codex".into(),
            name: if m.display_name.trim().is_empty() { m.slug.clone() } else { m.display_name },
            id: m.slug,
        })
        .collect())
}

fn read(home: &Path) -> (Vec<CatalogModel>, Vec<String>) {
    let file = home.join("models_cache.json");
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), vec!["Codex 모델 목록 파일이 없다(Codex를 한 번 띄우면 생긴다)".into()]);
        }
        Err(e) => return (Vec::new(), vec![format!("Codex 모델 목록 파일을 읽지 못했다({}): {e}", file.display())]),
    };
    match codex_models(&text) {
        Ok(m) => (m, Vec::new()),
        // 형식 오류 글은 줄·칸 위치만 싣는다(값을 담지 않는다)
        Err(e) => (
            Vec::new(),
            vec![format!("Codex 모델 목록 형식이 다르다({}): {}줄 {}칸", file.display(), e.line(), e.column())],
        ),
    }
}

pub(crate) async fn catalog() -> Result<AgentResponse, ErrorBody> {
    let home = crate::chat::codex_home();
    let (models, problems) = tokio::task::spawn_blocking(move || read(&home))
        .await
        .unwrap_or_else(|e| (Vec::new(), vec![format!("모델 목록 읽기가 끝나지 못했다: {e}")]));
    Ok(AgentResponse::ModelCatalog { models, problems })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_listed_codex_models_and_drops_hidden_and_account_fields() {
        let text = r#"{"fetched_at":"x","identity":{"account":"secret"},"models":[
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1-Sol","visibility":"list","priority":1},
            {"slug":"gpt-reserve","display_name":"GPT-Reserve","visibility":"hide"},
            {"slug":"gpt-6-luna","display_name":"","visibility":"list"}]}"#;
        let got: Vec<_> = codex_models(text).unwrap().into_iter().map(|m| (m.id, m.name)).collect();
        assert_eq!(got, vec![("gpt-6.1-sol".into(), "GPT-6.1-Sol".into()), ("gpt-6-luna".into(), "gpt-6-luna".into())]);
        assert!(codex_models("{").is_err());
    }
}
