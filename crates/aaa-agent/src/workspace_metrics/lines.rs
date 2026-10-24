//! Git 객체 기준의 증분 집계. dirty 파일은 읽지 않는다. 한 수집의 소스 bytes·파일 수·경과를 제한한다.
use aaa_proto::CodeLines;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const RULES: u32 = 1;
const MAX_BYTES: usize = 32 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    blob: String,
    language: String,
    category: String,
    lines: u32,
    mixed: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Cache {
    commit: String,
    files: BTreeMap<String, Entry>,
    snapshot: CodeLines,
}
fn git(cwd: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut child = Command::new("git")
        .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"])
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut bytes = Vec::new();
    let read = child.stdout.take().unwrap().take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes);
    if read
        .as_ref()
        .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_err()
        || bytes.len() > 2 * 1024 * 1024
    {
        aaa_common::diagnostic::ignored(child.kill(), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
        aaa_common::diagnostic::ignored(child.wait(), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
        bail!("Git 목록 비용 상한 또는 읽기 실패");
    }
    let status = child.wait()?;
    ensure!(status.success(), "Git {} 종료 실패: {status}", args.join(" "));
    Ok(bytes)
}
pub(super) fn text(cwd: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(git(cwd, args)?)?.trim().to_string())
}
fn language(path: &str) -> Option<&'static str> {
    if ["vendor/", "docs/", "doc/", "scratch/", "target/", "web/dist/", "web/public/", "web/src/gen/", ".aaa/"]
        .iter()
        .any(|p| path.starts_with(p))
    {
        return None;
    }
    match path.rsplit('.').next()? {
        "rs" => Some("Rust"),
        "ts" | "tsx" => Some("TypeScript"),
        "js" | "mjs" | "cjs" => Some("JavaScript"),
        "py" => Some("Python"),
        "sh" => Some("Shell"),
        "css" => Some("CSS"),
        "html" => Some("HTML"),
        _ => None,
    }
}
fn category(path: &str) -> &'static str {
    if path.starts_with("web/e2e/") || path.starts_with("e2e/") {
        "e2e"
    } else if path.split('/').any(|p| matches!(p, "tests" | "test"))
        || path.ends_with("tests.rs")
        || path.contains(".test.")
        || path.contains(".spec.")
        || path.rsplit('/').next().is_some_and(|p| p.starts_with("test_") || p.ends_with("_tests.py"))
    {
        "test"
    } else {
        "product"
    }
}
fn inline_tests(body: &str) -> bool {
    static ATTR: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?s)#\s*\[\s*(?:cfg(?:_attr)?\s*\([^]]*\btest\b|(?:[\w:]+::)?test\b)").unwrap()
    });
    ATTR.is_match(body)
}
pub(super) fn collect_main(cwd: &Path, store: &Path, known: &[String]) -> Result<Option<CodeLines>> {
    let started = Instant::now();
    collect_revision(cwd, store, known, || started.elapsed(), "refs/remotes/origin/main", Some("main"))
}
// Git 실행을 포함한 경과의 출처만 분리한다. 시험도 같은 750ms 검사·실패 기록·캐시 경로를 쓴다.
#[cfg(test)]
pub(super) fn collect_with_elapsed(
    cwd: &Path,
    store: &Path,
    known: &[String],
    elapsed: impl FnMut() -> Duration,
) -> Result<Option<CodeLines>> {
    collect_revision(cwd, store, known, elapsed, "HEAD", None)
}
fn collect_revision(
    cwd: &Path,
    store: &Path,
    known: &[String],
    mut elapsed: impl FnMut() -> Duration,
    revision: &str,
    branch: Option<&str>,
) -> Result<Option<CodeLines>> {
    let info = text(cwd, &["show", "-s", "--format=%H%n%T%n%ct", revision])?;
    let fields: Vec<_> = info.lines().collect();
    ensure!(fields.len() == 3, "커밋 자료 없음");
    let commit = fields[0];
    let tree = fields[1];
    if known.iter().any(|c| c == &format!("{commit}:{RULES}")) {
        return Ok(None);
    }
    let committed = fields[2].parse::<u64>()?.saturating_mul(1000);
    let path = store.join("lines.json");
    let mut cache: Cache = match fs::read(&path) {
        Ok(b) => serde_json::from_slice(&b)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Cache::default(),
        Err(e) => return Err(e.into()),
    };
    if cache.snapshot.rules_version != RULES {
        cache = Cache::default();
    }
    if cache.commit == commit {
        return Ok(Some(cache.snapshot));
    }
    // 비용 상한·손상 자료로 실패한 동일 커밋을 보고마다 다시 전체 스캔하지 않는다.
    // 다음 커밋은 마지막 성공 스냅샷에서 증분 집계를 다시 시작할 수 있다.
    let attempt_path = store.join("lines-attempt.json");
    let attempted: Option<(u32, String)> = fs::read(&attempt_path)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .and_then(|b| {
            serde_json::from_slice(&b)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        });
    ensure!(attempted.as_ref() != Some(&(RULES, commit.to_string())), "같은 커밋의 줄 수 집계는 이미 시도함");
    aaa_common::util::write_json(&attempt_path, &(RULES, commit))?;
    let branch = branch.map(str::to_string).unwrap_or_else(|| {
        text(cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_else(|_| "detached".into())
    });
    let changed = if cache.commit.is_empty() {
        None
    } else {
        Some(git(cwd, &["diff-tree", "--no-commit-id", "--name-only", "-r", "-z", &cache.commit, commit])?)
    };
    let changed: Option<Vec<String>> = changed
        .map(|b| b.split(|c| *c == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8(p.to_vec())).collect())
        .transpose()?;
    ensure!(changed.as_ref().is_none_or(|c| c.len() <= 512), "변경 파일 비용 상한");
    let mut args = vec!["ls-tree", "-r", "-z", "--long", commit];
    if let Some(changed) = &changed
        && !changed.is_empty()
    {
        args.push("--");
        args.extend(changed.iter().map(String::as_str));
    }
    let listing = if changed.as_ref().is_some_and(|c| c.is_empty()) { vec![] } else { git(cwd, &args)? };
    let mut objects = Vec::new();
    let mut total = 0usize;
    for row in listing.split(|c| *c == 0).filter(|r| !r.is_empty()) {
        let row = std::str::from_utf8(row)?;
        let Some((meta, name)) = row.split_once('\t') else { bail!("Git tree 형식 오류") };
        let Some(lang) = language(name) else { continue };
        let m: Vec<_> = meta.split_whitespace().collect();
        if m.len() != 4 || m[1] != "blob" || m[0] == "120000" {
            continue;
        }
        let size = m[3].parse::<usize>()?;
        total = total.saturating_add(size);
        ensure!(total <= MAX_BYTES && objects.len() < 2048, "최초 집계 비용 상한");
        objects.push((name.to_string(), m[2].to_string(), size, lang.to_string()));
    }
    // 삭제·rename과 분류 변경은 이전 값을 지운 뒤 새 경로로 넣는다.
    if let Some(changed) = &changed {
        for p in changed {
            cache.files.remove(p);
        }
    }
    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let answer = (|| -> Result<()> {
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        for (name, blob, size, lang) in &objects {
            ensure!(elapsed() < Duration::from_millis(750), "코드 집계 경과 상한");
            writeln!(input, "{blob}")?;
            input.flush()?;
            let mut header = String::new();
            output.read_line(&mut header)?;
            ensure!(header.trim() == format!("{blob} blob {size}"), "Git blob 응답 불일치");
            let mut bytes = vec![0; *size];
            output.read_exact(&mut bytes)?;
            let mut newline = [0];
            output.read_exact(&mut newline)?;
            let body = std::str::from_utf8(&bytes).context("소스 UTF-8 오류")?;
            let cat = category(name);
            let mixed = cat == "product" && lang == "Rust" && inline_tests(body);
            let lines = body.lines().filter(|l| !l.trim().is_empty()).count() as u32;
            cache.files.insert(
                name.clone(),
                Entry { blob: blob.clone(), language: lang.clone(), category: cat.into(), lines, mixed },
            );
        }
        Ok(())
    })();
    // stdin을 닫았으므로 정상 종료한다. 비용 상한도 다른 프로세스에는 영향을 주지 않는다.
    let status = child.wait()?;
    answer?;
    ensure!(status.success(), "Git batch 종료 실패");
    let mut by_language = BTreeMap::new();
    let mut breakdown = BTreeMap::new();
    let mut total_lines = 0u32;
    let mut mixed = false;
    for f in cache.files.values() {
        total_lines = total_lines.saturating_add(f.lines);
        *by_language.entry(f.language.clone()).or_insert(0u32) += f.lines;
        *breakdown.entry(f.category.clone()).or_insert(0u32) += f.lines;
        mixed |= f.mixed;
    }
    let snapshot = CodeLines {
        commit: commit.into(),
        tree: tree.into(),
        committed_at_ms: committed,
        observed_at_ms: aaa_common::util::now_ms(),
        branch,
        rules_version: RULES,
        counting_unit: "nonblank_source_lines_including_comments".into(),
        total_lines,
        by_language,
        breakdown: (!mixed).then_some(breakdown),
        split_status: if mixed { "mixed_inline_tests" } else { "path_rules" }.into(),
        scanned_files: cache.files.len() as u32,
        changed_files: objects.len() as u32,
        source: "git-object-batch".into(),
    };
    cache.commit = commit.into();
    cache.snapshot = snapshot.clone();
    aaa_common::util::write_json(&path, &cache)?;
    Ok(Some(snapshot))
}
