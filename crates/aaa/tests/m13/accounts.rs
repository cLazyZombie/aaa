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

fn world(upstream: &str, gate: &Gate, legacy_token: &str, harness_script: &str) -> World {
    let lab = Lab::new();
    let scripts = lab.root.join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let executable = scripts.join("fake-codex");
    std::fs::write(&executable, harness_script).unwrap();
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
    let path = std::env::join_paths(
        std::iter::once(w.scripts.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    w.lab.spawn_env(
        "host",
        &w.h,
        &["host"],
        &[("AAA_TEST_ACCOUNT_REFRESH_ORIGIN", upstream), ("AAA_LOG", "trace"), ("PATH", path.to_str().unwrap())],
    );
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
    let mut w = world(&format!("http://{}", upstream.addr), &gate, &claude, HARNESS);
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
    w.h.ok(&["account", "default", "codex", "work"]);
    let task = w.task("task.txt", "인증 실험실에서 기다린다");
    let mut jobs = vec![];
    for machine in ["labA", "labA", "labB", "labB"] {
        jobs.push(w.start(&["--title", "가짜 인증", "--profile", "file", "--machine", machine, "--task-file", &task]));
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
        assert_eq!(shown["account"], "work", "원격도 host 기본 계정을 쓴다");
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
    w.h.ok(&["account", "default", "codex", "--off"]);
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

// 이 fixture는 인증을 선택하지 않는다. 실제 자식이 받은 env/파일을 해시로 보고하고
// 제공자 갱신을 요청할 뿐이다. 선택·저장·재개는 제품 경로가 맡는다.
const POLICY_HARNESS: &str = r#"#!/usr/bin/env python3
import os,sys,json,pathlib,socket,hashlib,subprocess,urllib.request,uuid
if sys.argv[1:]==['--version']:
    print('codex-cli 0.163.0');sys.exit(0)
kind=os.environ['ACCOUNT_KIND']
tid=os.environ.get('AAA_THREAD','coordinator')
home=os.environ.get('CODEX_HOME') if kind=='codex' else None
if home: assert pathlib.Path(home).is_relative_to(pathlib.Path(os.environ['ACCOUNT_PROOF']).parent), 'authentication escaped lab'
auth=json.loads((pathlib.Path(home)/'auth.json').read_text()) if home else None
token=auth['tokens']['access_token'] if auth else os.environ.get('CLAUDE_CODE_OAUTH_TOKEN')
def digest(s): return hashlib.sha256(s.encode()).hexdigest() if s else None
sid=str(uuid.uuid5(uuid.NAMESPACE_URL,home or (os.getcwd()+tid)))
payload=dict(hook_event_name='SessionStart',session_id=sid,source='resume' if 'resume' in sys.argv or '--resume' in sys.argv else 'startup')
subprocess.run([os.environ['ACCOUNT_AAA'],'hook','--agent',kind,json.dumps(payload)],check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
for args in [['account','default',kind,'--off'],['account','login','denied-from-pane','--harness',kind],['account','remove','central-'+kind]]:
    refused=subprocess.run([os.environ['ACCOUNT_AAA'],*args],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    assert refused.returncode!=0, 'pane changed account administration'
if home:
    history=pathlib.Path(home)/'policy-history'
    if 'resume' in sys.argv: assert history.exists(), 'lost history'
    history.write_text('history')
def gate(phase):
    h,p=os.environ['ACCOUNT_GATE'].split(':')
    s=socket.create_connection((h,int(p)),timeout=30)
    s.sendall((json.dumps(dict(tid=tid,phase=phase,token=digest(token),home=home,resume='resume' in sys.argv or '--resume' in sys.argv))+'\n').encode())
    return s
s=gate(0)
command=s.recv(1);s.close()
if command==b'R':
    request=urllib.request.Request(os.environ['CODEX_REFRESH_TOKEN_URL_OVERRIDE'],data=json.dumps(dict(grant_type='refresh_token')).encode())
    with urllib.request.urlopen(request,timeout=25) as r: auth['tokens'].update(json.load(r))
    token=auth['tokens']['access_token']
s=gate(1)
assert s.recv(1)==b'X'
"#;

fn install_login_fixture(w: &World, gate: &Gate, values: &Value) -> PathBuf {
    let fixture = w.scripts.join("login-fixture.json");
    std::fs::write(&fixture, serde_json::to_vec(values).unwrap()).unwrap();
    let source = format!(
        r#"#!/usr/bin/env python3
import os,sys,json,pathlib,socket,stat,signal
signal.signal(signal.SIGHUP,signal.SIG_IGN)
kind=pathlib.Path(sys.argv[0]).name
expected=['setup-token'] if kind=='claude' else ['-c','cli_auth_credentials_store="file"','login']
assert sys.argv[1:]==expected, 'official login command'
for key in ['CLAUDE_CODE_OAUTH_TOKEN','OPENAI_API_KEY','CODEX_REFRESH_TOKEN_URL_OVERRIDE','AAA_THREAD','AAA_SESSION_ID']:
    assert key not in os.environ, 'inherited credential or pane identity'
home=pathlib.Path(os.environ['CLAUDE_CONFIG_DIR' if kind=='claude' else 'CODEX_HOME'])
assert home.parent==pathlib.Path(os.environ['HOME'])
assert stat.S_IMODE(home.stat().st_mode)==0o700
values=json.loads(pathlib.Path({fixture:?}).read_text())
s=socket.create_connection(({host:?},{port}),timeout=30)
if values.get('stubborn'):
    s.settimeout(None)
    if os.fork()==0:
        s.close();signal.pause();sys.exit(0)
s.sendall((json.dumps(dict(tid='login',phase=8,home=str(home.parent),harness=kind,pid=os.getpid()))+'\n').encode())
assert s.recv(1)==b'1'
value=values[kind]
raw=value if isinstance(value,str) else json.dumps(value)
print(raw,flush=True)
print(raw,file=sys.stderr,flush=True)
if values.get('fail') or values.get('fail_for')==kind: sys.exit(1)
if kind=='codex':
    p=home/'auth.json';p.write_text(raw);p.chmod(0o600)
"#,
        fixture = fixture,
        host = "127.0.0.1",
        port = gate.0.local_addr().unwrap().port(),
    );
    for name in ["claude", "codex"] {
        let path = w.scripts.join(name);
        std::fs::write(&path, &source).unwrap();
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    fixture
}

fn login_result(w: &World, id: &str, expected: &str) -> Value {
    let value = wait_for("로그인 종료", T, || {
        w.h.json(&["account", "login-status", id]).ok().filter(|v| v["login"]["state"] != "waiting")
    });
    assert_eq!(value["login"]["state"], expected);
    value
}

#[test]
fn host_login_defaults_pin_threads_and_coordinators_without_exposing_credentials() {
    let (legacy, legacy_hex) = canary("legacy");
    let (raw, raw_hex) = canary("login-claude");
    let claude = format!("sk-ant-oat01-{raw}");
    let pairs: Vec<_> =
        (0..2).map(|i| (canary(&format!("policy-access{i}")), canary(&format!("policy-refresh{i}")))).collect();
    let tokens: Vec<_> = pairs.iter().map(|(a, r)| (a.0.clone(), r.0.clone())).collect();
    let upstream = Upstream::new(tokens.clone());
    let gate = Gate::new();
    let mut w = world(&format!("http://{}", upstream.addr), &gate, &legacy, POLICY_HARNESS);
    w.add_remote();
    let task = w.task("policy.txt", "가짜 인증에서 기다린다");
    let values = serde_json::json!({"claude":claude,"codex":{"tokens":{"access_token":tokens[0].0,"refresh_token":tokens[0].1}}});
    let fixture = install_login_fixture(&w, &gate, &values);
    let old = w.start(&["--title", "기존 로컬", "--profile", "token", "--task-file", &task]);
    let old_first = gate.take(1, 0);
    assert_eq!(old_first[0].1["token"], sha(&legacy));
    assert!(old["account"].is_null());

    let mut responses = Vec::new();
    for (name, harness) in [("central-claude", "claude"), ("central-codex", "codex")] {
        let start = w.h.ok(&["account", "login", name, "--harness", harness]);
        let id = start["login"]["id"].as_str().unwrap();
        let ready = gate.take(1, 8);
        let home = PathBuf::from(ready[0].1["home"].as_str().unwrap());
        assert!(home.starts_with(w.h.state.join("host/account-logins")));
        Gate::release(ready, b'1');
        responses.push(start.clone());
        responses.push(login_result(&w, id, "succeeded"));
        assert!(!home.exists(), "성공 상태 전에 임시 인증 정리");
        w.h.ok(&["account", "default", harness, name]);
        assert!(w.h.json(&["account", "remove", name]).is_err(), "기본 계정 삭제 거부");
    }
    let defaults = w.h.ok(&["account", "default", "claude"]);
    assert_eq!(defaults["defaults"].as_array().unwrap().len(), 2);
    // 시작 전에 고른 로컬 인증은 기본 지정 뒤에도 바뀌지 않는다.
    Gate::release(old_first, b'1');
    let old_last = gate.take(1, 1);
    assert_eq!(old_last[0].1["token"], sha(&legacy));
    Gate::release(old_last, b'X');
    let old_id = old["id"].as_str().unwrap();
    w.wait_show("기존 작업 종료", old_id, |v| v["agent"]["activity"] == "exited");
    w.h.ok(&["thread", "restart", "demo", old_id, "--resume"]);
    let resumed = gate.take(1, 0);
    assert_eq!(resumed[0].1["token"], sha(&legacy));
    assert_eq!(resumed[0].1["resume"], true);
    Gate::release(resumed, b'1');
    Gate::release(gate.take(1, 1), b'X');
    w.wait_show("로컬 재개 종료", old_id, |v| v["agent"]["activity"] == "exited");

    let mut jobs = vec![];
    for machine in ["labA", "labB"] {
        jobs.push(w.start(&["--title", "기본", "--profile", "plain", "--machine", machine, "--task-file", &task]));
    }
    w.h.in_terminal(&["safety", "set", "start_threads", "auto"], &[("[y/N]", "y")], T);
    // A4 이후 평가는 일반 자동화가 띄우는 worker다. 그 시작에도 같은 host 기본을 고른다.
    let rule_dir = w.projects.join("demo/automations");
    std::fs::create_dir_all(&rule_dir).unwrap();
    std::fs::write(rule_dir.join("policy.toml"), "[on.schedule]\nevery='1s'\n[do.start]\ntitle='기본 평가'\nprofiles=['plain']\nmachines=['labA']\nreuse='open'\ntext='평가 작업'\n").unwrap();
    let listed = w.h.ok(&["routine", "list", "demo"]);
    let rule = listed["routines"].as_array().unwrap().iter().find(|r| r["name"] == "policy").unwrap();
    w.h.in_terminal(&["automation", "approve", "policy", rule["start_approval_sha256"].as_str().unwrap()], &[], T);
    let automatic = wait_for("기본 인증 자동 평가 시작", T, || {
        w.h.ticks(1);
        w.h.ok(&["events", "demo"])["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "thread_started" && e["actor"] == "routine:policy")
            .and_then(|e| e["thread"].as_str())
            .map(str::to_string)
    });
    jobs.push(w.show(&automatic));
    w.h.ok(&["routine", "toggle", "demo", "policy", "--off"]);
    let first = gate.take(3, 0);
    assert!(first.iter().all(|(_, v)| v["token"] == sha(&claude)));
    assert!(jobs.iter().all(|v| v["account"] == "central-claude"));
    Gate::release(first, b'1');
    Gate::release(gate.take(3, 1), b'X');
    for job in &jobs {
        w.wait_show("기본 작업 종료", job["id"].as_str().unwrap(), |v| v["agent"]["activity"] == "exited");
    }

    // 기본 참조가 깨져도 명시 --account는 우선한다. 지정이 없는 시작은 local로 우회하지 않는다.
    let other = w.scripts.join("other.token");
    std::fs::write(&other, &legacy).unwrap();
    w.h.ok(&["account", "import", "broken", "--harness", "claude", "--file", path_str(&other)]);
    w.h.ok(&["account", "default", "claude", "broken"]);
    std::fs::remove_file(w.h.state.join("host/accounts/broken.json")).unwrap();
    assert!(
        w.h.json(&["thread", "start", "demo", "--title", "거부", "--profile", "plain", "--task-file", &task]).is_err()
    );
    assert!(w.h.json(&["project", "open", "demo", "--profile", "plain", "--new"]).is_err());
    let explicit =
        w.start(&["--title", "명시 우선", "--profile", "plain", "--account", "central-claude", "--task-file", &task]);
    let first = gate.take(1, 0);
    assert_eq!(first[0].1["token"], sha(&claude));
    Gate::release(first, b'1');
    Gate::release(gate.take(1, 1), b'X');
    w.wait_show("명시 작업 종료", explicit["id"].as_str().unwrap(), |v| v["agent"]["activity"] == "exited");
    w.h.ok(&["account", "default", "claude", "--off"]);

    // worker pane을 직접 재개한 뒤 thread restart도 같은 인증 홈의 중복 CLI를 거부한다.
    let file_worker = w.start(&["--title", "파일 인증 작업", "--profile", "file", "--task-file", &task]);
    let worker_id = file_worker["id"].as_str().unwrap();
    let original = w.wait_show("파일 인증 기록 연결", worker_id, |v| v["agent"]["harness_session"].is_string());
    let worker_pane = original["pane"].as_str().unwrap();
    Gate::release(gate.take(1, 0), b'1');
    Gate::release(gate.take(1, 1), b'X');
    w.wait_show("파일 인증 종료", worker_id, |v| v["agent"]["activity"] == "exited");
    let worker_resume = w.h.ok(&["pane", "resume", worker_pane]);
    let held = gate.take(1, 0);
    assert_eq!(held[0].1["tid"], worker_id);
    assert_eq!(held[0].1["resume"], true);
    assert!(
        w.h.json(&["thread", "restart", "demo", worker_id, "--resume"]).is_err(),
        "재개된 worker의 인증 홈을 thread restart로 다시 사용하지 않는다"
    );
    Gate::release(held, b'1');
    Gate::release(gate.take(1, 1), b'X');
    wait_for("직접 재개 worker 종료", T, || {
        (w.h.pane(worker_resume["id"].as_str().unwrap())["state"] == "exited").then_some(())
    });

    // 새 coordinator, host 재시작, 중앙 갱신, 재부팅 복원, 수동 pane 재개가 같은 인증/기록 홈을 잇는다.
    let opened = w.h.ok(&["project", "open", "demo", "--profile", "file", "--new"]);
    let pane = opened["pane"]["id"].as_str().unwrap().to_string();
    assert_eq!(opened["pane"]["account"], "central-codex");
    let first = gate.take(1, 0);
    let home = first[0].1["home"].as_str().unwrap().to_string();
    assert_eq!(first[0].1["token"], sha(&tokens[0].0));
    w.lab.kill("host");
    w.lab.spawn_env(
        "policy-host2",
        &w.h,
        &["host"],
        &[("AAA_TEST_ACCOUNT_REFRESH_ORIGIN", &format!("http://{}", upstream.addr)), ("AAA_LOG", "trace")],
    );
    wait_for("host 재접속", T, || w.h.connected("labA").then_some(()));
    assert_eq!(w.h.ok(&["account", "default", "codex"])["defaults"][0]["account"], "central-codex");
    w.h.ok(&["account", "default", "codex", "--off"]);
    Gate::release(first, b'R');
    let refreshed = gate.take(1, 1);
    assert_eq!(refreshed[0].1["token"], sha(&tokens[1].0));
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);
    wait_for("coordinator 대화 연결", T, || w.h.pane(&pane)["agent"]["harness_session"].is_string().then_some(()));
    w.lab.kill("agentA");
    assert!(w.lab.kill_session_hosts() >= 1);
    drop(refreshed);
    w.lab.spawn_env(
        "policy-agent2",
        &w.h,
        &["agent", "--host", "local", "--name", "labA"],
        &[("AAA_TEST_BOOT_ID", "policy-after")],
    );
    let restored = gate.take(1, 0);
    assert_eq!(restored[0].1["token"], sha(&tokens[1].0));
    assert_eq!(restored[0].1["home"], home);
    assert_eq!(restored[0].1["resume"], true);
    let new = wait_for("복원 pane", T, || {
        w.h.ok(&["pane", "list"])["panes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["replaced"].as_array().is_some_and(|a| a.contains(&Value::String(pane.clone()))))
            .cloned()
    });
    let new_id = new["id"].as_str().unwrap();
    assert_eq!(new["account"], "central-codex");
    Gate::release(restored, b'1');
    Gate::release(gate.take(1, 1), b'X');
    wait_for("복원 종료", T, || {
        (!w.h.pane(new_id)["state"].as_str().is_some_and(|s| s == "running")).then_some(())
    });
    // 같은 coordinator 재개를 동시에 눌러도 하나의 인증 홈에 두 CLI를 띄우지 않는다.
    // 준비 중 중복 거부는 host의 account_policy owner가 응답을 보류해 검사한다.
    // 여기서는 제한시간이 있는 CLI 버전 확인을 붙잡지 않고 실제 두 호출을 경주시킨다.
    let results = std::thread::scope(|scope| {
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let calls: Vec<_> = (0..2)
            .map(|_| {
                let barrier = barrier.clone();
                let node = &w.h;
                scope.spawn(move || {
                    barrier.wait();
                    node.json(&["pane", "resume", new_id])
                })
            })
            .collect();
        calls.into_iter().map(|c| c.join().unwrap()).collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1, "인증 실행마다 살아 있는 pane 하나: {results:?}");
    let manual = results.into_iter().find_map(Result::ok).unwrap();
    let again = gate.take(1, 0);
    assert_eq!(again[0].1["token"], sha(&tokens[1].0));
    assert_eq!(again[0].1["home"], home);
    Gate::release(again, b'1');
    Gate::release(gate.take(1, 1), b'X');
    let manual_id = manual["id"].as_str().unwrap();
    wait_for("수동 재개 종료", T, || (w.h.pane(manual_id)["state"] == "exited").then_some(()));
    wait_for("인증 정리", T, || (!Path::new(&home).join("auth.json").exists()).then_some(()));

    let local = w.start(&["--title", "기본 해제 뒤 로컬", "--profile", "plain", "--task-file", &task]);
    let first = gate.take(1, 0);
    assert!(first[0].1["token"].is_null());
    assert!(local["account"].is_null());
    Gate::release(first, b'1');
    Gate::release(gate.take(1, 1), b'X');
    w.wait_show("해제 뒤 로컬 종료", local["id"].as_str().unwrap(), |v| v["agent"]["activity"] == "exited");
    let mut find = needles(&legacy, &legacy_hex);
    find.extend(needles(&raw, &raw_hex));
    for (a, r) in &pairs {
        find.extend(needles(&a.0, &a.1));
        find.extend(needles(&r.0, &r.1));
    }
    // 새 로그인 실패·취소도 원본 출력·인증을 남기지 않는다. 재시작한 host에는 실험실 PATH를 명시한다.
    w.lab.kill("policy-host2");
    let path = std::env::join_paths(
        std::iter::once(w.scripts.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    w.lab.spawn_env("policy-host3", &w.h, &["host"], &[("PATH", path.to_str().unwrap()), ("AAA_LOG", "trace")]);
    wait_for("host 로그인 재접속", T, || w.h.json(&["status"]).ok());
    let (failed, failed_hex) = canary("failed-login");
    find.extend(needles(&failed, &failed_hex));
    for (name, cancel) in [("failed-login", false), ("cancelled-login", true)] {
        std::fs::write(
            &fixture,
            serde_json::to_vec(
                &serde_json::json!({"claude":format!("sk-ant-oat01-{failed}"),"fail":true,"stubborn":cancel}),
            )
            .unwrap(),
        )
        .unwrap();
        let v = w.h.ok(&["account", "login", name, "--harness", "claude"]);
        let id = v["login"]["id"].as_str().unwrap();
        let ready = gate.take(1, 8);
        let home = PathBuf::from(ready[0].1["home"].as_str().unwrap());
        let held = if cancel {
            w.h.ok(&["account", "login-cancel", id]);
            Some(ready)
        } else {
            Gate::release(ready, b'1');
            None
        };
        responses.push(login_result(&w, id, if cancel { "cancelled" } else { "failed" }));
        drop(held);
        assert!(!home.exists());
        assert!(!w.h.state.join(format!("host/accounts/{name}.json")).exists());
    }
    for v in responses {
        for (_, n) in &find {
            assert!(!v.to_string().as_bytes().windows(n.len()).any(|b| b == n), "로그인 응답 노출");
        }
    }
    w.h.ticks(1);
    let pending = w.h.ok(&["account", "login", "shutdown-login", "--harness", "claude"]);
    let ready = gate.take(1, 8);
    let home = PathBuf::from(ready[0].1["home"].as_str().unwrap());
    assert_eq!(pending["login"]["state"], "waiting");
    w.lab.signal("policy-host3", libc::SIGTERM);
    assert!(w.lab.wait_exit("policy-host3", T).success(), "host가 로그인 자식 종료까지 마친다");
    drop(ready);
    assert!(!home.exists(), "host 정상 종료도 임시 인증을 정리한다");
    let leak = hits(
        &[
            w.h.state.clone(),
            w.lab.root.join("b/s"),
            w.projects.clone(),
            w.lab.root.join("host.log"),
            w.lab.root.join("policy-host2.log"),
            w.lab.root.join("policy-host3.log"),
        ],
        &find,
        &|_| false,
    );
    assert!(leak.is_empty(), "인증 비밀이 남은 경로: {leak:?}");
}

#[test]
fn parallel_host_logins_finish_independently_and_cancel_returns_after_cleanup() {
    let (token, encoded) = canary("opaque");
    let gate = Gate::new();
    let upstream = Upstream::new(vec![]);
    let mut w = world(&format!("http://{}", upstream.addr), &gate, &token, POLICY_HARNESS);
    let values = serde_json::json!({"claude":format!("sk-ant-oat01-{token}"),"codex":{"tokens":{"access_token":token,"refresh_token":token}}});
    let fixture = install_login_fixture(&w, &gate, &values);
    let start = |harness: &str, suffix: &str| {
        let v = w.h.ok(&["account", "login", &format!("parallel-{harness}-{suffix}"), "--harness", harness]);
        v["login"]["id"].as_str().unwrap().to_string()
    };
    let a = start("claude", "done");
    let b = start("codex", "done");
    let mut held = gate.take(2, 8);
    let b_at = held.iter().position(|(_, v)| v["harness"] == "codex").unwrap();
    let waiting = held.remove(b_at);
    let a_home = PathBuf::from(held[0].1["home"].as_str().unwrap());
    Gate::release(held, b'1');
    login_result(&w, &a, "succeeded");
    assert!(!a_home.exists(), "다른 로그인이 진행 중이어도 종료한 자식의 자료를 정리한다");
    assert_eq!(w.h.ok(&["account", "login-status", &b])["login"]["state"], "waiting");
    let b_home = PathBuf::from(waiting.1["home"].as_str().unwrap());
    Gate::release(vec![waiting], b'1');
    login_result(&w, &b, "succeeded");
    assert!(!b_home.exists());

    assert_eq!(w.h.ok(&["account", "login-cancel", &a])["login"]["state"], "succeeded");
    // 이미 자식이 실패해 끝난 작업도 다른 로그인과 독립적으로 waiting을 걷는다.
    let mut failed_values = values.clone();
    failed_values["fail_for"] = "claude".into();
    std::fs::write(&fixture, serde_json::to_vec(&failed_values).unwrap()).unwrap();
    let a = start("claude", "failed");
    let b = start("codex", "held");
    let mut held = gate.take(2, 8);
    let a_at = held.iter().position(|(_, v)| v["harness"] == "claude").unwrap();
    let done = held.remove(a_at);
    let a_home = PathBuf::from(done.1["home"].as_str().unwrap());
    Gate::release(vec![done], b'1');
    login_result(&w, &a, "failed");
    assert!(!a_home.exists());
    assert_eq!(w.h.ok(&["account", "login-cancel", &a])["login"]["state"], "failed");
    assert_eq!(w.h.ok(&["account", "login-status", &b])["login"]["state"], "waiting");
    assert_eq!(w.h.ok(&["account", "login-cancel", &b])["login"]["state"], "cancelled");
    assert!(!Path::new(held[0].1["home"].as_str().unwrap()).exists());
    drop(held);

    std::fs::write(
        &fixture,
        serde_json::to_vec(&serde_json::json!({"claude":token,"codex":values["codex"],"stubborn":true})).unwrap(),
    )
    .unwrap();
    let a = start("claude", "cancel");
    let b = start("codex", "cancel");
    let held = gate.take(2, 8);
    let ids = [&a, &a, &b];
    let results = std::thread::scope(|scope| {
        let barrier = Arc::new(std::sync::Barrier::new(ids.len()));
        let calls: Vec<_> = ids
            .iter()
            .map(|id| {
                let barrier = barrier.clone();
                let node = &w.h;
                scope.spawn(move || {
                    barrier.wait();
                    node.ok(&["account", "login-cancel", id])
                })
            })
            .collect();
        calls.into_iter().map(|c| c.join().unwrap()).collect::<Vec<_>>()
    });
    for v in results {
        assert_eq!(v["login"]["state"], "cancelled", "cancel 응답은 자식 종료·자료 정리 뒤의 최종 상태여야 한다");
    }
    for (_, v) in &held {
        assert!(!Path::new(v["home"].as_str().unwrap()).exists());
        assert_ne!(
            unsafe { libc::kill(v["pid"].as_i64().unwrap() as i32, 0) },
            0,
            "취소 응답 뒤 로그인 자식이 없어야 한다"
        );
    }
    drop(held);
    start("claude", "shutdown");
    start("codex", "shutdown");
    let held = gate.take(2, 8);
    w.lab.signal("host", libc::SIGTERM);
    assert!(w.lab.wait_exit("host", T).success());
    for (_, v) in &held {
        assert!(!Path::new(v["home"].as_str().unwrap()).exists());
    }
    drop(held);
    assert!(hits(&[w.h.state.clone(), w.lab.root.join("host.log")], &needles(&token, &encoded), &|_| false).is_empty());
}

// 공식 저장 형식과 cwd 우선 조회를 흉내 낸다. 인증·키체인·실제 CLI는 열지 않는다.
const TRUST_HARNESS: &str = r#"#!/usr/bin/env python3
import os,sys,json,pathlib,socket,tomllib,subprocess,uuid
if sys.argv[1:]==['--version']:
    print('codex-cli 0.163.0');sys.exit(0)
kind=os.environ['ACCOUNT_KIND']
cwd=str(pathlib.Path.cwd().resolve())
home=pathlib.Path(os.environ['CODEX_HOME' if kind=='codex' else 'CLAUDE_CONFIG_DIR'])
assert home.is_relative_to(pathlib.Path(os.environ['ACCOUNT_PROOF']).parent), 'home escaped lab'
if kind=='codex':
    config=tomllib.loads((home/'config.toml').read_text())
    projects=config.get('projects',{})
    trusted=projects.get(cwd,{}).get('trust_level')=='trusted'
    settings={}
else:
    config=json.loads((home/'.claude.json').read_text())
    assert config.get('hasCompletedOnboarding') is True
    projects=config.get('projects',{})
    trusted=projects.get(cwd,{}).get('hasTrustDialogAccepted') is True
    settings=json.loads((home/'settings.json').read_text()) if (home/'settings.json').exists() else {}
    # 격리 Claude 기록도 실제 hook → agent → 대화/계획 조회 경로를 거친다.
    sid=str(uuid.uuid4())
    transcript=home/'projects'/'fixture'/(sid+'.jsonl')
    transcript.parent.mkdir(parents=True,exist_ok=True)
    lines=[dict(type='assistant',message=dict(role='assistant',content=[dict(type='text',text='isolated-conversation-proof')])),
        dict(type='assistant',message=dict(content=[dict(type='tool_use',id='plan-call',name='TaskCreate',input=dict(subject='read isolated history'))])),
        dict(type='user',toolUseResult=dict(task=dict(id='1')),message=dict(content=[dict(type='tool_result',tool_use_id='plan-call',content='Task #1 created successfully')]))]
    transcript.write_text(''.join(json.dumps(line,separators=(',',':'))+'\n' for line in lines))
    subprocess.run([os.environ['ACCOUNT_AAA'],'hook','--agent','claude',json.dumps(dict(hook_event_name='SessionStart',session_id=sid,transcript_path=str(transcript),source='startup'))],check=True,stdout=subprocess.PIPE)
print('account-trust-ready' if trusted else 'Trust this folder?',flush=True)
if kind=='claude' and '--dangerously-skip-permissions' in sys.argv and not settings.get('skipDangerousModePermissionPrompt'):
    print('WARNING: Claude Code running in Bypass Permissions mode',flush=True)
h,p=os.environ['ACCOUNT_GATE'].split(':')
s=socket.create_connection((h,int(p)),timeout=30)
s.sendall((json.dumps(dict(tid=os.environ['AAA_THREAD'],phase=4,home=str(home),cwd=cwd,projects=projects,trusted=trusted,settings=settings,config=config))+'\n').encode())
assert s.recv(1)==b'X'
"#;

#[test]
fn fresh_account_claude_initializes_only_authorized_bypass_consent() {
    let gate = Gate::new();
    let mut w = world("http://127.0.0.1:1", &gate, "fake-local-token", TRUST_HARNESS);
    w.add_remote();
    let token = w.scripts.join("fake-token");
    std::fs::write(&token, "fake-claude-token").unwrap();
    w.h.ok(&["account", "import", "token", "--harness", "claude", "--file", path_str(&token)]);
    w.h.ok(&["account", "default", "claude", "token"]);
    let task = w.task("onboarding.md", "가짜 첫 실행 검증");
    let host = w.h.cfg.join("host.toml");
    let original = std::fs::read_to_string(&host).unwrap();
    // 실제 사용자 홈 대신 가짜 원본 홈을 가리켜도 첫 실행 키 외의 설정·인증은 복사하지 않는다.
    let source = w.scripts.join("source-claude");
    std::fs::create_dir(&source).unwrap();
    let source_config = r#"{"oauthAccount":{"accessToken":"fake-source-secret"},"theme":"light"}"#;
    let source_settings = r#"{"env":{"ANTHROPIC_API_KEY":"fake-source-secret"},"skipAutoPermissionPrompt":true}"#;
    std::fs::write(source.join(".claude.json"), source_config).unwrap();
    std::fs::write(source.join("settings.json"), source_settings).unwrap();
    let original = original.replace("env={ACCOUNT_GATE=", &format!("env={{CLAUDE_CONFIG_DIR={source:?},ACCOUNT_GATE="));
    for (yolo, args, accepted) in [
        (false, "[]", false),
        (true, "[]", true),
        (false, "[\"--permission-mode\",\"bypassPermissions\"]", true),
        (false, "[\"--permission-mode=bypassPermissions\"]", true),
        (false, "[\"--permission-mode\",\"default\"]", false),
    ] {
        let config = original.replace("[profiles.token]\n", &format!("[profiles.token]\nargs={args}\n"));
        std::fs::write(&host, format!("{config}\n[safety.demo]\nyolo={yolo}\ntrust_screens=\"coordinator\"\n"))
            .unwrap();
        for machine in ["labA", "labB"] {
            let started = w.start(&[
                "--title",
                "계정 첫 실행",
                "--profile",
                "token",
                "--machine",
                machine,
                "--kind",
                "dir",
                "--task-file",
                &task,
            ]);
            let id = started["id"].as_str().unwrap();
            let ready = gate.take(1, 4);
            let proof = &ready[0].1;
            let expected = if accepted {
                serde_json::json!({"skipDangerousModePermissionPrompt":true})
            } else {
                serde_json::json!({})
            };
            assert_eq!(proof["settings"], expected, "{machine}/{yolo}/{args}: {proof}");
            let config = proof["config"].as_object().unwrap();
            assert_eq!(config.len(), 2, "인증·다른 사용자 설정을 복사하지 않는다: {proof}");
            assert_eq!(config["hasCompletedOnboarding"], true);
            assert!(proof["trusted"].as_bool().unwrap());
            let pane = w.show(id)["pane"].as_str().unwrap().to_string();
            let screen = w.h.screen(&pane);
            assert!(!screen.contains("WARNING: Claude Code running in Bypass Permissions mode"), "{screen}");
            Gate::release(ready, b'X');
            w.wait_show("가짜 첫 실행 종료", id, |v| v["agent"]["activity"] == "exited");
        }
    }
    assert_eq!(std::fs::read_to_string(source.join(".claude.json")).unwrap(), source_config);
    assert_eq!(std::fs::read_to_string(source.join("settings.json")).unwrap(), source_settings);
}

#[test]
fn account_threads_trust_only_their_cwd_when_coordinator_answers() {
    let gate = Gate::new();
    let mut w = world("http://127.0.0.1:1", &gate, "fake-local-token", TRUST_HARNESS);
    w.add_remote();
    let file = w.scripts.join("fake-auth.json");
    std::fs::write(&file, r#"{"tokens":{"access_token":"fake-access-token","refresh_token":"fake-refresh-token"}}"#)
        .unwrap();
    w.h.ok(&["account", "import", "file", "--harness", "codex", "--file", path_str(&file)]);
    let token = w.scripts.join("fake-token");
    std::fs::write(&token, "fake-claude-token").unwrap();
    w.h.ok(&["account", "import", "token", "--harness", "claude", "--file", path_str(&token)]);
    w.h.ok(&["account", "default", "codex", "file"]);
    w.h.ok(&["account", "default", "claude", "token"]);
    // 실제 worktree여도 본 저장소·부모·이웃 작업은 신뢰 목록에 넣지 않는다.
    git(&w.repo, &["init", "-b", "main"]);
    git(&w.repo, &["config", "user.name", "Example"]);
    git(&w.repo, &["config", "user.email", "test@example.com"]);
    git(&w.repo, &["add", "."]);
    git(&w.repo, &["commit", "-m", "fixture"]);
    let task = w.task("trust-task.md", "가짜 신뢰 시작 검증");
    for policy in ["coordinator", "user"] {
        std::fs::write(w.projects.join("demo/project.toml"), format!("trust_screens = {policy:?}\n")).unwrap();
        for profile in ["file", "token"] {
            for machine in ["labA", "labB"] {
                for kind in ["worktree", "dir"] {
                    let mut args = vec![
                        "--title",
                        "격리 폴더 신뢰",
                        "--profile",
                        profile,
                        "--machine",
                        machine,
                        "--kind",
                        kind,
                        "--task-file",
                        &task,
                    ];
                    if kind == "worktree" {
                        args.extend(["--repo", path_str(&w.repo)]);
                    }
                    let started = w.start(&args);
                    let id = started["id"].as_str().unwrap();
                    let ready = gate.take(1, 4);
                    let proof = &ready[0].1;
                    assert_eq!(
                        proof["trusted"],
                        policy == "coordinator",
                        "{policy}/{profile}/{machine}/{kind}: {proof}"
                    );
                    let projects = proof["projects"].as_object().unwrap();
                    if policy == "coordinator" {
                        assert_eq!(projects.len(), 1, "{proof}");
                        assert!(projects.contains_key(proof["cwd"].as_str().unwrap()), "{proof}");
                    } else {
                        assert!(projects.is_empty(), "{proof}");
                        let pane = w.show(id)["pane"].as_str().unwrap().to_string();
                        w.h.wait_output(&pane, "Trust this folder?", T);
                    }
                    if profile == "token" {
                        let pane = w.show(id)["pane"].as_str().unwrap().to_string();
                        let chat = w.h.ok(&["pane", "chat", &pane]);
                        assert!(chat.to_string().contains("isolated-conversation-proof"), "{chat}");
                        w.wait_show("격리 Claude 계획 조회", id, |v| v["agent"]["plan"]["total"] == 1);
                    }
                    Gate::release(ready, b'X');
                    w.wait_show("가짜 신뢰 harness 종료", id, |v| v["agent"]["activity"] == "exited");
                }
            }
        }
    }
}
