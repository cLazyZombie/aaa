//! 머신 신원은 ID, 이름은 host가 관리하는 이름표다. 파일을 먼저 옮기고 DB를 확정하며 journal로 중단을 복구한다.
use crate::{AgentConn, Host, err};
use aaa_proto::{AgentRequest, ErrorBody, ErrorCode, FrontResponse};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const JOURNAL: &str = "machine-rename:pending";
#[derive(Serialize, Deserialize)]
pub(super) struct Change {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    id: String,
    to: String,
    files: Vec<Change>,
}
fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, format!("머신 이름 이전: {e}"))
}

/// DB 확정 전 중단이면 원문으로, 확정 뒤 중단이면 새 글로 복구한다. 뒤에 직접 고친 파일은 덮지 않는다.
pub(super) fn recover(db: &crate::db::Db) -> Result<()> {
    let Some(text) = db.meta(JOURNAL)? else { return Ok(()) };
    let journal: Journal = serde_json::from_str(&text)?;
    let committed = db.machine_by_id(&journal.id)?.is_some_and(|m| m.name == journal.to);
    let mut failures = Vec::new();
    for file in &journal.files {
        let result: Result<()> = (|| {
            let current =
                std::fs::read_to_string(&file.path).with_context(|| format!("복구 읽기 {}", file.path.display()))?;
            let target = if committed { &file.after } else { &file.before };
            anyhow::ensure!(
                current == file.before || current == file.after,
                "복구 중 직접 편집 발견: {}. 이름 이전 journal을 유지한다",
                file.path.display()
            );
            if &current != target {
                crate::orch::write_atomic(&file.path, target.as_bytes())
                    .with_context(|| format!("복구 저장 {}", file.path.display()))?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            failures.push(format!("{e:#}"));
        }
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
    db.del_meta(JOURNAL)?;
    Ok(())
}

fn owner(value: &str, from: &str, to: &str) -> String {
    match value.rsplit_once('@') {
        Some((profile, machine)) if machine == from => format!("{profile}@{to}"),
        _ => value.into(),
    }
}

/// 제목·메모·경로를 치환하지 않는다. parser가 읽은 owner 자리만 옮긴다(CRLF·task ID 유지).
pub(super) fn tasks(text: &str, from: &str, to: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            if !crate::tasks::is_task(line) {
                return line.to_string();
            }
            let parsed = crate::tasks::split_line(line.trim_start().get(5..).unwrap_or_default());
            let Some(old) = parsed.owner else { return line.to_string() };
            let new = owner(&old, from, to);
            if old == new {
                return line.to_string();
            }
            // 끝 owner 괄호의 공백과 옛 → thread 표기도 보존한다.
            let Some(open) = line.rfind('(') else { return line.to_string() };
            let tail = &line[open + 1..];
            let at = open + 1 + tail.len() - tail.trim_start().len();
            if line[at..].starts_with(&old) {
                format!("{}{}{}", &line[..at], new, &line[at + old.len()..])
            } else {
                line.to_string()
            }
        })
        .collect()
}
fn machine_key(key: &str) -> bool {
    matches!(key, "machine" | "machine_name" | "source_machine" | "query_machine" | "ci_machine" | "machines")
}

/// 구조화된 머신 칸만 바꾼다. ID·실행 명령·env·보고 본문·과거 감사 원문은 보존한다.
pub(super) fn json(value: &mut serde_json::Value, from: &str, to: &str, hashes: &BTreeMap<String, String>) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if matches!(key.as_str(), "env" | "body" | "payload" | "credential") {
                    continue;
                }
                if let Some(s) = child.as_str() {
                    let replacement = if machine_key(key) && s == from {
                        Some(to.into())
                    } else if key == "raw" && crate::tasks::is_task(s) {
                        let new = tasks(s, from, to);
                        (new != s).then_some(new)
                    } else if key == "run_id" && s.starts_with(&format!("{from}:")) {
                        Some(format!("{to}:{}", &s[from.len() + 1..]))
                    } else if key == "owner" {
                        let new = owner(s, from, to);
                        (new != s).then_some(new)
                    } else if matches!(key.as_str(), "approved_block" | "block_sha256" | "revision") {
                        hashes.get(s).cloned()
                    } else {
                        None
                    };
                    if let Some(s) = replacement {
                        *child = Value::String(s)
                    }
                } else if key == "machines" {
                    if let Some(array) = child.as_array_mut() {
                        for v in array {
                            if v.as_str() == Some(from) {
                                *v = to.into()
                            }
                        }
                    }
                } else {
                    json(child, from, to, hashes)
                }
            }
        }
        Value::Array(array) => {
            for v in array {
                json(v, from, to, hashes)
            }
        }
        _ => {}
    }
}

fn toml_value(value: &mut toml_edit::Value, key: &str, from: &str, to: &str) {
    if matches!(key, "env" | "body" | "payload") {
        return;
    }
    if machine_key(key) && value.as_str() == Some(from) {
        let decor = value.decor().clone();
        *value = to.into();
        *value.decor_mut() = decor;
    } else if let Some(array) = value.as_array_mut() {
        for child in array.iter_mut() {
            toml_value(child, key, from, to)
        }
    } else if let Some(table) = value.as_inline_table_mut() {
        for (key, child) in table.iter_mut() {
            toml_value(child, key.get(), from, to)
        }
    }
}
fn toml_item(item: &mut toml_edit::Item, key: &str, from: &str, to: &str) {
    if matches!(key, "env" | "body" | "payload") {
        return;
    }
    match item {
        toml_edit::Item::Value(v) => toml_value(v, key, from, to),
        toml_edit::Item::Table(table) => {
            for (k, v) in table.iter_mut() {
                toml_item(v, k.get(), from, to)
            }
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                for (k, v) in table.iter_mut() {
                    toml_item(v, k.get(), from, to)
                }
            }
        }
        _ => {}
    }
}
fn toml(text: &str, from: &str, to: &str) -> Result<String> {
    // host.local.toml에는 비밀 env도 있다. 오류의 원문 발췌 대신 위치와 parser 원인만 남긴다.
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        anyhow::anyhow!("TOML 구문 오류 (바이트 {}): {}", e.span().map_or(0, |s| s.start), e.message())
    })?;
    toml_item(doc.as_item_mut(), "", from, to);
    Ok(doc.to_string())
}

fn frontmatter(text: &str, from: &str, to: &str) -> Result<String> {
    let rest = text.strip_prefix("+++\n").context("+++ 머리말이 없다")?;
    let (front, suffix) = rest.split_once("\n+++").context("+++ 닫는 줄이 없다")?;
    Ok(format!("+++\n{}\n+++{}", toml(front, from, to)?.trim_end_matches('\n'), suffix))
}

fn add_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    match std::fs::read_dir(root) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let kind = entry.file_type()?;
                anyhow::ensure!(!kind.is_symlink(), "이름 이전 경로에 symlink가 있다: {}", entry.path().display());
                if kind.is_dir() {
                    add_files(&entry.path(), out)?
                } else if entry.path().extension().is_some_and(|e| e == "toml")
                    || (entry.path().extension().is_some_and(|e| e == "md")
                        && std::fs::read_to_string(entry.path())?.starts_with("+++\n"))
                {
                    out.push(entry.path())
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("이름 이전 목록 {}", root.display())),
    }
    Ok(())
}

impl Host {
    pub(super) fn sync_machine_label(self: &std::sync::Arc<Self>, id: &str, conn: &AgentConn, features: &[String]) {
        if !features.iter().any(|f| f == "machine_label.v1") {
            return;
        }
        let (conn, id, host) = (conn.clone(), id.to_string(), self.clone());
        tokio::spawn(async move {
            let _sync = host.machine_label_gate.lock().await;
            let name = host.machine_name(&id);
            match conn.request(AgentRequest::MachineLabel { name }).await {
                Ok(aaa_proto::AgentResponse::Done) => {}
                Ok(other) => {
                    tracing::error!(machine = %id, response = ?other, "agent 이름표 저장 응답 오류; 다음 연결에서 동기화")
                }
                Err(e) => {
                    tracing::error!(machine = %id, error = %e.message, "agent 이름표 저장 실패; 다음 연결에서 동기화")
                }
            }
        });
    }

    pub(super) fn machine_rename(
        self: &std::sync::Arc<Self>,
        from: &str,
        to: &str,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if !aaa_common::util::machine_name_ok(to) {
            return Err(err(ErrorCode::BadRequest, crate::MACHINE_NAME_RULE));
        }
        let _rename = self.machine_rename_gate.lock().unwrap();
        // 앞 실패가 남았으면 먼저 복구한다. 직접 편집과 충돌하면 구체적인 경로를 알린다.
        recover(&self.db).map_err(internal)?;
        let machine = self
            .db
            .machine_by_name(from)
            .map_err(internal)?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("머신이 없다: {from}")))?;
        if machine.local {
            return Err(err(ErrorCode::Forbidden, "host 자신의 이름은 바꿀 수 없다(로컬 agent 설정과 host 실행 신원)"));
        }
        if from == to {
            return Ok(FrontResponse::Done);
        }
        let _setup = self.setup_gate.lock().unwrap();
        let enrollment = self.enroll.lock().unwrap();
        if self.db.machine_name_exists(to).map_err(internal)?
            || self.db.machine_by_id(to).map_err(internal)?.is_some()
            || enrollment.pending.contains_key(to)
        {
            return Err(err(ErrorCode::NameTaken, format!("이미 쓰거나 등록을 기다리는 머신 이름이다: {to}")));
        }
        for text in self.db.machine_setups().map_err(internal)? {
            let job: crate::machine_setup::Job = serde_json::from_str(&text).map_err(internal)?;
            if !matches!(
                job.view.phase,
                aaa_proto::MachineSetupPhase::Failed
                    | aaa_proto::MachineSetupPhase::Installed
                    | aaa_proto::MachineSetupPhase::Unsupported
            ) && job.view.settings.as_ref().is_some_and(|s| s.name == from || s.name == to)
            {
                return Err(err(
                    ErrorCode::Conflict,
                    "이 이름으로 머신 설치·등록 작업이 남아 있다. 끝내거나 취소한 뒤 바꾼다",
                ));
            }
        }
        let _metrics = self
            .metrics_gate
            .try_lock()
            .map_err(|_| err(ErrorCode::Conflict, "workspace 지표 회수 중이다. 끝난 뒤 이름을 바꾼다"))?;
        let _queue = self
            .queue_gate
            .try_lock()
            .map_err(|_| err(ErrorCode::Conflict, "예약 기동·편집 중이다. 끝난 뒤 이름을 바꾼다"))?;
        let projects = self.project_directories();
        let dispatches: Vec<_> = projects.iter().map(|(p, _)| self.automation_dispatch_lock(p)).collect();
        let _dispatch: Vec<_> = dispatches
            .iter()
            .map(|l| l.try_lock().map_err(|_| err(ErrorCode::Conflict, "자동화 처리 중이다. 끝난 뒤 이름을 바꾼다")))
            .collect::<Result<_, _>>()?;
        let mut paths = Vec::new();
        for (_, dir) in &projects {
            for file in ["aaa.toml", "project.toml", "TASKS.md"] {
                if dir.join(file).exists() {
                    paths.push(dir.join(file))
                }
            }
            for folder in ["automations", "routines", "missions"] {
                add_files(&dir.join(folder), &mut paths).map_err(internal)?;
            }
        }
        if let Some(dir) = self.config_dir.get() {
            for file in ["host.toml", "host.local.toml"] {
                if dir.join(file).exists() {
                    paths.push(dir.join(file))
                }
            }
        }
        paths.sort();
        paths.dedup();
        let locks: Vec<_> = paths.iter().map(|p| self.file_locks.of(p)).collect();
        let _guards: Vec<_> = locks.iter().map(|l| l.lock().unwrap()).collect();
        let mut files = Vec::new();
        let mut hashes = BTreeMap::new();
        for path in paths {
            let meta = std::fs::symlink_metadata(&path).map_err(internal)?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(err(ErrorCode::BadRequest, format!("일반 파일이 아니다: {}", path.display())));
            }
            let before = std::fs::read_to_string(&path).map_err(|e| internal(format!("{}: {e}", path.display())))?;
            let after = if path.file_name().is_some_and(|n| n == "TASKS.md") {
                let after = tasks(&before, from, to);
                let a = crate::tasks::parse(&before);
                let b = crate::tasks::parse(&after);
                for (old, new) in a.iter().zip(&b) {
                    if old.block_sha256 != new.block_sha256 {
                        hashes.insert(old.block_sha256.clone(), new.block_sha256.clone());
                    }
                }
                after
            } else if path.extension().is_some_and(|e| e == "md") {
                frontmatter(&before, from, to).map_err(|e| internal(format!("{}: {e:#}", path.display())))?
            } else {
                toml(&before, from, to).map_err(|e| internal(format!("{}: {e:#}", path.display())))?
            };
            if before != after {
                hashes.insert(crate::tasks::sha256(&before), crate::tasks::sha256(&after));
                if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                    let parent = path.parent().unwrap();
                    let parse = |text: &str| {
                        if parent.file_name().is_some_and(|n| n == "routines") {
                            crate::routine::parse(name, text)
                        } else {
                            crate::automation::parse(parent.parent().unwrap(), name, text)
                        }
                    };
                    if let (Ok(old), Ok(new)) = (parse(&before), parse(&after)) {
                        hashes.insert(old.command_sha256(), new.command_sha256());
                    }
                }
                files.push(Change { path, before, after })
            }
        }
        let journal = Journal { id: machine.id.clone(), to: to.into(), files };
        self.db.set_meta(JOURNAL, &serde_json::to_string(&journal).map_err(internal)?).map_err(internal)?;
        let result: Result<()> = (|| {
            for f in &journal.files {
                anyhow::ensure!(
                    std::fs::read_to_string(&f.path)? == f.before,
                    "이름 이전 중 파일이 바뀌었다: {}",
                    f.path.display()
                );
                crate::orch::write_atomic(&f.path, f.after.as_bytes())
                    .with_context(|| format!("저장 {}", f.path.display()))?;
            }
            self.db.machine_rename(&machine.id, from, to, actor, &hashes)
        })();
        if let Err(e) = result {
            let recovery = recover(&self.db);
            return Err(internal(match recovery {
                Ok(()) => format!("{e:#}; 원문 복구 완료"),
                Err(r) => format!("{e:#}; 복구 실패: {r:#}"),
            }));
        }
        // DB가 정본이다. 연결·generation·pane·lease를 건드리지 않고 화면 상태만 갱신한다.
        {
            let mut inner = self.inner.lock().unwrap();
            if let Some(m) = inner.machines.get_mut(&machine.id) {
                m.name = to.into();
            }
        }
        if let Err(e) = self.db.del_meta(JOURNAL) {
            tracing::error!(error = %e, "머신 이름은 확정됐으나 journal 정리 실패; 다음 시작에서 정리");
        }
        if let Some(conn) = self.conn_of(&machine.id) {
            let features =
                self.inner.lock().unwrap().machines.get(&machine.id).map(|m| m.features.clone()).unwrap_or_default();
            self.sync_machine_label(&machine.id, &conn, &features);
        }
        for (project, _) in projects {
            let _ = self.task_events.send((project.clone(), "machine-rename".into()));
            match self.threads_of(Some(&project), None) {
                Ok(threads) => {
                    for thread in threads {
                        let _ = self.thread_events.send(thread);
                    }
                }
                Err(e) => tracing::error!(%project, error = %e.message, "머신 이름 확정 뒤 thread 화면 갱신 실패"),
            }
        }
        drop(enrollment);
        self.bump();
        self.enroll_changed.send_modify(|n| *n += 1);
        self.inbox_changed.send_modify(|n| *n += 1);
        self.asks_changed.send_modify(|n| *n += 1);
        self.rollout_changed.send_modify(|n| *n += 1);
        for question in self.questions(true).into_iter().filter(|q| q.machine == to) {
            let _ = self.question_events.send(question);
        }
        self.workspace_metrics_changed.send_modify(|n| *n += 1);
        self.tick_wake.notify_one();
        Ok(FrontResponse::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{DeviceRole, FrontRequest, Role, ThreadKind};
    use std::sync::Arc;
    struct Lab {
        root: PathBuf,
        h: Arc<Host>,
    }
    impl Lab {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("aaa-rename-{}", aaa_common::util::new_id()));
            let mut cfg = aaa_config::HostConfig::default();
            cfg.projects.root = root.join("projects").display().to_string();
            cfg.notify.web_push = false;
            let h = Host::open(cfg.clone(), root.join("host")).unwrap();
            h.db.upsert_machine("m-host", "mac-a", None, true).unwrap();
            h.db.upsert_machine("m-remote", "mac-b", Some("test-key"), false).unwrap();
            drop(h);
            let h = Host::open(cfg, root.join("host")).unwrap();
            h.project_create("demo", Default::default(), "local:cli").unwrap();
            Self { root, h }
        }
        fn dir(&self) -> PathBuf {
            self.h.project_dir("demo").unwrap()
        }
        async fn rename(&self, from: &str, to: &str) -> Result<FrontResponse, ErrorBody> {
            self.h
                .call(DeviceRole::Local, "local:cli", FrontRequest::MachineRename { name: from.into(), to: to.into() })
                .await
        }
    }
    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn structured_fields_preserve_text_commands_paths_comments_and_task_ids() {
        let text = "## 일\r\n- [ ] mac-b 제목 (codex@mac-b) [app] · t-0001 <!-- aaa-task:AAAAAAAAAAAAAAAAAAAAAAAAAA -->\r\n  mac-b 메모\r\n- [x] 완료 (@mac-b)\r\n- [ ] 남김 (codex@mac-b-more)\r\n";
        let after = tasks(text, "mac-b", "mac-c");
        assert_eq!(after, text.replace("(codex@mac-b)", "(codex@mac-c)").replace("(@mac-b)", "(@mac-c)"));
        let cfg = "# mac-b\nrepos = [{path='/work/mac-b', machine='mac-b'}]\n[do.start]\nmachines = ['mac-b','mac-b-more'] # 유지\ntitle = 'mac-b'\n[env]\nmachine = 'mac-b'\n";
        let after = toml(cfg, "mac-b", "mac-c").unwrap();
        assert!(after.contains("# mac-b") && after.contains("path='/work/mac-b'") && after.contains("# 유지"));
        let t: toml::Value = toml::from_str(&after).unwrap();
        assert_eq!(t["repos"][0]["machine"].as_str(), Some("mac-c"));
        assert_eq!(t["env"]["machine"].as_str(), Some("mac-b"));
        assert_eq!(t["do"]["start"]["machines"][1].as_str(), Some("mac-b-more"));
        let mut v = serde_json::json!({"machine":"mac-b","machine_id":"m-remote","body":"mac-b","env":{"machine":"mac-b"},"owner":"codex@mac-b","machines":["mac-b","mac-b-more"]});
        json(&mut v, "mac-b", "mac-c", &BTreeMap::new());
        assert_eq!(
            v,
            serde_json::json!({"machine":"mac-c","machine_id":"m-remote","body":"mac-b","env":{"machine":"mac-b"},"owner":"codex@mac-c","machines":["mac-c","mac-b-more"]})
        );
    }

    #[tokio::test]
    async fn rename_preserves_registration_connection_history_workspace_queue_and_approval() {
        let l = Lab::new();
        l.h.project_set("demo", "repos.add", "/work/app@mac-b", "local:cli").unwrap();
        let view = l.h.workspace_access_view("demo", "mac-b").unwrap();
        l.h.workspace_access_set("demo", "mac-b", "app", true, &view.revision, "local:cli").unwrap();
        let before = "## 일\n- [ ] 진행 (codex@mac-b) [app] <!-- aaa-task:AAAAAAAAAAAAAAAAAAAAAAAAAA -->\n";
        std::fs::write(l.dir().join("TASKS.md"), before).unwrap();
        let task = crate::tasks::parse(before).remove(0);
        let q = serde_json::json!({"machine":"mac-b","task":task,"approved_block":task.block_sha256,"selection":{"owner":"codex@mac-b","machine":"mac-b"}});
        l.h.db.set_meta("automation:demo:pending", &q.to_string()).unwrap();
        let mut task = task.clone();
        crate::tasks::fill_agent(std::slice::from_mut(&mut task), &["codex".into()]);
        let reservation = aaa_proto::TaskReservation {
            selection: aaa_proto::TaskChoicePin {
                owner: task.owner.clone(),
                profile: "codex".into(),
                machine: Some("mac-b".into()),
                actor: "local:cli".into(),
                at_ms: 1,
            },
            task_id: task.id.clone().unwrap(),
            approved_block: task.block_sha256.clone(),
            task: task.clone(),
            profile: "codex".into(),
            model: "model".into(),
            effort: "high".into(),
            profile_revision: "profile-rev".into(),
            machine: Some("mac-b".into()),
            position: 1,
            state: aaa_proto::QueueState::Queued,
            reason: String::new(),
            launch_id: None,
            thread: None,
            after: None,
            after_short_id: None,
            stage: None,
            actor: "local:cli".into(),
            created_ms: 1,
            updated_ms: 1,
        };
        l.h.db
            .queue_change("demo", "local:cli", |rows| {
                rows.push(reservation);
                Ok(())
            })
            .unwrap();
        let legacy = l.dir().join("routines/check-legacy.md");
        std::fs::write(&legacy, "+++\nschedule='every 1h'\ncommand='echo mac-b'\nmachine='mac-b'\n+++\nmac-b 본문\n")
            .unwrap();
        let config = l.root.join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("host.toml"), "schema=1\n[projects]\nci_machine='mac-b' # 유지\n").unwrap();
        l.h.config_dir.set(config.clone()).unwrap();

        l.h.db.set_meta("deploy:job", r#"{"machine":"m-remote","project":"demo"}"#).unwrap();
        l.h.db.set_meta("workspace-collection::ws-test:mac-b", "[1,[]]").unwrap();
        let collected = aaa_proto::WorkspaceCollected {
            runs: vec![aaa_proto::MetricRun {
                run_id: "run-a".into(),
                commit: "test-commit".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        l.h.db.save_workspace_metrics("", "ws-test", "mac-b", &collected).unwrap();

        let auto = l.dir().join("automations");
        std::fs::create_dir_all(&auto).unwrap();
        let text = "schema=2\n[trigger]\nkind='schedule'\nevery='1h'\n[action]\nkind='command'\ncommand='echo mac-b'\nmachine='mac-b'\n";
        std::fs::write(auto.join("check.toml"), text).unwrap();
        let routine = crate::automation::parse(&l.dir(), "check", text).unwrap();
        l.h.db.set_meta("routine_ok:demo:check", &routine.command_sha256()).unwrap();
        for status in [aaa_proto::ThreadStatus::Open, aaa_proto::ThreadStatus::Resolved] {
            let mut rec =
                crate::orch::ThreadRec::starting("demo", "기록", "m-remote", "codex", Role::Worker, ThreadKind::Dir, 1);
            rec.status = status;
            let id =
                l.h.db
                    .thread_insert_next("demo", |id| {
                        rec.id = id.into();
                        serde_json::to_string(&rec).unwrap()
                    })
                    .unwrap();
            l.h.db.thread_save("demo", &id, status.as_str(), &serde_json::to_string(&rec).unwrap()).unwrap();
        }
        let (tx, _rx) = aaa_common::conn::frame_channel();
        l.h.inner.lock().unwrap().machines.get_mut("m-remote").unwrap().conn = Some(AgentConn::new(tx, 42));
        l.rename("mac-b", "mac-c").await.unwrap();
        let m = l.h.db.machine_by_name("mac-c").unwrap().unwrap();
        assert_eq!((m.id.as_str(), m.fingerprint.as_deref()), ("m-remote", Some("test-key")));
        assert!(l.h.db.machine_by_name("mac-b").unwrap().is_none());
        assert_eq!(l.h.conn_of("m-remote").unwrap().generation, 42);
        assert_eq!(l.h.resolve_machine(Some("mac-c")).unwrap(), "m-remote");
        assert!(l.h.resolve_machine(Some("mac-b")).is_err());
        assert!(l.h.workspace_allowed("demo", "app", "mac-c").unwrap());
        assert_eq!(l.h.settings("demo").repos[0].machine.as_deref(), Some("mac-c"));
        assert!(l.h.threads_of(Some("demo"), None).unwrap().iter().all(|r| r.machine == "mac-c"));
        let after = std::fs::read_to_string(l.dir().join("TASKS.md")).unwrap();
        let new_task = crate::tasks::parse(&after).remove(0);
        let q: serde_json::Value =
            serde_json::from_str(&l.h.db.meta("automation:demo:pending").unwrap().unwrap()).unwrap();
        assert_eq!(q["approved_block"], new_task.block_sha256);
        assert_eq!(q["selection"]["owner"], "codex@mac-c");
        assert_eq!(q["task"]["owner"], "codex@mac-c");
        let queued = l.h.db.queue_list("demo").unwrap().remove(0);
        assert_eq!(queued.machine.as_deref(), Some("mac-c"));
        assert_eq!(queued.selection.machine.as_deref(), Some("mac-c"));
        assert_eq!(queued.approved_block, new_task.block_sha256);
        assert_eq!(queued.task.raw, new_task.raw);
        assert_eq!(queued.profile_revision, "profile-rev");
        assert_eq!(l.h.profile_config().unwrap().projects.ci_machine.as_deref(), Some("mac-c"));
        let legacy_text = std::fs::read_to_string(&legacy).unwrap();
        assert_eq!(crate::routine::parse("check-legacy", &legacy_text).unwrap().machine.as_deref(), Some("mac-c"));
        assert!(legacy_text.contains("mac-b 본문"));

        assert!(l.h.db.meta("deploy:job").unwrap().unwrap().contains("m-remote"));
        assert!(l.h.db.meta("workspace-collection::ws-test:mac-c").unwrap().is_some());
        let (runs, _, _) = l.h.db.workspace_records("", "ws-test", 0).unwrap();
        assert_eq!(runs[0].machine, "mac-c");
        assert_eq!(runs[0].run_id, "mac-c:run-a");
        l.h.db.save_workspace_metrics("", "ws-test", "mac-c", &collected).unwrap();
        assert_eq!(
            l.h.db.workspace_records("", "ws-test", 0).unwrap().0.len(),
            1,
            "이름 변경 뒤 재회수는 중복을 만들지 않는다"
        );
        let new_routine =
            crate::automation::parse(&l.dir(), "check", &std::fs::read_to_string(auto.join("check.toml")).unwrap())
                .unwrap();
        assert!(l.h.routine_approved("demo", &new_routine));
        let h = Host::open(l.h.cfg.clone(), l.root.join("host")).unwrap();
        assert_eq!(h.resolve_machine(Some("mac-c")).unwrap(), "m-remote");
        assert!(h.db.meta(JOURNAL).unwrap().is_none());
        let hello: aaa_proto::AgentHello = serde_json::from_value(serde_json::json!({
            "proto":aaa_proto::AGENT_LINK,"machine_id":"m-remote","machine_name":"mac-b",
            "boot_id":"boot-test","os":"test","arch":"test","software":"aaa test"
        }))
        .unwrap();
        let (tx, _rx) = aaa_common::conn::frame_channel();
        h.agent_connected(&hello, false, AgentConn::new(tx, 43));
        assert_eq!(h.machine_name("m-remote"), "mac-c", "구판 agent의 옛 hello가 host 이름표를 바꾸지 않는다");
    }

    #[tokio::test]
    async fn rejects_collisions_invalid_names_host_readers_and_session_actors_without_writes() {
        let l = Lab::new();
        for (from, to, code) in [
            ("mac-b", "mac-a", ErrorCode::NameTaken),
            ("mac-a", "mac-c", ErrorCode::Forbidden),
            ("missing", "mac-c", ErrorCode::NotFound),
            ("mac-b", "bad name", ErrorCode::BadRequest),
        ] {
            assert_eq!(l.rename(from, to).await.unwrap_err().code, code);
        }
        for (role, actor) in [
            (DeviceRole::Reader, "device:r"),
            (DeviceRole::Local, "worker:p"),
            (DeviceRole::Local, "local:cli@coordinator:p"),
        ] {
            assert_eq!(
                l.h.call(role, actor, FrontRequest::MachineRename { name: "mac-b".into(), to: "mac-c".into() })
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
        assert!(l.h.db.machine_by_name("mac-b").unwrap().is_some());
        assert!(l.h.db.meta(JOURNAL).unwrap().is_none());
        l.rename("mac-b", "mac-b").await.unwrap();
    }

    #[tokio::test]
    async fn queue_and_automation_dispatch_prevent_a_stale_inflight_write() {
        let l = Lab::new();
        let queue = l.h.queue_gate.lock().await;
        assert_eq!(l.rename("mac-b", "mac-c").await.unwrap_err().code, ErrorCode::Conflict);
        drop(queue);
        let dispatch = l.h.automation_dispatch_lock("demo");
        let held = dispatch.lock().await;
        assert_eq!(l.rename("mac-b", "mac-c").await.unwrap_err().code, ErrorCode::Conflict);
        drop(held);
        assert!(l.h.db.machine_by_name("mac-b").unwrap().is_some());
        assert!(l.h.db.meta(JOURNAL).unwrap().is_none());
    }

    #[tokio::test]
    async fn malformed_config_and_db_failure_restore_all_files_and_keep_identity() {
        let l = Lab::new();
        let path = l.dir().join("TASKS.md");
        let text = "- [ ] 준비 (codex@mac-b)\n";
        std::fs::write(&path, text).unwrap();
        std::fs::write(l.dir().join("project.toml"), "invalid [").unwrap();
        assert_eq!(l.rename("mac-b", "mac-c").await.unwrap_err().code, ErrorCode::Internal);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::write(l.dir().join("project.toml"), "repos=[]\n").unwrap();
        // JSON을 읽지 못하면 DB 트랜잭션과 먼저 쓴 파일을 모두 원복한다.
        l.h.db.thread_import("demo", "t-9999", "active", "bad json").unwrap();
        let e = l.rename("mac-b", "mac-c").await.unwrap_err();
        assert!(e.message.contains("threads.data") && e.message.contains("원문 복구 완료"), "{}", e.message);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(l.h.db.machine_by_name("mac-b").unwrap().is_some());
        assert!(l.h.db.meta(JOURNAL).unwrap().is_none());
    }

    #[tokio::test]
    async fn interrupted_journal_recovers_in_both_directions_and_refuses_direct_edits() {
        let l = Lab::new();
        let path = l.dir().join("TASKS.md");
        let before = "- [ ] 일 (codex@mac-b)\n";
        let after = tasks(before, "mac-b", "mac-c");
        let journal = Journal {
            id: "m-remote".into(),
            to: "mac-c".into(),
            files: vec![Change { path: path.clone(), before: before.into(), after: after.clone() }],
        };
        l.h.db.set_meta(JOURNAL, &serde_json::to_string(&journal).unwrap()).unwrap();
        std::fs::write(&path, &after).unwrap();
        recover(&l.h.db).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        l.h.db.machine_rename("m-remote", "mac-b", "mac-c", "local:cli", &BTreeMap::new()).unwrap();
        l.h.db.set_meta(JOURNAL, &serde_json::to_string(&journal).unwrap()).unwrap();
        recover(&l.h.db).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
        l.h.db.set_meta(JOURNAL, &serde_json::to_string(&journal).unwrap()).unwrap();
        std::fs::write(&path, "직접 고친 글").unwrap();
        assert!(recover(&l.h.db).unwrap_err().to_string().contains("직접 편집"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "직접 고친 글");
        assert!(l.h.db.meta(JOURNAL).unwrap().is_some());
    }
}
