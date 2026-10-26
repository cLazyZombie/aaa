//! 계정 이름 이전: 새 AAD 금고 → 파일 → DB 확정. 중단이면 journal로 전부 복구한다.
use crate::{Host, err};
use aaa_proto::{ErrorBody, ErrorCode, FrontResponse};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod gate;
pub(crate) use gate::Gate;

pub(crate) const JOURNAL: &str = "account-rename:pending";
#[derive(Serialize, Deserialize)]
struct FileChange {
    path: PathBuf,
    before: String,
    after: String,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    from: String,
    to: String,
    committed: bool,
    files: Vec<FileChange>,
}
fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, format!("계정 이름 이전: {e}"))
}
fn conflict(text: &str) -> ErrorBody {
    err(ErrorCode::Conflict, text)
}

/// 계정/파일 원문은 로그로 보내지 않는다. 비밀 없는 단계·경로·OS 오류만 보고한다.
fn durable(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().context("부모 경로가 없다")?;
    let tmp = parent.join(format!(".account-rename-{}", aaa_common::util::new_id()));
    let result = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        sync_parent(path)?;
        Ok::<_, std::io::Error>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.with_context(|| format!("저장 {}", path.display()))
}
fn sync_parent(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    std::fs::File::open(path.parent().unwrap())?.sync_all()?;
    let _ = path;
    Ok(())
}
fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => sync_parent(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e).with_context(|| format!("금고 정리 {}", path.display())),
    }
    Ok(())
}

/// DB transaction의 committed 표지가 결정점이다. 재기동 때 인증을 읽기 전에 끝낸다.
pub(crate) fn recover(db: &crate::db::Db, state: &Path) -> Result<()> {
    let Some(text) = db.meta(JOURNAL)? else { return Ok(()) };
    let journal: Journal = serde_json::from_str(&text)?;
    anyhow::ensure!(
        crate::accounts::valid_name(&journal.from)
            && crate::accounts::valid_name(&journal.to)
            && journal.from != journal.to,
        "이름 이전 journal의 별칭을 확인한다"
    );
    for f in &journal.files {
        let current = std::fs::read_to_string(&f.path).with_context(|| format!("복구 읽기 {}", f.path.display()))?;
        anyhow::ensure!(
            current == f.before || current == f.after,
            "이름 이전 뒤 직접 편집 발견: {}. journal을 유지한다",
            f.path.display()
        );
        let target = if journal.committed { &f.after } else { &f.before };
        if &current != target {
            durable(&f.path, target.as_bytes())?;
        }
    }
    let obsolete = if journal.committed { &journal.from } else { &journal.to };
    remove(&state.join("accounts").join(format!("{obsolete}.json")))?;
    db.del_meta(JOURNAL)?;
    Ok(())
}

/// 구조화된 계정 칸만 바꾼다. profile 이름·한도 그룹·명령·보고·과거 감사는 다른 신원이다.
pub(crate) fn json(v: &mut serde_json::Value, from: &str, to: &str) {
    match v {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if matches!(key.as_str(), "env" | "body" | "payload" | "credential") {
                    continue;
                }
                if matches!(key.as_str(), "account" | "required_account") && child.as_str() == Some(from) {
                    *child = to.into();
                } else {
                    json(child, from, to);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                json(item, from, to);
            }
        }
        _ => (),
    }
}
fn toml_item(item: &mut toml_edit::Item, key: &str, from: &str, to: &str) {
    if matches!(key, "env" | "body" | "payload") {
        return;
    }
    match item {
        toml_edit::Item::Value(v) => toml_value(v, key, from, to),
        toml_edit::Item::Table(t) => {
            for (k, v) in t.iter_mut() {
                toml_item(v, k.get(), from, to);
            }
        }
        toml_edit::Item::ArrayOfTables(ts) => {
            for t in ts.iter_mut() {
                for (k, v) in t.iter_mut() {
                    toml_item(v, k.get(), from, to);
                }
            }
        }
        _ => (),
    }
}
fn toml_value(v: &mut toml_edit::Value, key: &str, from: &str, to: &str) {
    if matches!(key, "env" | "body" | "payload") {
        return;
    }
    if key == "account" && v.as_str() == Some(from) {
        let decor = v.decor().clone();
        *v = to.into();
        *v.decor_mut() = decor;
    } else if let Some(t) = v.as_inline_table_mut() {
        for (k, v) in t.iter_mut() {
            toml_value(v, k.get(), from, to);
        }
    } else if let Some(a) = v.as_array_mut() {
        for v in a.iter_mut() {
            toml_value(v, key, from, to);
        }
    }
}
fn config(text: &str, from: &str, to: &str) -> Result<String> {
    let (front, tail) = if let Some(rest) = text.strip_prefix("+++\n") {
        let (front, tail) = rest.split_once("\n+++").context("+++ 닫는 줄이 없다")?;
        (front, Some(tail))
    } else {
        (text, None)
    };
    let mut doc: toml_edit::DocumentMut = front.parse().map_err(|e: toml_edit::TomlError| {
        anyhow::anyhow!("TOML 구문 오류 (바이트 {}): {}", e.span().map_or(0, |s| s.start), e.message())
    })?;
    toml_item(doc.as_item_mut(), "", from, to);
    let new = doc.to_string();
    Ok(if let Some(tail) = tail { format!("+++\n{}\n+++{}", new.trim_end_matches('\n'), tail) } else { new })
}
fn paths(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !root.try_exists()? {
        return Ok(());
    }
    anyhow::ensure!(
        !root.symlink_metadata()?.file_type().is_symlink(),
        "이름 이전 폴더가 symlink다: {}",
        root.display()
    );
    for entry in root.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        anyhow::ensure!(!kind.is_symlink(), "이름 이전 경로가 symlink다: {}", path.display());
        if kind.is_dir() {
            paths(&path, out)?;
        } else if path.extension().is_some_and(|x| x == "toml")
            || (path.extension().is_some_and(|x| x == "md") && std::fs::read_to_string(&path)?.starts_with("+++\n"))
        {
            out.push(path);
        }
    }
    Ok(())
}

impl Host {
    pub(crate) async fn account_rename(
        self: &std::sync::Arc<Self>,
        from: &str,
        to: &str,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if !crate::accounts::valid_name(from) || !crate::accounts::valid_name(to) || from == to {
            return Err(err(ErrorCode::BadRequest, "새 계정 이름은 다른 이름으로, 영문·숫자·-·_ 1~64자를 쓴다"));
        }
        let _rename = self.account_rename_gate.write().await;
        recover(&self.db, &self.state).map_err(internal)?;
        let list = self.accounts.list()?;
        if !list.iter().any(|a| a.name == from) {
            return Err(err(ErrorCode::NotFound, "바꿀 계정이 없다"));
        }
        if self.state.join("accounts").join(format!("{to}.json")).symlink_metadata().is_ok() {
            return Err(err(ErrorCode::NameTaken, "이미 쓰는 계정 이름이다"));
        }
        let jobs = self.accounts.logins.lock().unwrap();
        for job in jobs.values() {
            let v = job.view.lock().unwrap();
            if (v.account == from || v.account == to) && matches!(v.state.as_str(), "waiting" | "mismatch") {
                return Err(conflict("이 계정의 로그인·메일 확인을 끝내거나 취소한 뒤 이름을 바꾼다"));
            }
        }
        let _queue = self.queue_gate.try_lock().map_err(|_| conflict("예약 처리 중이다. 끝난 뒤 이름을 바꾼다"))?;
        let projects = self.project_directories();
        let dispatches: Vec<_> = projects.iter().map(|(p, _)| self.automation_dispatch_lock(p)).collect();
        let _dispatch: Vec<_> = dispatches
            .iter()
            .map(|l| l.try_lock().map_err(|_| conflict("자동화 처리 중이다. 끝난 뒤 이름을 바꾼다")))
            .collect::<Result<_, _>>()?;
        let records: Vec<crate::orch::ThreadRec> = self
            .db
            .threads(None)
            .map_err(internal)?
            .into_iter()
            .map(|s| serde_json::from_str(&s).map_err(internal))
            .collect::<Result<_, _>>()?;
        let mut thread_guards = Vec::new();
        for r in records.iter().filter(|r| r.account.as_deref() == Some(from)) {
            thread_guards.push(self.claim_thread(&r.project, &r.id)?);
            if r.holds_slot() {
                return Err(conflict("이 계정을 쓰는 열린 thread가 있다. 작업을 보류하거나 완료한 뒤 이름을 바꾼다"));
            }
        }
        // coordinator·thread의 인증 중계는 실행 당시 별칭을 쓴다. 살아 있거나 시작 중인 binding은 옮기지 않는다.
        for (_, text) in self.db.account_rename_bindings().map_err(internal)? {
            let binding: crate::account_policy::PaneAccount = serde_json::from_str(&text).map_err(internal)?;
            if binding.account == from
                && binding
                    .pane
                    .as_ref()
                    .is_none_or(|p| self.inner.lock().unwrap().panes.get(p).is_some_and(|p| p.info.state.is_running()))
            {
                return Err(conflict("이 계정의 pane이 실행·준비 중이다. 종료한 뒤 이름을 바꾼다"));
            }
        }
        for text in self.db.account_rename_images().map_err(internal)? {
            let image: serde_json::Value = serde_json::from_str(&text).map_err(internal)?;
            if image["view"]["account"] == from
                && matches!(image["view"]["state"].as_str(), Some("running" | "unknown"))
            {
                return Err(conflict(
                    "이 계정의 이미지 생성이 실행 중이거나 종료가 불명확하다. 끝내거나 정리한 뒤 이름을 바꾼다",
                ));
            }
        }
        let _meta = self.account_meta_gate.lock().unwrap();
        // 조회 중인 cache는 완료 시 새 별칭의 DB 관측을 다시 읽는다.
        let mut cache = self.account_quota.try_lock().ok();
        let _records = self.rec_lock.lock().unwrap();
        let mut candidates = Vec::new();
        for (_, dir) in &projects {
            for name in ["aaa.toml", "project.toml"] {
                if dir.join(name).try_exists().map_err(internal)? {
                    candidates.push(dir.join(name));
                }
            }
            for name in ["automations", "routines", "missions"] {
                paths(&dir.join(name), &mut candidates).map_err(internal)?;
            }
        }
        candidates.sort();
        candidates.dedup();
        let locks: Vec<_> = candidates.iter().map(|p| self.file_locks.of(p)).collect();
        let _files: Vec<_> = locks.iter().map(|l| l.lock().unwrap()).collect();
        let mut journal = Journal { from: from.into(), to: to.into(), committed: false, files: vec![] };
        for path in candidates {
            if !path.symlink_metadata().map_err(internal)?.is_file() {
                return Err(internal("일반 설정 파일이 아니다"));
            }
            let before = std::fs::read_to_string(&path).map_err(internal)?;
            let after = config(&before, from, to).map_err(internal)?;
            if before != after {
                journal.files.push(FileChange { path, before, after });
            }
        }
        self.db.set_meta(JOURNAL, &serde_json::to_string(&journal).map_err(internal)?).map_err(internal)?;
        let result: Result<()> = (|| {
            self.accounts.stage_rename(from, to).map_err(|e| anyhow::anyhow!("금고 재암호화: {}", e.message))?;
            for f in &journal.files {
                anyhow::ensure!(
                    std::fs::read_to_string(&f.path)? == f.before,
                    "이름 이전 중 설정 파일이 바뀌었다: {}",
                    f.path.display()
                );
                durable(&f.path, f.after.as_bytes())?;
            }
            journal.committed = true;
            self.db.account_rename(from, to, actor, &serde_json::to_string(&journal)?)?;
            Ok(())
        })();
        if let Err(e) = result {
            let recovery = recover(&self.db, &self.state);
            return Err(internal(match recovery {
                Ok(()) => format!("{e:#}; 원래 상태로 복구 완료"),
                Err(r) => format!("{e:#}; 복구 실패: {r:#}"),
            }));
        }
        if let Some(cache) = cache.as_mut() {
            cache.rename(from, to);
        }
        for job in jobs.values() {
            let mut v = job.view.lock().unwrap();
            if v.account == from {
                v.account = to.into();
            }
        }
        if let Err(e) = recover(&self.db, &self.state) {
            tracing::error!(error=%e,"계정 이름 확정 뒤 정리 실패; journal로 다음 시작에서 정리");
        }
        drop(_records);
        for r in records.iter().filter(|r| r.account.as_deref() == Some(from)) {
            match self.thread_show(&r.project, &r.id) {
                Ok(thread) => {
                    let _ = self.thread_events.send(thread);
                }
                Err(e) => tracing::error!(thread=%r.id, error=%e.message, "계정 이름 변경 뒤 thread 화면 갱신 실패"),
            }
        }
        for (project, _) in projects {
            let _ = self.task_events.send((project, "account-rename".into()));
        }
        self.accounts_bump();
        self.inbox_changed.send_modify(|n| *n += 1);
        self.tick_wake.notify_one();
        Ok(FrontResponse::Done)
    }
}

#[cfg(test)]
mod tests;
