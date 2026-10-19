//! 변환 계획을 먼저 검증하고, 실패하면 이번 변환의 파일만 되돌린다.
use super::*;
use std::{collections::BTreeMap, io::Write, path::Path};
#[derive(Default)]
pub(super) struct Plan {
    writes: BTreeMap<String, String>,
    originals: BTreeMap<String, String>,
}
fn add(plan: &mut Plan, root: &Path, file: String, text: String) -> Result<(), String> {
    if plan.writes.contains_key(&file) {
        return Err(format!("{file}:1:1 이름: 변환 결과 이름이 겹친다"));
    }
    if root.join(&file).symlink_metadata().is_ok() && !plan.originals.contains_key(&file) {
        return Err(format!("{file}:1:1 이름: 이미 있는 파일은 덮어쓰지 않는다"));
    }
    plan.writes.insert(file, text);
    Ok(())
}
fn rule(plan: &mut Plan, root: &Path, name: &str, mut spec: File) -> Result<(), String> {
    let (text, external) = spec.action.texts();
    if external.is_none() && text.as_ref().is_some_and(|s| s.len() > 4096) {
        let body = text.clone().unwrap();
        let path = format!("automations/{name}.md");
        add(plan, root, path.clone(), format!("{body}\n"))?;
        match &mut spec.action {
            Action::Inbox(a) => {
                a.text = None;
                a.text_file = Some(path)
            }
            Action::Message(a) => {
                a.text = None;
                a.text_file = Some(path)
            }
            Action::Wake(a) => {
                a.text = None;
                a.text_file = Some(path)
            }
        }
    }
    let serialized =
        format!("#:schema ./automation.schema.json\n{}", toml::to_string_pretty(&spec).map_err(|e| e.to_string())?);
    let parsed = parse_with(name, &serialized, |path| {
        plan.writes.get(path).cloned().map(Ok).unwrap_or_else(|| project_text(root, path))
    })?;
    // Re-encode normalized values, rather than comparing file bytes or cosmetic title.
    let normalized = File::from_routine(&parsed);
    let expected_text = match spec.action.texts() {
        (Some(text), _) => text.trim().to_string(),
        (_, Some(path)) => {
            plan.writes.get(path).cloned().map(Ok).unwrap_or_else(|| project_text(root, path))?.trim().to_string()
        }
        _ => String::new(),
    };
    if normalized.action.texts().0.as_deref().unwrap_or("") != expected_text {
        return Err("변환한 글이 정규화 값과 다르다".into());
    }
    add(plan, root, format!("automations/{name}.toml"), serialized)
}
pub(super) fn prepare(root: &Path) -> Result<Plan, String> {
    let (routines, broken) = crate::routine::load_all(root);
    if !broken.is_empty() {
        return Err(broken.iter().map(|(_, _, e)| e.as_str()).collect::<Vec<_>>().join("\n"));
    }
    let mut plan = Plan::default();
    for r in routines.iter().filter(|r| r.file.starts_with("routines/")) {
        let original = project_text(root, &r.file)?;
        plan.originals.insert(r.file.clone(), original);
        rule(&mut plan, root, &r.name, File::from_routine(r))?;
        let converted = parse_with(&r.name, &plan.writes[&format!("automations/{}.toml", r.name)], |path| {
            plan.writes.get(path).cloned().map(Ok).unwrap_or_else(|| project_text(root, path))
        })?;
        if r.trigger != converted.trigger
            || r.command_sha256() != converted.command_sha256()
            || r.action != converted.action
            || r.limits != converted.limits
        {
            return Err(format!("{}:1:1 변환: 조건·동작·승인 값이 다르다", r.file));
        }
    }
    let files: Vec<_> = std::fs::read_dir(root.join("missions"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|f| f.ends_with(".md"))
        .collect();
    for file in files {
        let original = project_text(root, &format!("missions/{file}"))?;
        if !original.starts_with("+++\n") {
            continue;
        }
        let name = file.trim_end_matches(".md");
        let converted = crate::mission::convert_legacy(name, &original).map_err(|e| e.message)?;
        plan.originals.insert(format!("missions/{file}"), original);
        add(
            &mut plan,
            root,
            format!("missions/{name}.toml"),
            format!("#:schema ./mission.schema.json\n{}", converted.toml),
        )?;
        add(&mut plan, root, format!("missions/{name}.md"), converted.brief)?;
        for (name, spec) in converted.rules {
            rule(&mut plan, root, &name, spec)?;
        }
    }
    if !plan.originals.is_empty() {
        for (file, value) in [
            ("automations/automation.schema.json", super::schema()),
            ("missions/mission.schema.json", crate::mission::files::schema()),
        ] {
            let data = format!("{}\n", serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?);
            if root.join(file).exists() {
                if project_text(root, file)? != data {
                    return Err(format!("{file}:1:1 schema: 이미 있는 다른 스키마는 덮어쓰지 않는다"));
                }
            } else {
                add(&mut plan, root, file.into(), data)?;
            }
        }
    }
    Ok(plan)
}
fn parent(root: &Path, file: &str) -> Result<(), String> {
    let mut at = root.to_path_buf();
    let path = Path::new(file);
    for c in path.parent().unwrap().components() {
        at.push(c.as_os_str());
        if let Ok(m) = at.symlink_metadata()
            && (!m.is_dir() || m.file_type().is_symlink())
        {
            return Err(format!("{file}:1:1 경로: 일반 폴더만 받는다"));
        }
    }
    Ok(())
}
fn archive(file: &str) -> String {
    let p = Path::new(file);
    format!("{}/.converted/{}", p.parent().unwrap().to_string_lossy(), p.file_name().unwrap().to_string_lossy())
}

impl Plan {
    pub(super) fn files(&self) -> Vec<String> {
        self.writes.keys().cloned().collect()
    }
}
pub(super) fn apply(root: &Path, plan: Plan) -> Result<Vec<String>, String> {
    if plan.originals.is_empty() {
        return Ok(vec![]);
    }
    for file in plan.writes.keys().chain(plan.originals.keys()) {
        parent(root, file)?;
    }
    for (file, text) in &plan.originals {
        if project_text(root, file)? != *text {
            return Err(format!("{file}:1:1 변환: 계획 뒤 원본이 바뀌었다"));
        }
        let target = archive(file);
        parent(root, &target)?;
        if root.join(&target).symlink_metadata().is_ok() {
            return Err(format!("{target}:1:1 보존: 이미 있는 사본은 덮어쓰지 않는다"));
        }
    }
    let mut archived = Vec::new();
    let mut created = Vec::new();
    let mut staged = Vec::new();
    let result = (|| -> Result<(), String> {
        // 완전히 쓴 일반 파일만 최종 이름으로 공개한다. watcher가 반쪽 TOML을 읽지 않는다.
        for (file, text) in &plan.writes {
            let dest = root.join(file);
            std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
            let temp = dest.parent().unwrap().join(format!(".aaa-convert-{}.tmp", aaa_common::util::new_id()));
            let mut f =
                std::fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|e| e.to_string())?;
            staged.push((file.clone(), temp));
            f.write_all(text.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| e.to_string())?;
        }
        for file in plan.originals.keys() {
            let target = archive(file);
            std::fs::create_dir_all(root.join(&target).parent().unwrap()).map_err(|e| e.to_string())?;
            // hard_link의 create-new 성질로 기존 사본을 덮지 않는다.
            std::fs::hard_link(root.join(file), root.join(&target)).map_err(|e| e.to_string())?;
            archived.push((file.clone(), target));
            std::fs::remove_file(root.join(file)).map_err(|e| e.to_string())?;
        }
        for (file, temp) in &staged {
            std::fs::hard_link(temp, root.join(file)).map_err(|e| e.to_string())?;
            created.push(file.clone());
            std::fs::remove_file(temp).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for (_, temp) in &staged {
            if let Err(e) = std::fs::remove_file(temp)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                failures.push(e.to_string());
            }
        }
        for file in created.iter().rev() {
            if let Err(e) = std::fs::remove_file(root.join(file)) {
                failures.push(e.to_string());
            }
        }
        for (file, backup) in archived.iter().rev() {
            if root.join(file).exists() && project_text(root, file).ok().as_ref() != plan.originals.get(file) {
                failures.push(format!("{file}: 원본 자리에 다른 파일이 있어 {backup} 보존"));
                continue;
            }
            if !root.join(file).exists()
                && let Err(e) = std::fs::hard_link(root.join(backup), root.join(file))
            {
                failures.push(e.to_string());
                continue;
            }
            if let Err(e) = std::fs::remove_file(root.join(backup)) {
                failures.push(e.to_string());
            }
        }
        return Err(format!(
            "변환 실패: {error}{}",
            if failures.is_empty() {
                " · 원본 복구 완료".into()
            } else {
                format!(" · 복구 확인 필요: {}", failures.join("; "))
            }
        ));
    }
    Ok(plan.writes.keys().cloned().chain(archived.into_iter().map(|(_, path)| path)).collect())
}
