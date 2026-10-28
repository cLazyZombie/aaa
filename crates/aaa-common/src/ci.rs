//! GitHub Actions 조건부 조회 규칙. 참고: t3code gitHubConditionalChecks.ts의 validator·쪽 넘김.
use crate::{github::sanitize, util};
use aaa_proto::{CiAnswer, CiCommit, CiJob, CiQuery, CiRun};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const LOG_JOB_CAP: usize = 32 * 1024;
pub const LOG_TOTAL_CAP: usize = 128 * 1024;
pub const MAX_PAGES: u32 = 100;

/// 병합 첫 줄 끝에 붙은 번호만 받는다. GitHub 제목·본문을 입력 지시로 넘기지 않는다.
pub fn owners(message: &str) -> Vec<String> {
    let line = message.lines().next().unwrap_or("").trim();
    if !line.starts_with("병합:") {
        return vec![];
    }
    let Some(id) = line.strip_suffix(')').and_then(|s| s.rsplit_once('(').map(|(_, id)| id)) else { return vec![] };
    if let Some(id) = aaa_proto::canonical_short_id(id, 't') { vec![id] } else { vec![] }
}
fn text(v: &Value, key: &str) -> String {
    sanitize(v[key].as_str().unwrap_or(""))
}
fn num(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or(0)
}
pub fn failed(c: &str) -> bool {
    matches!(c, "failure" | "timed_out" | "action_required" | "startup_failure" | "stale")
}
pub fn valid_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 구조화한 GET만 만든다. arbitrary URL·shell·머리 주입은 받지 않는다.
pub fn api_args(repo: &str, q: &CiQuery) -> Result<Vec<String>> {
    if repo.split('/').count() != 2
        || repo.split('/').any(|s| {
            s.is_empty()
                || matches!(s, "." | "..")
                || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        bail!("GitHub owner/repo 형식이 아니다");
    }
    let (path, etag) = match q {
        CiQuery::Runs { etag, since, page } => {
            page_ok(*page)?;
            if let Some(s) = since
                && (s.len() > 32 || !s.bytes().all(|b| b.is_ascii_digit() || b"-:TZ.".contains(&b)))
            {
                bail!("created 시각 형식이 아니다");
            }
            let per_page = if since.is_some() { 100 } else { 30 };
            (
                format!(
                    "actions/runs?event=push&per_page={per_page}&exclude_pull_requests=true&page={page}{}",
                    since.as_ref().map(|s| format!("&created=%3E%3D{s}")).unwrap_or_default()
                ),
                etag.as_ref(),
            )
        }
        CiQuery::Jobs { run, attempt, page } => {
            page_ok(*page)?;
            if *run == 0 || *attempt == 0 {
                bail!("run/attempt가 0이다");
            }
            (format!("actions/runs/{run}/attempts/{attempt}/jobs?per_page=100&page={page}"), None)
        }
        CiQuery::Compare { base, head, page } => {
            page_ok(*page)?;
            if !valid_sha(base) || !valid_sha(head) {
                bail!("commit SHA 형식이 아니다");
            }
            (format!("compare/{base}...{head}?per_page=100&page={page}"), None)
        }
        CiQuery::Log { .. } => bail!("로그는 gh run view로 읽는다"),
    };
    let mut args = vec![
        "api".into(),
        "-i".into(),
        "--hostname".into(),
        "github.com".into(),
        "--method".into(),
        "GET".into(),
        "-H".into(),
        "Accept: application/vnd.github+json".into(),
    ];
    if let Some(etag) = etag {
        if etag.len() > 512 || etag.chars().any(char::is_control) {
            bail!("ETag 형식이 아니다");
        }
        args.extend(["-H".into(), format!("If-None-Match: {etag}")]);
    }
    args.push(format!("repos/{repo}/{path}"));
    Ok(args)
}
fn page_ok(page: u32) -> Result<()> {
    if page == 0 || page > MAX_PAGES {
        bail!("쪽 수 상한이다");
    }
    Ok(())
}

/// gh는 304에서 exit 1이다. HTTP 상태 줄을 먼저 읽고, 200에서만 exit 성공을 요구한다.
pub fn response(out: &str, success: bool, q: &CiQuery) -> Result<CiAnswer> {
    let normalized = out.replace("\r\n", "\n");
    let (headers, body) = normalized.split_once("\n\n").context("HTTP 머리가 없다")?;
    let status = headers
        .lines()
        .next()
        .and_then(|s| s.strip_prefix("HTTP/"))
        .and_then(|s| s.split_whitespace().nth(1))
        .and_then(|s| {
            s.parse::<u16>()
                .inspect_err(|error| crate::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
                .ok()
        })
        .context("HTTP 상태 줄이 없다")?;
    let header = |name: &str| {
        headers
            .lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_string())
    };
    let mut answer = CiAnswer {
        etag: header("etag"),
        next: header("link").is_some_and(|l| l.contains("rel=\"next\"")),
        ..Default::default()
    };
    if status == 304 && matches!(q, CiQuery::Runs { etag: Some(_), .. }) {
        answer.not_modified = true;
        return Ok(answer);
    }
    if status != 200 || !success {
        bail!("GitHub HTTP {status}");
    }
    let v: Value = serde_json::from_str(body).context("GitHub 응답 JSON 형식이 아니다")?;
    match q {
        CiQuery::Runs { .. } => {
            for r in v["workflow_runs"].as_array().context("workflow_runs가 없다")? {
                if r["event"] != "push" {
                    continue;
                }
                let sha = text(r, "head_sha");
                if num(r, "id") == 0 || num(r, "workflow_id") == 0 {
                    bail!("run/workflow ID가 없다");
                }
                if !valid_sha(&sha) {
                    bail!("run의 SHA 형식이 아니다");
                }
                answer.runs.push(CiRun {
                    id: num(r, "id"),
                    workflow: num(r, "workflow_id"),
                    name: text(r, "name"),
                    number: num(r, "run_number"),
                    attempt: num(r, "run_attempt").max(1) as u32,
                    branch: text(r, "head_branch"),
                    sha,
                    status: text(r, "status"),
                    conclusion: text(r, "conclusion"),
                    created_ms: util::parse_iso(r["created_at"].as_str().unwrap_or("")).unwrap_or(0),
                    owners: owners(r["head_commit"]["message"].as_str().unwrap_or("")),
                });
            }
        }
        CiQuery::Jobs { .. } => {
            for j in v["jobs"].as_array().context("jobs가 없다")? {
                if failed(j["conclusion"].as_str().unwrap_or("")) {
                    let mut steps: Vec<_> = j["steps"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|s| failed(s["conclusion"].as_str().unwrap_or("")))
                        .map(|s| text(s, "name"))
                        .collect();
                    steps.sort();
                    steps.dedup();
                    answer.jobs.push(CiJob { id: num(j, "id"), name: text(j, "name"), steps, failures: vec![] });
                }
            }
        }
        CiQuery::Compare { .. } => {
            for c in v["commits"].as_array().context("commits가 없다")? {
                answer.commits.push(CiCommit {
                    sha: text(c, "sha"),
                    owners: owners(c["commit"]["message"].as_str().unwrap_or("")),
                });
            }
        }
        CiQuery::Log { .. } => bail!("로그는 HTTP 응답이 아니다"),
    }
    Ok(answer)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pending,
    Cancelled,
    Passed { rerun: bool },
    Failed,
    Stuck,
}
/// 같은 workflow 재실행은 최신 attempt로 판정하되, attempt > 1 통과는 별도로 표시한다.
pub fn verdict(runs: &[CiRun], now: u64, stuck_ms: u64) -> Verdict {
    let mut latest = BTreeMap::new();
    for r in runs {
        let slot = latest.entry(r.workflow).or_insert(r);
        if (r.id, r.attempt) > (slot.id, slot.attempt) {
            *slot = r;
        }
    }
    let active: Vec<_> = latest.values().filter(|r| r.conclusion != "cancelled").collect();
    if active.is_empty() {
        return Verdict::Cancelled;
    }
    if active.iter().any(|r| r.status != "completed") {
        return if active
            .iter()
            .any(|r| r.status != "completed" && r.created_ms > 0 && now.saturating_sub(r.created_ms) >= stuck_ms)
        {
            Verdict::Stuck
        } else {
            Verdict::Pending
        };
    }
    if active
        .iter()
        .any(|r| failed(&r.conclusion) || !matches!(r.conclusion.as_str(), "success" | "neutral" | "skipped"))
    {
        Verdict::Failed
    } else {
        Verdict::Passed { rerun: active.iter().any(|r| r.attempt > 1) }
    }
}
/// 이름·단계 집합만 비교한다. run/job id는 fingerprint에 넣지 않는다.
pub fn failure_set(jobs: &[CiJob]) -> BTreeSet<(String, Vec<String>)> {
    jobs.iter().map(|j| (j.name.clone(), j.steps.clone())).collect()
}
/// 같은 job·단계 안에서 다른 시험이 실패했는지 가른다. 표지를 못 뽑은 job(옛 agent)은 넣지 않는다.
pub fn failed_tests(jobs: &[CiJob]) -> BTreeSet<(String, Vec<String>)> {
    jobs.iter().filter(|j| !j.failures.is_empty()).map(|j| (j.name.clone(), j.failures.clone())).collect()
}

pub const FAILURES_CAP: usize = 64;
const FAILURE_LEN: usize = 300;
const LINE_CAP: usize = 4096;
/// 실패 로그를 흘려 읽으며 실패 시험 이름을 모은다. 로그 꼬리만 남겨도 앞쪽 실패를 놓치지 않는다.
/// 시험 이름이 없으면 panic 위치(`path:line`)·끝나지 않은 시험으로 가른다.
#[derive(Default)]
pub struct FailureScan {
    tests: BTreeSet<String>,
    others: BTreeSet<String>,
    slow: BTreeSet<String>,
    completed: BTreeSet<String>,
    failed_steps: BTreeSet<String>,
    web_summary: Option<String>,
    stack_overflows: BTreeSet<String>,
    stack_overflow_exit: bool,
    line: Vec<u8>,
}
impl FailureScan {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if b == b'\n' {
                self.end_line();
            } else if self.line.len() < LINE_CAP {
                self.line.push(b);
            }
        }
    }
    fn end_line(&mut self) {
        let line = String::from_utf8_lossy(&self.line).trim_end().to_string();
        self.line.clear();
        // ci-local은 Python suite를 기다린 뒤 실제 종료 코드를 모든 출력 줄에 붙인다.
        // fixture의 가짜 Rust 실패가 annotation에 다시 실려도 성공한 바깥 suite에 속한다.
        // 실패·중단된 suite와 표지가 없는 옛 로그는 그대로 수집한다(시험 이름으로 제외하지 않는다).
        // gh의 여러 줄 annotation은 시각 없이 job/step 뒤 탭부터 본문이 시작하기도 한다.
        if line.starts_with("[python exit=0] ")
            || line.contains(" [python exit=0] ")
            || line.contains("\t[python exit=0] ")
        {
            return;
        }
        // 원문·gh job/step/time·ci-local 단계·annotation 앞머리만 벗긴다.
        // FAIL 문구를 인용한 성공 fixture·진단 본문까지 검색해 실패로 만들지 않는다.
        let body = line
            .splitn(3, '\t')
            .nth(2)
            .filter(|body| body.split_once(' ').is_some_and(|(stamp, _)| util::parse_iso(stamp).is_some()))
            .unwrap_or(&line);
        let body =
            body.split_once(' ').filter(|(stamp, _)| util::parse_iso(stamp).is_some()).map_or(body, |(_, rest)| rest);
        let body = body.strip_prefix("[test] ").unwrap_or(body);
        let body = body.strip_prefix("[e2e] ").unwrap_or(body);
        let body = body.strip_prefix("##[error]").unwrap_or(body);
        // Rust는 abort 때 FAILED를 출력하지 않는다. thread 표지와 Cargo 종료 유형을
        // 따로 읽어 이름 없는 종료도 남기되, 인용문·성공한 Python fixture는 받지 않는다.
        if let Some((name, rest)) = body.strip_prefix("thread '").and_then(|s| s.split_once("' ")) {
            let rest = if let Some(pid) = rest.strip_prefix('(') {
                pid.split_once(") ")
                    .filter(|(pid, _)| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
                    .map(|(_, rest)| rest)
            } else {
                Some(rest)
            };
            if rest == Some("has overflowed its stack") && !name.is_empty() && self.stack_overflows.len() < FAILURES_CAP
            {
                self.stack_overflows.insert(format!("stack overflow: {}", clip(name)));
            }
        }
        if let Some(exit) = body
            .trim_start()
            .strip_prefix("process didn't exit successfully: ")
            .and_then(|s| s.rsplit_once(" (exit code: "))
            .and_then(|(_, s)| s.strip_suffix(')'))
        {
            self.stack_overflow_exit |=
                matches!(exit, "0xc00000fd, STATUS_STACK_OVERFLOW" | "0xc00000fd" | "3221225725" | "-1073741571");
        }
        if let Some(rest) = body.strip_prefix("FAIL ") {
            // harness의 첫 ': ' 앞은 고정된 단계 이름이며 뒤의 오류·DOM·URL은 신원에 넣지 않는다.
            let name = rest.split_once(": ").map_or(rest, |(name, _)| name).trim();
            if !name.is_empty() && self.failed_steps.len() < FAILURES_CAP {
                self.failed_steps.insert(format!("FAIL: {}", clip(name)));
            }
        }
        if let Some(json) = body.strip_prefix("## 실행 ")
            && let Ok(row) = serde_json::from_str::<Value>(json)
            && row["event"] == "suite-end"
            && let (Some(jobs), Some(failed)) = (row["jobs"].as_u64(), row["failed"].as_u64())
            && failed > 0
            && failed <= jobs
        {
            self.web_summary = Some(format!("web: {failed}/{jobs} 작업 실패"));
        }
        // gh --log-failed 줄은 `job\t단계\t시각 본문`이다. 앞머리 없이 온 줄도 같은 끝 모양으로 읽는다.
        let named = |rest: &str| {
            let (before, name) = rest.rsplit_once(' ')?;
            (before == "test" || before.ends_with(" test")).then(|| clip(name)).filter(|n| !n.is_empty())
        };
        // annotation이 뒤에서 같은 60초 경고를 다시 실어도 완료한 시험을 정지로 세지 않는다.
        // 완료는 경고만 없앤다. 같은 이름의 실제 FAILED/panic 증거는 지우지 않는다.
        let outcome = line.split_once(" ... ").and_then(|(rest, outcome)| {
            let status = outcome.split_whitespace().next()?;
            matches!(status, "ok" | "FAILED" | "ignored").then(|| (rest, status))
        });
        if let Some((rest, _)) = outcome
            && let Some(name) = named(rest)
            && self.slow.contains(&name)
        {
            self.completed.insert(name);
        }
        let (set, item) = if let Some(name) =
            outcome.filter(|(_, status)| *status == "FAILED").and_then(|(rest, _)| named(rest))
        {
            (&mut self.tests, name)
        } else if let Some(name) = line.split_once(" has been running for over ").and_then(|(r, _)| named(r)) {
            (&mut self.slow, name)
        } else if let Some((_, at)) = line.split_once("panicked at ") {
            let at = at.split_whitespace().next().unwrap_or("").trim_end_matches([':', ',']).replace('\\', "/");
            // 열 번호는 버린다: 같은 줄의 다른 식이 같은 실패다.
            let at = match at.rsplit_once(':') {
                Some((head, col)) if head.contains(':') && col.bytes().all(|b| b.is_ascii_digit()) => head.to_string(),
                _ => at,
            };
            if at.is_empty() {
                return;
            }
            (&mut self.others, format!("panic: {}", clip(&at)))
        } else {
            return;
        };
        if set.len() < FAILURES_CAP {
            set.insert(item);
        }
    }
    pub fn finish(mut self) -> Vec<String> {
        self.end_line();
        if self.stack_overflows.is_empty() && self.stack_overflow_exit {
            self.stack_overflows.insert("exit: STATUS_STACK_OVERFLOW".into());
        }
        // 보통 FAILED가 함께 있어도 abort 증거가 fallback others에 가려지면 안 된다.
        for failure in self.stack_overflows {
            if self.tests.len() < FAILURES_CAP {
                self.tests.insert(failure);
            }
        }
        // 단계 이름이 있으면 요약은 중복이다. 이름이 잘려 없어져도 실패 수는 남긴다.
        if self.failed_steps.is_empty()
            && let Some(summary) = self.web_summary
            && self.tests.len() < FAILURES_CAP
        {
            self.tests.insert(summary);
        }
        for name in self.failed_steps {
            if self.tests.len() < FAILURES_CAP {
                self.tests.insert(name);
            }
        }
        for name in self.slow.difference(&self.completed) {
            if self.others.len() < FAILURES_CAP {
                self.others.insert(format!("끝나지 않음: {name}"));
            }
        }
        let set = if self.tests.is_empty() { self.others } else { self.tests };
        set.into_iter().collect()
    }
}
fn clip(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(FAILURE_LEN).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests;
