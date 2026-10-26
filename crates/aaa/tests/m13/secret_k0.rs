//! 키 관리자 K0(plan §8.307): pane·thread env의 값이 aaa가 남기는 기록(감사 기록·DB·로그·`AAA_LOG=trace`·프로젝트
//! export)에 나오지 않는다. pane 안에서는 값이 그대로 보인다.
//!
//! 카나리(무작위 가짜 값)는 셋이다.
//! - A·C: host.toml profile env → `pane_run`의 resolved profile. host DB 파일·WAL 전체에서 0건이어야 한다. C는 `/`·`"`·`+`·
//!   공백·`&`를 넣어 URL·JSON escape 변형이 원문과 달라지게 한다.
//! - B: thread launch env. 감사 기록·export·로그에서 0건이어야 한다. `threads.data`의 `launch.env`와 agent
//!   `sessions/*/spec.json`·`threads/` 준비 기록은 K2(참조만 저장)에서 막으므로 여기서 찾지 않는다(키 관리자 기획 §2.1).
use super::*;
use base64::Engine;
use sha2::Digest;

pub(super) fn canary(_tag: &str) -> (String, String) {
    let value = aaa_test_support::fresh_token();
    (value.clone(), value)
}

/// 찾는 꼴: 원문, 무작위 부분의 8자 조각, base64(표준·URL, 앞 0~2바이트 정렬마다 끝 4자를 뺀 가운데), URL 인코딩, JSON escape
pub(super) fn needles(value: &str, hex: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![("원문".to_string(), value.as_bytes().to_vec())];
    for i in 0..=hex.len() - 8 {
        out.push((format!("조각 {i}"), hex.as_bytes()[i..i + 8].to_vec()));
    }
    for (name, engine) in [
        ("base64", &base64::engine::general_purpose::STANDARD),
        ("base64url", &base64::engine::general_purpose::URL_SAFE),
    ] {
        for pad in 0..3 {
            let mut raw = vec![b'#'; pad];
            raw.extend_from_slice(value.as_bytes());
            let enc = engine.encode(&raw);
            out.push((format!("{name} 정렬 {pad}"), enc.as_bytes()[4..enc.len() - 4].to_vec()));
        }
    }
    let url: String = value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    out.push(("URL".into(), url.into_bytes()));
    let json = serde_json::to_string(value).unwrap();
    out.push(("JSON".into(), json.as_bytes()[1..json.len() - 1].to_vec()));
    out
}

/// `files`(폴더면 그 아래 모두, `skip`에 걸린 경로는 뺀다)에서 찾은 곳: 「파일 @바이트 (꼴)」
pub(super) fn hits(files: &[PathBuf], needles: &[(String, Vec<u8>)], skip: &dyn Fn(&Path) -> bool) -> Vec<String> {
    let mut all = Vec::new();
    let mut stack: Vec<PathBuf> = files.to_vec();
    while let Some(p) = stack.pop() {
        if skip(&p) {
            continue;
        }
        if p.is_dir() {
            stack.extend(std::fs::read_dir(&p).unwrap().map(|e| e.unwrap().path()));
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else { continue };
        for (label, n) in needles {
            if let Some(at) = bytes.windows(n.len()).position(|w| w == n.as_slice()) {
                all.push(format!("{} @{at} ({label})", p.display()));
            }
        }
    }
    all
}

pub(super) fn sha(v: &str) -> String {
    hex::encode(sha2::Sha256::digest(v.as_bytes()))
}

/// 받은 env의 hash만 실험실 상태 밖 파일에 쓰고, pane이면 셸로 남는다(값이 실제로 갔다는 증거)
const RECEIVER: &str = "#!/bin/sh\nh() { if command -v sha256sum >/dev/null 2>&1; then sha256sum; else shasum -a 256; fi; }\n\
for v in \"$K0_API_TOKEN\" \"$K0_C\" \"$K0_B\"; do [ -n \"$v\" ] && printf '%s' \"$v\" | h | cut -c1-64; done > \"$K0_OUT.tmp\"\n\
mv \"$K0_OUT.tmp\" \"$K0_OUT\"\necho k0-ready\nexec /bin/sh\n";

#[test]
fn env_values_stay_out_of_audit_db_logs_and_export() {
    let mut lab = Lab::new();
    let (a, a_hex) = canary("a");
    let (b, b_hex) = canary("b");
    let (c_hex, _) = canary("c");
    let c = format!("/\"+ &{c_hex}");
    let scripts = lab.root.join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("recv.sh"), RECEIVER).unwrap();
    std::fs::write(lab.root.join("task.txt"), "가만히 있는다").unwrap();
    let projects = lab.root.join("projects");
    let q = |s: &str| serde_json::to_string(s).unwrap();
    let recv = q(path_str(&scripts.join("recv.sh")));
    let host_toml = format!(
        "schema = 1\n[host]\nagent_listen = \"127.0.0.1:0\"\n[web]\nlisten = \"\"\n[local_agent]\nmode = \"none\"\n\
         [projects]\nroot = {}\nallow_profiles = [\"k0thread\"]\n\
         [profiles.k0pane]\nagent = \"custom\"\ncommand = [\"/bin/sh\", {recv}]\n\
         env = {{ K0_API_TOKEN = {}, K0_C = {}, K0_OUT = {} }}\n\
         [profiles.k0thread]\nagent = \"custom\"\ncommand = [\"/bin/sh\", {recv}]\nenv = {{ K0_B = {}, K0_OUT = {} }}\n",
        q(path_str(&projects)),
        q(&a),
        q(&c),
        q(path_str(&scripts.join("pane.got"))),
        q(&b),
        q(path_str(&scripts.join("thread.got"))),
    );
    let h = lab.node("h", Some(&host_toml));
    std::fs::write(
        h.cfg.join("agent.toml"),
        format!("schema = 1\n[agent]\nwork_root = {}\n", q(path_str(&lab.root.join("workA")))),
    )
    .unwrap();
    let trace = [("AAA_LOG", "trace"), ("RUST_LOG", "trace"), ("AAA_TEST_BRIEF_SETTLE_MS", BRIEF_SETTLE_MS)];
    lab.spawn_env("host", &h, &["host"], &trace);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    // core dump 끄기를 보려고 agent를 core 한도를 연 채 띄운다(pane은 agent·session host의 한도를 물려받는다)
    let mut agent = h.cmd(&["agent", "--host", "local", "--name", "labA"]);
    agent.env("AAA_LOG", "trace").env("RUST_LOG", "trace");
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut agent, || {
            let mut l = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            libc::getrlimit(libc::RLIMIT_CORE, &mut l);
            l.rlim_cur = l.rlim_max;
            libc::setrlimit(libc::RLIMIT_CORE, &l);
            Ok(())
        });
    }
    lab.spawn_command("agentA", agent);
    wait_for("labA 연결", T, || h.connected("labA").then_some(()));
    h.ok(&["project", "create", "demo"]);

    // 1) pane: 웹 새 세션 창처럼 profile로 띄운다(env는 resolved profile로 간다)
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let pane = rt.block_on(async {
        use aaa_proto::{FrontRequest, FrontResponse};
        let mut c = aaa_common::front::FrontClient::connect_as(&h.state, "web-k0").await.unwrap();
        let run = FrontRequest::PaneRun {
            machine: Some("labA".into()),
            name: None,
            argv: vec![],
            cwd: Some(path_str(&scripts).to_string()),
            cols: None,
            rows: None,
            profile: Some("k0pane".into()),
        };
        match c.call(run).await.unwrap() {
            Ok(FrontResponse::Pane(p)) => p.id,
            other => panic!("profile로 띄우기: {other:?}"),
        }
    });
    let got = |f: &str| wait_for(f, T, || std::fs::read_to_string(scripts.join(f)).ok());
    assert_eq!(got("pane.got"), format!("{}\n{}\n", sha(&a), sha(&c)), "pane에 값이 실제로 갔다");
    h.wait_output(&pane, "k0-ready", T);

    // 2) thread: launch env로 간다
    let v = h.ok(&[
        "thread",
        "start",
        "demo",
        "--title",
        "k0",
        "--profile",
        "k0thread",
        "--kind",
        "dir",
        "--task-file",
        path_str(&lab.root.join("task.txt")),
    ]);
    assert!(v["pane"].is_string(), "{v}");
    assert_eq!(got("thread.got"), format!("{}\n", sha(&b)), "thread pane에 값이 실제로 갔다");

    // 3) 프로젝트 export(삭제가 휴지통에 남기는 기록)
    let d = h.ok(&["project", "delete", "demo", "--force"]);
    let export = PathBuf::from(d["project"]["path"].as_str().unwrap()).join("aaa-records.json");

    // 4) 카나리 찾기
    let db = h.state.join("host/host.db");
    let audit = || -> Vec<String> {
        let c = rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let mut st = c.prepare("SELECT detail FROM audit WHERE action='pane_run'").unwrap();
        st.query_map([], |r| r.get::<_, String>(0)).unwrap().map(Result::unwrap).collect()
    };
    let logs = [lab.root.join("host.log"), lab.root.join("agentA.log")];
    let host_db: Vec<PathBuf> =
        ["host.db", "host.db-wal", "host.db-shm"].iter().map(|f| h.state.join("host").join(f)).collect();
    // K2 범위: agent의 session spec·화면 저장물과 thread 준비 기록, host thread 기록(DB)
    let k2 = |p: &Path| {
        let s = p.to_string_lossy();
        s.contains("/agent/sessions/") || s.contains("/agent/thread-preparations/") || s.contains("/host/host.db")
    };
    let (na, nb, nc) = (needles(&a, &a_hex), needles(&b, &b_hex), needles(&c, &c_hex));
    let mut found = Vec::new();
    for (name, n) in [("A", &na), ("C", &nc)] {
        found.extend(hits(&host_db, n, &|_| false).into_iter().map(|h| format!("{name} {h}")));
    }
    for (name, n) in [("A", &na), ("B", &nb), ("C", &nc)] {
        let mut places = logs.to_vec();
        places.push(export.clone());
        places.push(h.state.clone());
        found.extend(hits(&places, n, &k2).into_iter().map(|h| format!("{name} {h}")));
        found.extend(
            audit()
                .iter()
                .filter(|r| n.iter().any(|(_, x)| r.contains(std::str::from_utf8(x).unwrap())))
                .map(|r| format!("{name} audit {r}")),
        );
    }
    assert!(found.is_empty(), "aaa 기록에 env 값이 남았다:\n{}", found.join("\n"));
    assert!(lab.log("host").contains("K0_API_TOKEN"), "host.toml 평문 비밀 경고: {}", lab.log("host"));
    // 로그 단계는 `AAA_LOG`가 정한다(host는 이 길에 trace 사건이 없다. agent의 trace 줄로 단계가 열린 것을 본다)
    assert!(lab.log("agentA").contains(" TRACE "), "trace 로그로 찾았다");

    // 5) 기록의 모양: env 이름은 남는다(값만 뺐다)
    let rows = audit();
    let names = |row: &str, key: &[&str]| {
        let v: Value = serde_json::from_str(row).unwrap();
        key.iter().fold(&v, |v, k| &v[*k]).clone()
    };
    assert!(
        rows.iter()
            .any(|r| names(r, &["resolved_profile", "env"]) == serde_json::json!(["K0_API_TOKEN", "K0_C", "K0_OUT"])),
        "{rows:?}"
    );
    assert!(rows.iter().any(|r| names(r, &["env"]).as_array().is_some_and(|e| e.contains(&"K0_B".into()))), "{rows:?}");

    let rec: Value = serde_json::from_str(&std::fs::read_to_string(&export).unwrap()).unwrap();
    let data: Value = serde_json::from_str(rec["threads"][0]["data"].as_str().unwrap()).unwrap();
    let env = data["launch"]["env"].as_array().expect("export의 launch env");
    assert!(env.contains(&"K0_B".into()) && env.iter().all(Value::is_string), "export에는 env 이름만: {data}");

    // 6) pane 안에서는 값이 그대로 보이고(env 전달 기본 동작), core dump는 꺼져 있다. 이 출력은 pane 화면 저장물(K2)에 남으므로
    // 그 뒤에는 DB만 다시 본다
    h.ok(&["pane", "run", &pane, "printenv K0_API_TOKEN; echo core-$(ulimit -c)"]);
    h.wait_output(&pane, &a, T);
    h.wait_output(&pane, "core-0", T);
    let after: Vec<String> = hits(&host_db, &na, &|_| false);
    assert!(after.is_empty(), "printenv 뒤 host DB: {after:?}");
}

/// 실제 TCP+TLS agent의 바뀌지 않은 328KiB 로그를 원본 hash로 검증한 뒤 사본만 가린다.
#[test]
fn remote_report_with_registered_auth_bytes_copies_without_false_hash_failure() {
    let (secret, _) = canary("report0638");
    let extra = format!("[profiles.fake.env]\nOPENAI_API_KEY = {}\n", serde_json::to_string(&secret).unwrap());
    let mut w = world_with(&extra);
    w.add_remote();
    let task = w.task("report-transfer.md", "noreport");
    let started = w.start(&[
        "--machine",
        "labB",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--title",
        "보고 전송",
        "--task-file",
        &task,
    ]);
    let id = started["id"].as_str().unwrap();
    let ready = w.wait_show("원격 brief 전달", id, |v| v["brief"] == "delivered");
    let work = Path::new(ready["work_dir"].as_str().unwrap());
    let source = work.join("library/stable.log");
    let mut bytes = vec![b'x'; 328 * 1024 + 17];
    bytes[29..29 + secret.len()].copy_from_slice(secret.as_bytes());
    std::fs::write(&source, &bytes).unwrap();
    let body = format!("[병합 준비됨] 원격 보고 전송\n{secret}\n\n## Next\n");
    let revision = sha(&body);
    std::fs::write(work.join("report.md.tmp"), &body).unwrap();
    std::fs::rename(work.join("report.md.tmp"), work.join("report.md")).unwrap();
    let copied = w.wait_show("원본 hash와 다른 가림 사본 회수", id, |v| {
        v["report_hash"] == revision && v["copy"] == "complete"
    });
    assert_eq!(copied["stage"], "merge_ready");
    assert_eq!(std::fs::read(&source).unwrap(), bytes, "원본 파일은 바꾸지 않는다");
    aaa_common::redact::remember(&secret);
    let expected = aaa_common::redact::bytes(&bytes);
    assert_ne!(expected, bytes);
    let dir = w.projects.join("demo");
    assert_eq!(std::fs::read(dir.join("library").join(id).join("stable.log")).unwrap(), expected);
    let stored = std::fs::read(dir.join("threads").join(format!("{id}.md"))).unwrap();
    assert_eq!(stored, aaa_common::redact::bytes(body.as_bytes()));
    let rid = copied["report_id"].as_str().unwrap();
    assert_eq!(std::fs::read(dir.join("library").join(rid).join("stable.log")).unwrap(), expected);
    assert_eq!(std::fs::read(dir.join("threads").join(format!("{rid}.md"))).unwrap(), stored);
    assert!(!w.lab.log("host").contains("받는 동안 바뀌었다"));
    // 재시작도 원격 agent가 pin한 같은 listen 주소를 쓴다(임의 포트를 새로 고르면 접속할 수 없다).
    let config = w.h.cfg.join("host.toml");
    std::fs::write(&config, std::fs::read_to_string(&config).unwrap().replace("127.0.0.1:0", &w.addr)).unwrap();
    w.lab.kill("host");
    w.spawn_host("host-report-restored");
    wait_for("원격 사본 host 재연결", T, || w.h.connected("labB").then_some(()));
    let restored = w.show(id);
    assert_eq!(restored["report_hash"], revision);
    assert_eq!(restored["stage"], "merge_ready");
}
