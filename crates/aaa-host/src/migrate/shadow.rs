//! shadow 비교(plan §15 2): `scripts/m4/shadow-capture.sh`가 같은 시각에 모은 원자료를 다시 읽어 표를 낸다.
//!
//! aaa host는 herdr pane을 보지 못한다(소유 도구가 다르다). 그래서 지금 운영 thread에 대해서는 같은 시각·같은 입력
//! (thread 기록, herdr agent 상태와 그 지속 시간, 진행 보고, 보고 사본)을 두 규칙에 넣어 비교한다.
//!
//! - `hp_cli`: herdr-projects CLI가 그때 낸 묶음(정답).
//! - `hp_replay`: 같은 입력에 herdr-projects `thread::group()`(1f05c00 src/thread.rs:565)을 옮긴 규칙. `hp_cli`와
//!   다르면 입력을 다 모으지 못한 것이라 그 줄의 aaa 차이는 판정하지 않는다.
//! - `aaa`: 같은 입력을 aaa 기록으로 바꿔 aaa `orch::thread_group`(실제 코드)에 넣은 결과.
//! - `aaa_live`: 원격 thread는 0.2 ticker가 약 1분마다 ssh로 본 상태를 쓴다. aaa는 그 머신 agent가 바로 알리므로,
//!   그 tick의 그 머신 herdr 스냅샷 상태를 넣은 결과를 따로 둔다.
//!
//! aaa thread(pilot 등)는 aaa host가 낸 묶음·harness 상태와 그 pane 화면 맨 아래 줄을 나란히 둔다.

use std::collections::BTreeMap;
use std::path::Path;

use aaa_proto::{AgentActivity, AgentStatus, BriefState, ThreadGroup, ThreadStatus};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::source::{self, HpThread, SnapAgent, Snapshot};
use super::{cell, write_new};
use crate::orch::{ThreadRec, thread_group};

/// herdr-projects `thread.rs` 상수
const STARTING_TIMEOUT_SECS: i64 = 300;
const BLOCKED_DEBOUNCE_SECS: i64 = 30;
const NOT_READY_SECS: i64 = 60;
/// `progress.rs`
const ACTIVITY_TTL_SECS: i64 = 300;
const WAITING: &str = "Waiting for you";

/// 진행 보고(`herdr-projects report`) 기록
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelfReport {
    pub pane_id: String,
    pub terminal_id: String,
    pub activity: String,
    pub percent: Option<u8>,
    pub reported_at: i64,
}

/// 두 규칙에 똑같이 넣는 입력
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Facts {
    pub status: String,
    pub created_s: i64,
    pub prompt_pending: bool,
    pub brief_stuck: bool,
    pub pane_exists: bool,
    pub report_hash: String,
    pub acked_hash: String,
    pub pr_state: String,
    pub pr_review: String,
    /// herdr agent 상태(idle·working·blocked·done·unknown). agent를 못 찾으면 None
    pub agent_state: Option<String>,
    pub state_secs: i64,
    /// 진행 보고(있으면 activity·percent·지난 초)
    pub self_activity: Option<String>,
    pub self_percent: Option<u8>,
    pub self_age_secs: Option<i64>,
}

impl Facts {
    fn self_waiting(&self) -> bool {
        self.self_activity.as_deref() == Some(WAITING) && self.agent_state.as_deref() != Some("working")
    }

    fn self_working(&self) -> bool {
        self.self_activity.is_some()
            && self.self_percent != Some(100)
            && self.self_activity.as_deref() != Some(WAITING)
            && self.self_age_secs.is_some_and(|a| a < ACTIVITY_TTL_SECS)
    }
}

/// herdr-projects `thread::group()`을 옮겼다(줄 번호 주석도 원본대로). 결과는 0.2 token.
pub fn hp_group(f: &Facts, now_s: i64) -> &'static str {
    let state = f.agent_state.as_deref();
    let has_report = !f.report_hash.is_empty();
    // 1
    if f.status == "resolved" {
        return "resolved";
    }
    // 2
    if f.status == "starting" {
        return if now_s - f.created_s < STARTING_TIMEOUT_SECS { "working" } else { "waiting-on-you" };
    }
    // 3
    let ready = |s: &str| matches!(s, "idle" | "done");
    let stuck_launch =
        f.prompt_pending && (f.brief_stuck || (state.is_some_and(|s| !ready(s)) && f.state_secs >= NOT_READY_SECS));
    if f.status == "failed" || (!f.pane_exists && !has_report) || stuck_launch {
        return "waiting-on-you";
    }
    // 4
    if (state == Some("blocked") && f.state_secs >= BLOCKED_DEBOUNCE_SECS) || f.self_waiting() {
        return "waiting-on-you";
    }
    // 5
    let pr_open = f.pr_state.eq_ignore_ascii_case("open");
    if pr_open && f.pr_review.eq_ignore_ascii_case("approved") {
        return "landing";
    }
    let new_report = has_report && f.report_hash != f.acked_hash;
    if (new_report || (has_report && pr_open)) && !f.prompt_pending && state != Some("working") {
        return "ready-for-review";
    }
    // 6
    if matches!(state, Some("working") | Some("blocked")) || f.prompt_pending || f.self_working() {
        return "working";
    }
    // 7
    "idle"
}

/// herdr agent 상태 → aaa harness 활동. `unknown`(셸·모르는 프로그램)은 aaa에서 「agent 없음」이다.
fn activity_of(state: &str) -> Option<AgentActivity> {
    match state {
        "working" => Some(AgentActivity::Working),
        "idle" | "done" => Some(AgentActivity::Idle),
        "blocked" => Some(AgentActivity::Question),
        _ => None,
    }
}

pub fn token(g: ThreadGroup) -> &'static str {
    match g {
        ThreadGroup::WaitingOnYou => "waiting-on-you",
        ThreadGroup::ReadyForReview => "ready-for-review",
        ThreadGroup::Landing => "landing",
        ThreadGroup::Working => "working",
        ThreadGroup::Idle => "idle",
        ThreadGroup::Resolved => "resolved",
    }
}

/// 같은 입력을 aaa 기록으로 바꿔 aaa 규칙에 넣는다
pub fn aaa_group(f: &Facts, now_s: i64) -> ThreadGroup {
    let now_ms = (now_s.max(0) as u64) * 1000;
    let rec = ThreadRec {
        project: String::new(),
        id: "t-0000".into(),
        title: String::new(),
        machine_id: String::new(),
        profile: String::new(),
        role: aaa_proto::Role::Worker,
        kind: aaa_proto::ThreadKind::Worktree,
        repo: None,
        base: None,
        branch: None,
        pr: None,
        pr_state: (!f.pr_state.is_empty()).then(|| f.pr_state.clone()),
        pr_review: (!f.pr_review.is_empty()).then(|| f.pr_review.clone()),
        cwd: None,
        work_dir: None,
        pane: None,
        status: ThreadStatus::parse(&f.status).unwrap_or(ThreadStatus::Open),
        // 0.2 prompt_pending = brief를 아직 못 넣음, brief_stuck = 넣었는지 모름(aaa는 다시 보내지 않는 unknown)
        brief: match (f.prompt_pending, f.brief_stuck) {
            (false, _) => BriefState::Delivered,
            (true, true) => BriefState::Unknown,
            (true, false) => BriefState::Pending,
        },
        brief_attempts: 0,
        delivered_at_ms: None,
        delivered_report: None,
        launch: None,
        report_hash: (!f.report_hash.is_empty()).then(|| f.report_hash.clone()),
        acked_hash: (!f.acked_hash.is_empty()).then(|| f.acked_hash.clone()),
        next: Vec::new(),
        copy: None,
        copy_notes: Vec::new(),
        last_group: None,
        review_item_hash: None,
        group_changed_ms: None,
        report_changed_ms: None,
        merged_seen_ms: None,
        error: None,
        eval_cursor: 0,
        eval_batches: 0,
        eval_last_batch_ms: None,
        leftover: None,
        created_at_ms: (f.created_s.max(0) as u64) * 1000,
        updated_at_ms: now_ms,
    };
    let agent = f.agent_state.as_deref().and_then(activity_of).map(|activity| AgentStatus {
        activity,
        since_ms: now_ms.saturating_sub((f.state_secs.max(0) as u64) * 1000),
        harness: String::new(),
        message: None,
        percent: f.self_percent,
        progress: f.self_activity.clone(),
        progress_at_ms: f.self_age_secs.map(|a| now_ms.saturating_sub((a.max(0) as u64) * 1000)),
        harness_session: None,
        // herdr `done`은 「끝남(안 봄)」이다. 묶음 규칙은 이 값을 보지 않는다
        seen: f.agent_state.as_deref() != Some("done"),
        plan: None,
        background: None,
    });
    thread_group(&rec, f.pane_exists, agent.as_ref(), now_ms)
}

/// 차이의 까닭(규칙을 읽고 정한 분류)
pub fn reason(f: &Facts, hp: &str, _aaa: &str) -> String {
    if f.agent_state.as_deref() == Some("unknown") {
        return "herdr `unknown`(셸·모르는 프로그램)을 aaa는 agent 없음으로 본다".into();
    }
    if f.agent_state.is_none() && f.self_activity.is_some() {
        return "agent 없이 진행 보고만 있다: aaa 상태는 harness 활동이 있어야 진행 보고를 담는다".into();
    }
    if hp == "ready-for-review" && f.pane_exists && f.self_age_secs.is_some() && f.self_percent != Some(100) {
        return "진행을 보고하는 agent의 중간 보고: aaa는 `Done`일 때만 검토 대기다(plan §8.85)".into();
    }
    "분류 안 됨(규칙을 다시 본다)".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub tick: String,
    pub at_s: i64,
    /// herdr(지금 운영 thread)·aaa(aaa thread)
    pub side: String,
    pub thread: String,
    pub title: String,
    pub machine: String,
    pub hp_cli: Option<String>,
    pub hp_replay: Option<String>,
    pub aaa: String,
    pub aaa_live: Option<String>,
    /// aaa thread: host가 본 harness 활동과 화면 맨 아래 줄
    pub aaa_activity: Option<String>,
    pub screen_tail: Vec<String>,
    /// 화면 단서(herdr claude.toml 표지 일부: `esc to interrupt` → working, `Do you want to proceed?` → blocked)
    pub screen_hint: Option<String>,
    pub facts: Option<Facts>,
    /// hp_cli와 aaa가 같다(입력이 재현될 때만 판정)
    pub agree: Option<bool>,
    pub reason: Option<String>,
}

fn read(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name))
        .ok()
        .filter(|_| std::fs::read_to_string(dir.join(format!("{name}.rc"))).map(|rc| rc.trim() == "0").unwrap_or(true))
}

/// `=== 이름` 줄로 나뉜 묶음
fn sections(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("=== ") {
            out.push((h.trim().to_string(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    out
}

fn secs_since(ts: &str, now_s: i64) -> i64 {
    aaa_common::util::parse_iso(ts).map_or(0, |ms| now_s - (ms / 1000) as i64)
}

/// 화면 단서(herdr `src/detect/manifests/claude.toml` live_turn_working·blocked 표지 일부만. 판정 엔진은 D8이 옮긴다)
pub fn screen_hint(lines: &[String]) -> Option<&'static str> {
    let bottom: Vec<&str> = lines.iter().rev().map(|l| l.trim()).filter(|l| !l.is_empty()).take(12).collect();
    let low: Vec<String> = bottom.iter().map(|l| l.to_lowercase()).collect();
    if low.iter().any(|l| l.contains("do you want to proceed?") || l.contains("esc to cancel")) {
        return Some("blocked");
    }
    if low.iter().any(|l| l.contains("esc to interrupt")) {
        return Some("working");
    }
    if bottom.iter().any(|l| l.starts_with('❯') || l.starts_with('›')) {
        return Some("idle");
    }
    None
}

pub struct Opts {
    /// projects root가 있는 머신(record `machine`이 빈 thread)
    pub home_machine: String,
}

/// tick 폴더 하나 → 줄들
pub fn analyze_tick(dir: &Path, opts: &Opts) -> Result<Vec<Row>> {
    let tick = dir.file_name().unwrap().to_string_lossy().to_string();
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("tick.json")).context("tick.json")?)?;
    let now_s = meta["started"].as_i64().unwrap_or(0);
    let mut rows = Vec::new();
    // 머신별 herdr 스냅샷·진행 보고
    let mut snaps: BTreeMap<String, Snapshot> = BTreeMap::new();
    let mut reports: BTreeMap<String, Vec<SelfReport>> = BTreeMap::new();
    for e in std::fs::read_dir(dir)?.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(m) = name.strip_prefix("herdr-").and_then(|n| n.strip_suffix(".json")) {
            if let Some(s) = read(dir, &name).and_then(|t| source::parse_snapshot(&t).ok()) {
                snaps.insert(m.to_string(), s);
            }
        } else if let Some(m) = name.strip_prefix("progress-").and_then(|n| n.strip_suffix(".txt"))
            && let Some(t) = read(dir, &name)
        {
            let v = sections(&t).into_iter().filter_map(|(_, b)| serde_json::from_str(&b).ok()).collect();
            reports.insert(m.to_string(), v);
        }
    }
    if let Some(text) = read(dir, "hp-threads.json") {
        let list: Vec<serde_json::Value> = serde_json::from_str(&text).context("hp-threads.json")?;
        for v in list {
            let t: HpThread = serde_json::from_value(v.clone()).unwrap_or_default();
            if t.status == "resolved" {
                continue;
            }
            let hp_cli = v["group_token"].as_str().map(str::to_string);
            let machine = if t.is_remote() { t.machine.clone() } else { opts.home_machine.clone() };
            let snap = snaps.get(&machine);
            let agent: Option<&SnapAgent> = snap.and_then(|s| s.agents.iter().find(|a| source::agent_matches(&t, a)));
            let mut f = Facts {
                status: t.status.clone(),
                created_s: aaa_common::util::parse_iso(&t.created).map_or(0, |ms| (ms / 1000) as i64),
                prompt_pending: t.prompt_pending,
                brief_stuck: t.brief_stuck,
                pane_exists: true,
                report_hash: t.report_hash.clone(),
                acked_hash: t.acked_report_hash.clone(),
                pr_state: t.pr_state.clone(),
                pr_review: t.pr_review.clone(),
                ..Facts::default()
            };
            let live_state = agent.map(|a| a.agent_status.clone());
            if t.is_remote() {
                // 0.2 CLI는 원격 thread를 ticker가 마지막에 본 기록으로 보인다(threads.rs row: 「the CLI makes no ssh call」)
                f.agent_state = (!t.last_state.is_empty()).then(|| t.last_state.clone());
                f.state_secs = secs_since(&t.last_state_change, now_s);
            } else if let Some(s) = snap {
                let foreign = agent.is_none() && s.agents.iter().any(|a| a.pane_id == t.pane_id) && t.kind != "adopted";
                f.pane_exists = (agent.is_some() || s.panes.iter().any(|p| source::pane_matches(&t, p))) && !foreign;
                f.agent_state = live_state.clone();
                f.state_secs = match &f.agent_state {
                    Some(st) if *st == t.last_state => secs_since(&t.last_state_change, now_s),
                    _ => 0,
                };
                let terminal = agent.map(|a| a.terminal_id.clone()).unwrap_or_default();
                if let Some(r) = reports.get(&machine).and_then(|v| {
                    v.iter().find(|r| {
                        r.pane_id == t.pane_id
                            && (terminal.is_empty() || r.terminal_id.is_empty() || r.terminal_id == terminal)
                            && r.reported_at > 0
                    })
                }) {
                    f.self_activity = Some(r.activity.clone());
                    f.self_percent = r.percent;
                    f.self_age_secs = Some(now_s - r.reported_at);
                }
            } else {
                // 스냅샷을 못 읽은 tick: 0.2 CLI도 「session unreachable」로 기록을 보인다
                f.agent_state = (!t.last_state.is_empty()).then(|| t.last_state.clone());
            }
            let hp_replay = hp_group(&f, now_s).to_string();
            let aaa = token(aaa_group(&f, now_s)).to_string();
            let aaa_live = if t.is_remote() {
                let mut lf = f.clone();
                lf.agent_state = live_state.clone();
                lf.state_secs = match &live_state {
                    Some(st) if *st == t.last_state => secs_since(&t.last_state_change, now_s),
                    _ => 0,
                };
                lf.pane_exists =
                    agent.is_some() || snap.is_some_and(|s| s.panes.iter().any(|p| source::pane_matches(&t, p)));
                Some(token(aaa_group(&lf, now_s)).to_string())
            } else {
                None
            };
            // 입력이 재현될 때만 aaa 차이를 판정한다. 보고 첫 줄(`hp-reports.txt`)은 aaa가 태그로 묶음을 정하지 않아
            // 더 읽지 않는다(plan §8.78).
            let reproduced = hp_cli.as_deref() == Some(hp_replay.as_str());
            let (agree, why) = match &hp_cli {
                Some(cli) if reproduced => {
                    let same = *cli == aaa;
                    (Some(same), (!same).then(|| reason(&f, cli, &aaa)))
                }
                Some(_) => (None, Some("입력 재현 안 됨".into())),
                None => (None, None),
            };
            rows.push(Row {
                tick: tick.clone(),
                at_s: now_s,
                side: "herdr".into(),
                thread: t.id.clone(),
                title: t.title.clone(),
                machine,
                hp_cli,
                hp_replay: Some(hp_replay),
                aaa,
                aaa_live,
                aaa_activity: live_state,
                screen_tail: Vec::new(),
                screen_hint: None,
                facts: Some(f),
                agree,
                reason: why,
            });
        }
    }
    // aaa thread
    if let Some(text) = read(dir, "aaa-threads.json") {
        let v: serde_json::Value = serde_json::from_str(&text).context("aaa-threads.json")?;
        let mut screens: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for e in std::fs::read_dir(dir)?.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("aaa-screens-")
                && name.ends_with(".txt")
                && let Some(t) = read(dir, &name)
            {
                for (h, body) in sections(&t) {
                    let id = h.split_whitespace().next().unwrap_or("").to_string();
                    let project = name.trim_start_matches("aaa-screens-").trim_end_matches(".txt");
                    screens.insert(format!("{project}/{id}"), body.lines().map(str::to_string).collect());
                }
            }
        }
        for t in v["threads"].as_array().into_iter().flatten() {
            if t["status"] == "resolved" {
                continue;
            }
            let key = format!("{}/{}", t["project"].as_str().unwrap_or(""), t["id"].as_str().unwrap_or(""));
            let screen = screens.get(&key).cloned().unwrap_or_default();
            let mut tail: Vec<String> = screen.iter().rev().filter(|l| !l.trim().is_empty()).take(4).cloned().collect();
            tail.reverse();
            rows.push(Row {
                tick: tick.clone(),
                at_s: now_s,
                side: "aaa".into(),
                thread: key,
                title: t["title"].as_str().unwrap_or("").into(),
                machine: t["machine"].as_str().unwrap_or("").into(),
                hp_cli: None,
                hp_replay: None,
                aaa: t["group"].as_str().unwrap_or("").replace('_', "-"),
                aaa_live: None,
                aaa_activity: t["agent"]["activity"].as_str().map(str::to_string),
                screen_hint: screen_hint(&screen).map(str::to_string),
                screen_tail: tail,
                facts: None,
                agree: None,
                reason: None,
            });
        }
    }
    Ok(rows)
}

/// 읽지 못한 tick(이름, 까닭)
pub type BadTicks = Vec<(String, String)>;

/// tick 폴더들(이름 순) → 모든 줄
pub fn analyze(ticks: &Path, opts: &Opts) -> Result<(Vec<Row>, BadTicks)> {
    let mut dirs: Vec<_> =
        std::fs::read_dir(ticks)?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    let mut rows = Vec::new();
    let mut bad = Vec::new();
    for d in dirs {
        match analyze_tick(&d, opts) {
            Ok(r) => rows.extend(r),
            Err(e) => bad.push((d.file_name().unwrap().to_string_lossy().to_string(), format!("{e:#}"))),
        }
    }
    Ok((rows, bad))
}

fn hhmm(tick: &str) -> String {
    // 20261017T150320Z → 10-17 15:03Z
    if tick.len() >= 13 {
        format!("{}-{} {}:{}Z", &tick[4..6], &tick[6..8], &tick[9..11], &tick[11..13])
    } else {
        tick.into()
    }
}

pub fn render_md(rows: &[Row], bad: &[(String, String)]) -> String {
    let ticks: Vec<&str> = {
        let mut t: Vec<&str> = rows.iter().map(|r| r.tick.as_str()).collect();
        t.dedup();
        t
    };
    let herdr: Vec<&Row> = rows.iter().filter(|r| r.side == "herdr").collect();
    let judged: Vec<&Row> = herdr.iter().copied().filter(|r| r.agree.is_some()).collect();
    let agree = judged.iter().filter(|r| r.agree == Some(true)).count();
    let mut s = String::from("# shadow 비교\n\n");
    s.push_str(&format!(
        "- tick {}개({} ~ {}), 읽지 못한 tick {}개\n- 지금 운영 thread 줄 {}개: 판정 {}개 중 같음 {}개, 다름 {}개, 판정 안 함 {}개(입력 재현 안 됨 등)\n- aaa thread 줄 {}개\n\n",
        ticks.len(),
        ticks.first().map(|t| hhmm(t)).unwrap_or_default(),
        ticks.last().map(|t| hhmm(t)).unwrap_or_default(),
        bad.len(),
        herdr.len(),
        judged.len(),
        agree,
        judged.len() - agree,
        herdr.len() - judged.len(),
        rows.len() - herdr.len()
    ));
    // 시간대(1시간)별
    let mut hours: BTreeMap<String, (usize, usize, usize, usize)> = BTreeMap::new();
    for r in &herdr {
        let h = hhmm(&r.tick)[..8].to_string() + ":00Z";
        let e = hours.entry(h).or_default();
        e.0 += 1;
        match r.agree {
            Some(true) => e.1 += 1,
            Some(false) => e.2 += 1,
            None => e.3 += 1,
        }
    }
    s.push_str("## 시간대별(지금 운영 thread, 0.2 CLI 묶음 대 aaa 규칙)\n\n| 시각(UTC) | 줄 | 같음 | 다름 | 판정 안 함 |\n|---|---:|---:|---:|---:|\n");
    for (h, (n, a, d, u)) in &hours {
        s.push_str(&format!("| {h} | {n} | {a} | {d} | {u} |\n"));
    }
    // 차이 묶음: (thread, 0.2, aaa, 까닭)별 처음·마지막 tick
    // (thread, 0.2, aaa, 까닭) → (처음 tick, 마지막 tick, 줄 수)
    type Key = (String, String, String, String);
    let mut eps: BTreeMap<Key, (String, String, usize)> = BTreeMap::new();
    for r in herdr.iter().filter(|r| r.agree == Some(false) || r.agree.is_none()) {
        let k = (
            r.thread.clone(),
            r.hp_cli.clone().unwrap_or_default(),
            if r.agree.is_none() { r.hp_replay.clone().unwrap_or_default() } else { r.aaa.clone() },
            r.reason.clone().unwrap_or_default(),
        );
        let e = eps.entry(k).or_insert((r.tick.clone(), r.tick.clone(), 0));
        e.1 = r.tick.clone();
        e.2 += 1;
    }
    s.push_str("\n## 다른 줄과 판정 안 한 줄\n\n「판정 안 함」은 3열이 0.2 규칙 재현값이다(0.2 CLI와 달라 입력이 모자랐다는 뜻).\n\n| thread | 0.2 CLI | aaa(또는 재현) | 까닭 | 처음 | 마지막 | 줄 |\n|---|---|---|---|---|---|---:|\n");
    for ((t, hp, aaa, why), (first, last, n)) in &eps {
        s.push_str(&format!("| {t} | {hp} | {aaa} | {} | {} | {} | {n} |\n", cell(why), hhmm(first), hhmm(last)));
    }
    // 원격 thread: 0.2 기록 상태 대 그 머신 지금 상태
    let lag: Vec<&Row> = herdr.iter().copied().filter(|r| r.aaa_live.as_ref().is_some_and(|l| *l != r.aaa)).collect();
    s.push_str(&format!(
        "\n## 원격 thread: 0.2 ticker 기록 대 그 머신 지금 상태\n\n원격 thread {}줄 중 {}줄은 그 머신의 지금 herdr 상태를 넣으면 aaa 묶음이 달라진다(0.2는 약 1분마다 ssh로 보고, aaa는 agent가 바로 알린다).\n\n",
        herdr.iter().filter(|r| r.aaa_live.is_some()).count(),
        lag.len()
    ));
    if !lag.is_empty() {
        s.push_str("| tick | thread | 기록 상태 → 묶음 | 지금 상태 → 묶음 |\n|---|---|---|---|\n");
        for r in lag.iter().take(40) {
            let f = r.facts.as_ref();
            s.push_str(&format!(
                "| {} | {} | {} → {} | {} → {} |\n",
                hhmm(&r.tick),
                r.thread,
                f.and_then(|f| f.agent_state.as_deref()).unwrap_or("-"),
                r.aaa,
                r.aaa_activity.as_deref().unwrap_or("못 찾음"),
                r.aaa_live.as_deref().unwrap_or("-")
            ));
        }
    }
    // aaa thread
    let aaa_rows: Vec<&Row> = rows.iter().filter(|r| r.side == "aaa").collect();
    if !aaa_rows.is_empty() {
        s.push_str("\n## aaa thread: host가 본 묶음·harness 활동 대 화면\n\n| tick | thread | 묶음 | harness | 화면 단서 | 화면 맨 아래 |\n|---|---|---|---|---|---|\n");
        for r in &aaa_rows {
            s.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                hhmm(&r.tick),
                r.thread,
                r.aaa,
                r.aaa_activity.as_deref().unwrap_or("-"),
                r.screen_hint.as_deref().unwrap_or("-"),
                cell(&r.screen_tail.last().cloned().unwrap_or_default().chars().take(60).collect::<String>())
            ));
        }
    }
    if !bad.is_empty() {
        s.push_str("\n## 읽지 못한 tick\n\n");
        for (t, e) in bad {
            s.push_str(&format!("- {t}: {}\n", cell(e)));
        }
    }
    s
}

pub fn write(out: &Path, rows: &[Row], bad: &[(String, String)]) -> Result<()> {
    let mut jsonl = String::new();
    for r in rows {
        jsonl.push_str(&serde_json::to_string(r)?);
        jsonl.push('\n');
    }
    write_new(&out.join("shadow.jsonl"), jsonl.as_bytes())?;
    write_new(&out.join("shadow.md"), render_md(rows, bad).as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn open() -> Facts {
        Facts { status: "open".into(), created_s: NOW - 3600, pane_exists: true, ..Facts::default() }
    }

    /// 0.2 규칙과 aaa 규칙이 같아야 하는 경우와 의도해서 다른 경우
    #[test]
    fn rules_agree_and_differ_where_intended() {
        let both = |f: &Facts| (hp_group(f, NOW), token(aaa_group(f, NOW)));
        let mut f = open();
        f.agent_state = Some("working".into());
        assert_eq!(both(&f), ("working", "working"));
        f.agent_state = Some("idle".into());
        assert_eq!(both(&f), ("idle", "idle"));
        // 새 보고: 둘 다 검토 대기. 작업 중이면 작업 중이 이긴다
        f.report_hash = "h".into();
        assert_eq!(both(&f), ("ready-for-review", "ready-for-review"));
        f.agent_state = Some("working".into());
        assert_eq!(both(&f), ("working", "working"));
        // 확인창이 30초 넘게: 둘 다 사람 차례. 금방 뜬 것은 작업 중
        f.agent_state = Some("blocked".into());
        f.state_secs = 31;
        assert_eq!(both(&f), ("waiting-on-you", "waiting-on-you"));
        // 금방 뜬 확인창: 새 보고(규칙 5)가 작업 중(규칙 6)보다 먼저다. 보고가 없으면 작업 중
        f.state_secs = 5;
        assert_eq!(both(&f), ("ready-for-review", "ready-for-review"));
        f.report_hash.clear();
        assert_eq!(both(&f), ("working", "working"));
        // 보고 첫 줄 태그로는 묶음을 정하지 않는다(D9, plan §8.78): 쉬는 agent의 새 보고는 둘 다 검토 대기
        let mut d = open();
        d.agent_state = Some("idle".into());
        d.report_hash = "h".into();
        assert_eq!(both(&d), ("ready-for-review", "ready-for-review"));
        // PR 승인: 둘 다 병합 중(D11). 확인한 보고라도 PR이 열려 있으면 둘 다 검토 대기
        let mut l = open();
        l.agent_state = Some("idle".into());
        l.pr_state = "OPEN".into();
        l.pr_review = "APPROVED".into();
        assert_eq!(both(&l), ("landing", "landing"));
        l.pr_review.clear();
        l.report_hash = "h".into();
        l.acked_hash = "h".into();
        assert_eq!(both(&l), ("ready-for-review", "ready-for-review"));
    }

    #[test]
    fn launch_and_self_report_rules_match() {
        let both = |f: &Facts| (hp_group(f, NOW), token(aaa_group(f, NOW)));
        // brief를 아직 못 넣었고 agent가 준비 안 된 채 60초: 둘 다 사람 차례
        let mut f = open();
        f.prompt_pending = true;
        f.agent_state = Some("working".into());
        f.state_secs = 61;
        assert_eq!(both(&f), ("waiting-on-you", "waiting-on-you"));
        f.state_secs = 10;
        assert_eq!(both(&f), ("working", "working"));
        // brief 전달 모름
        f.brief_stuck = true;
        assert_eq!(both(&f), ("waiting-on-you", "waiting-on-you"));
        // 닫힌 pane, 보고 없음
        let mut g = open();
        g.pane_exists = false;
        assert_eq!(both(&g), ("waiting-on-you", "waiting-on-you"));
        // 진행 보고 「Waiting for you」, agent idle
        let mut w = open();
        w.agent_state = Some("idle".into());
        w.self_activity = Some(WAITING.into());
        w.self_age_secs = Some(10);
        assert_eq!(both(&w), ("waiting-on-you", "waiting-on-you"));
        // 5분 안 진행 보고(100% 아님)면 idle이어도 작업 중, 5분 지나면 쉼
        w.self_activity = Some("시험 중".into());
        w.self_percent = Some(40);
        assert_eq!(both(&w), ("working", "working"));
        w.self_age_secs = Some(301);
        assert_eq!(both(&w), ("idle", "idle"));
        // 시작 중: 5분까지 작업 중, 넘으면 사람 차례
        let mut s = open();
        s.status = "starting".into();
        s.created_s = NOW - 10;
        assert_eq!(both(&s), ("working", "working"));
        s.created_s = NOW - 301;
        assert_eq!(both(&s), ("waiting-on-you", "waiting-on-you"));
    }

    #[test]
    fn screen_hint_reads_bottom_lines() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(screen_hint(&l(&["out", "✻ Thinking… (3s · esc to interrupt)", ""])), Some("working"));
        assert_eq!(screen_hint(&l(&["Do you want to proceed?", "❯ 1. Yes", "Esc to cancel"])), Some("blocked"));
        assert_eq!(screen_hint(&l(&["done", "❯ ", "  ? for shortcuts"])), Some("idle"));
        assert_eq!(screen_hint(&l(&["$ ls"])), None);
    }
}
