//! 공통 검증 입구. 원문 출력은 터미널로만 보내고 집계값만 Git 공용 원장에 남긴다.
use super::*;
use aaa_common::child_process::StdCommandExt as _;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    time::Instant,
};

#[derive(PartialEq, Eq, Serialize)]
struct Snapshot {
    commit: String,
    tree: String,
    remote: String,
    clean: bool,
}
impl Snapshot {
    fn read(cwd: &Path) -> Result<Self> {
        Ok(Self {
            commit: lines::text(cwd, &["rev-parse", "HEAD"])?,
            tree: lines::text(cwd, &["rev-parse", "HEAD^{tree}"])?,
            remote: mainline::remote_id(&lines::text(cwd, &["remote", "get-url", "origin"])?)
                .context("origin 신원 미확인")?,
            clean: lines::text(cwd, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty(),
        })
    }
}

fn command(cwd: &Path, args: &[OsString]) -> Result<Vec<OsString>> {
    if !args.is_empty() {
        return Ok(args.to_vec());
    }
    for script in ["scripts/verify.sh", "scripts/verify_engine.sh", "scripts/ci-local.sh", "verify.sh"] {
        if cwd.join(script).is_file() {
            return Ok(vec!["bash".into(), script.into()]);
        }
    }
    bail!("검증 스크립트 없음: aaa verify -- <명령>으로 지정한다");
}

#[derive(Default)]
struct OutputSummary {
    tests: Option<MetricCounts>,
    build_ms: Option<u64>,
    profiles: BTreeSet<String>,
    incomplete: bool,
}
impl OutputSummary {
    fn line(&mut self, line: &[u8], stderr: bool) {
        let Ok(line) = std::str::from_utf8(line) else { return };
        if !stderr && line.starts_with("test result: ") {
            if let Some(c) = counts(line) {
                let all = self.tests.get_or_insert_with(|| MetricCounts {
                    unit: "rust-test (출력에 종료 요약이 있는 범위)".into(),
                    ..Default::default()
                });
                all.passed = all.passed.saturating_add(c.passed);
                all.failed = all.failed.saturating_add(c.failed);
                all.skipped = all.skipped.saturating_add(c.skipped);
                all.filtered = all.filtered.saturating_add(c.filtered);
            } else {
                self.incomplete = true;
            }
        }
        // Cargo가 출력한 컴파일 경과만 읽는다. 명령 전체·개별 테스트 시간으로 바꾸지 않는다.
        if stderr {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("Finished `")
                && let Some((profile, rest)) = rest.split_once("` profile ")
                && profile.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                && let Some((_, duration)) = rest.rsplit_once(" target(s) in ")
                && let Some(ms) = cargo_duration(duration.trim())
            {
                self.build_ms = Some(self.build_ms.unwrap_or(0).saturating_add(ms));
                self.profiles.insert(profile.into());
            }
        }
    }
}

fn cargo_duration(text: &str) -> Option<u64> {
    let (minutes, seconds) = match text.split_once("m ") {
        Some((m, s)) => (m.parse::<u64>().ok()?, s),
        None => (0, text),
    };
    let seconds = seconds.strip_suffix('s')?.parse::<f64>().ok()?;
    millis(minutes as f64 * 60.0 + seconds)
}

fn tee(mut input: impl Read, mut output: impl Write, stderr: bool) -> std::io::Result<OutputSummary> {
    let mut summary = OutputSummary::default();
    let mut chunk = [0u8; 8192];
    let mut line = Vec::new();
    let mut overlong = false;
    loop {
        let n = input.read(&mut chunk)?;
        if n == 0 {
            if !overlong {
                summary.line(&line, stderr);
            }
            return Ok(summary);
        }
        // pipe를 닫은 소비자가 있어도 자식 출력을 끝까지 비운다. 출력 원문은 보관하지 않는다.
        if output.write_all(&chunk[..n]).and_then(|()| output.flush()).is_err() {
            summary.incomplete = true;
        }
        for b in &chunk[..n] {
            if *b == b'\n' {
                if !overlong {
                    summary.line(&line, stderr);
                }
                line.clear();
                overlong = false;
            } else if line.len() < 16 * 1024 && !overlong {
                line.push(*b);
            } else {
                overlong = true;
                summary.incomplete = true;
            }
        }
    }
}

fn execute(
    cwd: &Path,
    args: &[OsString],
    stdout_output: impl Write + Send + 'static,
    stderr_output: impl Write + Send + 'static,
) -> Result<(ExitStatus, OutputSummary)> {
    let mut child = Command::new(&args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .env("CARGO_TERM_COLOR", "never")
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_guarded()
        .context("검증 명령 실행 실패")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || tee(stdout, stdout_output, false));
    let err = std::thread::spawn(move || tee(stderr, stderr_output, true));
    let status = child.wait();
    let out = out.join();
    let err = err.join();
    let mut summary = OutputSummary::default();
    for result in [out, err] {
        match result {
            Ok(Ok(part)) => {
                if part.tests.is_some() {
                    summary.tests = part.tests;
                }
                summary.build_ms = part.build_ms.or(summary.build_ms);
                summary.profiles.extend(part.profiles);
                summary.incomplete |= part.incomplete;
            }
            Ok(Err(error)) => {
                eprintln!("aaa verify: 출력 읽기 실패: {error}");
                summary.incomplete = true;
            }
            Err(_) => {
                eprintln!("aaa verify: 출력 수집 thread 실패");
                summary.incomplete = true;
            }
        }
    }
    Ok((status.context("검증 명령 종료 대기 실패")?, summary))
}

fn details(cwd: &Path, start: u64, end: u64, run: &mut MetricRun) {
    READ_BYTES.with(|bytes| bytes.set(0));
    BUDGET_EXHAUSTED.with(|flag| flag.set(false));
    let root = cwd.join("target/verification");
    let mut names: Vec<String> = fs::read_dir(&root)
        .into_iter()
        .flatten()
        .take(64)
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            (entry.file_type().ok()?.is_dir() && !name.starts_with('.') && entry.path().join("report.json").is_file())
                .then_some(name)
        })
        .collect();
    names.sort();
    let e2e = [
        "target/verification/e2e.json",
        "target/pt-e2e/report.json",
        "target/rc-e2e/report.json",
        "target/no2-e2e/all-headless.json",
    ]
    .iter()
    .map(|p| cwd.join(p))
    .find(|p| version(p).is_some_and(|(_, at)| start <= at && at <= end))
    .unwrap_or_else(|| root.join("e2e.json"));
    let mut detailed =
        MetricRun { slow_tests: missing_tests(), slow_e2e: top(vec![], "scenario-wall", false), ..Default::default() };
    runtime_reports(&mut detailed, &root, start, end, &names.iter().map(String::as_str).collect::<Vec<_>>(), &e2e);
    if !names.is_empty() {
        if run.tests.is_none() {
            run.tests = detailed.tests;
        }
        run.slow_tests = detailed.slow_tests;
    }
    run.e2e_files = detailed.e2e_files;
    run.e2e_unit = detailed.e2e_unit;
    run.slow_e2e = detailed.slow_e2e;
    if BUDGET_EXHAUSTED.with(|flag| flag.get()) {
        run.notes.push("상세 보고 읽기 상한으로 일부 미수집".into());
    }
    for item in run.slow_tests.items.iter_mut().chain(run.slow_e2e.items.iter_mut()) {
        item.name = aaa_common::redact::text(&item.name).chars().take(200).collect();
    }
}

fn archive_dirs(store: &Path) -> Result<BTreeSet<PathBuf>> {
    if !store.exists() {
        return Ok(BTreeSet::new());
    }
    let mut dirs = BTreeSet::new();
    for entry in fs::read_dir(store)?.take(4097) {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.file_name().to_string_lossy().starts_with("run-") {
            dirs.insert(entry.path());
        }
    }
    anyhow::ensure!(dirs.len() <= 4096, "검증 원장 개수 상한");
    Ok(dirs)
}

// 기존 writer가 같은 checkout에서 이번 구간에 시작·종료한 원장을 남겼으면 그것을 사용한다.
// 다른 worktree의 원장은 로컬 시작 표지가 없으므로 공통 실행을 대신하지 못한다.
fn native_recorded(cwd: &Path, store: &Path, old: &BTreeSet<PathBuf>, source: &Snapshot, start: u64, end: u64) -> bool {
    let mut markers = vec![cwd.join("target/verification/workspace-source.json")];
    if let Ok(dirs) = fs::read_dir(cwd.join("target/ci-local")) {
        markers.extend(dirs.take(512).flatten().map(|d| d.path().join("workspace-source.json")));
    }
    if !markers.iter().any(|p| version(p).is_some_and(|(_, at)| start <= at && at <= end)) {
        return false;
    }
    let Ok(dirs) = archive_dirs(store) else { return false };
    dirs.difference(old).any(|dir| {
        let Ok(text) = read(&dir.join("provenance.json")) else { return false };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return false };
        matches!(v["format"].as_u64(), Some(1 | 2))
            && matches!(v["runner"].as_str().unwrap_or("ci-local"), "ci-local" | "rc" | "pt" | "engine")
            && v["clean"] == true
            && v["commit"] == source.commit
            && v["tree"] == source.tree
            && v["remote"] == source.remote
            && v["started_at_ms"].as_u64().is_some_and(|at| start <= at && at <= end)
            && v["ended_at_ms"].as_u64().is_some_and(|at| start <= at && at <= end)
            && summary_run(dir, end, String::new()).is_ok_and(|run| {
                matches!(run.outcome.as_str(), "succeeded" | "failed") && run.verification_ms.is_some()
            })
    })
}

fn save(store: &Path, source: &Snapshot, start: u64, end: u64, run: &MetricRun) -> Result<()> {
    fs::create_dir_all(store)?;
    let id = format!("run-{start}-{}", aaa_common::util::new_id());
    let staging = store.join(format!(".writing-{id}"));
    fs::create_dir(&staging)?;
    let result = (|| -> Result<()> {
        let mut provenance = serde_json::to_value(source)?;
        provenance["format"] = 3.into();
        provenance["runner"] = "aaa-verify".into();
        provenance["started_at_ms"] = start.into();
        provenance["ended_at_ms"] = end.into();
        provenance["profile"] = serde_json::to_value(&run.profile)?;
        fs::write(staging.join("metrics.json"), serde_json::to_vec(run)?)?;
        fs::write(staging.join("provenance.json"), serde_json::to_vec(&provenance)?)?;
        fs::rename(&staging, store.join(id))?;
        Ok(())
    })();
    if staging.exists() {
        aaa_common::diagnostic::ignored(fs::remove_dir_all(&staging), "자기 검증 원장 임시 폴더 정리");
    }
    result
}

/// 설정·host 연결 없이 실행한다. 수집 실패가 원래 검증 종료 코드를 덮지 않는다.
pub fn run(cwd: &Path, args: &[OsString], json_output: bool) -> Result<ExitStatus> {
    if json_output {
        run_with_output(cwd, args, std::io::stderr(), std::io::stderr())
    } else {
        run_with_output(cwd, args, std::io::stdout(), std::io::stderr())
    }
}

pub(super) fn run_with_output(
    cwd: &Path,
    args: &[OsString],
    stdout: impl Write + Send + 'static,
    stderr: impl Write + Send + 'static,
) -> Result<ExitStatus> {
    READ_BYTES.with(|bytes| bytes.set(0));
    BUDGET_EXHAUSTED.with(|flag| flag.set(false));
    let root = PathBuf::from(lines::text(cwd, &["rev-parse", "--show-toplevel"])?);
    let args = command(&root, args)?;
    let common = PathBuf::from(lines::text(&root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?);
    let store = common.join("aaa-workspace-runs");
    let source = Snapshot::read(&root);
    let old = archive_dirs(&store);
    let start = aaa_common::util::now_ms();
    let clock = Instant::now();
    let (status, output) = execute(&root, &args, stdout, stderr)?;
    let duration_ms = clock.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let end = aaa_common::util::now_ms();
    let recorded = (|| -> Result<()> {
        let source = source?;
        let after = Snapshot::read(&root)?;
        anyhow::ensure!(source.clean && source == after, "미커밋 변경 또는 실행 중 출처 변경으로 원장 미수집");
        anyhow::ensure!(start <= end, "검증 중 시스템 시각 역행으로 원장 미수집");
        if native_recorded(&root, &store, &old?, &source, start, end) {
            eprintln!("aaa verify: 기존 검증 원장을 사용함");
            return Ok(());
        }
        let mut run = MetricRun {
            outcome: if status.success() { "succeeded" } else { "failed" }.into(),
            verification_ms: Some(duration_ms),
            build_ms: output.build_ms,
            tests: if output.incomplete { None } else { output.tests },
            profile: (output.profiles.len() == 1).then(|| output.profiles.first().unwrap().clone()),
            slow_tests: missing_tests(),
            slow_e2e: top(vec![], "scenario-wall", false),
            ..Default::default()
        };
        if run.build_ms.is_some() {
            run.notes.push("빌드 시간: Cargo Finished 컴파일 경과 합계(clippy 포함, 병렬 구간은 작업 합계)".into());
        }
        if output.profiles.len() > 1 {
            run.notes
                .push(format!("여러 Cargo profile: {}", output.profiles.into_iter().collect::<Vec<_>>().join(", ")));
        }
        if output.incomplete {
            run.notes.push("출력 읽기·전달 또는 한 줄 크기 상한으로 테스트 수 일부 미수집".into());
        }
        details(&root, start, end, &mut run);
        if run.tests.is_none() {
            run.notes.push("Rust 종료 요약·상세 결과가 없어 테스트 수 미수집".into());
        }
        if run.e2e_files.is_none() {
            run.notes.push("이번 실행의 E2E 상세 보고가 없어 미수집".into());
        }
        save(&store, &source, start, end, &run)?;
        eprintln!("aaa verify: 종료 원장 기록함 · main 반영 뒤 보고 회수 시 집계");
        Ok(())
    })();
    if let Err(error) = recorded {
        eprintln!("aaa verify: {}", aaa_common::redact::text(&format!("{error:#}")));
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_keeps_bytes_and_counts_without_persisting_unbounded_lines() {
        let line = "test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 3 filtered out; finished in 0.00s\n";
        let input = format!("{}\n{line}", "x".repeat(20_000));
        let mut output = Vec::new();
        let summary = tee(input.as_bytes(), &mut output, false).unwrap();
        assert_eq!(output, input.as_bytes());
        assert_eq!(summary.tests.unwrap().passed, 2);
        assert!(summary.incomplete, "큰 출력은 일부 수집으로 표시한다");
        let summary = tee(line.as_bytes(), std::io::sink(), true).unwrap();
        assert!(summary.tests.is_none(), "stderr에 복사한 요약을 중복 합산하지 않는다");
    }

    #[test]
    fn cargo_compile_work_and_mixed_profiles_do_not_become_test_times() {
        let text = "    Finished `verify` profile [unoptimized + debuginfo] target(s) in 1m 02.50s\n    Finished `e2e` profile [unoptimized] target(s) in 0.25s\n    Finished `other` profile [unoptimized] target(s) in NaNs\n";
        let summary = tee(text.as_bytes(), std::io::sink(), true).unwrap();
        assert_eq!(summary.build_ms, Some(62750));
        assert_eq!(summary.profiles.into_iter().collect::<Vec<_>>(), ["e2e", "verify"]);
        assert!(summary.tests.is_none());
    }
}
