//! 같은 검증 실행의 재수집은 이미 알려진 값을 지우거나 다른 실행을 섞지 않는다.
use aaa_proto::{MetricRanking, MetricRun};
use anyhow::Result;

pub fn enrich(old: &MetricRun, new: MetricRun) -> Result<MetricRun> {
    anyhow::ensure!(old.run_id == new.run_id, "검증 ID 불일치");
    anyhow::ensure!(old.commit == new.commit, "같은 검증 ID의 commit 불일치");
    anyhow::ensure!(old.observed_at_ms == new.observed_at_ms, "같은 검증 ID의 종료 시각 불일치");
    anyhow::ensure!(old.machine == new.machine, "같은 검증 ID의 머신 불일치");
    anyhow::ensure!(old.outcome == new.outcome, "같은 검증 ID의 성공/실패 불일치");
    let mut run = old.clone();
    run.profile = run.profile.or(new.profile);
    run.verification_ms = run.verification_ms.or(new.verification_ms);
    run.build_ms = run.build_ms.or(new.build_ms);
    // 개수와 순위가 다른 실행 범위를 가리키지 않게 묶어서 보강한다.
    if run.tests.is_none() || new.tests.is_none() || run.tests == new.tests {
        run.tests = run.tests.or(new.tests);
        enrich_ranking(&mut run.slow_tests, new.slow_tests);
    }
    if (run.e2e_files.is_none() || new.e2e_files.is_none() || run.e2e_files == new.e2e_files)
        && (run.e2e_unit.is_none() || new.e2e_unit.is_none() || run.e2e_unit == new.e2e_unit)
        && (run.e2e_jobs.is_none() || new.e2e_jobs.is_none() || run.e2e_jobs == new.e2e_jobs)
    {
        run.e2e_files = run.e2e_files.or(new.e2e_files);
        run.e2e_unit = run.e2e_unit.or(new.e2e_unit);
        run.e2e_jobs = run.e2e_jobs.or(new.e2e_jobs);
        enrich_ranking(&mut run.slow_e2e, new.slow_e2e);
    }
    if run.stages.is_empty() {
        run.stages = new.stages;
    }
    if run.artifact_bytes.is_none() || new.artifact_bytes.is_none() || run.artifact_bytes == new.artifact_bytes {
        run.artifact_bytes = run.artifact_bytes.or(new.artifact_bytes);
        run.artifact_name = run.artifact_name.or(new.artifact_name);
    }
    for note in new.notes {
        if !run.notes.contains(&note) && run.notes.len() < 32 {
            run.notes.push(note);
        }
    }
    if run.tests.is_some() {
        run.notes.retain(|note| {
            !matches!(
                note.as_str(),
                "Rust 결과 일부가 없어서 테스트 수 미수집" | "테스트 수는 기존 결과 일부 누락으로 미수집"
            )
        });
    }
    Ok(run)
}

fn enrich_ranking(old: &mut MetricRanking, new: MetricRanking) {
    // TOP 5끼리 합치면 서로 다른 집계 범위를 완전한 순위로 오인하게 된다.
    // 완전한 순위는 유지하고 부분 순위는 같은 단위의 더 온전한 입력으로만 교체한다.
    if !old.complete
        && (old.items.is_empty() || old.unit == new.unit)
        && (old.items.is_empty() || !new.items.is_empty())
        && (new.complete || (!new.items.is_empty() && old.items.iter().all(|item| new.items.contains(item))))
    {
        *old = new;
    }
}
