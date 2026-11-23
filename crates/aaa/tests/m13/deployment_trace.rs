//! 배포 시험의 PTY 입력·셸 실행과 양쪽 backend 기록을 실패한 shard에 남긴다.
use super::{Node, T, World, lab};
use aaa_proto::{DeployGuardView, RolloutView};
use std::{fs, path::Path, time::UNIX_EPOCH};

fn modified_ms(path: &Path) -> Option<u128> {
    fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|time| time.as_millis())
}

pub(super) fn backend(role: &str, event: &str) {
    eprintln!("deploy backend utc_ms={} role={role} event={event}", aaa_common::util::now_ms());
}

pub(super) fn probe(w: &World, agent: &Node, pane: &str, number: &mut usize, args: &str) -> (String, String, String) {
    *number += 1;
    let n = *number;
    let prefix = w.lab.root.join(format!("deploy-probe-{n}"));
    let marker = format!("deploy-finished-{n}");
    let script = w.task(
        &format!("deploy-probe-{n}.sh"),
        &format!(
            ": > '{prefix}.started'\n'{bin}' --json {args} > '{prefix}.out' 2> '{prefix}.err'\nprintf '%s' \"$?\" > '{prefix}.rc'\nprintf '{marker}\\n' && : > '{prefix}.emitted'\n",
            prefix = prefix.display(),
            bin = lab::BIN,
        ),
    );
    let event = |event| eprintln!("deploy probe utc_ms={} n={n} event={event}", aaa_common::util::now_ms());
    let read = |suffix| fs::read_to_string(prefix.with_extension(suffix));
    let stages = || {
        let times = ["started", "rc", "emitted"].map(|ext| modified_ms(&prefix.with_extension(ext)));
        let exit = read("rc").ok().and_then(|text| text.parse::<u8>().ok());
        eprintln!("deploy probe n={n} shell_start,cli_exit,pty_write_utc_ms={times:?} exit={exit:?}");
    };
    let failure = |stage: &'static str, error: String| -> ! {
        event(stage);
        stages();
        snapshot(w, agent);
        panic!("deploy probe {n} {stage}: {error}\n화면:\n{}", w.h.screen(pane));
    };

    event("submitted");
    w.h.json(&["pane", "run", pane, &format!("post-probe {script}")])
        .unwrap_or_else(|error| failure("submit_failed", error));
    event("pty_input_acked");
    let ms = T.as_millis().to_string();
    w.h.json(&["pane", "wait-output", pane, &format!("--match={marker}"), "--timeout", &ms])
        .unwrap_or_else(|error| failure("output_wait_failed", error));
    event("output_observed");
    stages();
    (read("rc").unwrap(), read("out").unwrap(), read("err").unwrap())
}

fn snapshot(w: &World, agent: &Node) {
    let supervisor = w.lab.root.join("service-calls");
    let calls = fs::read_to_string(&supervisor).ok().map(|text| text.lines().count());
    eprintln!("deploy supervisor calls={calls:?} last_utc_ms={:?}", modified_ms(&supervisor));
    for role in ["host", "host-restored", "agentA", "agentA-restored", "agentB-copy", "agentB-restarted"] {
        for line in w.lab.log(role).lines() {
            for (message, event) in [
                ("agent 연결 끊김", "disconnected"),
                ("agent 연결 machine=", "accepted"),
                ("host에 연결했다", "connected"),
                ("rollout 기록/진행 실패", "rollout_driver_failed"),
            ] {
                if line.contains(message)
                    && let Some(time) = line.split_whitespace().next().filter(|time| time.ends_with('Z'))
                {
                    let peer = ["labA", "labB"].iter().find(|name| line.contains(&format!("machine={name}")));
                    eprintln!("deploy backend utc={time} role={role} event={event} peer={peer:?}");
                    break;
                }
            }
        }
    }
    if let Err(error) = host_rollouts(&w.h.state.join("host/host.db")) {
        eprintln!("deploy host 상태 읽기 실패: {error}");
    }
    if let Ok(entries) = fs::read_dir(agent.state.join("agent/deployments")) {
        for file in entries.flatten().filter(|file| file.file_name().to_string_lossy().ends_with(".guard.json")) {
            let path = file.path();
            if let Ok(bytes) = fs::read(&path)
                && let Ok(guard) = serde_json::from_slice::<DeployGuardView>(&bytes)
            {
                eprintln!(
                    "deploy guard id={} phase={:?} pid={} updated_utc_ms={:?} restart_index={} restart_started={} restart_utc_ms={} error={}",
                    guard.spec.id,
                    guard.phase,
                    guard.worker_pid,
                    modified_ms(&path),
                    guard.restart_index,
                    guard.restart_started,
                    guard.agent_restart_ms,
                    guard.error.is_some()
                );
            }
        }
    }
}

fn host_rollouts(path: &Path) -> anyhow::Result<()> {
    use rusqlite::{Connection, OpenFlags};
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::ZERO)?;
    // 같은 접두어 아래 완료 알림 표지도 저장한다. ULID가 바로 뒤에 오는 상태 행만 읽는다.
    let mut query = db.prepare("SELECT value FROM meta WHERE key GLOB 'rollout:*' AND length(key) = 34")?;
    for row in query.query_map([], |row| row.get::<_, String>(0))? {
        let view: RolloutView = serde_json::from_str(&row?)?;
        eprintln!(
            "deploy host id={} state={:?} cursor={} steps={} updated_utc_ms={} guard={:?} error={}",
            view.id,
            view.state,
            view.cursor,
            view.steps.len(),
            view.updated_ms,
            view.recovery.as_ref().map(|guard| &guard.spec.id),
            view.error.is_some()
        );
    }
    Ok(())
}
