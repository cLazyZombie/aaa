//! 한 판 동안만 남기는 옛 임무 파일·요청의 변환 경계. 실행 경로는 없다.
use crate::{Host, err};
use aaa_proto::{ErrorBody, ErrorCode, Role, ThreadStatus};
use std::path::Path;

pub(crate) const REMOVED: &str = "없어짐: 자동화로 바꿨다(aaa automation convert)";
pub(crate) fn removed() -> ErrorBody {
    err(ErrorCode::BadRequest, REMOVED)
}
pub(crate) fn files(root: &Path) -> Vec<String> {
    let mut files: Vec<_> = std::fs::read_dir(root.join("missions"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md") || (n.ends_with(".toml") && !n.ends_with(".schema.json")))
        .map(|n| format!("missions/{n}"))
        .collect();
    files.sort();
    files
}
impl Host {
    pub(crate) fn legacy_mission_notice(&self, project: &str) {
        let Ok(root) = self.project_dir(project) else { return };
        let notices = self.db.inbox_list(Some(project), true).unwrap_or_default();
        if !files(&root).is_empty() && !notices.iter().any(|i| i.subject == "missions" && i.event == "removed") {
            self.inbox_once(project, "config-error", "missions", "removed", REMOVED);
        }
        for rec in self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|s| serde_json::from_str::<crate::orch::ThreadRec>(s).ok())
        {
            if rec.role == Role::Evaluator
                && rec.status != ThreadStatus::Resolved
                && !notices.iter().any(|i| i.subject == rec.id && i.event == "removed role")
            {
                self.inbox_once(
                    project,
                    "config-error",
                    &rec.id,
                    "removed role",
                    "옛 evaluator 행을 일반 작업 목록에 보인다. 자동화 규칙으로 옮긴다",
                );
            }
        }
    }
}

pub(crate) struct Definition {
    pub value: toml::Value,
    pub body: String,
    pub originals: Vec<(String, String)>,
}
pub(crate) fn definition(root: &Path, name: &str) -> Result<Definition, String> {
    if !aaa_proto::valid_project(name) {
        return Err("임무 이름 형식이 아니다".into());
    }
    let path = format!("missions/{name}.toml");
    let (value, body, originals) = if root.join(&path).symlink_metadata().is_ok() {
        let text = crate::automation::project_text(root, &path)?;
        let value: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
        let brief = value.get("brief_file").and_then(|v| v.as_str()).ok_or("brief_file이 없다")?;
        let body = crate::automation::project_text(root, brief)?;
        let originals = vec![(path, text), (brief.into(), body.clone())];
        (value, body, originals)
    } else {
        let path = format!("missions/{name}.md");
        let text = crate::automation::project_text(root, &path)?;
        let rest = text.strip_prefix("+++\n").ok_or("옛 임무 front matter가 없다")?;
        let (front, body) = rest.split_once("\n+++\n").ok_or("닫는 +++가 없다")?;
        let value = toml::from_str(front).map_err(|e| e.to_string())?;
        (value, body.to_owned(), vec![(path, text)])
    };
    Ok(Definition { value, body, originals })
}
impl Definition {
    pub(crate) fn start(&self, name: &str, machine: &str) -> Result<crate::automation::Start, String> {
        use crate::automation::*;
        let string = |key: &str| self.value.get(key).and_then(|v| v.as_str()).map(str::to_string);
        let profile = string("profile").filter(|v| !v.is_empty()).ok_or("옛 임무 profile이 없다")?;
        if profile == "codex" {
            return Err("Codex reuse=open은 지원하지 않는다. 다른 profile로 옮긴다".into());
        }
        if self.body.trim().is_empty() {
            return Err("옛 임무 본문이 비었다".into());
        }
        Ok(Start {
            reuse: Reuse::Open,
            memory: true,
            reports: self.value.get("reports").and_then(|v| v.as_bool()).unwrap_or(false),
            close_idle: Some("30m".into()),
            title: string("title").filter(|v| !v.is_empty()).unwrap_or_else(|| name.into()),
            repo: string("repo"),
            profiles: vec![profile],
            machines: vec![string("machine").unwrap_or_else(|| machine.into())],
            profile_pick: Default::default(),
            machine_pick: Default::default(),
            wait: vec![],
            wait_max: None,
            text: None,
            text_file: None,
            brief_file: Some(format!("automations/{name}.brief.md")),
        })
    }
    pub(crate) fn merge_limits(&self, into: &mut crate::automation::Limits) {
        let own = self.limits();
        let longer = |a: Option<String>, b: Option<String>| match (a, b) {
            (Some(a), Some(b)) => Some(
                if crate::automation::duration_ms(&a).unwrap_or(0) >= crate::automation::duration_ms(&b).unwrap_or(0) {
                    a
                } else {
                    b
                },
            ),
            (a, b) => a.or(b),
        };
        into.batch = longer(into.batch.take(), own.batch);
        into.min_gap = longer(into.min_gap.take(), own.min_gap);
        into.max_per_day = match (into.max_per_day, own.max_per_day) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        into.usage_session_below = match (into.usage_session_below, own.usage_session_below) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        into.usage_week_below = match (into.usage_week_below, own.usage_week_below) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }
    pub(crate) fn limits(&self) -> crate::automation::Limits {
        let get = |key: &str| self.value.get("limits").and_then(|v| v.get(key)).or_else(|| self.value.get(key));
        crate::automation::Limits {
            batch: get("batch")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or_else(|| get("batch_s").and_then(|v| v.as_integer()).map(|s| format!("{s}s")))
                .or_else(|| Some("5m".into())),
            min_gap: get("min_gap").and_then(|v| v.as_str()).map(str::to_string),
            max_per_day: get("max_wakes")
                .and_then(|v| v.as_integer())
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0),
            usage_session_below: get("usage_session_below")
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64))),
            usage_week_below: get("usage_week_below")
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64))),
            ..Default::default()
        }
    }
}

impl Host {
    /// 파일 변환 뒤 중단됐어도 다음 tick에서 큐 이전을 마친다. 변환 결과 바이트가 다르면 실행하지 않는다.
    pub(crate) fn legacy_mission_recover(&self, p: &str) -> Result<(), ErrorBody> {
        let key = format!("mission_conversion:{p}");
        let Some(text) = self.db.meta(&key).map_err(|e| err(ErrorCode::Internal, e.to_string()))? else {
            return Ok(());
        };
        let journal: Vec<serde_json::Value> =
            serde_json::from_str(&text).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        let root = self.project_dir(p)?;
        for item in journal {
            let name = item["rule"].as_str().ok_or_else(removed)?;
            let raw = crate::automation::project_text(&root, &format!("automations/{name}.toml"))
                .map_err(|e| err(ErrorCode::BadRequest, e))?;
            if raw != item["text"].as_str().unwrap_or("") {
                return Err(err(
                    ErrorCode::BadRequest,
                    "임무 변환 journal과 규칙 파일이 다르다. 원본·대기 입력을 보존했다",
                ));
            }
            let r = crate::automation::parse(&root, name, &raw).map_err(|e| err(ErrorCode::BadRequest, e))?;
            self.automation_import_legacy(p, name, &item["data"], &r.action)?;
            if let Some(target) = item["target"].as_str() {
                let key = format!("mission_target:{p}:{target}");
                self.db
                    .update_meta(&key, |old| Ok(old.unwrap_or_else(|| name.into())))
                    .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
            }
        }
        self.db.set_meta(&key, "[]").map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        Ok(())
    }
}
