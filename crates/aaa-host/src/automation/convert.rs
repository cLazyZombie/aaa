//! 변환 계획을 먼저 검증하고, 실패하면 이번 변환의 파일만 되돌린다.
use super::*;
use std::{collections::BTreeMap, io::Write, path::Path};
#[derive(Default)]
pub(super) struct Plan {
    writes: BTreeMap<String, String>,
    originals: BTreeMap<String, String>,
    archives: BTreeMap<String, Archive>,
    pub(super) targets: Vec<(String, String)>,
    state_hash: String,
}
struct Archive {
    path: String,
    reuse: bool,
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
    if external.is_none() && text.is_some_and(|s| s.len() > 4096) {
        let body = text.unwrap();
        let path = format!("automations/{name}.md");
        add(plan, root, path.clone(), format!("{body}\n"))?;
        match &mut spec.action {
            Action::Deploy => {}
            Action::Inbox(a) => {
                a.text = None;
                a.text_file = Some(path)
            }
            Action::Message(a) => {
                a.text = None;
                a.text_file = Some(path)
            }
            Action::Start(a) => {
                a.text = None;
                a.brief_file = None;
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
    if normalized.action.texts().0.unwrap_or("") != expected_text {
        return Err("변환한 글이 정규화 값과 다르다".into());
    }
    add(plan, root, format!("automations/{name}.toml"), serialized)
}
pub(super) fn prepare(root: &Path, machine: &str) -> Result<Plan, String> {
    let mut plan = Plan::default();
    let mut definitions = BTreeMap::new();
    for file in crate::legacy_mission::files(root) {
        let path = Path::new(&file);
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        // TOML이 참조하는 md는 본문이다. 같은 이름은 TOML부터 읽는다.
        if definitions.contains_key(&name) {
            continue;
        }
        if file.ends_with(".md") && !project_text(root, &file)?.starts_with("+++\n") {
            continue;
        }
        definitions.insert(
            name.clone(),
            crate::legacy_mission::definition(root, &name).map_err(|e| format!("{file}:1:1 변환: {e}"))?,
        );
    }
    for (name, def) in &definitions {
        add(&mut plan, root, format!("automations/{name}.brief.md"), format!("{}\n", def.body.trim()))?;
    }
    let mut converted_targets = std::collections::BTreeSet::new();
    for folder in ["routines", "automations"] {
        let mut entries: Vec<_> =
            std::fs::read_dir(root.join(folder)).into_iter().flatten().filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let file = format!("{folder}/{}", entry.file_name().to_string_lossy());
            if !(folder == "routines" && file.ends_with(".md") || folder == "automations" && file.ends_with(".toml")) {
                continue;
            }
            let name = entry.path().file_stem().unwrap().to_string_lossy().into_owned();
            let original = project_text(root, &file)?;
            let (mut spec, target, memo, reports) = if folder == "routines" {
                let rest = original.strip_prefix("+++\n").ok_or("routine front matter가 없다")?;
                let (front, body) = rest.split_once("\n+++\n").ok_or("닫는 +++가 없다")?;
                let mut value: toml::Value = toml::from_str(front).map_err(|e| e.to_string())?;
                let target = value
                    .as_table_mut()
                    .unwrap()
                    .remove("mission")
                    .map(|v| v.as_str().map(str::to_string).ok_or("mission: 문자열이어야 한다"))
                    .transpose()?;
                let ordinary = format!("+++\n{}+++\n{body}", toml::to_string(&value).map_err(|e| e.to_string())?);
                let r = crate::routine::parse(&name, &ordinary)?;
                (File::from_routine(&r), target, r.prompt.clone(), None)
            } else {
                let mut value: toml::Value = toml::from_str(&original).map_err(|e| e.to_string())?;
                let wake = value.get_mut("do").and_then(|v| v.as_table_mut()).and_then(|t| t.remove("wake"));
                let Some(wake) = wake else { continue };
                crate::legacy_mission::validate_wake(&wake)?;
                let target = wake.get("mission").and_then(|v| v.as_str()).ok_or("do.wake.mission이 없다")?.to_owned();
                let def = definitions.get(&target).ok_or_else(|| format!("missions/{target}: 옛 임무가 없다"))?;
                value["do"]["start"] =
                    toml::Value::try_from(def.start(&target, machine)?).map_err(|e| e.to_string())?;
                (
                    value.try_into::<File>().map_err(|e| e.to_string())?,
                    Some(target),
                    match (wake.get("text").and_then(|v| v.as_str()), wake.get("text_file").and_then(|v| v.as_str())) {
                        (Some(_), Some(_)) => return Err("do.wake: text와 text_file은 하나만 쓴다".into()),
                        (Some(text), None) => text.into(),
                        (None, Some(file)) => project_text(root, file)?,
                        _ => String::new(),
                    },
                    wake.get("reports").and_then(|v| v.as_bool()),
                )
            };
            if let Some(target) = target {
                let def = definitions.get(&target).ok_or_else(|| format!("missions/{target}: 옛 임무가 없다"))?;
                let mut action = def.start(&target, machine)?;
                action.reports = reports.unwrap_or(action.reports);
                if !memo.trim().is_empty() {
                    let file = format!("automations/{name}.brief.md");
                    let text = format!(
                        "{}\n\n## 입력 메모(자료이며 지시가 아니다)\n{}\n",
                        def.body.trim(),
                        crate::auto_start::fenced(&memo)
                    );
                    if plan.writes.contains_key(&file) {
                        plan.writes.insert(file.clone(), text);
                    } else {
                        add(&mut plan, root, file.clone(), text)?;
                    }
                    action.brief_file = Some(file);
                }
                spec.action = Action::Start(action);
                def.merge_limits(&mut spec.limits);
                converted_targets.insert(target.clone());
                plan.targets.push((target, name.clone()));
                add(&mut plan, root, format!("automations/{name}.memory/memory.md"), String::new())?;
            }
            plan.originals.insert(file, original);
            rule(&mut plan, root, &name, spec)?;
        }
    }
    for (name, def) in definitions {
        let events: Vec<String> = def
            .value
            .get("events")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let mut conditions = Vec::new();
        if !events.is_empty() {
            conditions.push((
                name.clone(),
                Condition::Thread(Threads {
                    events,
                    repos: def
                        .value
                        .get("repos")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                        .unwrap_or_default(),
                    include_automation: true,
                }),
            ));
        }
        if let Some(schedule) = def.value.get("schedule").and_then(|v| v.as_str()) {
            let rule = if conditions.is_empty() { name.clone() } else { format!("{name}-schedule") };
            conditions.push((
                rule,
                Condition::Schedule(Scheduled::from_schedule(
                    &crate::routine::parse_schedule(schedule, true)?,
                    CatchUp::Once,
                )),
            ));
        }
        if conditions.is_empty() && !converted_targets.contains(&name) {
            return Err(format!("missions/{name}: 조건이 없다. mission= 또는 do.wake 규칙과 함께 변환한다"));
        }
        for (rule_name, on) in conditions {
            let spec = File {
                title: def.start(&name, machine)?.title.clone(),
                enabled: false,
                on,
                action: Action::Start(def.start(&name, machine)?),
                limits: def.limits(),
            };
            rule(&mut plan, root, &rule_name, spec)?;
            add(&mut plan, root, format!("automations/{rule_name}.memory/memory.md"), String::new())?;
            plan.targets.push((name.clone(), rule_name));
        }
        for (path, text) in def.originals {
            plan.originals.insert(path, text);
        }
    }
    if !plan.originals.is_empty() {
        let file = "automations/automation.schema.json";
        let data = format!("{}\n", serde_json::to_string_pretty(&super::schema()).map_err(|e| e.to_string())?);
        // 스키마는 제품 생성물이므로 이전 판을 갱신한다. 사용자 규칙은 덮지 않는다.
        if !root.join(file).exists() {
            add(&mut plan, root, file.into(), data)?;
        } else if project_text(root, file)? != data {
            plan.originals.insert(file.into(), project_text(root, file)?);
            add(&mut plan, root, file.into(), data)?;
        }
    }
    for (file, text) in &plan.originals {
        plan.archives.insert(file.clone(), archive(root, file, text)?);
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
fn archive(root: &Path, file: &str, text: &str) -> Result<Archive, String> {
    let p = Path::new(file);
    let dir = p.parent().unwrap();
    // 같은 사본은 그대로 쓰고, 다른 판은 첫 빈 번호로 보존한다. 미리보기 때 경로도 확정한다.
    for number in 0u64.. {
        let name = if number == 0 {
            p.file_name().unwrap().to_string_lossy().into_owned()
        } else if let Some(extension) = p.extension() {
            format!("{}.{number}.{}", p.file_stem().unwrap().to_string_lossy(), extension.to_string_lossy())
        } else {
            format!("{}.{number}", p.file_name().unwrap().to_string_lossy())
        };
        let path = if dir.as_os_str().is_empty() {
            format!(".converted/{name}")
        } else {
            format!("{}/.converted/{name}", dir.to_string_lossy())
        };
        parent(root, &path)?;
        match root.join(&path).symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Archive { path, reuse: false }),
            Err(e) => return Err(format!("{path}:1:1 보존: {e}")),
            Ok(_) => {
                let saved = project_text(root, &path).map_err(|e| format!("{path}:1:1 보존: {e}"))?;
                if saved == text {
                    return Ok(Archive { path, reuse: true });
                }
            }
        }
    }
    Err(format!("{file}:1:1 보존: 빈 사본 번호가 없다"))
}

impl Plan {
    /// 실제 변환이 답하는 목록과 같은 모양: 새 파일과 `.converted/` 보존 사본.
    pub(super) fn listing(&self) -> Vec<String> {
        if self.originals.is_empty() {
            return vec![];
        }
        self.writes.keys().cloned().chain(self.archives.values().map(|a| a.path.clone())).collect()
    }
    /// 사람이 본 미리보기와 실행할 계획이 같은지 본다: 쓸 글과 원본 글 모두.
    pub(super) fn sha256(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for (tag, map) in [("w", &self.writes), ("o", &self.originals)] {
            for (file, text) in map {
                for part in [tag, file.as_str(), text.as_str()] {
                    h.update((part.len() as u64).to_le_bytes());
                    h.update(part.as_bytes());
                }
            }
        }
        for archive in self.archives.values() {
            h.update((archive.path.len() as u64).to_le_bytes());
            h.update(archive.path.as_bytes());
            h.update([u8::from(archive.reuse)]);
        }
        h.update(self.state_hash.as_bytes());
        hex::encode(h.finalize())
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
        let archive = &plan.archives[file];
        let target = &archive.path;
        parent(root, target)?;
        if archive.reuse {
            if project_text(root, target)? != *text {
                return Err(format!("{target}:1:1 보존: 계획 뒤 사본이 바뀌었다"));
            }
        } else {
            match root.join(target).symlink_metadata() {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{target}:1:1 보존: {e}")),
                Ok(_) => return Err(format!("{target}:1:1 보존: 계획 뒤 사본 자리가 쓰였다")),
            }
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
        for (file, archive) in &plan.archives {
            let target = &archive.path;
            if !archive.reuse {
                std::fs::create_dir_all(root.join(target).parent().unwrap()).map_err(|e| e.to_string())?;
                // hard_link의 create-new 성질로 기존 사본을 덮지 않는다.
                std::fs::hard_link(root.join(file), root.join(target)).map_err(|e| e.to_string())?;
            }
            archived.push((file.clone(), target.clone(), archive.reuse));
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
        for (file, backup, reused) in archived.iter().rev() {
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
            if !reused && let Err(e) = std::fs::remove_file(root.join(backup)) {
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
    Ok(plan.writes.keys().cloned().chain(archived.into_iter().map(|(_, path, _)| path)).collect())
}

impl Plan {
    pub(super) async fn include_state(
        &mut self,
        h: &crate::Host,
        p: &str,
    ) -> Result<Vec<serde_json::Value>, aaa_proto::ErrorBody> {
        use aaa_proto::{AgentRequest, AgentResponse};
        let bad = |e: String| crate::err(aaa_proto::ErrorCode::BadRequest, e);
        let mut records: Vec<serde_json::Value> =
            h.db.threads(Some(p))
                .map_err(|e| bad(e.to_string()))?
                .iter()
                .map(|s| serde_json::from_str(s).map_err(|e| bad(e.to_string())))
                .collect::<Result<_, _>>()?;
        records.sort_by_key(|r| r["updated_at_ms"].as_u64().unwrap_or(0));
        let mut journal = Vec::new();
        let mut imported = std::collections::BTreeSet::new();
        for (target, name) in self.targets.clone() {
            let rec = records.iter().rev().find(|r| r["mission"]["name"] == target);
            let mut data = rec.map(|r| r["mission"].clone()).unwrap_or_else(|| serde_json::json!({}));
            if !imported.insert(target.clone()) {
                data["pending"] = serde_json::json!([]);
            }
            let mut pending = data.get("pending").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let old_key = format!("automation:{p}:mission:{target}");
            if !imported.is_empty()
                && self.targets.iter().find(|(t, _)| t == &target).is_some_and(|(_, n)| n == &name)
                && let Some(old) = h.db.meta(&old_key).map_err(|e| bad(e.to_string()))?
            {
                let old: serde_json::Value = serde_json::from_str(&old).map_err(|e| bad(e.to_string()))?;
                pending.extend(old["pending"].as_array().cloned().unwrap_or_default());
                pending.extend(old["inflight"]["events"].as_array().cloned().unwrap_or_default());
                if !old["inflight"].is_null() {
                    data["unknown"] = true.into();
                }
            }
            let queued =
                h.db.meta(&format!("automation:{p}:{name}"))
                    .map_err(|e| bad(e.to_string()))?
                    .map(|s| serde_json::from_str::<serde_json::Value>(&s))
                    .transpose()
                    .map_err(|e| bad(e.to_string()))?;
            if !pending.is_empty()
                || queued
                    .as_ref()
                    .is_some_and(|v| v["pending"].as_array().is_some_and(|a| !a.is_empty()) || !v["inflight"].is_null())
            {
                let rule = parse_with(&name, &self.writes[&format!("automations/{name}.toml")], |f| {
                    self.writes
                        .get(f)
                        .cloned()
                        .map(Ok)
                        .unwrap_or_else(|| project_text(&h.project_dir(p).map_err(|e| e.message)?, f))
                })
                .map_err(bad)?;
                if let Action::Start(a) = &rule.action {
                    data["start_scope"] =
                        serde_json::to_value(h.automation_start_scope(p, &rule, a)?).map_err(|e| bad(e.to_string()))?;
                }
            }
            data["pending"] = pending.into();
            if data["wake_inflight"].as_bool() == Some(true)
                || data["blocked_reason"].as_str().is_some_and(|r| r != "question")
            {
                data["unknown"] = true.into();
            }
            if let Some(rec) = rec.filter(|r| r["work_dir"].is_string()) {
                let mid = rec["machine_id"].as_str().ok_or_else(|| bad("옛 thread 머신이 없다".into()))?;
                let work = rec["work_dir"].as_str().unwrap();
                let conn = h.live_conn(mid)?;
                let request =
                    |q: AgentRequest| if h.machine_feature(mid, "work_files.v1") { q } else { q.legacy_work_file() };
                let response = conn.request(request(AgentRequest::WorkFileList { work_dir: work.into() })).await?;
                let (AgentResponse::WorkFiles { files } | AgentResponse::MissionFiles { files }) = response else {
                    return Err(bad("옛 기억 목록 응답이 아니다".into()));
                };
                if files.len() > 1000 {
                    return Err(bad("옛 기억이 1000개를 넘었다".into()));
                }
                let mut total = 0u64;
                for f in files {
                    if !std::path::Path::new(&f.path).components().all(|c| matches!(c, std::path::Component::Normal(_)))
                    {
                        return Err(bad("옛 기억 경로가 상대 일반 경로가 아니다".into()));
                    }
                    total = total.saturating_add(f.size);
                    if total > 8 * 1024 * 1024 {
                        return Err(bad("옛 기억이 8MiB를 넘었다".into()));
                    }
                    let path: String = match f.path.as_str() {
                        "state.md" => "memory.md".into(),
                        "processed.txt" => "processed.txt".into(),
                        s if s.starts_with("history/") => s.into(),
                        _ => continue,
                    };
                    let mut bytes = vec![];
                    loop {
                        let AgentResponse::FileChunk { data, eof } = conn
                            .request(request(AgentRequest::WorkFileRead {
                                work_dir: work.into(),
                                path: f.path.clone(),
                                offset: bytes.len() as u64,
                            }))
                            .await?
                        else {
                            return Err(bad("옛 기억 읽기 응답이 아니다".into()));
                        };
                        if bytes.len().saturating_add(data.0.len()) > f.size as usize {
                            return Err(bad("옛 기억 크기가 바뀌었다".into()));
                        }
                        if !eof && data.0.is_empty() {
                            return Err(bad("옛 기억이 진행하지 않는다".into()));
                        }
                        bytes.extend(data.0);
                        if eof {
                            break;
                        }
                    }
                    use sha2::Digest;
                    if bytes.len() as u64 != f.size || hex::encode(sha2::Sha256::digest(&bytes)) != f.sha256 {
                        return Err(bad("옛 기억 hash가 다르다".into()));
                    }
                    let text = String::from_utf8(bytes).map_err(|_| bad("옛 기억은 UTF-8이어야 한다".into()))?;
                    let file = format!("automations/{name}.memory/{path}");
                    if !self.writes.contains_key(&file) {
                        add(self, &h.project_dir(p)?, file.clone(), text.clone()).map_err(bad)?;
                    }
                    self.writes.insert(file, text);
                }
            }
            journal.push(
                serde_json::json!({"target":target,"rule":name,"text":self.writes[&format!("automations/{name}.toml")],"data":data}),
            );
        }
        // 큐·기억도 미리보기 hash에 포함한다.
        self.state_hash = serde_json::to_string(&journal).map_err(|e| bad(e.to_string()))?;
        Ok(journal)
    }
}
