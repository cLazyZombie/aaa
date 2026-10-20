//! owner: 실제 CLI→host→로컬/원격 agent→session→PTY의 인증 전달·회전·비노출 계약.
//! 기존 K0는 전달 비밀·PTY 가림·refresh를 다루지 않는다. 임의 sleep·운영 인증은 쓰지 않는다.
use super::secret_k0::{canary, hits, needles, sha};
use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const HARNESS: &str = r#"#!/usr/bin/env python3
import os,sys,json,pathlib,socket,urllib.request,urllib.error,hashlib,base64,stat,subprocess
if sys.argv[1:]==['--version']:
    print('codex-cli 0.163.0');sys.exit(0)
root=pathlib.Path(os.environ['ACCOUNT_PROOF'])
tid=os.environ['AAA_THREAD']
kind=os.environ['ACCOUNT_KIND']
def digest(s): return hashlib.sha256(s.encode()).hexdigest()
def gate(phase,proof):
    h,p=os.environ['ACCOUNT_GATE'].split(':')
    s=socket.create_connection((h,int(p)),timeout=30)
    s.sendall((json.dumps(dict(tid=tid,phase=phase,**proof))+'\n').encode())
    return s
def emit(secret):
    # chunk·ANSI·접힌 줄·base64가 출력 경계를 넘는다. 실제 비밀은 아니다.
    for i,c in enumerate(secret):
        os.write(1,('\x1b[31m'+c+'\x1b[0m').encode())
        if i%11==10: os.write(1,b'\r\n')
    print('\n'+base64.b64encode(secret.encode()).decode(),flush=True)
    p=subprocess.run([os.environ['ACCOUNT_AAA'],'post','coordinator','--data','--topic','account-canary','--title','fake auth','--body-file','-'],input=secret.encode(),stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
    os.write(1,p.stdout)
if kind=='claude':
    token=os.environ.get('CLAUDE_CODE_OAUTH_TOKEN')
    s=gate(0,dict(token=digest(token) if token else None,home=None))
    assert s.recv(1)==b'1'
    if token: emit(token)
    s.close()
    s=gate(1,dict(token=digest(token) if token else None,home=None))
    assert s.recv(1)==b'X'
    if token: raise RuntimeError('fake crash '+token)
    sys.exit(0)
home=pathlib.Path(os.environ['CODEX_HOME'])
auth=json.loads((home/'auth.json').read_text())
assert stat.S_IMODE(home.stat().st_mode)==0o700
assert stat.S_IMODE((home/'auth.json').stat().st_mode)==0o600
assert 'cli_auth_credentials_store = "file"' in (home/'config.toml').read_text()
assert os.environ.get('CLAUDE_CODE_OAUTH_TOKEN') is None
url=os.environ['CODEX_REFRESH_TOKEN_URL_OVERRIDE']
sid='019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8'+tid[-4:]
roll=home/'sessions'/'2026'/'01'/'01'/('rollout-test-'+sid+'.jsonl')
if 'resume' in sys.argv:
    assert roll.exists(), 'resume history missing'
    assert 'account-history' in roll.read_text()
    try:
        urllib.request.urlopen(urllib.request.Request(url,data=json.dumps(dict(grant_type='refresh_token')).encode()),timeout=25)
        raise AssertionError('expired refresh accepted')
    except urllib.error.HTTPError as e: assert e.code==503
    s=gate(3,dict(token=digest(auth['tokens']['access_token']),refresh=digest(auth['tokens']['refresh_token']),home=str(home)))
    assert s.recv(1)==b'X'
    sys.exit(0)
roll.parent.mkdir(parents=True,exist_ok=True)
roll.write_text(json.dumps(dict(type='response_item',payload=dict(type='message',role='assistant',content=[dict(type='output_text',text='account-history')])) )+'\n')
subprocess.run([os.environ['ACCOUNT_AAA'],'hook','--agent','codex',json.dumps({'type':'agent-turn-complete','thread-id':sid})],check=True,stdout=subprocess.PIPE)

s=gate(0,dict(token=digest(auth['tokens']['access_token']),home=str(home)))
assert s.recv(1)==b'1'
s.close()
for phase in [1,2]:
    body=json.dumps(dict(grant_type='refresh_token',client_id='app_EMoamEEZ73f0CkXaXp7hrann',refresh_token=auth['tokens']['refresh_token'])).encode()
    request=urllib.request.Request(url,data=body,headers={'Content-Type':'application/json'})
    with urllib.request.urlopen(request,timeout=25) as r: refreshed=json.load(r)
    auth['tokens'].update(refreshed)
    tmp=home/'fake-auth.tmp'
    tmp.write_text(json.dumps(auth));tmp.chmod(0o600);tmp.replace(home/'auth.json')
    emit(auth['tokens']['access_token'])
    emit(auth['tokens']['refresh_token'])
    print('account-phase-complete',flush=True)
    s=gate(phase,dict(token=digest(auth['tokens']['access_token']),refresh=digest(auth['tokens']['refresh_token']),home=str(home)))
    assert s.recv(1)==(b'2' if phase==1 else b'X')
    s.close()
"#;

struct Upstream {
    addr: String,
    stop: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    join: Option<std::thread::JoinHandle<()>>,
}
impl Upstream {
    fn new(tokens: Vec<(String, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (s, c) = (stop.clone(), calls.clone());
        let join = std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                if s.load(Ordering::SeqCst) {
                    break;
                }
                stream.set_read_timeout(Some(T)).unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(line.starts_with("POST /oauth/token "));
                let mut size = 0;
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        size = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; size];
                reader.read_exact(&mut body).unwrap();
                let request: Value = serde_json::from_slice(&body).unwrap();
                let n = c.fetch_add(1, Ordering::SeqCst);
                // 회전한 token을 재사용하면 거부하는 독립 제공자 모델이다.
                let valid = n + 1 < tokens.len() && request["refresh_token"].as_str() == Some(&tokens[n].1);
                let (status, body) = if valid {
                    (
                        "200 OK",
                        if n == 1 {
                            serde_json::json!({"access_token":tokens[n+1].0,"refresh_token":null}).to_string()
                        } else {
                            serde_json::json!({"access_token":tokens[n+1].0,"refresh_token":tokens[n+1].1}).to_string()
                        },
                    )
                } else {
                    ("401 Unauthorized", "{\"error\":\"refresh_token_reused\"}".to_string())
                };
                write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        Self { addr, stop, calls, join: Some(join) }
    }
}
impl Drop for Upstream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.addr);
        if let Some(j) = self.join.take() {
            j.join().unwrap();
        }
    }
}

struct Gate(TcpListener);
impl Gate {
    fn new() -> Self {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        Self(l)
    }
    fn address(&self) -> String {
        self.0.local_addr().unwrap().to_string()
    }
    fn take(&self, n: usize, phase: u64) -> Vec<(TcpStream, Value)> {
        (0..n)
            .map(|_| {
                let (stream, _) = wait_for("가짜 harness 사건", T, || self.0.accept().ok());
                // BSD는 listener의 비차단 모드를 accepted socket에도 물려준다.
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(T)).unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
                let v: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(v["phase"], phase);
                (stream, v)
            })
            .collect()
    }
    fn release(items: Vec<(TcpStream, Value)>, byte: u8) {
        for (mut s, _) in items {
            s.write_all(&[byte]).unwrap();
        }
    }
}

fn world(upstream: &str, gate: &Gate, legacy_token: &str) -> World {
    let lab = Lab::new();
    let scripts = lab.root.join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let executable = scripts.join("fake-codex");
    std::fs::write(&executable, HARNESS).unwrap();
    std::fs::set_permissions(&executable, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let projects = lab.root.join("projects");
    let proof = lab.root.join("proof");
    std::fs::create_dir_all(&proof).unwrap();
    let mut config = format!(
        "schema = 1\n[host]\nagent_listen=\"127.0.0.1:0\"\n[web]\nlisten=\"\"\n[local_agent]\nmode=\"none\"\n[projects]\nroot={projects:?}\nallow_profiles=[\"token\",\"file\",\"plain\"]\n"
    );
    for (profile, harness) in [("token", "claude"), ("file", "codex"), ("plain", "claude")] {
        let extra =
            if profile == "token" { format!(",CLAUDE_CODE_OAUTH_TOKEN={legacy_token:?}") } else { String::new() };
        config.push_str(&format!("[profiles.{profile}]\nagent={harness:?}\ncommand=[{:?}]\nmodel=\"example-model\"\neffort=\"high\"\nenv={{ACCOUNT_GATE={:?},ACCOUNT_PROOF={proof:?},ACCOUNT_KIND={harness:?},ACCOUNT_AAA={:?}{extra}}}\n",executable,gate.address(),lab::BIN));
    }
    let h = lab.node("h", Some(&config));
    let mut w = World { lab, h, projects, repo: scripts.clone(), scripts, addr: String::new(), settle_ms: None };
    w.lab.spawn_env("host", &w.h, &["host"], &[("AAA_TEST_ACCOUNT_REFRESH_ORIGIN", upstream), ("AAA_LOG", "trace")]);
    let status = wait_for("host 준비", T, || w.h.json(&["status"]).ok());
    w.addr = status["agent_listen"].as_str().unwrap().into();
    let mut cmd = w.h.cmd(&["agent", "--host", "local", "--name", "labA"]);
    cmd.env("AAA_LOG", "trace");
    for key in ["CLAUDE_CODE_OAUTH_TOKEN", "OPENAI_API_KEY", "CODEX_HOME"] {
        cmd.env_remove(key);
    }
    w.lab.spawn_command("agentA", cmd);
    wait_for("labA 연결", T, || w.h.connected("labA").then_some(()));
    w.h.ok(&["project", "create", "demo"]);
    w
}

#[test]
fn account_credentials_parallel_refresh_and_all_records_are_redacted() {
    let pairs: Vec<_> = (0..3).map(|i| (canary(&format!("access{i}")), canary(&format!("refresh{i}")))).collect();
    let token_pairs: Vec<_> = pairs.iter().map(|(a, r)| (a.0.clone(), r.0.clone())).collect();
    let (claude, claude_hex) = canary("claude");
    let upstream = Upstream::new(token_pairs.clone());
    let gate = Gate::new();
    let mut w = world(&format!("http://{}", upstream.addr), &gate, &claude);
    w.add_remote();
    let credential = w.scripts.join("import.json");
    std::fs::write(
        &credential,
        serde_json::json!({"tokens":{"access_token":token_pairs[0].0,"refresh_token":token_pairs[0].1}}).to_string(),
    )
    .unwrap();
    w.h.ok(&["account", "import", "work", "--harness", "codex", "--file", path_str(&credential)]);
    let claude_file = w.scripts.join("import.token");
    std::fs::write(&claude_file, &claude).unwrap();
    w.h.ok(&["account", "import", "token", "--harness", "claude", "--file", path_str(&claude_file)]);
    let task = w.task("task.txt", "인증 실험실에서 기다린다");
    let mut jobs = vec![];
    for machine in ["labA", "labA", "labB", "labB"] {
        jobs.push(w.start(&[
            "--title",
            "가짜 인증",
            "--profile",
            "file",
            "--machine",
            machine,
            "--account",
            "work",
            "--task-file",
            &task,
        ]));
    }
    let first = gate.take(4, 0);
    let mut homes: Vec<_> = first.iter().map(|(_, v)| v["home"].as_str().unwrap().to_string()).collect();
    homes.sort();
    homes.dedup();
    assert_eq!(homes.len(), 4, "작업별 인증 홈");
    assert!(first.iter().all(|(_, v)| v["token"] == sha(&token_pairs[0].0)));
    for job in &jobs {
        let id = job["id"].as_str().unwrap();
        let shown = w.wait_show("Codex 기록 연결", id, |v| v["agent"]["harness_session"].is_string());
        let pane = shown["pane"].as_str().unwrap();
        let chat = w.h.ok(&["pane", "chat", pane]);
        assert!(chat["messages"].as_array().unwrap().iter().any(|m| m["text"] == "account-history"));
    }
    Gate::release(first, b'1');
    let wave1 = gate.take(4, 1);
    assert!(wave1.iter().all(|(_, v)| v["token"] == sha(&token_pairs[1].0)));
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1, "첫 회전은 한 번");
    Gate::release(wave1, b'2');
    let wave2 = gate.take(4, 2);
    assert!(wave2.iter().all(|(_, v)| v["token"] == sha(&token_pairs[2].0)));
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2, "둘째 회전도 한 번");
    assert!(wave2.iter().all(|(_, v)| v["refresh"] == sha(&token_pairs[1].1)), "부분 응답은 기존 refresh 유지");
    Gate::release(wave2, b'X');
    for job in &jobs {
        let id = job["id"].as_str().unwrap();
        w.wait_show("pane 종료", id, |v| v["agent"]["activity"] == "exited");
    }
    for home in &homes {
        wait_for("임시 인증 정리", T, || (!Path::new(home).join("auth.json").exists()).then_some(()));
    }
    let id = jobs[0]["id"].as_str().unwrap();
    w.h.ok(&["thread", "restart", "demo", id, "--resume"]);
    let resumed = gate.take(1, 3);
    assert_eq!(resumed[0].1["token"], sha(&token_pairs[2].0));
    assert!(homes.contains(&resumed[0].1["home"].as_str().unwrap().to_string()), "재개는 같은 기록 홈");
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 3, "거부된 갱신은 한 번, 우회/자동 재시도 없음");
    Gate::release(resumed, b'X');
    w.wait_show("재개 종료", id, |v| v["agent"]["activity"] == "exited");
    let mut find = needles(&claude, &claude_hex);
    for (a, r) in &pairs {
        find.extend(needles(&a.0, &a.1));
        find.extend(needles(&r.0, &r.1));
    }
    // 화면/이벤트 JSON 응답도 별도 검사한다. 토큰 값은 실패 문구에 넣지 않는다.
    for job in &jobs {
        let shown = w.show(job["id"].as_str().unwrap());
        let pane = shown["pane"].as_str().unwrap();
        let output = w.h.screen(pane);
        for (_, n) in &find {
            assert!(!output.as_bytes().windows(n.len()).any(|v| v == n), "pane 비밀 조각 노출");
        }
    }
    // Claude 동시 전달과 계정 미지정 profile 인증은 원문을 유지한다.
    let mut token_jobs = vec![];
    for (selected, profile) in [(true, "token"), (true, "token"), (false, "token"), (false, "plain")] {
        let mut args = vec!["--title", "token", "--profile", profile, "--task-file", &task];
        if selected {
            args.extend(["--account", "token"]);
        }
        token_jobs.push(w.start(&args));
    }
    let first = gate.take(4, 0);
    assert_eq!(first.iter().filter(|(_, v)| v["token"] == sha(&claude)).count(), 3);
    assert_eq!(first.iter().filter(|(_, v)| v["token"].is_null()).count(), 1);
    // ACK 이전 durable outbox를 검사한다. 연결 성공 뒤 삭제된 파일만 보지 않는다.
    w.lab.kill("host");
    Gate::release(first, b'1');
    let done = gate.take(4, 1);
    let outbox = w.h.state.join("agent/post/out");
    assert!(std::fs::read_dir(&outbox).unwrap().next().is_some(), "미전송 post가 남아야 한다");
    assert!(hits(&[outbox], &needles(&claude, &claude_hex), &|_| false).is_empty(), "미전송 post 비밀 노출");
    w.lab.spawn_env(
        "host2",
        &w.h,
        &["host"],
        &[("AAA_TEST_ACCOUNT_REFRESH_ORIGIN", &format!("http://{}", upstream.addr)), ("AAA_LOG", "trace")],
    );
    wait_for("host 재접속", T, || w.h.connected("labA").then_some(()));
    Gate::release(done, b'X');
    for job in token_jobs {
        w.wait_show("token pane 종료", job["id"].as_str().unwrap(), |v| v["agent"]["activity"] == "exited");
    }
    // post 수신 뒤 ticker 완료는 기록 쓰기가 끝난 울타리다.
    w.h.ticks(1);
    let inbox = w.h.ok(&["post", "list"]);
    assert!(!inbox.to_string().contains(&claude));
    let b_state = w.lab.root.join("b/s");
    let leak = hits(
        &[
            w.h.state.clone(),
            b_state,
            w.projects.clone(),
            w.lab.root.join("host.log"),
            w.lab.root.join("host2.log"),
            w.lab.root.join("agentA.log"),
            w.lab.root.join("agentB.log"),
        ],
        &find,
        &|_| false,
    );
    assert!(leak.is_empty(), "비밀이 남은 경로: {leak:?}");
}
