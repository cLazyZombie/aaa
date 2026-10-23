//! GitHub Actions 조건부 조회 규칙. 참고: t3code gitHubConditionalChecks.ts의 validator·쪽 넘김.
use crate::{pr::sanitize, util};
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
    if id.len() == 6 && id.starts_with("t-") && id[2..].bytes().all(|c| c.is_ascii_digit()) {
        vec![id.into()]
    } else {
        vec![]
    }
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
        // gh --log-failed 줄은 `job\t단계\t시각 본문`이다. 앞머리 없이 온 줄도 같은 끝 모양으로 읽는다.
        let named = |rest: &str| {
            let (before, name) = rest.rsplit_once(' ')?;
            (before == "test" || before.ends_with(" test")).then(|| clip(name)).filter(|n| !n.is_empty())
        };
        let (set, item) = if let Some(name) = line.strip_suffix(" ... FAILED").and_then(named) {
            (&mut self.tests, name)
        } else if let Some(name) = line.split_once(" has been running for over ").and_then(|(r, _)| named(r)) {
            (&mut self.others, format!("끝나지 않음: {name}"))
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
        let set = if self.tests.is_empty() { self.others } else { self.tests };
        set.into_iter().collect()
    }
}
fn clip(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(FAILURE_LEN).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests;
