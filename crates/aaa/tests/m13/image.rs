//! 실제 CLI→hook(peer PID·token)→TLS agent link→host 계정→가짜 Codex→조각 전송.
use super::secret_k0::{canary, hits, needles, sha};
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

fn probe_script(command: &str, out: &Path, done: &Path) -> String {
    let writing = done.with_extension("done.tmp");
    // 셸은 printf 실행 전에 파일을 연다. 같은 폴더에서 쓰기·close 뒤 rename해야
    // 완료 표지를 읽은 쪽이 빈 파일을 보지 않는다. 출력 파일도 이때 이미 닫혔다.
    format!("{command} > {out:?} 2>&1\nprintf '%s' \"$?\" > {writing:?} && mv {writing:?} {done:?}\n")
}

fn probe(w: &World, pane: &str, name: &str, command: &str) -> (i32, String) {
    let out = w.lab.root.join(format!("image-{name}.out"));
    let done = w.lab.root.join(format!("image-{name}.done"));
    let script = w.task(&format!("image-{name}.sh"), &probe_script(command, &out, &done));
    w.h.ok(&["pane", "run", pane, &format!("post-probe {script}")]);
    let exit = wait_for_diag(
        "이미지 CLI 완료 사건",
        T,
        || std::fs::read_to_string(&done).ok(),
        || format!("{}\n{}", w.h.screen(pane), std::fs::read_to_string(&out).unwrap_or_default()),
    );
    let rc = exit.parse().unwrap();
    (rc, std::fs::read_to_string(out).unwrap())
}

/// printf의 리다이렉션이 파일을 연 바로 뒤를 pipe로 붙잡는다. 파일 생성과 쓰기 사이를
/// 실제로 관측하므로 빠른 머신에서도 빈 완료 표지 경합을 결정적으로 구별한다.
#[test]
fn probe_completion_is_published_only_after_exit_code_is_written() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    let lab = Lab::new();
    let out = lab.root.join("probe.out");
    let done = lab.root.join("probe.done");
    let script = format!(
        "exec 3>&1\nprintf() {{ command printf 'exit-code-open\\n' >&3; IFS= read -r release; command printf \"$@\"; }}\n{}",
        probe_script("(echo probe-output; exit 7)", &out, &done)
    );
    let mut child =
        Command::new("/bin/sh").args(["-c", &script]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let early = std::fs::read_to_string(&done).ok();
    let writing = std::fs::read_to_string(done.with_extension("done.tmp")).ok();
    child.stdin.take().unwrap().write_all(b"release\n").unwrap();
    let status = child.wait().unwrap();
    assert_eq!(line.trim(), "exit-code-open");
    assert!(status.success(), "{status}");
    assert!(early.is_none(), "완료 전에 표지를 공개했다: {early:?}");
    assert_eq!(writing.as_deref(), Some(""), "쓰기 전 경계를 실제로 관측한다");
    assert_eq!(std::fs::read_to_string(&done).unwrap(), "7");
    assert!(!done.with_extension("done.tmp").exists());
    assert_eq!(std::fs::read_to_string(out).unwrap(), "probe-output\n");
}
fn result(text: &str) -> Value {
    text.lines()
        .rev()
        .find_map(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or_else(|| panic!("JSON 응답 없음: {text}"))
}
fn attempts(w: &World) -> Vec<Value> {
    std::fs::read_to_string(w.scripts.join("image-attempts.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
fn control(w: &World, value: Value) {
    std::fs::write(w.scripts.join("image-control.json"), value.to_string()).unwrap();
}
fn restart_host(w: &mut World, tag: &str, exe: &Path, upstream: &str) {
    // 실제 원격 agent는 주소를 고정한다. host 재시작에서도 같은 listener를 유지한다.
    let config = w.h.cfg.join("host.toml");
    let text = std::fs::read_to_string(&config)
        .unwrap()
        .replace("agent_listen = \"127.0.0.1:0\"", &format!("agent_listen = {:?}", w.addr));
    std::fs::write(config, text).unwrap();
    w.lab.spawn_env(
        tag,
        &w.h,
        &["host"],
        &[("AAA_TEST_IMAGE_CODEX", path_str(exe)), ("AAA_LOG", "trace"), ("AAA_TEST_ACCOUNT_REFRESH_ORIGIN", upstream)],
    );
    let status = wait_for("이미지 host 준비", T, || w.h.json(&["status"]).ok());
    w.addr = status["agent_listen"].as_str().unwrap().into();
    wait_for("이미지 로컬 agent 재접속", T, || w.h.connected("labA").then_some(()));
}

#[test]
fn host_account_images_transfer_to_local_and_remote_threads_without_credentials_or_duplicates() {
    // 원시 PNG와 무관한 인증 조각이 base64 offset 3787에서만 겹친다.
    // 로컬·원격 agent 양쪽의 실제 가림 규칙에 등록한다.
    let mut w = world_with(&format!(
        "[profiles.fake.env]\nOPENAI_API_KEY = {:?}\n",
        aaa_test_support::token("image-collision")
    ));
    w.lab.kill("host");
    let cfg = w.h.cfg.join("host.toml");
    let text = std::fs::read_to_string(&cfg).unwrap().replace("allow_profiles = [", "allow_profiles = [\"image\", ");
    std::fs::write(&cfg, format!("{text}\n[profiles.image]\nagent=\"codex\"\nmodel=\"model-a\"\neffort=\"high\"\n"))
        .unwrap();
    let exe = w.scripts.join("fake-image-codex");
    std::fs::write(&exe, include_str!("../../../../scripts/fixtures/image-codex.py")).unwrap();
    std::fs::write(w.scripts.join("fixture_secret.py"), include_str!("../../../../scripts/fixture_secret.py")).unwrap();
    std::fs::set_permissions(&exe, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let (access, access_hex) = canary("a");
    let (refresh, refresh_hex) = canary("r");
    let (other, other_hex) = canary("b");
    let (renewed, renewed_hex) = canary("c");
    let (renewed_refresh, renewed_refresh_hex) = canary("d");
    let upstream = super::accounts::Upstream::new(vec![
        (access.clone(), refresh.clone()),
        (renewed.clone(), renewed_refresh.clone()),
    ]);
    let origin = format!("http://{}", upstream.addr);
    control(&w, serde_json::json!({"refresh":true}));
    restart_host(&mut w, "image-host", &exe, &origin);
    w.add_remote();
    for (alias, token) in [("work-codex", &access), ("other-codex", &other)] {
        let path = w.scripts.join("image-import.json");
        std::fs::write(&path, serde_json::json!({"tokens":{"access_token":token,"refresh_token":refresh}}).to_string())
            .unwrap();
        w.h.ok(&["account", "import", alias, "--harness", "codex", "--file", path_str(&path)]);
        // 기본 이미지 계정의 권한 경로를 자동 배정과 분리한다.
        w.h.ok(&["account", "pool", "remove", alias]);
        std::fs::remove_file(path).unwrap();
    }
    w.h.ok(&["account", "default", "codex", "work-codex"]);
    let task = w.task("image-task.txt", "noreport");
    let local = w.start(&["--title", "이미지 로컬", "--profile", "fake", "--task-file", &task]);
    let remote = w.start(&["--title", "이미지 원격", "--profile", "fake", "--machine", "labB", "--task-file", &task]);
    let local =
        w.wait_show("이미지 로컬 thread 안내", local["id"].as_str().unwrap(), |v| v["brief"] == "delivered");
    let remote =
        w.wait_show("이미지 원격 thread 안내", remote["id"].as_str().unwrap(), |v| v["brief"] == "delivered");
    let a = local["pane"].as_str().unwrap();
    let b = remote["pane"].as_str().unwrap();
    let prompt = w.scripts.join("image-prompt.txt");
    std::fs::write(&prompt, "작은 초록 원의 PNG 이미지").unwrap();
    let reference = w.scripts.join("image-reference.png");
    std::fs::write(
        &reference,
        STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==")
            .unwrap(),
    )
    .unwrap();
    let id = aaa_common::util::new_id();
    let generate = |id: &str, dir: &Path| {
        format!(
            "{:?} --json image generate --id {id} --profile image --prompt-file {prompt:?} --image {reference:?} --output-dir {dir:?}",
            lab::BIN
        )
    };
    let received = w.lab.root.join("received-a");
    for (name, command, reason) in [
        (
            "profile-denied",
            generate(&aaa_common::util::new_id(), &received).replace("--profile image", "--profile other"),
            "허용",
        ),
        (
            "account-denied",
            format!("{} --account other-codex", generate(&aaa_common::util::new_id(), &received)),
            "기본 계정",
        ),
    ] {
        let (rc, text) = probe(&w, a, name, &command);
        assert_ne!(rc, 0);
        assert!(text.contains(reason), "{text}");
    }
    assert!(attempts(&w).is_empty(), "허용하지 않은 profile·계정은 helper를 띄우지 않는다");
    let private_profile =
        generate(&aaa_common::util::new_id(), &received).replace("--profile image", &format!("--profile {access}"));
    let (rc, text) = probe(&w, a, "private-error", &private_profile);
    assert_ne!(rc, 0);
    assert!(!text.contains(&access) && text.contains("허용"), "이미지 오류 응답에도 인증 원문이 없어야 한다");
    let (rc, text) = probe(&w, a, "first", &generate(&id, &received));
    assert_eq!(rc, 0, "{text}");
    let first = result(&text);
    assert_eq!(first["job"]["state"], "succeeded");
    assert_eq!(first["job"]["account"], "work-codex");
    assert_eq!(first["job"]["total_tokens"], 123);
    let output = Path::new(first["files"][0].as_str().unwrap());
    assert_eq!(std::fs::metadata(output).unwrap().len(), 544 * 1024);
    assert_eq!(attempts(&w).len(), 1);
    assert_eq!(upstream.calls.load(std::sync::atomic::Ordering::SeqCst), 1, "기존 중앙 회전 owner로 한 번 갱신");
    let accounts = w.h.ok(&["account", "list"]);
    assert_eq!(
        accounts["accounts"].as_array().unwrap().iter().find(|v| v["name"] == "work-codex").unwrap()["version"],
        2
    );
    assert_eq!(attempts(&w)[0]["access_hash"], sha(&access));
    assert_eq!(attempts(&w)[0]["inputs"], 1);
    assert!(!Path::new(attempts(&w)[0]["home"].as_str().unwrap()).exists(), "인증·임시 홈 정리 뒤 성공");

    w.h.ok(&["account", "default", "codex", "other-codex"]);
    let (rc, text) = probe(&w, a, "same", &generate(&id, &received));
    assert_eq!(rc, 0, "{text}");
    assert_eq!(result(&text)["job"]["account"], "work-codex");
    assert_eq!(attempts(&w).len(), 1, "같은 ID는 기본 계정 변경 뒤에도 재생성하지 않는다");
    let id_b = aaa_common::util::new_id();
    control(&w, serde_json::json!({}));
    let (rc, text) = probe(&w, b, "remote", &generate(&id_b, &w.lab.root.join("received-b")));
    assert_eq!(rc, 0, "{text}");
    let second = result(&text);
    assert_eq!(second["job"]["account"], "other-codex");
    assert_eq!(attempts(&w).len(), 2);
    assert_eq!(attempts(&w)[1]["access_hash"], sha(&other));
    assert_eq!(std::fs::read(output).unwrap(), std::fs::read(second["files"][0].as_str().unwrap()).unwrap());
    for (name, command, reason) in [
        ("foreign", format!("{:?} image status {id}", lab::BIN), "자기 이미지 작업"),
        ("token", format!("env AAA_HOOK_TOKEN=forged {:?} image status {id_b}", lab::BIN), "token"),
        ("missing", format!("env -u AAA_SESSION_ID {:?} image status {id_b}", lab::BIN), "세션 안"),
    ] {
        let (rc, text) = probe(&w, b, name, &command);
        assert_ne!(rc, 0);
        assert!(text.contains(reason), "{text}");
    }
    let changed = w.scripts.join("changed-prompt.txt");
    std::fs::write(&changed, "다른 이미지").unwrap();
    let (rc, text) = probe(&w, a, "changed", &generate(&id, &received).replace(path_str(&prompt), path_str(&changed)));
    assert_ne!(rc, 0);
    assert!(text.contains("다른 요청"), "{text}");
    control(&w, serde_json::json!({"version":"codex-cli 0.0.0"}));
    let arbitrary_version = aaa_common::util::new_id();
    let (rc, text) = probe(&w, a, "arbitrary-version", &generate(&arbitrary_version, &received));
    assert_eq!(rc, 0, "{text}");
    assert_eq!(attempts(&w).len(), 3, "판은 생성 허용 여부를 결정하지 않는다");
    control(&w, serde_json::json!({"mode":"request"}));
    let denied = aaa_common::util::new_id();
    let (rc, _) = probe(&w, a, "approval", &generate(&denied, &received));
    assert_ne!(rc, 0);
    assert_eq!(attempts(&w).len(), 4);
    let (rc, _) =
        probe(&w, a, "approval-again", &format!("{:?} image start {denied} --output-dir {received:?}", lab::BIN));
    assert_ne!(rc, 0);
    assert_eq!(attempts(&w).len(), 4, "실패한 생성은 명시 Start에도 재실행하지 않는다");

    w.lab.kill("image-host");
    restart_host(&mut w, "image-host2", &exe, &origin);
    wait_for("이미지 원격 재접속", T, || w.h.connected("labB").then_some(()));
    let (rc, text) = probe(&w, b, "after-restart", &format!("{:?} --json image status {id_b}", lab::BIN));
    assert_eq!(rc, 0, "{text}");
    assert_eq!(result(&text)["job"]["state"], "succeeded");
    w.h.ok(&["pane", "kill", a]);
    wait_for("이미지 원래 pane 종료", T, || (w.h.pane(a)["state"] == "exited").then_some(()));
    w.h.ok(&["thread", "restart", "demo", local["id"].as_str().unwrap()]);
    let restarted = w.wait_show("이미지 thread 새 pane 안내", local["id"].as_str().unwrap(), |v| {
        v["pane"].as_str() != Some(a) && v["brief"] == "delivered"
    });
    let a = restarted["pane"].as_str().unwrap();
    let (rc, text) = probe(
        &w,
        a,
        "same-thread-new-pane",
        &format!("{:?} --json image fetch {id} --output-dir {received:?}", lab::BIN),
    );
    assert_eq!(rc, 0, "{text}");
    assert_eq!(attempts(&w).len(), 4, "같은 thread 재시작은 기존 이미지를 읽는다");
    let (rc, text) = probe(&w, a, "own-list", &format!("{:?} --json image list", lab::BIN));
    assert_eq!(rc, 0, "{text}");
    assert!(
        !result(&text)["jobs"].as_array().unwrap().iter().any(|j| j["id"] == id_b),
        "worker는 다른 thread 목록을 보지 않는다"
    );
    let text = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(
        &cfg,
        format!(
            "{text}\n[profiles.image-coord]\nagent=\"custom\"\ncommand=[\"/bin/sh\",{:?}]\n",
            w.scripts.join("coord.sh")
        ),
    )
    .unwrap();
    let opened = w.h.ok(&["project", "open", "demo", "--profile", "image-coord"]);
    let coord = opened["pane"]["id"].as_str().unwrap();
    w.wait_screen(coord, "❯");
    let (rc, text) = probe(&w, coord, "project-list", &format!("{:?} --json image list", lab::BIN));
    assert_eq!(rc, 0, "{text}");
    let listed = result(&text);
    assert!(listed["jobs"].as_array().unwrap().iter().any(|j| j["id"] == id));
    assert!(listed["jobs"].as_array().unwrap().iter().any(|j| j["id"] == id_b));
    let (rc, text) = probe(&w, coord, "forget", &format!("{:?} --json image forget {id}", lab::BIN));
    assert_eq!(rc, 0, "{text}");
    assert_eq!(result(&text)["job"]["state"], "forgotten");
    let (rc, _) = probe(&w, a, "forgotten-again", &generate(&id, &received));
    assert_ne!(rc, 0);
    assert_eq!(attempts(&w).len(), 4);

    let mut find = needles(&access, &access_hex);
    find.extend(needles(&refresh, &refresh_hex));
    find.extend(needles(&other, &other_hex));
    find.extend(needles(&renewed, &renewed_hex));
    find.extend(needles(&renewed_refresh, &renewed_refresh_hex));
    assert!(
        hits(
            &[
                w.h.state.clone(),
                w.lab.root.join("b/s"),
                w.projects.clone(),
                w.lab.root.join("image-host.log"),
                w.lab.root.join("image-host2.log")
            ],
            &find,
            &|_| false
        )
        .is_empty(),
        "인증이 원격·기록에 남았다"
    );
}
