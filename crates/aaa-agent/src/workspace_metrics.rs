//! 보고 회수 때 기존 결과를 한 번 읽는다. 조회·검증 경로에서는 실행하지 않는다.
use aaa_proto::{MetricCounts, MetricRanking, MetricRun, MetricTiming, WorkspaceCollected};
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path, time::UNIX_EPOCH};

mod lines;
const MAX_INPUT: u64 = 8 * 1024 * 1024;
thread_local! {
    static READ_BYTES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static BUDGET_EXHAUSTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    versions: BTreeMap<String, String>,
    runs: BTreeMap<String, MetricRun>,
    #[serde(default)]
    errors: BTreeMap<String, String>,
}
struct CollectionLock(fs::File);
impl Drop for CollectionLock {
    fn drop(&mut self) {
        // close만 하면 다른 스레드의 fork~exec 사이 자식이 공유 잠금을 붙잡을 수 있다.
        // Windows는 private_file::lock의 비상속 handle 예약이 close로 풀린다.
        #[cfg(unix)]
        if let Err(e) = self.0.unlock() {
            tracing::error!(%e, "workspace 수집 잠금 해제 실패");
        }
        #[cfg(windows)]
        let _ = &self.0;
    }
}
fn read(path: &Path) -> Result<String> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.len() > MAX_INPUT {
        bail!("자료 크기 상한 또는 일반 파일 아님");
    }
    READ_BYTES.with(|read| {
        let total = read.get().saturating_add(m.len());
        if total > MAX_INPUT {
            BUDGET_EXHAUSTED.with(|flag| flag.set(true));
            bail!("이번 회수 읽기 비용 상한");
        }
        read.set(total);
        Ok(())
    })?;
    fs::read_to_string(path).context("기존 지표 자료 읽기")
}
fn version(path: &Path) -> Option<(String, u64)> {
    let m = fs::symlink_metadata(path).ok()?;
    if !m.is_file() {
        return None;
    }
    let at = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((format!("{}:{}", m.len(), at.as_nanos()), at.as_millis() as u64))
}
fn missing_tests() -> MetricRanking {
    MetricRanking {
        unit: "individual-test".into(),
        missing_reason: Some("개별 테스트 시간이 기존 결과에 없음".into()),
        ..Default::default()
    }
}
fn push_top(items: &mut Vec<MetricTiming>, item: MetricTiming) {
    items.push(item);
    items.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms).then(a.name.cmp(&b.name)));
    items.truncate(5);
}
fn top(source: Vec<MetricTiming>, unit: &str, complete: bool) -> MetricRanking {
    let mut items = Vec::with_capacity(6);
    for item in source {
        push_top(&mut items, item);
    }
    MetricRanking {
        missing_reason: items.is_empty().then(|| "기존 기록에 시간 없음".into()),
        items,
        unit: unit.into(),
        complete,
    }
}
fn millis(n: f64) -> Option<u64> {
    (n.is_finite() && (0.0..=86400.0 * 30.0).contains(&n)).then(|| (n * 1000.0).round() as u64)
}
fn number(v: &serde_json::Value) -> Option<u64> {
    v.as_f64().and_then(millis)
}
fn counts(text: &str) -> Option<MetricCounts> {
    let re = Regex::new(r"(?m)^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;").unwrap();
    let mut all = MetricCounts { unit: "rust-test (doctest 포함)".into(), ..Default::default() };
    let mut found = false;
    for c in re.captures_iter(text) {
        found = true;
        all.passed = all.passed.saturating_add(c[1].parse().ok()?);
        all.failed = all.failed.saturating_add(c[2].parse().ok()?);
        all.skipped = all.skipped.saturating_add(c[3].parse().ok()?);
        all.filtered = all.filtered.saturating_add(c[5].parse().ok()?);
    }
    found.then_some(all)
}
/// binary/분할 시간은 순위에 넣지 않는다. 집계 대상 결과의 개수만 읽는다.
fn rust_counts(dir: &Path) -> Result<Option<MetricCounts>> {
    let summary: serde_json::Value = serde_json::from_str(&read(&dir.join("rust/summary.json"))?)?;
    let jobs = summary["jobs"].as_array().context("Rust jobs 없음")?;
    let mut all = MetricCounts { unit: "rust-test (doctest 포함)".into(), ..Default::default() };
    let mut complete = !jobs.is_empty();
    let mut bytes = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    for job in jobs.iter().take(128) {
        let Some(name) = job["log"].as_str().filter(|n| !n.contains(['/', '\\']) && !n.starts_with('.')) else {
            complete = false;
            continue;
        };
        if !seen.insert(name) {
            return Ok(None);
        }
        match read(&dir.join("rust").join(name)) {
            Ok(t) => {
                bytes += t.len();
                if bytes > MAX_INPUT as usize {
                    return Ok(None);
                }
                if let Some(c) = counts(&t) {
                    all.passed = all.passed.saturating_add(c.passed);
                    all.failed = all.failed.saturating_add(c.failed);
                    all.skipped = all.skipped.saturating_add(c.skipped);
                    all.filtered = all.filtered.saturating_add(c.filtered);
                } else {
                    complete = false;
                }
            }
            Err(_) => complete = false,
        }
    }
    if !complete || jobs.len() > 128 {
        return Ok(None);
    }
    Ok(Some(all))
}
fn ci_run(dir: &Path, at: u64, run_id: String) -> Result<MetricRun> {
    let text = read(&dir.join("workspace-summary.tsv"))?;
    let mut rows = text.lines();
    let head: Vec<_> = rows.next().context("요약 머리 없음")?.split('|').collect();
    if head.len() != 4 || head[0] != "v1" {
        bail!("지표 요약 판이 다름");
    }
    let mut run = MetricRun {
        run_id,
        observed_at_ms: at,
        source: "ci-local 기존 종료 요약".into(),
        verification_ms: head[2].parse::<f64>().ok().and_then(millis),
        outcome: head[3].to_string(),
        slow_tests: missing_tests(),
        slow_e2e: top(vec![], "spec-work-sum", false),
        ..Default::default()
    };
    for row in rows {
        let fields: Vec<_> = row.split('|').collect();
        if fields.len() < 5 {
            continue;
        }
        if !matches!(fields[1], "ok" | "FAIL") {
            continue;
        }
        let Some(duration_ms) = fields[4].parse::<f64>().ok().and_then(millis) else {
            continue;
        };
        if fields[0] == "build" {
            run.build_ms = Some(duration_ms);
        }
        run.stages.push(MetricTiming {
            name: fields[0].into(),
            duration_ms,
            start_ms: None,
            outcome: fields[1].into(),
        });
    }
    if let Ok(value) = rust_counts(dir) {
        run.tests = value;
    }
    if run.tests.is_none() {
        run.notes.push("Rust 결과 일부가 없어서 테스트 수 미수집".into());
    }
    if let Ok(e2e) = read(&dir.join("e2e.log")) {
        let files = Regex::new(r"(?m)^## 요약: 파일 (\d+)개 × (\d+)번").unwrap();
        let jobs = Regex::new(r"(?m)^## 작업: (\d+)개").unwrap();
        run.e2e_unit = Some("spec-file".into());
        run.e2e_files = files.captures(&e2e).and_then(|c| c[1].parse().ok());
        run.e2e_jobs = jobs.captures(&e2e).and_then(|c| c[1].parse().ok());
        // 반복 실행의 중간값·최장값은 이번 개별 작업 합계와 다르므로 순위에서 제외한다.
        let re =
            Regex::new(r"(?m)^##   (\S+)\s+(통과|실패)\s+\d+/\d+\s+([\d.]+)초(?: \(독립 작업 합계\))?\s*$").unwrap();
        let items = re
            .captures_iter(&e2e)
            .filter_map(|c| {
                Some(MetricTiming {
                    name: c[1].to_string(),
                    duration_ms: millis(c[3].parse().ok()?)?,
                    outcome: c[2].to_string(),
                    start_ms: None,
                })
            })
            .collect();
        let complete =
            run.e2e_files.is_some_and(|n| re.captures_iter(&e2e).count() == n as usize) && run.outcome == "succeeded";
        run.slow_e2e = top(items, "spec-work-sum", complete);
    }
    // ci-local이 실제 사본·cmp를 확인한 원본 실행 파일 경로만 stat한다. bytes를 읽거나 해시하지 않는다.
    if read(&dir.join("e2e-binary.log")).is_ok_and(|s| s.contains("이번 빌드의 웹 E2E 실행 사본:"))
        && let Some(root) = dir.parent().and_then(Path::parent).and_then(Path::parent)
    {
        let binary = root.join("target/debug/aaa");
        if version(&binary).is_some_and(|(_, modified)| modified <= at)
            && let Ok(meta) = fs::symlink_metadata(binary)
        {
            run.artifact_bytes = Some(meta.len());
            run.artifact_name = Some("aaa".into());
        }
    }

    Ok(run)
}
fn fresh_json(path: &Path, start: u64, end: u64) -> Option<serde_json::Value> {
    let (_, at) = version(path)?;
    if at < start || at > end {
        return None;
    }
    serde_json::from_str(&read(path).ok()?).ok()
}
fn rc_run(path: &Path, at: u64, run_id: String) -> Result<MetricRun> {
    let v: serde_json::Value = serde_json::from_str(&read(path)?)?;
    if v["format"] != 1 {
        bail!("verification summary 판 미지원");
    }
    let dir = path.parent().context("verification 폴더 없음")?;
    let outcome = match v["status"].as_str() {
        Some("passed") => "succeeded",
        Some("failed") => "failed",
        _ => "partial",
    };
    let stages = v["gates"]
        .as_array()
        .map(|g| {
            g.iter()
                .filter_map(|s| {
                    Some(MetricTiming {
                        name: s["name"].as_str()?.into(),
                        duration_ms: number(&s["seconds"])?,
                        start_ms: None,
                        outcome: s["status"].as_str().unwrap_or("unknown").into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let mut run = MetricRun {
        run_id,
        observed_at_ms: at,
        source: "verification/summary.json".into(),
        profile: v["cargoProfile"].as_str().map(str::to_string),
        outcome: outcome.into(),
        verification_ms: number(&v["totalSeconds"]),
        stages,
        slow_tests: missing_tests(),
        slow_e2e: top(vec![], "scenario-step-sum", false),
        ..Default::default()
    };
    if let Some(mode) = v["mode"].as_str() {
        run.notes.push(format!("검증 모드 {mode} · Cargo profile과 별도"));
    }
    let Some((_, start)) = version(&dir.join("gates/.started")) else {
        run.notes.push("실행 시작 표지가 없어 부속 결과를 연결하지 않음".into());
        return Ok(run);
    };
    runtime_reports(
        &mut run,
        dir,
        start,
        at,
        &["default-tests", "workspace-tests"],
        &dir.parent().unwrap().join("rc-e2e/report.json"),
    );
    Ok(run)
}
fn runtime_reports(run: &mut MetricRun, dir: &Path, start: u64, at: u64, names: &[&str], e2e: &Path) {
    let mut counts = MetricCounts { unit: "rust-test (구성별 실행 범위 합계)".into(), ..Default::default() };
    let mut items = Vec::new();
    let mut count_complete = true;
    let mut timing_complete = true;
    // 이 두 보고는 rc의 기존 test_binaries.py가 이미 보존한 결과다. 추가 실행은 없다.
    for name in names {
        let Some(report) = fresh_json(&dir.join(name).join("report.json"), start, at) else {
            count_complete = false;
            timing_complete = false;
            continue;
        };
        let Some(binaries) = report.as_array() else {
            count_complete = false;
            continue;
        };
        if binaries.is_empty() {
            count_complete = false;
        }
        for binary in binaries {
            let tests = binary["tests"].as_array();
            let suites = binary["suites"].as_array();
            let (Some(tests), Some(suites)) = (tests, suites) else {
                count_complete = false;
                continue;
            };
            if suites.is_empty() {
                count_complete = false;
            }
            let mut expected = 0u64;
            for suite in suites {
                let (Some(passed), Some(failed), Some(ignored)) =
                    (suite["passed"].as_u64(), suite["failed"].as_u64(), suite["ignored"].as_u64())
                else {
                    count_complete = false;
                    continue;
                };
                expected = expected.saturating_add(passed + failed + ignored);
                counts.passed = counts.passed.saturating_add(passed.min(u32::MAX as u64) as u32);
                counts.failed = counts.failed.saturating_add(failed.min(u32::MAX as u64) as u32);
                counts.skipped = counts.skipped.saturating_add(ignored.min(u32::MAX as u64) as u32);
                counts.filtered = counts
                    .filtered
                    .saturating_add(suite["filtered_out"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32);
            }
            if expected != tests.len() as u64 {
                count_complete = false;
            }
            if binary["exit"] != 0 {
                timing_complete = false;
            }
            for test in tests.iter().filter(|t| t["event"] != "ignored") {
                if let (Some(test_name), Some(duration_ms)) = (test["name"].as_str(), number(&test["exec_time"])) {
                    let target = binary["target"].as_str().unwrap_or("binary");
                    push_top(
                        &mut items,
                        MetricTiming {
                            name: format!("{name}/{target}::{test_name}"),
                            duration_ms,
                            start_ms: None,
                            outcome: test["event"].as_str().unwrap_or("unknown").into(),
                        },
                    );
                } else {
                    timing_complete = false;
                }
            }
        }
    }
    run.tests = count_complete.then_some(counts);
    run.slow_tests = top(items, "individual-test", timing_complete && count_complete);
    if let Some(report) = fresh_json(e2e, start, at)
        && let Some(scenarios) = report["reports"].as_array()
    {
        run.e2e_files = u32::try_from(scenarios.len()).ok();
        run.e2e_unit = Some("scenario".into());
        let items: Vec<_> = scenarios
            .iter()
            .filter_map(|s| {
                Some(MetricTiming {
                    name: s["scenario"].as_str()?.into(),
                    duration_ms: s["duration_ms"].as_u64()?,
                    start_ms: None,
                    outcome: s["status"].as_str().unwrap_or("unknown").into(),
                })
            })
            .collect();
        let complete = items.len() == scenarios.len() && report["status"] == "passed";
        run.slow_e2e = top(items, "scenario-step-sum", complete);
    }
    if run.tests.is_none() {
        run.notes.push("테스트 수는 기존 결과 일부 누락으로 미수집".into());
    }
}
fn engine_run(path: &Path, at: u64, run_id: String) -> Result<MetricRun> {
    let source = read(path)?;
    let header = source.lines().next().context("엔진 시간 요약 머리 없음")?;
    ensure_engine_header(header)?;
    let re = Regex::new(r"(?m)^(.*?)\s+(\d+)s$").unwrap();
    let mut run = MetricRun {
        run_id,
        observed_at_ms: at,
        source: "verify_engine.sh 기존 시간 요약".into(),
        outcome: "succeeded".into(),
        slow_tests: missing_tests(),
        slow_e2e: top(vec![], "scenario-step-sum", false),
        notes: vec![header.into()],
        ..Default::default()
    };
    for row in re.captures_iter(&source) {
        let duration_ms = row[2].parse::<f64>().ok().and_then(millis).context("엔진 시간 범위 오류")?;
        if &row[1] == "total" {
            run.verification_ms = Some(duration_ms);
        } else {
            run.stages.push(MetricTiming {
                name: row[1].into(),
                duration_ms,
                start_ms: None,
                outcome: "passed".into(),
            });
        }
    }
    let dir = path.parent().context("target 없음")?.join("verification");
    let start = fs::read_dir(dir.join("gates"))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|e| e == "status"))
        .filter_map(|e| version(&e.path()).map(|(_, at)| at))
        .min();
    if let Some(start) = start {
        runtime_reports(
            &mut run,
            &dir,
            start,
            at,
            &["workspace-tests", "automation-tests", "2d-tests", "3d-tests"],
            &path.parent().unwrap().join("no2-e2e/all-headless.json"),
        );
    } else {
        run.notes.push("실행 시작 범위가 없어 부속 결과 미수집".into());
    }
    Ok(run)
}
fn ensure_engine_header(header: &str) -> Result<()> {
    if !header.starts_with("verify_engine.sh timing (mode ") || !header.ends_with("all gates passed)") {
        bail!("지원하는 엔진 종료 요약이 아님");
    }
    Ok(())
}
/// 수집 원장도 agent 상태에 보관한다. 같은 입력 판은 전체 로그를 다시 읽지 않는다.
pub fn collect(
    state: &Path,
    cwd: &Path,
    workspace_key: &str,
    known_code_commits: &[String],
) -> Result<WorkspaceCollected> {
    READ_BYTES.with(|read| read.set(0));
    BUDGET_EXHAUSTED.with(|flag| flag.set(false));
    let key = hex::encode(Sha256::digest(cwd.to_string_lossy().as_bytes()));
    let store = state.join("workspace-metrics").join(key);
    fs::create_dir_all(&store)?;
    let _lock = CollectionLock(aaa_common::private_file::lock(&store.join("collect.lock"))?);
    let cache_path = store.join("runs.json");
    let mut cache: Cache = match fs::read(&cache_path) {
        Ok(b) => serde_json::from_slice(&b)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Cache::default(),
        Err(e) => return Err(e.into()),
    };
    let mut inputs = Vec::new();
    if let Ok(dirs) = fs::read_dir(cwd.join("target/ci-local")) {
        for dir in dirs.take(512).flatten() {
            if !dir.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = dir.path().join("workspace-summary.tsv");
            if let Some((v, at)) = version(&path) {
                inputs.push((path, v, at, "ci"));
            }
        }
    }
    let rc = cwd.join("target/verification/summary.json");
    if let Some((v, at)) = version(&rc) {
        inputs.push((rc, v, at, "rc"));
    }
    let engine = cwd.join("target/verify-timing.txt");
    if cwd.join("scripts/verify_engine.sh").is_file()
        && let Some((v, at)) = version(&engine)
    {
        inputs.push((engine, v, at, "engine"));
    }
    inputs.sort_by_key(|x| std::cmp::Reverse(x.2));
    let mut result = WorkspaceCollected::default();
    if inputs.len() > 32 {
        result.notes.push("최근 32개 종료 입력까지만 회수함".into());
    }
    for (path, v, at, kind) in inputs.into_iter().take(32) {
        let rel = path.strip_prefix(cwd)?.to_string_lossy().into_owned();
        if cache.versions.get(&rel) == Some(&v) {
            continue;
        }
        let id = hex::encode(Sha256::digest(format!("{}:{rel}:{v}", cwd.display()).as_bytes()));
        let parsed = match kind {
            "ci" => ci_run(path.parent().unwrap(), at, id.clone()),
            "engine" => engine_run(&path, at, id.clone()),
            _ => rc_run(&path, at, id.clone()),
        };
        let exhausted = BUDGET_EXHAUSTED.with(|flag| flag.get());
        if exhausted {
            result.notes.push("읽기 비용 상한으로 일부 지표 미수집".into());
        }
        match parsed {
            Ok(mut run) => {
                if exhausted {
                    run.notes.push("읽기 비용 상한으로 부속 결과 일부 미수집".into());
                }
                cache.runs.insert(id, run);
                cache.errors.remove(&rel);
                cache.versions.insert(rel, v);
            }
            Err(_) => {
                cache.errors.insert(rel.clone(), "기존 결과 형식·크기 문제로 해당 실행 미수집".into());
                cache.versions.insert(rel, v);
            }
        }
        if exhausted {
            break;
        }
    }
    while cache.runs.len() > 256 {
        let Some(id) = cache.runs.values().min_by_key(|r| r.observed_at_ms).map(|r| r.run_id.clone()) else { break };
        cache.runs.remove(&id);
    }
    result.notes.extend(cache.errors.values().cloned());
    result.notes.sort();
    result.notes.dedup();
    let bytes = serde_json::to_vec(&cache)?;
    aaa_common::util::write_private(&cache_path, &bytes)?;
    result.runs = cache.runs.into_values().collect();
    result.runs.sort_by_key(|r| std::cmp::Reverse(r.observed_at_ms));
    if result.runs.len() > 64 {
        result.runs.truncate(64);
        result.notes.push("이번 전송은 최근 64개 요약까지".into());
    }
    for run in &mut result.runs {
        run.stages.truncate(32);
        for item in run.slow_tests.items.iter_mut().chain(run.slow_e2e.items.iter_mut()) {
            item.name = item.name.chars().take(200).collect();
        }
    }
    {
        let lines_store = state.join("workspace-metrics").join(hex::encode(Sha256::digest(workspace_key.as_bytes())));
        fs::create_dir_all(&lines_store)?;
        let _lines_lock = CollectionLock(aaa_common::private_file::lock(&lines_store.join("lines.lock"))?);
        match lines::collect(cwd, &lines_store, known_code_commits) {
            Ok(l) => result.code_lines = l,
            Err(_) => result.notes.push("코드 줄 수 수집 대기(비용 상한 또는 Git 자료 없음)".into()),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
