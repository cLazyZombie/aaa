//! inventory를 바탕으로 aaa 상태 폴더에 프로젝트를 만든다(plan §15 4: 끝난 일의 검증한 사본).
//!
//! - 원본은 읽기만 한다. 대상 프로젝트 폴더가 이미 있거나 그 상태 폴더의 host가 떠 있으면 하지 않는다.
//! - 파일은 복사한 뒤 사본 sha256을 inventory와 대조한다. inventory 뒤 원본이 바뀐 파일은 「바뀜」으로 따로 적는다
//!   (실패가 아니다: 사본은 지금 원본과 같다). 복사·대조 실패는 「실패」다.
//! - 끝난(resolved) thread만 host DB에 기록으로 넣는다. 열린 thread는 넣지 않는다(소유 도구 하나, §15 3).
//! - routine은 `enabled = false`로 바꿔 넣는다(두 ticker가 같은 일을 두 번 하지 않게). 전환 때 켠다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aaa_proto::{BriefState, FlowEvent, InboxItem, Role, ThreadGroup, ThreadKind, ThreadStatus, next_lines};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::inventory::{FileEntry, Inventory};
use super::source::{self, HpThread};
use super::{cell, human_bytes, sha256_file, write_new};
use crate::db::Db;
use crate::orch::{ThreadRec, write_skeleton};

pub struct Dest {
    /// aaa 상태 폴더 root(host DB는 `<root>/host/host.db`)
    pub state_root: PathBuf,
    /// aaa projects root
    pub projects_root: PathBuf,
    /// 만들 aaa 프로젝트 이름
    pub project: String,
    /// library도 복사한다(같은 APFS volume이면 clone이라 공간을 거의 쓰지 않는다)
    pub library: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Outcome {
    pub project: String,
    pub project_dir: String,
    pub made_at: String,
    pub copied_files: u64,
    pub copied_bytes: u64,
    /// 사본 hash가 inventory와 같다
    pub verified: u64,
    /// inventory에 hash가 없어(크기 상한) 사본 hash만 남긴 파일
    pub unhashed: u64,
    /// inventory 뒤 원본이 바뀌었다(사본은 지금 원본과 같다)
    pub changed: Vec<String>,
    /// inventory 뒤 새로 생긴 파일(복사했다)
    pub added: Vec<String>,
    /// 일부러 고쳐 넣은 파일(까닭)
    pub rewritten: Vec<(String, String)>,
    /// 복사하지 않은 파일·폴더(inventory와 같은 「뺀 곳」 규칙)
    pub excluded: Vec<(String, String)>,
    pub failed: Vec<(String, String)>,
    pub threads_imported: Vec<String>,
    /// 넣지 않은 thread(id, 까닭)
    pub threads_skipped: Vec<(String, String)>,
    pub inbox_open: u64,
    pub inbox_done: u64,
    /// 옮기지 않은 설정(키, 값, 까닭)
    pub settings_not_mapped: Vec<(String, String, String)>,
    pub warnings: Vec<String>,
}

/// 0.2 PROJECT.md 설정 → aaa에서 갈 곳. 같은 것이 없으면 까닭(차이 목록 4장 G「프로젝트 설정 바꾸기」).
fn setting_note(key: &str) -> Option<&'static str> {
    Some(match key {
        "name" => "aaa 프로젝트는 폴더 이름(slug) 하나다. 표시 이름 없음",
        "nudge" => "aaa nudge는 host.toml nudge_idle_secs로 늘 켜져 있다(3장 G6 `nudge` 설정 옮길 예정)",
        "mute" => "aaa 알림은 기기별 설정이다(D4). 프로젝트 mute 없음",
        "coordinator_profile" => "aaa는 coordinator를 띄우지 않는다(사람이 프로젝트 폴더에서 agent를 띄운다)",
        "paused" | "archived" => "aaa에 멈춤·보관이 없다(4장 G「멈추기·보관」 옮길 예정)",
        _ => return None,
    })
}

fn front_value(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// routine front matter의 `enabled`를 false로 바꾼다(나머지 글자는 그대로)
pub fn disable_routine(text: &str) -> Option<String> {
    let (fm, body) = source::front_matter(text);
    let fm = fm?;
    let mut doc: toml_edit::DocumentMut = fm.parse().ok()?;
    doc["enabled"] = toml_edit::value(false);
    Some(format!("+++\n{}+++\n{body}", doc))
}

fn host_running(state_root: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::connect(state_root.join("host").join("front.sock")).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = state_root;
        false
    }
}

fn kind_of(t: &HpThread) -> ThreadKind {
    if t.kind == "worktree" { ThreadKind::Worktree } else { ThreadKind::Dir }
}

fn opt(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

/// 0.2 thread 기록 → aaa 기록(끝난 thread). `report`는 집으로 복사된 보고 사본(threads/<id>.md) 내용이다.
pub(crate) fn thread_rec(
    project: &str,
    t: &HpThread,
    machine_id: &str,
    report: Option<&[u8]>,
    now_ms: u64,
) -> ThreadRec {
    // aaa의 report_hash는 host가 가진 사본의 hash다. 0.2 기록의 hash와 사본이 같을 때만 확인(ack) 상태를 옮긴다.
    let copy_hash = report.map(|b| hex::encode(<sha2::Sha256 as sha2::Digest>::digest(b)));
    let acked = !t.acked_report_hash.is_empty() && t.acked_report_hash == t.report_hash;
    let next = report.map(|b| next_lines(&String::from_utf8_lossy(b))).unwrap_or_default();
    let created = aaa_common::util::parse_iso(&t.created).unwrap_or(now_ms);
    ThreadRec {
        project: project.to_string(),
        id: t.id.clone(),
        title: t.title.clone(),
        machine_id: machine_id.to_string(),
        profile: t.profile.clone(),
        role: Role::Worker,
        kind: kind_of(t),
        repo: opt(&t.repo),
        base: opt(&t.base),
        branch: opt(&t.branch),
        pr: None,
        pr_state: None,
        pr_review: None,
        cwd: opt(&t.cwd),
        work_dir: opt(&t.thread_dir),
        pane: None,
        status: ThreadStatus::Resolved,
        brief: BriefState::Delivered,
        brief_attempts: 0,
        delivered_at_ms: None,
        delivered_report: None,
        launch: None,
        acked_hash: if acked && copy_hash.as_deref() == Some(t.report_hash.as_str()) {
            copy_hash.clone()
        } else {
            None
        },
        report_hash: copy_hash,
        next,
        copy: report.map(|_| "complete".to_string()),
        copy_notes: vec![],
        last_group: Some(ThreadGroup::Resolved),
        review_item_hash: None,
        group_changed_ms: None,
        report_changed_ms: None,
        merged_seen_ms: None,
        error: opt(&t.error),
        eval_cursor: 0,
        eval_batches: 0,
        eval_last_batch_ms: None,
        created_at_ms: created,
        updated_at_ms: aaa_common::util::parse_iso(&t.updated).unwrap_or(created),
    }
}

struct Copier<'a> {
    src: &'a Path,
    dst: &'a Path,
    by_path: BTreeMap<&'a str, &'a FileEntry>,
    out: &'a mut Outcome,
}

impl Copier<'_> {
    /// 원본 `rel`을 대상 `to`(프로젝트 폴더 기준)로 복사하고 대조한다
    fn copy(&mut self, rel: &str, to: &str) {
        let from = self.src.join(rel);
        let dest = self.dst.join(to);
        let res = (|| -> std::io::Result<(u64, String)> {
            if let Some(d) = dest.parent() {
                std::fs::create_dir_all(d)?;
            }
            // macOS APFS에서 std::fs::copy는 clone(fclonefileat)을 먼저 쓴다
            let n = std::fs::copy(&from, &dest)?;
            Ok((n, sha256_file(&dest)?))
        })();
        match res {
            Ok((n, got)) => {
                self.out.copied_files += 1;
                self.out.copied_bytes += n;
                match self.by_path.get(rel).map(|e| e.sha256.as_deref()) {
                    Some(Some(want)) if want == got => self.out.verified += 1,
                    Some(Some(_)) => self.out.changed.push(rel.to_string()),
                    Some(None) => self.out.unhashed += 1,
                    None => self.out.added.push(rel.to_string()),
                }
            }
            Err(e) => self.out.failed.push((rel.to_string(), e.to_string())),
        }
    }

    /// 원본 폴더 아래 파일을 모두(symlink는 빼고) 같은 자리로
    fn copy_tree(&mut self, rel_dir: &str) {
        let mut stack = vec![PathBuf::from(rel_dir)];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(self.src.join(&d)) else { continue };
            let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let rel = d.join(e.file_name());
                let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
                let r = rel.to_string_lossy().replace('\\', "/");
                if let Some(why) = super::inventory::exclusion(&r, m.is_dir()) {
                    self.out.excluded.push((r, why.to_string()));
                    continue;
                }
                if m.is_dir() {
                    stack.push(rel);
                } else if m.is_file() {
                    self.copy(&r, &r);
                }
            }
        }
    }
}

pub fn convert(inv: &Inventory, dest: &Dest, now_ms: u64) -> Result<Outcome> {
    if !aaa_proto::valid_project(&dest.project) {
        bail!("aaa 프로젝트 이름 형식이 아니다(영문 소문자·숫자·-·_): {}", dest.project);
    }
    let src_root = PathBuf::from(&inv.root);
    let p = source::read(&src_root, &inv.project)?;
    let dir = dest.projects_root.join(&dest.project);
    if dir.exists() {
        bail!("대상 프로젝트 폴더가 이미 있다(덮어쓰지 않는다): {}", dir.display());
    }
    if host_running(&dest.state_root) {
        bail!(
            "이 상태 폴더의 host가 떠 있다({}). host를 멈춘 뒤 하거나 다른 상태 폴더를 쓴다",
            dest.state_root.join("host").display()
        );
    }
    let host_dir = dest.state_root.join("host");
    aaa_common::util::ensure_private_dir(&host_dir)?;
    let db = Db::open(&host_dir.join("host.db")).context("host DB")?;
    let machines: BTreeMap<String, String> = db.machines()?.into_iter().map(|m| (m.name, m.id)).collect();

    let mut out = Outcome {
        project: dest.project.clone(),
        project_dir: dir.to_string_lossy().to_string(),
        made_at: aaa_common::util::utc_iso(now_ms),
        ..Outcome::default()
    };
    std::fs::create_dir_all(&dir)?;
    let mut c = Copier {
        src: &p.dir,
        dst: &dir,
        by_path: inv.files.iter().map(|f| (f.path.as_str(), f)).collect(),
        out: &mut out,
    };
    for f in ["MEMORY.md", "TASKS.md", "later.md"] {
        if p.dir.join(f).is_file() {
            c.copy(f, f);
        }
    }
    if p.dir.join("AGENTS.md").is_file() {
        c.copy("AGENTS.md", "imported/AGENTS.herdr.md");
    }
    for d in ["memory", "uploads", "scratch", "tools"] {
        c.copy_tree(d);
    }
    // 끝난 thread: 보고·task·Follow-ups 사본과 library
    let history: Vec<&HpThread> = p.threads.iter().filter(|t| t.status == "resolved").collect();
    for t in &history {
        for ext in ["md", "task.md", "next.md"] {
            let rel = format!("threads/{}.{ext}", t.id);
            if p.dir.join(&rel).is_file() {
                c.copy(&rel, &rel);
            }
        }
        if dest.library && p.library_dir(&t.id).is_dir() {
            c.copy_tree(&format!("library/{}", t.id));
        }
    }
    // inbox 투영 파일은 0.2.35와 aaa가 같은 형식이라 그대로 둔다
    for i in &p.inbox {
        let rel = if i.done { format!("inbox/done/{}.md", i.id) } else { format!("inbox/{}.md", i.id) };
        if p.dir.join(&rel).is_file() {
            c.copy(&rel, &rel);
        }
    }
    drop(c);
    // routine: 꺼서 넣는다
    for e in std::fs::read_dir(p.dir.join("routines")).into_iter().flatten().filter_map(|e| e.ok()) {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "md") || !path.is_file() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let rel = format!("routines/{name}");
        match std::fs::read_to_string(&path).ok().and_then(|t| disable_routine(&t)) {
            Some(text) => match write_new(&dir.join(&rel), text.as_bytes()) {
                Ok(()) => out.rewritten.push((rel, "enabled = false(전환 때 켠다)".into())),
                Err(e) => out.failed.push((rel, e.to_string())),
            },
            None => out.failed.push((rel, "front matter를 읽지 못했다".into())),
        }
    }
    // 설정: 원본 그대로 보존하고, 가는 곳이 없는 키는 까닭을 남긴다
    let settings_text = toml::to_string(&p.settings).unwrap_or_default();
    write_new(&dir.join("imported/PROJECT.herdr.toml"), settings_text.as_bytes())?;
    let mut project_toml = String::from("schema = 1\n");
    for (k, v) in &p.settings {
        match k.as_str() {
            // project.toml에서 같은 이름·같은 뜻이다
            "start_threads" | "max_parallel_threads" | "auto_resolve_days" | "goal" | "thread_profile" | "repos" => {
                project_toml.push_str(&format!("{k} = {v}\n"))
            }
            _ => {
                let note = setting_note(k).unwrap_or("aaa에 같은 설정이 없다");
                out.settings_not_mapped.push((k.clone(), front_value(v), note.to_string()));
            }
        }
    }
    write_new(&dir.join("project.toml"), project_toml.as_bytes())?;

    // DB: 끝난 thread 기록
    let mut unknown_machines: BTreeMap<String, usize> = BTreeMap::new();
    for t in &history {
        let machine = if t.is_remote() { t.machine.clone() } else { inv.home_machine.clone() };
        let machine_id = match machines.get(&machine) {
            Some(id) => id.clone(),
            None => {
                *unknown_machines.entry(machine.clone()).or_default() += 1;
                machine.clone()
            }
        };
        let report = std::fs::read(dir.join(format!("threads/{}.md", t.id))).ok();
        let rec = thread_rec(&dest.project, t, &machine_id, report.as_deref(), now_ms);
        if db.thread_import(&dest.project, &t.id, rec.status.as_str(), &serde_json::to_string(&rec)?)? {
            out.threads_imported.push(t.id.clone());
        } else {
            out.threads_skipped.push((t.id.clone(), "이 host DB에 같은 ID가 이미 있다".into()));
        }
    }
    // 넣었다고 센 것과 DB에 실제로 있는 것이 같은지 다시 읽어 본다
    let in_db = db.threads(Some(&dest.project))?.len();
    if in_db != out.threads_imported.len() {
        bail!("DB의 thread 기록 수({in_db})가 넣은 수({})와 다르다", out.threads_imported.len());
    }
    for (m, n) in unknown_machines {
        out.warnings.push(format!("머신 {m}이 이 host에 등록돼 있지 않다: thread {n}개에 이름을 그대로 기록했다"));
    }
    for t in p.threads.iter().filter(|t| t.status != "resolved") {
        let why = match t.status.as_str() {
            "open" => "열린 thread: herdr가 끝까지 맡는다(재개 정보는 inventory)",
            "failed" => "실패한 thread: herdr에서 사람이 정리한다",
            _ => "시작 중 thread: herdr가 맡는다",
        };
        out.threads_skipped.push((t.id.clone(), why.into()));
    }
    // DB: inbox
    for i in &p.inbox {
        let rel = if i.done { format!("inbox/done/{}.md", i.id) } else { format!("inbox/{}.md", i.id) };
        let created = aaa_common::util::parse_iso(&i.created).unwrap_or(now_ms);
        let done_at = i.done.then(|| {
            std::fs::metadata(p.dir.join(&rel))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(created, |d| d.as_millis() as u64)
        });
        let item = InboxItem {
            id: i.id.clone(),
            project: dest.project.clone(),
            item_kind: i.kind.clone(),
            subject: i.subject.clone(),
            event: i.event.clone(),
            summary: i.summary.clone(),
            body: i.body.clone(),
            done: i.done,
            // 끝내지 않은 항목은 0.2 coordinator가 이미 알림을 받았다. aaa에서 다시 알리지 않는다
            seen: true,
            created_at_ms: created,
            done_at_ms: done_at,
        };
        db.inbox_put(&item)?;
        if i.done {
            out.inbox_done += 1;
        } else {
            out.inbox_open += 1;
        }
    }
    let detail = format!(
        "herdr-projects `{}`에서 옮김: thread {}개(끝난 것), inbox {}개, 파일 {}개",
        inv.project,
        out.threads_imported.len(),
        out.inbox_done + out.inbox_open,
        out.copied_files
    );
    db.flow_insert(&FlowEvent {
        project: dest.project.clone(),
        seq: 0,
        at_ms: now_ms,
        boot_id: aaa_common::util::boot_id(),
        kind: "project_imported".into(),
        thread: None,
        role: None,
        profile: None,
        machine: None,
        evidence: Some("imported/convert.json".into()),
        detail: detail.clone(),
    })?;
    db.audit("migrate", "project_import", &detail);
    // 마지막: aaa 뼈대(없는 파일만)와 PROJECT.md(이게 생겨야 프로젝트로 보인다)
    let instructions = format!("# {}\n\n{}", dest.project, p.instructions.trim_start());
    write_skeleton(&dir, &dest.project, &instructions)?;
    write_new(&dir.join("imported/convert.json"), serde_json::to_string_pretty(&out)?.as_bytes())?;
    Ok(out)
}

pub fn render_md(o: &Outcome) -> String {
    let mut s = format!("# 옮기기 결과: {}\n\n- 시각: {}\n- 폴더: `{}`\n\n", o.project, o.made_at, o.project_dir);
    s.push_str(&format!(
        "## 요약\n\n| 항목 | 값 |\n|---|---|\n| 복사 | {}개 · {} |\n| inventory hash와 같음 | {}개 |\n| hash 없던 파일(크기 상한) | {}개 |\n| inventory 뒤 바뀜 | {}개 |\n| inventory 뒤 생김 | {}개 |\n| 고쳐 넣음 | {}개 |\n| 뺀 곳(복사 안 함) | {}개 |\n| **실패** | **{}개** |\n| thread 넣음 | {}개 |\n| thread 넣지 않음 | {}개 |\n| inbox(끝냄·열림) | {}·{} |\n\n",
        o.copied_files,
        human_bytes(o.copied_bytes),
        o.verified,
        o.unhashed,
        o.changed.len(),
        o.added.len(),
        o.rewritten.len(),
        o.excluded.len(),
        o.failed.len(),
        o.threads_imported.len(),
        o.threads_skipped.len(),
        o.inbox_done,
        o.inbox_open
    ));
    let list = |s: &mut String, title: &str, v: &[String]| {
        if !v.is_empty() {
            s.push_str(&format!("## {title}\n\n"));
            for x in v {
                s.push_str(&format!("- `{x}`\n"));
            }
            s.push('\n');
        }
    };
    if !o.failed.is_empty() {
        s.push_str("## 실패\n\n");
        for (p, e) in &o.failed {
            s.push_str(&format!("- `{p}`: {}\n", cell(e)));
        }
        s.push('\n');
    }
    list(&mut s, "inventory 뒤 바뀐 파일", &o.changed);
    list(&mut s, "inventory 뒤 생긴 파일", &o.added);
    if !o.rewritten.is_empty() {
        s.push_str("## 고쳐 넣은 파일\n\n");
        for (p, why) in &o.rewritten {
            s.push_str(&format!("- `{p}`: {why}\n"));
        }
        s.push('\n');
    }
    if !o.excluded.is_empty() {
        s.push_str("## 뺀 곳(복사하지 않음)\n\n");
        for (p, why) in &o.excluded {
            s.push_str(&format!("- `{p}`: {why}\n"));
        }
        s.push('\n');
    }
    if !o.settings_not_mapped.is_empty() {
        s.push_str("## 옮기지 않은 설정\n\n| 키 | 값 | 까닭 |\n|---|---|---|\n");
        for (k, v, why) in &o.settings_not_mapped {
            let v: String = v.chars().take(80).collect();
            s.push_str(&format!("| {k} | `{}` | {} |\n", cell(&v), cell(why)));
        }
        s.push('\n');
    }
    let skipped: BTreeMap<&str, Vec<&str>> = o.threads_skipped.iter().fold(BTreeMap::new(), |mut m, (id, why)| {
        m.entry(why.as_str()).or_insert_with(Vec::new).push(id.as_str());
        m
    });
    if !skipped.is_empty() {
        s.push_str("## 넣지 않은 thread\n\n");
        for (why, ids) in skipped {
            s.push_str(&format!("- {why}: {}\n", ids.join(", ")));
        }
        s.push('\n');
    }
    if !o.warnings.is_empty() {
        s.push_str("## 경고\n\n");
        for w in &o.warnings {
            s.push_str(&format!("- {}\n", cell(w)));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_is_disabled_and_body_kept() {
        let t = "+++\nschedule = \"every 10m\"\nenabled = true\n+++\n\n열린 thread를 본다.\n";
        let out = disable_routine(t).unwrap();
        assert!(out.contains("enabled = false") && !out.contains("enabled = true"), "{out}");
        assert!(out.contains("schedule = \"every 10m\"") && out.ends_with("\n열린 thread를 본다.\n"), "{out}");
        let no_key = disable_routine("+++\nschedule = \"daily 03:00\"\n+++\n본문\n").unwrap();
        assert!(no_key.contains("enabled = false"));
        assert!(disable_routine("front matter 없음").is_none());
    }

    #[test]
    fn ack_moves_only_when_copy_matches_record() {
        let report = b"[\xec\x99\x84\xeb\xa3\x8c] done\n\n## Next\n- Merge the PR\n";
        let h = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(report));
        let mut t = HpThread {
            id: "t-0007".into(),
            status: "resolved".into(),
            kind: "worktree".into(),
            report_hash: h.clone(),
            acked_report_hash: h.clone(),
            created: "2026-10-01T00:00:00Z".into(),
            ..HpThread::default()
        };
        let r = thread_rec("p", &t, "m1", Some(report), 0);
        assert_eq!(r.report_hash.as_deref(), Some(h.as_str()));
        assert_eq!(r.acked_hash.as_deref(), Some(h.as_str()));
        assert_eq!(r.next, ["Merge the PR"]);
        assert_eq!((r.status, r.kind, r.brief), (ThreadStatus::Resolved, ThreadKind::Worktree, BriefState::Delivered));
        assert_eq!(r.created_at_ms, 1_790_812_800_000);
        // 사본이 기록과 다르면(ticker가 아직 못 본 새 보고) 확인 상태를 옮기지 않는다
        let r2 = thread_rec("p", &t, "m1", Some(b"other"), 0);
        assert_eq!(r2.acked_hash, None);
        // 확인하지 않은 보고
        t.acked_report_hash.clear();
        assert_eq!(thread_rec("p", &t, "m1", Some(report), 0).acked_hash, None);
        // 보고 사본이 없는 thread
        let r3 = thread_rec("p", &t, "m1", None, 0);
        assert_eq!((r3.report_hash, r3.copy), (None, None));
    }
}

#[cfg(test)]
mod e2e {
    use super::*;
    use crate::migrate::inventory;

    fn put(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// 가짜 0.2 프로젝트를 inventory → convert 하고, 원본·대상·DB를 본다
    #[test]
    fn inventory_then_convert() {
        let base = std::env::temp_dir().join(format!("aaa-migrate-{}", aaa_common::util::new_id()));
        let root = base.join("hp");
        let src = root.join("demo");
        put(
            &src,
            "PROJECT.md",
            "+++\nname = \"Demo\"\ngoal = \"배포\"\nmax_parallel_threads = 2\nstart_threads = \"auto\"\nauto_resolve_days = 3\n\
             thread_profile = \"opus\"\ncoordinator_profile = \"opus\"\n\n[[repos]]\npath = \"/srv/app\"\nmachine = \"mac-a\"\n\n\
             [[repos]]\npath = \"/work/lib\"\n+++\n\n# 지침\n규칙\n",
        );
        put(&src, "AGENTS.md", "0.2 안내\n");
        put(&src, "MEMORY.md", "- [a](memory/a.md)\n");
        put(&src, "memory/a.md", "기억\n");
        put(&src, ".env", "TOKEN=secret\n");
        put(&src, "scratch/brief.md", "brief\n");
        put(&src, "scratch/k.pem", "-----BEGIN-----\n");
        put(&src, "routines/audit.md", "+++\nschedule = \"every 10m\"\nenabled = true\n+++\n점검\n");
        let report = "[완료] 끝\n\n## Next\n- 정리\n";
        let h = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(report.as_bytes()));
        put(
            &src,
            "threads/t-0001.toml",
            &format!(
                "id = \"t-0001\"\ntitle = \"끝난 일\"\nstatus = \"resolved\"\nkind = \"worktree\"\nprofile = \"opus-high\"\n\
                 created = \"2026-10-01T00:00:00Z\"\nreport_hash = \"{h}\"\nacked_report_hash = \"{h}\"\n"
            ),
        );
        put(&src, "threads/t-0001.md", report);
        put(&src, "threads/t-0001.task.md", "할 일\n");
        put(&src, "library/t-0001/out.txt", "결과물\n");
        put(
            &src,
            "threads/t-0002.toml",
            "id = \"t-0002\"\ntitle = \"열린 일\"\nstatus = \"open\"\nkind = \"worktree\"\nmachine = \"mac-b\"\nagent = \"claude\"\n\
             pane_id = \"w1:p1\"\ncwd = \"/w\"\nprofile = \"opus-high\"\nprofile_args = [\"--effort\", \"high\"]\n",
        );
        put(
            &src,
            "inbox/done/20261001T000000Z-thread-state-t-0001-1.md",
            "+++\nid = \"20261001T000000Z-thread-state-t-0001-1\"\nkind = \"thread-state\"\nsubject = \"t-0001\"\ncreated = \"2026-10-01T00:00:00Z\"\nsummary = \"new report\"\nevent = \"new report\"\n+++\n",
        );
        put(
            &src,
            "inbox/20261002T000000Z-routine-audit-2.md",
            "+++\nid = \"20261002T000000Z-routine-audit-2\"\nkind = \"routine\"\nsubject = \"audit\"\ncreated = \"2026-10-02T00:00:00Z\"\nsummary = \"점검\"\nevent = \"due\"\n+++\n본문\n",
        );
        let snap = base.join("snap.json");
        std::fs::write(
            &snap,
            r#"{"result":{"snapshot":{"version":"0.9.5","agents":[{"agent":"claude","agent_status":"working","cwd":"/w","pane_id":"w1:p1","agent_session":{"value":"sess-2"}}],"panes":[]}}}"#,
        )
        .unwrap();
        let opts = inventory::Opts {
            hash_max: None,
            snapshots: vec![("mac-b".into(), snap)],
            home_machine: "mac-a".into(),
            source_version: Some("herdr-projects 0.2.35".into()),
            home_profiles: BTreeMap::new(),
            now_ms: 1_800_000_000_000,
        };
        let inv = inventory::build(&root, "demo", &opts).unwrap();
        assert!(inv.unreadable.is_empty(), "{:?}", inv.unreadable);
        assert!(inv.files.iter().all(|f| f.path != ".env" && f.path != "scratch/k.pem"), "뺀 곳은 목록에 없다");
        assert_eq!(inv.excluded.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), [".env", "scratch/k.pem"]);
        let open = inv.threads.iter().find(|t| t.id == "t-0002").unwrap();
        assert_eq!(open.plan, "herdr");
        let r = open.resume.as_ref().unwrap();
        assert_eq!(r.command.as_deref(), Some("claude --resume sess-2 --effort high"));
        assert_eq!((r.args_from.as_str(), r.live_state.as_deref()), ("record", Some("working")));
        assert_eq!((inv.inbox.open, inv.inbox.done), (1, 1));
        let md = inventory::render_md(&inv);
        assert!(md.contains("sess-2") && md.contains("비밀값"), "{md}");

        // inventory 뒤 원본 하나가 바뀐다
        put(&src, "memory/a.md", "기억(고침)\n");
        let dest = Dest {
            state_root: base.join("state"),
            projects_root: base.join("projects"),
            project: "demo".into(),
            library: true,
        };
        let o = convert(&inv, &dest, 1_800_000_000_000).unwrap();
        assert!(o.failed.is_empty(), "{:?}", o.failed);
        assert_eq!(o.changed, ["memory/a.md"]);
        assert!(o.added.is_empty(), "{:?}", o.added);
        assert_eq!(o.excluded.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["scratch/k.pem"]);
        assert_eq!(o.threads_imported, ["t-0001"]);
        assert_eq!((o.inbox_open, o.inbox_done), (1, 1));
        let dir = base.join("projects/demo");
        assert!(!dir.join(".env").exists() && !dir.join("scratch/k.pem").exists());
        assert_eq!(std::fs::read_to_string(dir.join("library/t-0001/out.txt")).unwrap(), "결과물\n");
        assert!(std::fs::read_to_string(dir.join("routines/audit.md")).unwrap().contains("enabled = false"));
        // 같은 이름·같은 뜻인 설정은 project.toml로 옮기고, aaa가 그대로 읽는다
        assert!(std::fs::read_to_string(dir.join("project.toml")).unwrap().starts_with("schema = 1\n"));
        let set = crate::project::ProjectSettings::load(&dir);
        assert_eq!(
            (set.goal.as_str(), set.max_parallel_threads, set.start_threads.as_str(), set.auto_resolve_days),
            ("배포", 2, "auto", 3)
        );
        assert_eq!(set.thread_profile, "opus");
        assert_eq!(
            set.repos,
            [
                crate::project::Repo { path: "/srv/app".into(), machine: Some("mac-a".into()) },
                crate::project::Repo { path: "/work/lib".into(), machine: None }
            ]
        );
        let not: Vec<&str> = o.settings_not_mapped.iter().map(|(k, _, _)| k.as_str()).collect();
        assert_eq!(not, ["coordinator_profile", "name"], "{not:?}");
        assert!(std::fs::read_to_string(dir.join("PROJECT.md")).unwrap().starts_with("# demo\n\n# 지침\n규칙"));
        assert!(dir.join("AGENTS.md").is_file() && dir.join("imported/AGENTS.herdr.md").is_file());
        assert!(dir.join("MEMORY.md").is_file() && !dir.join("decisions.md").exists());
        // 원본은 그대로
        assert!(src.join(".env").is_file() && src.join("routines/audit.md").is_file());
        assert!(std::fs::read_to_string(src.join("routines/audit.md")).unwrap().contains("enabled = true"));
        // DB
        let db = Db::open(&base.join("state/host/host.db")).unwrap();
        let rec: ThreadRec = serde_json::from_str(&db.thread_get("demo", "t-0001").unwrap().unwrap()).unwrap();
        assert_eq!((rec.status, rec.acked_hash.as_deref()), (ThreadStatus::Resolved, Some(h.as_str())));
        assert_eq!(rec.machine_id, "mac-a", "등록 안 된 머신은 이름을 그대로");
        assert!(db.thread_get("demo", "t-0002").unwrap().is_none(), "열린 thread는 넣지 않는다");
        let items = db.inbox_list(Some("demo"), true).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.seen), "다시 알리지 않는다");
        assert!(db.flow_after(Some("demo"), 0, 10).unwrap().iter().any(|e| e.kind == "project_imported"));
        // 두 번 하면 거부한다(덮어쓰지 않는다)
        assert!(convert(&inv, &dest, 0).unwrap_err().to_string().contains("이미 있다"));
        // Windows는 열린 DB 파일을 지우지 못한다(os error 32)
        drop(db);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
