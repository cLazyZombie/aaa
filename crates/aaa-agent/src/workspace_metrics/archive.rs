//! CI가 보존한 작은 종료 요약. 실행한 커밋이 push된 main 본선에 올라온 뒤에만 확정한다.
use super::*;
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct Provenance {
    format: u32,
    commit: String,
    tree: String,
    remote: String,
    clean: bool,
    started_at_ms: u64,
    ended_at_ms: u64,
    #[serde(default)]
    artifact_bytes: Option<u64>,
    #[serde(default)]
    runner: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    profile: Option<String>,
}

pub(super) fn collect(state: &Path, cwd: &Path, remote: &str, commit: &str) -> Result<(Vec<MetricRun>, Vec<String>)> {
    let common = lines::text(cwd, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let root = Path::new(&common).join("aaa-workspace-runs");
    if !root.exists() {
        return Ok((vec![], vec![]));
    }
    let key = hex::encode(Sha256::digest(format!("{remote}:{common}:archive").as_bytes()));
    let store = state.join("workspace-metrics-v2").join(key);
    fs::create_dir_all(&store)?;
    let _lock = CollectionLock(aaa_common::private_file::lock(&store.join("collect.lock"))?);
    let path = store.join("runs.json");
    let mut cache: Cache = match fs::read(&path) {
        Ok(b) => serde_json::from_slice(&b)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Cache::default(),
        Err(e) => return Err(e.into()),
    };
    // 후보 브랜치의 조상이라는 이유만으로 main 검증으로 섞지 않는다.
    let history: BTreeSet<_> = lines::text(cwd, &["rev-list", "--first-parent", "--max-count=4096", commit])?
        .lines()
        .map(str::to_string)
        .collect();
    let mut dirs = Vec::new();
    for entry in fs::read_dir(root)?.take(4096) {
        let entry = entry?;
        if !entry.file_type()?.is_dir() || !entry.file_name().to_string_lossy().starts_with("run-") {
            continue;
        }
        if let Some((v, at)) = version(&entry.path().join("provenance.json")) {
            dirs.push((entry.path(), v, at));
        }
    }
    dirs.sort_by_key(|(_, _, at)| std::cmp::Reverse(*at));
    let mut notes = Vec::new();
    let mut pending = 0;
    let mut collected = 0;
    for (dir, v, _) in &dirs {
        // 구형 parser가 새 runner를 CI로 오인했거나 format 2를 거부한 캐시도 재해석한다.
        // runs는 그대로 두어 이미 원본이 지워진 확정 기록을 잃지 않는다.
        let v = format!("parser-2:{v}");
        if BUDGET_EXHAUSTED.with(|flag| flag.get()) {
            notes.push("읽기 비용 상한으로 CI 일부 미수집".into());
            break;
        }
        let id = dir.file_name().unwrap().to_string_lossy().into_owned();
        if cache.versions.get(&id) == Some(&v) {
            continue;
        }
        if collected == 32 {
            notes.push("이번 CI 회수는 새 종료 입력 32개까지".into());
            break;
        }
        let parsed = (|| -> Result<Option<MetricRun>> {
            let source: Provenance = serde_json::from_str(&read(&dir.join("provenance.json"))?)?;
            anyhow::ensure!(
                matches!(source.format, 1 | 2) && source.clean && source.remote == remote,
                "CI 실행 출처 불일치"
            );
            anyhow::ensure!(
                source.commit.len() == 40
                    && source.commit.bytes().all(|b| b.is_ascii_hexdigit())
                    && source.started_at_ms <= source.ended_at_ms,
                "CI 커밋·시각 형식 오류"
            );
            if !history.contains(&source.commit) {
                return Ok(None);
            }
            anyhow::ensure!(
                lines::text(cwd, &["rev-parse", &format!("{}^{{tree}}", source.commit)])? == source.tree,
                "CI 실행 tree 불일치"
            );
            let mut run = ci_run(dir, source.ended_at_ms, id.clone())?;
            let runner = source.runner.as_deref().unwrap_or("ci-local");
            let names: &[&str] = match runner {
                "ci-local" | "pt" => &[],
                "rc" => &["default-tests", "workspace-tests"],
                "engine" => &["workspace-tests", "automation-tests", "2d-tests", "3d-tests"],
                _ => bail!("검증 runner 미지원: {runner}"),
            };
            if !names.is_empty() {
                runtime_reports(
                    &mut run,
                    &dir.join("verification"),
                    source.started_at_ms,
                    source.ended_at_ms,
                    names,
                    &dir.join("e2e.json"),
                );
            }
            run.commit = source.commit;
            run.source = format!("{runner} 종료 원장 (main 반영 확인)");
            run.profile = source.profile;
            if let Some(mode) = source.mode {
                run.notes.push(format!("검증 모드 {mode} · Cargo profile과 별도"));
            }
            if let Some(scope) = source.scope {
                run.notes.push(format!("검증 범위 {scope}"));
            }
            run.artifact_bytes = source.artifact_bytes;
            run.artifact_name = source.artifact_bytes.map(|_| "aaa".into());
            Ok(Some(run))
        })();
        match parsed {
            Ok(Some(run)) => {
                collected += 1;
                cache.runs.insert(id.clone(), run);
                cache.versions.insert(id.clone(), v.clone());
                cache.errors.remove(&id);
            }
            Ok(None) => {
                // push 전 부정 결과는 입력 판 캐시에 넣지 않는다. 같은 파일이 push 후 유효해진다.
                pending += 1;
            }
            Err(error) => {
                tracing::warn!(%error, run=%id, "workspace CI 종료 요약 미수집");
                if !BUDGET_EXHAUSTED.with(|flag| flag.get()) {
                    cache.errors.insert(id.clone(), format!("CI 종료 요약 미수집: {error}"));
                    cache.versions.insert(id, v.clone());
                }
            }
        }
        if BUDGET_EXHAUSTED.with(|flag| flag.get()) {
            notes.push("읽기 비용 상한으로 CI 일부 미수집".into());
            break;
        }
    }
    if pending > 0 {
        notes.push(format!("main 본선 반영 대기 CI 실행 {pending}개"));
    }
    notes.extend(cache.errors.values().cloned());
    while cache.runs.len() > 256 {
        let Some(id) = cache.runs.values().min_by_key(|r| r.observed_at_ms).map(|r| r.run_id.clone()) else { break };
        cache.runs.remove(&id);
    }
    anyhow::ensure!(mainline::pushed(cwd)? == (remote.to_string(), commit.to_string()), "CI 수집 중 main 변경");
    aaa_common::util::write_private(&path, &serde_json::to_vec(&cache)?)?;
    Ok((cache.runs.into_values().collect(), notes))
}
