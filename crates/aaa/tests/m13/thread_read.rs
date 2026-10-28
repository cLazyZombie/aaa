//! front.sock 없는 원격 thread의 읽기 CLI와 hook·host 권한 경계를 실제 TLS로 확인한다.
use super::*;

// Python fixture도 제품 hooks::exchange와 같은 오류 경계를 쓴다.
// JSON을 읽고 먼저 닫은 peer의 응답은 ENOTCONN 뒤에도 읽을 수 있다.
const HOOK_EXCHANGE: &str = r#"
def hook_exchange(s, msg):
    s.sendall(json.dumps(msg).encode())
    try:
        s.shutdown(socket.SHUT_WR)
    except OSError as error:
        if error.errno != errno.ENOTCONN:
            raise
    data = b''
    while True:
        chunk = s.recv(65536)
        if not chunk: break
        data += chunk
    return data.decode()
"#;

#[test]
fn python_hook_reads_closed_peer_reply_without_hiding_other_errors() {
    let script = format!(
        r#"
import errno, json, socket, threading
{HOOK_EXCHANGE}
for reply in [b'ok\nthread_read.v1 {{}}', b'error: peer pid\n', b'']:
    client, server = socket.socketpair()
    closed = threading.Event()
    request = dict(op='projects', all=True)
    peer_errors = []
    def peer():
        try:
            with server:
                expected = json.dumps(request).encode()
                received = b''
                while len(received) < len(expected):
                    chunk = server.recv(len(expected) - len(received))
                    assert chunk, 'request ended early'
                    received += chunk
                assert received == expected
                server.sendall(reply)
        except BaseException as error:
            peer_errors.append(error)
        finally:
            closed.set()
    class AfterPeerClosed:
        def sendall(self, data):
            client.sendall(data)
            assert closed.wait(3), 'peer close not observed'
        def shutdown(self, how):
            client.shutdown(how)
        def recv(self, size):
            return client.recv(size)
    worker = threading.Thread(target=peer)
    worker.start()
    try:
        with client:
            assert hook_exchange(AfterPeerClosed(), request).encode() == reply
    finally:
        worker.join(3)
        assert not worker.is_alive()
        assert not peer_errors, peer_errors
class BrokenShutdown:
    def sendall(self, data): pass
    def shutdown(self, how): raise OSError(errno.EIO, 'shutdown failure')
    def recv(self, size): raise AssertionError('must not read after EIO')
try:
    hook_exchange(BrokenShutdown(), request)
except OSError as error:
    assert error.errno == errno.EIO
else:
    raise AssertionError('shutdown failure was hidden')
"#
    );
    let out = lab::captured(Command::new("python3").args(["-c", &script])).unwrap();
    assert!(out.status.success(), "Python hook 회귀 실패: {}\n{}", out.status, String::from_utf8_lossy(&out.stderr));
}

#[test]
fn remote_thread_reads_host_reports_library_and_search_without_write_access() {
    let mut w = report_shell_world();
    w.add_remote();
    let source = w.start(&[
        "--title",
        "source",
        "--task-file",
        &w.task("source.md", "원격읽기고유본문"),
        "--profile",
        "fake",
        "--kind",
        "dir",
    ]);
    let source_id = source["id"].as_str().unwrap();
    w.wait_show("원본 보고 회수", source_id, |v| v["report_hash"].is_string());
    let report = w.h.ok(&["library", "show", source_id]);
    let report_id = report["report_id"].as_str().unwrap();
    // UTF-8 경계와 base64 포함 binary가 64KiB 조각을 넘어도 바이트를 보존한다.
    let bytes: Vec<u8> = (0..150_001).map(|n| (n % 251) as u8).collect();
    let dir = w.lab.root.join("read-library");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("raw.bin"), &bytes).unwrap();
    w.h.ok(&["library", "publish", path_str(&dir), "--name", "read-files"]);
    let reader = w.start(&[
        "--title",
        "reader",
        "--task-file",
        &w.task("reader.md", "noreport"),
        "--machine",
        "labB",
        "--profile",
        "fake",
        "--kind",
        "dir",
    ]);
    let id = reader["id"].as_str().unwrap();
    let reader = w.wait_show("원격 reader 준비", id, |v| v["brief"] == "delivered");
    let pane = reader["pane"].as_str().unwrap();
    let victim = w.start(&[
        "--title",
        "victim",
        "--task-file",
        &w.task("victim.md", "noreport"),
        "--machine",
        "labB",
        "--profile",
        "fake",
        "--kind",
        "dir",
    ]);
    let victim_pane = victim["pane"].as_str().unwrap();
    let py = w.task(
        "read-probe.py",
        &format!(
            r#"
import errno, hashlib, json, os, socket, subprocess
{HOOK_EXCHANGE}
AA = {bin:?}
def call(*args, ok=True):
    p = subprocess.run([AA, '--json', *args], capture_output=True)
    assert (p.returncode == 0) == ok, (args, p.returncode, p.stdout, p.stderr)
    return json.loads(p.stdout) if ok else p.stderr.decode()
assert not os.path.exists(os.path.join(os.environ['AAA_STATE_DIR'], 'host/front.sock'))
source = call('thread', 'show', {source_id:?})
assert source['id'] == {source_id:?} and source['report_hash']
assert call('thread', 'list')['threads']
assert call('thread', 'summary', {source_id:?})
assert call('thread', 'library', {source_id:?})['files']
report = call('library', 'show', {report_id:?})
assert report['report_id'] == {report_id:?} and '원격읽기고유본문' in report['report']
assert call('library', 'ls')['reports']
page = call('search', '원격읽기고유본문')
hit = next(h for h in page['hits'] if h['kind'] == 'report')
doc = call('record', hit['project'], hit['key'])
assert '원격읽기고유본문' in doc['body']
p = subprocess.run([AA, 'library', 'cat', 'read-files', 'raw.bin'], capture_output=True)
assert p.returncode == 0, p.stderr
assert p.stdout == bytes(n % 251 for n in range(150001))
assert '상대 경로' in call('library', 'cat', 'read-files', '../raw.bin', ok=False)
call('thread', 'stop', {source_id:?}, ok=False)
# 토큰과 실제 peer 세션 둘을 모두 요구한다. 공개 출력에 token은 쓰지 않는다.
def hook(session, token, request):
    msg = dict(session=session, token=token, harness='thread_read', payload=request)
    with socket.socket(socket.AF_UNIX) as s:
        s.connect(os.environ['AAA_HOOK_SOCK'])
        return hook_exchange(s, msg)
session = os.environ['AAA_SESSION_ID']; token = os.environ['AAA_HOOK_TOKEN']
assert 'token' in hook(session, 'bad', dict(op='projects', all=True))
with open(os.path.join(os.path.dirname(os.path.dirname(os.environ['AAA_HOOK_SOCK'])), 'sessions', {victim_pane:?}, 'spec.json')) as f:
    victim_token = hashlib.sha256(('aaa-hook:' + json.load(f)['token']).encode()).hexdigest()[:32]
assert 'peer pid' in hook({victim_pane:?}, victim_token, dict(op='projects', all=True))
assert '허용' in hook(session, token, dict(op='ticker_tick'))
assert '허용' in hook(session, token, dict(op='library_mark_read', reports=[]))
print('remote-read-contract-ok')
"#,
            bin = lab::BIN,
            victim_pane = victim_pane
        ),
    );
    let out = w.lab.root.join("remote-read.out");
    let done = w.lab.root.join("remote-read.done");
    let script = w.task("read-probe.sh", &format!(
        "python3 {py:?} > {out:?} 2>&1\nprintf '%s' \"$?\" > {done:?}.tmp\nmv {done:?}.tmp {done:?}\necho remote-read-done\n"
    ));
    w.h.ok(&["pane", "run", pane, &format!("post-probe {script}")]);
    w.h.wait_output(pane, "remote-read-done", T);
    let output = std::fs::read_to_string(out).unwrap();
    assert_eq!(std::fs::read_to_string(done).unwrap(), "0", "{output}");
    assert!(output.contains("remote-read-contract-ok"), "{output}");
    assert_eq!(w.show(source_id)["status"], "open", "쓰기 거부 뒤 원본 thread가 열려 있다");
}
