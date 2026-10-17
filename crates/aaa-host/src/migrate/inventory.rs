//! 읽기 전용 inventory(plan §15 1): 0.2 프로젝트 폴더의 모든 파일(분류·크기·sha256), thread별 옮김 계획,
//! 열린 thread의 재개 정보(herdr 세션 스냅샷과 pane·작업 폴더로 맞춘 agent 세션 ID), inbox·routine·설정.
//!
//! - 원본에는 쓰지 않는다. 결과는 `inventory.json`(기계용)과 `inventory.md`(사람용)다.
//! - 비밀값이 있을 수 있는 곳(`.env*`, `.herdr-web-ui/`, `.claude/`, `.git/`, 키·인증서 확장자)은 내용을 읽지 않고
//!   개수·크기만 「뺀 것」으로 남긴다. symlink는 따라가지 않는다(0.2.35 사본 규칙과 같다).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::source::{self, HpProject, HpThread, Snapshot};
use super::{SOURCE_PIN, cell, human_bytes, sha256_file};

pub const SCHEMA: u32 = 1;

pub struct Opts {
    /// 이보다 큰 파일은 hash하지 않는다(None이면 모두)
    pub hash_max: Option<u64>,
    /// (머신 이름, `herdr api snapshot` 출력 파일)
    pub snapshots: Vec<(String, PathBuf)>,
    /// projects root가 있는 머신 이름(record의 `machine`이 빈 thread가 여기서 돈다)
    pub home_machine: String,
    /// `herdr-projects --version` 출력(기록용)
    pub source_version: Option<String>,
    /// home 머신 profile 이름 → 인자(`herdr-projects profile resolve <name>`). 로컬 thread 기록에는 profile 인자가
    /// 저장되지 않는다(`remote_profile = false`)
    pub home_profiles: BTreeMap<String, Vec<String>>,
    pub now_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    pub path: String,
    pub category: String,
    pub size: u64,
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub symlink: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Count {
    pub files: u64,
    pub bytes: u64,
    pub hashed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Excluded {
    pub path: String,
    pub reason: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReportInfo {
    pub sha256: String,
    /// thread 기록의 `report_hash`와 같은가(다르면 ticker가 아직 새 보고를 못 봤다)
    pub matches_record: bool,
    pub state_tag: Option<String>,
    pub next: usize,
}

/// 열린 thread를 그 자리에서 이어 가려면 필요한 것(§15 4: 사용자가 정한 idle 시점에 명시적으로 재개)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resume {
    pub machine: String,
    pub cwd: String,
    pub agent: String,
    pub profile: String,
    pub args: Vec<String>,
    /// 인자의 출처: record(thread 기록, 원격 thread는 시작 때 해석한 값)·home-profile(지금 home 설정으로 해석)·none
    pub args_from: String,
    /// herdr가 본 harness 세션 ID(스냅샷이 없거나 agent를 못 찾으면 없음)
    pub session: Option<String>,
    /// herdr가 본 지금 상태(idle·working·blocked·done·unknown)
    pub live_state: Option<String>,
    /// 재개 명령(제안). herdr 0.9.5 복원과 같은 꼴: `claude --resume <id>`·`codex resume <id>` + profile 인자
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThreadEntry {
    pub id: String,
    pub title: String,
    pub status: String,
    pub machine: String,
    pub kind: String,
    pub profile: String,
    pub last_group: String,
    pub report: Option<ReportInfo>,
    /// 지금 보고를 확인(ack)했다
    pub acked: bool,
    pub library_files: u64,
    pub library_bytes: u64,
    /// history(끝난 일, 기록으로 가져온다)·herdr(herdr가 끝까지 맡는다)·check(실패·시작 중, 사람이 본다)
    pub plan: String,
    pub resume: Option<Resume>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InboxSummary {
    pub open: u64,
    pub done: u64,
    /// 끝내지 않은 항목(id, 종류, 대상, 한 줄)
    pub open_items: Vec<(String, String, String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoutineEntry {
    pub name: String,
    pub schedule: String,
    pub enabled: bool,
    /// `command`가 있다(aaa에서 명령 digest를 다시 승인해야 돈다)
    pub command: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentEntry {
    pub pane_id: String,
    pub agent: String,
    pub status: String,
    pub cwd: String,
    pub session: Option<String>,
    /// thread·coordinator·other(이 프로젝트 밖의 사람 세션)
    pub role: String,
    pub thread: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionEntry {
    pub machine: String,
    pub herdr_version: String,
    pub workspaces: usize,
    pub panes: usize,
    pub agents: Vec<AgentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Inventory {
    pub schema: u32,
    pub made_at: String,
    pub pin: String,
    pub source_version: Option<String>,
    pub root: String,
    pub project: String,
    pub home_machine: String,
    /// PROJECT.md front matter
    pub settings: serde_json::Value,
    pub counts: BTreeMap<String, Count>,
    pub files: Vec<FileEntry>,
    pub excluded: Vec<Excluded>,
    pub threads: Vec<ThreadEntry>,
    pub inbox: InboxSummary,
    pub routines: Vec<RoutineEntry>,
    pub sessions: Vec<SessionEntry>,
    pub unreadable: Vec<(String, String)>,
}

/// 내용을 읽지 않는 곳과 까닭(convert도 같은 규칙으로 복사하지 않는다)
pub fn exclusion(rel: &str, is_dir: bool) -> Option<&'static str> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if name == ".env" || name.starts_with(".env.") {
        return Some("비밀값(환경 변수·token)");
    }
    if is_dir {
        return match rel {
            ".git" => Some("git 이력(프로젝트 폴더 자체 백업, aaa로 옮기지 않음)"),
            ".herdr-web-ui" => Some("herdr-web-ui 기기·설정 기록(비밀값이 있을 수 있음)"),
            ".claude" => Some("agent 설정 폴더"),
            _ => None,
        };
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("p12" | "pfx" | "key" | "pem" | "cer" | "keychain" | "keychain-db") => Some("키·인증서"),
        _ => None,
    }
}

/// 파일 분류(맨 앞 경로)
pub fn category(rel: &str) -> &'static str {
    let first = rel.split('/').next().unwrap_or(rel);
    match first {
        "PROJECT.md" | "AGENTS.md" | "MEMORY.md" | "TASKS.md" | "later.md" | ".gitignore" => "project",
        "memory" => "memory",
        "routines" => "routines",
        "threads" => "threads",
        "inbox" if rel.starts_with("inbox/done/") => "inbox-done",
        "inbox" => "inbox",
        "library" => "library",
        "uploads" => "uploads",
        "scratch" => "scratch",
        "tools" => "tools",
        ".state" | "state" => "state",
        _ => "other",
    }
}

fn dir_size(dir: &Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().filter_map(|e| e.ok()) {
            let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else {
                files += 1;
                bytes += m.len();
            }
        }
    }
    (files, bytes)
}

/// 프로젝트 폴더를 걷는다: (파일 목록, 뺀 것, 읽지 못한 것)
fn walk(dir: &Path, hash_max: Option<u64>) -> (Vec<FileEntry>, Vec<Excluded>, Vec<(String, String)>) {
    let mut files = Vec::new();
    let mut excluded = Vec::new();
    let mut unreadable = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel_dir) = stack.pop() {
        let mut entries: Vec<_> = match std::fs::read_dir(dir.join(&rel_dir)) {
            Ok(r) => r.filter_map(|e| e.ok()).collect(),
            Err(e) => {
                unreadable.push((rel_dir.to_string_lossy().to_string(), e.to_string()));
                continue;
            }
        };
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let rel_path = rel_dir.join(e.file_name());
            let rel = rel_path.to_string_lossy().replace('\\', "/");
            let Ok(meta) = std::fs::symlink_metadata(e.path()) else {
                unreadable.push((rel, "stat 실패".into()));
                continue;
            };
            if let Some(reason) = exclusion(&rel, meta.is_dir()) {
                let (n, b) = if meta.is_dir() { dir_size(&e.path()) } else { (1, meta.len()) };
                excluded.push(Excluded { path: rel, reason: reason.into(), files: n, bytes: b });
                continue;
            }
            if meta.is_dir() {
                stack.push(rel_path);
                continue;
            }
            let symlink = meta.file_type().is_symlink();
            let sha256 = if symlink || hash_max.is_some_and(|m| meta.len() > m) {
                None
            } else {
                match sha256_file(&e.path()) {
                    Ok(h) => Some(h),
                    Err(err) => {
                        unreadable.push((rel.clone(), err.to_string()));
                        None
                    }
                }
            };
            files.push(FileEntry {
                category: category(&rel).into(),
                path: rel,
                size: if symlink { 0 } else { meta.len() },
                sha256,
                symlink,
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    excluded.sort_by(|a, b| a.path.cmp(&b.path));
    (files, excluded, unreadable)
}

fn plan_of(t: &HpThread) -> &'static str {
    match t.status.as_str() {
        "resolved" => "history",
        "open" => "herdr",
        _ => "check",
    }
}

/// 재개 명령(제안): herdr 0.9.5가 서버 재시작 뒤 복원하는 꼴에 profile 인자를 붙인다
pub fn resume_command(agent: &str, session: &str, args: &[String]) -> Option<String> {
    let quoted: Vec<String> = args
        .iter()
        .map(|a| {
            if a.chars().all(|c| c.is_ascii_alphanumeric() || "-_=./:@".contains(c)) {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect();
    let base = match agent {
        "claude" => format!("claude --resume {session}"),
        "codex" => format!("codex resume {session}"),
        _ => return None,
    };
    Some(if quoted.is_empty() { base } else { format!("{base} {}", quoted.join(" ")) })
}

fn routine_entry(path: &Path) -> Option<RoutineEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let (fm, _) = source::front_matter(&text);
    let t: toml::Table = fm.and_then(|f| toml::from_str(f).ok()).unwrap_or_default();
    let schedule = t
        .get("schedule")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| t.get("on").and_then(|v| v.as_str()).map(|o| format!("on = {o}")))
        .unwrap_or_default();
    Some(RoutineEntry {
        name: path.file_stem()?.to_string_lossy().to_string(),
        schedule,
        enabled: t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        command: t.contains_key("command"),
    })
}

pub fn build(root: &Path, slug: &str, opts: &Opts) -> Result<Inventory> {
    let p: HpProject = source::read(root, slug)?;
    let (files, excluded, mut unreadable) = walk(&p.dir, opts.hash_max);
    unreadable.extend(p.unreadable.iter().cloned());
    let mut counts: BTreeMap<String, Count> = BTreeMap::new();
    for f in &files {
        let c = counts.entry(f.category.clone()).or_default();
        c.files += 1;
        c.bytes += f.size;
        c.hashed += u64::from(f.sha256.is_some());
    }
    let mut snaps: Vec<(String, Snapshot)> = Vec::new();
    for (machine, path) in &opts.snapshots {
        match std::fs::read_to_string(path).map_err(anyhow::Error::from).and_then(|t| source::parse_snapshot(&t)) {
            Ok(s) => snaps.push((machine.clone(), s)),
            Err(e) => unreadable.push((path.to_string_lossy().to_string(), format!("{e:#}"))),
        }
    }
    let machine_of = |t: &HpThread| if t.is_remote() { t.machine.clone() } else { opts.home_machine.clone() };
    let lib_bytes = |id: &str| {
        let prefix = format!("library/{id}/");
        files.iter().filter(|f| f.path.starts_with(&prefix)).fold((0u64, 0u64), |(n, b), f| (n + 1, b + f.size))
    };
    let threads: Vec<ThreadEntry> = p
        .threads
        .iter()
        .map(|t| {
            let report = std::fs::read(p.report_path(&t.id)).ok().map(|bytes| {
                let sha = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&bytes));
                let text = String::from_utf8_lossy(&bytes);
                let next = aaa_proto::next_lines(&text).len();
                ReportInfo { matches_record: sha == t.report_hash, sha256: sha, state_tag: state_tag(&text), next }
            });
            let (library_files, library_bytes) = lib_bytes(&t.id);
            let machine = machine_of(t);
            let resume = (t.status == "open").then(|| {
                let agent = snaps
                    .iter()
                    .filter(|(m, _)| *m == machine)
                    .flat_map(|(_, s)| s.agents.iter())
                    .find(|a| source::agent_matches(t, a));
                let session = agent.and_then(|a| a.agent_session.as_ref()).map(|s| s.value.clone());
                // 원격 thread는 시작 때 해석한 profile 인자를 기록에 둔다. 로컬은 두지 않으므로 지금 home 설정으로 푼다.
                let (mut args, args_from) = if !t.profile_args.is_empty() {
                    (t.profile_args.clone(), "record")
                } else if let Some(a) = opts.home_profiles.get(&t.profile).filter(|_| !t.is_remote()) {
                    (a.clone(), "home-profile")
                } else {
                    (Vec::new(), "none")
                };
                args.extend(t.agent_args.iter().cloned());
                Resume {
                    machine: machine.clone(),
                    cwd: t.cwd.clone(),
                    agent: t.agent.clone(),
                    profile: t.profile.clone(),
                    command: session.as_deref().and_then(|s| resume_command(&t.agent, s, &args)),
                    args,
                    args_from: args_from.into(),
                    session,
                    live_state: agent.map(|a| a.agent_status.clone()),
                }
            });
            ThreadEntry {
                id: t.id.clone(),
                title: t.title.clone(),
                status: t.status.clone(),
                machine,
                kind: t.kind.clone(),
                profile: t.profile.clone(),
                last_group: t.last_group.clone(),
                acked: !t.report_hash.is_empty() && t.report_hash == t.acked_report_hash,
                report,
                library_files,
                library_bytes,
                plan: plan_of(t).into(),
                resume,
            }
        })
        .collect();
    let open_items: Vec<_> = p
        .inbox
        .iter()
        .filter(|i| !i.done)
        .map(|i| (i.id.clone(), i.kind.clone(), i.subject.clone(), i.summary.clone()))
        .collect();
    let inbox = InboxSummary {
        open: open_items.len() as u64,
        done: p.inbox.iter().filter(|i| i.done).count() as u64,
        open_items,
    };
    let mut routines: Vec<RoutineEntry> = std::fs::read_dir(p.dir.join("routines"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|x| x.extension().is_some_and(|e| e == "md"))
        .filter_map(|x| routine_entry(&x))
        .collect();
    routines.sort_by(|a, b| a.name.cmp(&b.name));
    let project_dir = p.dir.to_string_lossy().to_string();
    let sessions = snaps
        .iter()
        .map(|(machine, s)| SessionEntry {
            machine: machine.clone(),
            herdr_version: s.version.clone(),
            workspaces: s.workspaces.len(),
            panes: s.panes.len(),
            agents: s
                .agents
                .iter()
                .map(|a| {
                    let thread = p.threads.iter().find(|t| machine_of(t) == *machine && source::agent_matches(t, a));
                    let coord = a.cwd == project_dir;
                    AgentEntry {
                        pane_id: a.pane_id.clone(),
                        agent: a.agent.clone(),
                        status: a.agent_status.clone(),
                        cwd: a.cwd.clone(),
                        session: a.agent_session.as_ref().map(|s| s.value.clone()),
                        role: if thread.is_some() {
                            "thread"
                        } else if coord {
                            "coordinator"
                        } else {
                            "other"
                        }
                        .into(),
                        thread: thread.map(|t| t.id.clone()),
                    }
                })
                .collect(),
        })
        .collect();
    Ok(Inventory {
        schema: SCHEMA,
        made_at: aaa_common::util::utc_iso(opts.now_ms),
        pin: SOURCE_PIN.into(),
        source_version: opts.source_version.clone(),
        root: root.to_string_lossy().to_string(),
        project: slug.into(),
        home_machine: opts.home_machine.clone(),
        settings: serde_json::to_value(&p.settings)?,
        counts,
        files,
        excluded,
        threads,
        inbox,
        routines,
        sessions,
        unreadable,
    })
}

/// 0.2 → aaa 대응(convert가 하는 일과 같다)
const MAPPING: &[(&str, &str, &str)] = &[
    ("PROJECT.md 본문(지침)", "PROJECT.md", "그대로 복사"),
    (
        "PROJECT.md 설정(+++ front matter)",
        "`imported/PROJECT.herdr.toml`",
        "기록으로 보존. aaa에 같은 설정이 없는 항목은 convert 결과에 까닭을 적는다",
    ),
    ("AGENTS.md(0.2 coordinator 안내)", "`imported/AGENTS.herdr.md`", "보존만. aaa 안내(AGENTS.md)는 aaa 것을 쓴다"),
    ("MEMORY.md·memory/·TASKS.md·later.md", "같은 이름", "그대로 복사(hash 확인)"),
    (
        "routines/*.md",
        "routines/*.md",
        "복사하되 `enabled = false`로 바꾼다(두 ticker가 같은 일을 두 번 하지 않게). 전환 때 켠다",
    ),
    (
        "threads/<id>.toml(끝난 thread)",
        "host DB `threads`(resolved)",
        "기록·보고 hash·확인 상태를 옮긴다. pane·세션은 없다",
    ),
    ("threads/<id>.md·.task.md·.next.md(끝난 thread)", "같은 이름", "그대로 복사(hash 확인)"),
    ("library/<id>/(끝난 thread)", "library/<id>/", "같은 APFS volume이면 clone 복사, 사본 hash를 inventory와 대조"),
    ("열린 thread", "가져오지 않음", "herdr가 끝까지 맡는다(소유 도구 하나). 재개 정보만 inventory에 남긴다"),
    ("inbox/done/*.md", "host DB `inbox`(done) + `inbox/done/`", "id·시각 그대로"),
    (
        "inbox/*.md(끝내지 않음)",
        "host DB `inbox`(열림, 봤음)",
        "전환 gate는 0개를 요구한다. 남았으면 다시 알리지 않게 「봤음」으로 둔다",
    ),
    ("uploads/·scratch/·tools/", "같은 이름", "그대로 복사(hash 확인)"),
    (".state/·state/", "가져오지 않음", "0.2 ticker 내부 상태(커서·잠금). aaa는 자기 DB를 쓴다"),
];

pub fn render_md(inv: &Inventory) -> String {
    let mut s = String::new();
    s.push_str(&format!("# inventory: {} (읽기 전용)\n\n", inv.project));
    s.push_str(&format!(
        "- 만든 시각: {}\n- 원본: `{}` · {}\n- 이 도구가 형식을 확인한 판: {}\n- 로컬 thread가 도는 머신: {}\n\n",
        inv.made_at,
        inv.root,
        inv.source_version.as_deref().unwrap_or("판 기록 없음"),
        inv.pin,
        inv.home_machine
    ));
    let total: u64 = inv.counts.values().map(|c| c.bytes).sum();
    let nfiles: u64 = inv.counts.values().map(|c| c.files).sum();
    let hashed: u64 = inv.counts.values().map(|c| c.hashed).sum();
    s.push_str(&format!(
        "## 요약\n\n파일 {nfiles}개 · {} · sha256 {hashed}개. 뺀 곳 {}개. 읽지 못한 것 {}개.\n\n",
        human_bytes(total),
        inv.excluded.len(),
        inv.unreadable.len()
    ));
    s.push_str("| 분류 | 파일 | 크기 | hash |\n|---|---:|---:|---:|\n");
    for (k, c) in &inv.counts {
        s.push_str(&format!("| {k} | {} | {} | {} |\n", c.files, human_bytes(c.bytes), c.hashed));
    }
    s.push_str("\n## 옮기는 곳\n\n| 0.2 | aaa | 방법 |\n|---|---|---|\n");
    for (a, b, c) in MAPPING {
        s.push_str(&format!("| {a} | {b} | {c} |\n"));
    }
    let by = |plan: &str| inv.threads.iter().filter(|t| t.plan == plan).collect::<Vec<_>>();
    let (hist, herdr, check) = (by("history"), by("herdr"), by("check"));
    s.push_str(&format!(
        "\n## thread {}개\n\n- 끝난 thread(기록으로 가져옴): {}개, 보고 {}개, library {}\n- 열린 thread(herdr가 맡음): {}개\n- 사람이 볼 것(실패·시작 중): {}개\n",
        inv.threads.len(),
        hist.len(),
        hist.iter().filter(|t| t.report.is_some()).count(),
        human_bytes(hist.iter().map(|t| t.library_bytes).sum()),
        herdr.len(),
        check.len()
    ));
    let stale: Vec<_> = inv.threads.iter().filter(|t| t.report.as_ref().is_some_and(|r| !r.matches_record)).collect();
    if !stale.is_empty() {
        s.push_str(&format!(
            "- 보고 사본 hash가 기록과 다른 thread {}개(사본이 기록보다 새것이거나 옛것): {}\n",
            stale.len(),
            stale.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    if !herdr.is_empty() || !check.is_empty() {
        s.push_str("\n### 열린 thread와 재개 정보\n\n인자 출처: record = thread 기록(시작 때 값), home-profile = 지금 home 설정으로 해석(시작 뒤 설정이 바뀌었으면 다를 수 있다), none = 못 찾음(재개 전에 사람이 확인).\n\n| thread | 머신 | 상태(기록·herdr) | profile | 세션 | 재개 명령(제안) | 인자 출처 |\n|---|---|---|---|---|---|---|\n");
        for t in herdr.iter().chain(check.iter()) {
            let r = t.resume.as_ref();
            s.push_str(&format!(
                "| {} {} | {} | {} · {} | {} | {} | {} | {} |\n",
                t.id,
                cell(&t.title),
                t.machine,
                t.last_group,
                r.and_then(|r| r.live_state.as_deref()).unwrap_or("못 찾음"),
                t.profile,
                r.and_then(|r| r.session.as_deref()).map(|x| format!("`{x}`")).unwrap_or_else(|| "없음".into()),
                r.and_then(|r| r.command.as_deref()).map(|x| format!("`{}`", cell(x))).unwrap_or_else(|| "-".into()),
                r.map(|r| r.args_from.as_str()).unwrap_or("-"),
            ));
        }
    }
    s.push_str(&format!("\n## inbox\n\n끝내지 않음 {} · 끝냄 {}\n", inv.inbox.open, inv.inbox.done));
    for (id, kind, subject, summary) in &inv.inbox.open_items {
        s.push_str(&format!("- `{id}` {kind} {subject}: {}\n", cell(summary)));
    }
    s.push_str("\n## routine\n\n| 이름 | 일정 | 켜짐 | 명령 |\n|---|---|---|---|\n");
    for r in &inv.routines {
        s.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            r.name,
            cell(&r.schedule),
            if r.enabled { "예" } else { "아니오" },
            if r.command { "있음(digest 다시 승인)" } else { "없음" }
        ));
    }
    if !inv.sessions.is_empty() {
        s.push_str("\n## herdr 세션\n\n| 머신 | herdr | workspace | pane | agent(thread·coordinator·그 밖) |\n|---|---|---:|---:|---|\n");
        for x in &inv.sessions {
            let n = |r: &str| x.agents.iter().filter(|a| a.role == r).count();
            s.push_str(&format!(
                "| {} | {} | {} | {} | {}·{}·{} |\n",
                x.machine,
                x.herdr_version,
                x.workspaces,
                x.panes,
                n("thread"),
                n("coordinator"),
                n("other")
            ));
        }
        s.push_str(
            "\n「그 밖」 agent는 이 프로젝트 thread가 아닌 사람 세션이다. aaa로 옮기지 않는다(herdr에서 계속 쓴다).\n",
        );
    }
    s.push_str("\n## 뺀 곳(내용을 읽지 않음)\n\n| 경로 | 까닭 | 파일 | 크기 |\n|---|---|---:|---:|\n");
    for e in &inv.excluded {
        s.push_str(&format!("| `{}` | {} | {} | {} |\n", e.path, e.reason, e.files, human_bytes(e.bytes)));
    }
    if !inv.unreadable.is_empty() {
        s.push_str("\n## 읽지 못한 것\n\n");
        for (p, e) in &inv.unreadable {
            s.push_str(&format!("- `{p}`: {}\n", cell(e)));
        }
    }
    s
}

/// 0.2 운영 보고 첫 줄의 상태 태그(`[완료]` 등). aaa는 태그로 묶음을 정하지 않는다(plan §8.78). 목록에 적기만 한다.
fn state_tag(report: &str) -> Option<String> {
    let first = report.lines().find(|l| !l.trim().is_empty())?.trim();
    first.strip_prefix('[').and_then(|r| r.split_once(']')).map(|(s, _)| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_and_categories() {
        assert!(exclusion(".env", false).is_some());
        assert!(exclusion(".env.local", false).is_some());
        assert!(exclusion("scratch/a.pem", false).is_some());
        assert!(exclusion(".herdr-web-ui", true).is_some());
        assert!(exclusion("scratch/.git", true).is_none(), "맨 위 .git만 뺀다");
        assert!(exclusion("memory/env.md", false).is_none());
        assert_eq!(category("inbox/done/x.md"), "inbox-done");
        assert_eq!(category("inbox/x.md"), "inbox");
        assert_eq!(category("threads/t-0001.toml"), "threads");
        assert_eq!(category("PROJECT.md"), "project");
        assert_eq!(category("zzz"), "other");
    }

    #[test]
    fn resume_command_quotes_args() {
        let args = vec!["--model".to_string(), "claude-opus-5-5[1m]".to_string(), "--effort".into(), "high".into()];
        assert_eq!(
            resume_command("claude", "s-1", &args).unwrap(),
            "claude --resume s-1 --model 'claude-opus-5-5[1m]' --effort high"
        );
        assert_eq!(resume_command("codex", "s-2", &[]).unwrap(), "codex resume s-2");
        assert_eq!(resume_command("droid", "s", &[]), None);
    }
}
