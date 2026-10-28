"""실제 계정 Codex의 startup/compact hook: 격리 host·agent와 loopback 모델 fixture.
운영 인증·홈·서비스는 쓰지 않으며 요청 원문은 저장하지 않는다.
"""
import argparse, base64, json, os, pathlib, queue, subprocess, threading, time, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
parser = argparse.ArgumentParser()
parser.add_argument('--aaa', required=True)
parser.add_argument('--codex', required=True)
parser.add_argument('--root', required=True)
parser.add_argument('--canary', required=True)
a = parser.parse_args()
root = pathlib.Path(a.root).resolve()
root.mkdir(parents=True, exist_ok=False)
aaa = str(pathlib.Path(a.aaa).resolve())
codex = str(pathlib.Path(a.codex).resolve())
env = {k: v for k, v in os.environ.items() if not k.startswith(('AAA_', 'CODEX_')) and k not in ('OPENAI_API_KEY', 'OPENAI_BASE_URL', 'CLAUDE_CONFIG_DIR')}
env.update(AAA_CONFIG_DIR=str(root / 'config'), AAA_STATE_DIR=str(root / 'state'), AAA_INSTANCE='aaa-test-hook', LANG='en_US.UTF-8')
(root / 'config').mkdir()
children = []
pane = None
requests = []
phase = 'startup'
changed = threading.Condition()

def run(*args):
    p = subprocess.run([aaa, '--json', *args], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
    if p.returncode:
        raise RuntimeError(f"CLI {args}: {p.stderr.decode(errors='replace')} {p.stdout.decode(errors='replace')}")
    return json.loads(p.stdout)

def event(value):
    return 'data: ' + json.dumps(value) + '\n\n'

class Handler(BaseHTTPRequestHandler):

    def log_message(self, *args):
        pass

    def do_GET(self):
        self.respond('{"data":[]}', False)

    def respond(self, body, sse):
        b = body.encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream' if sse else 'application/json')
        self.send_header('Content-Length', str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        inputs = req.get('input', [])
        messages = [i for i in inputs if i.get('type') == 'message' or i.get('role')]
        # brief·요약의 인용으로 통과하지 않고 developer hook 문맥만 확인한다.
        hook_messages = [m for m in messages if m.get('role') == 'developer' and '# 진행 보고(aaa)' in json.dumps(m, ensure_ascii=False)]
        with changed:
            record = dict(phase=phase, tools=len(req.get('tools', [])), startup_contexts=len(hook_messages), memory_context=any(('AAA_REENTRY_PROOF' in json.dumps(m) for m in hook_messages)))
            requests.append(record)
            (root / 'requests.json').write_text(json.dumps(requests, indent=2), encoding='utf8')
            changed.notify_all()
        marker = 'AAA_COMPACT_SUMMARY' if phase == 'compact' else 'AAA_HOOK_STARTUP_OK' if phase == 'startup' else 'AAA_HOOK_AFTER_COMPACT_OK'
        item = dict(type='message', role='assistant', id='msg_' + uuid.uuid4().hex, content=[dict(type='output_text', text=marker)])
        self.respond(event(dict(type='response.created', response=dict(id='resp_' + uuid.uuid4().hex))) + event(dict(type='response.output_item.done', item=item)) + event(dict(type='response.completed', response=dict(id='resp_' + uuid.uuid4().hex, usage=dict(input_tokens=120, output_tokens=10, total_tokens=130)))), True)
server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()

def start(kind, ready):
    p = subprocess.Popen([aaa, kind] + (['--host', 'local', '--dev-mode'] if kind == 'agent' else []), env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    children.append(p)
    q = queue.Queue()

    def read():
        with (root / (kind + '.log')).open('wb') as out:
            for line in p.stderr:
                out.write(line)
                out.flush()
                q.put(line.decode(errors='replace'))
        q.put(None)
    threading.Thread(target=read, daemon=True).start()
    until = time.monotonic() + 20
    while True:
        line = q.get(timeout=max(0.001, until - time.monotonic()))
        if line is None:
            raise RuntimeError(kind + ' EOF')
        if ready in line:
            return
try:
    git = 'git'
    if os.name == 'nt':
        git = 'C:\\Program Files\\Git\\cmd\\git.exe'
        env['PATH'] = str(pathlib.Path(git).parent) + os.pathsep + env['PATH']
    repo = root / 'repo'
    repo.mkdir()
    for args in [('init', '-b', 'main'), ('config', 'user.name', 'Example'), ('config', 'user.email', 'test@example.com')]:
        subprocess.run([git, '-C', str(repo), *args], check=True, stdout=subprocess.DEVNULL)
    (repo / 'seed.txt').write_text('fixture')
    subprocess.run([git, '-C', str(repo), 'add', '.'], check=True)
    subprocess.run([git, '-C', str(repo), 'commit', '-m', 'fixture'], check=True, stdout=subprocess.DEVNULL)
    provider = 'model_providers.fixture={name="fixture",base_url="http://127.0.0.1:' + str(server.server_port) + '/v1",wire_api="responses",requires_openai_auth=false}'
    args = ['--disable', 'plugins', '--disable', 'remote_plugin', '-c', 'model_provider="fixture"', '-c', provider, '-c', 'check_for_update_on_startup=false', '-c', 'analytics.enabled=false']

    def arr(v):
        return '[' + ','.join((json.dumps(x, ensure_ascii=False) for x in v)) + ']'
    (root / 'config/host.toml').write_text('schema=1\n[host]\nagent_listen=""\n[web]\nlisten=""\n[local_agent]\nmode="none"\nname="mac-a"\n[projects]\nroot=' + json.dumps(str(root / 'projects')) + '\nallow_profiles=["fixture"]\n[safety.probe]\nyolo=true\ntrust_screens="coordinator"\n[profiles.fixture]\nharness="codex"\nmodel="gpt-5.4"\ncommand=' + arr([codex]) + '\nargs=' + arr(args) + '\n', encoding='utf8')
    (root / 'config/agent.toml').write_text('schema=1\n[agent]\nname="mac-a"\nwork_root=' + json.dumps(str(root / 'work')) + '\n', encoding='utf8')
    start('host', 'host 시작')
    start('agent', 'host에 연결했다')
    run('project', 'create', 'probe', '--thread-profile', 'fixture', '--repo', str(repo) + '@mac-a')
    token = pathlib.Path(a.canary).read_text().strip()
    claims = {'sub': 'fixture-user', 'exp': 4102444800, 'https://api.openai.com/auth': {'chatgpt_account_id': 'fixture-account', 'chatgpt_plan_type': 'plus'}}
    payload = base64.urlsafe_b64encode(json.dumps(claims).encode()).decode().rstrip('=')
    auth = root / 'fake-auth.json'
    auth.write_text(json.dumps(dict(auth_mode='apikey', OPENAI_API_KEY=token, tokens=dict(id_token='eyJhbGciOiJub25lIn0.' + payload + '.fixture', access_token=token, refresh_token=token, account_id='fixture-account'))))
    run('account', 'import', 'fixture', '--harness', 'codex', '--file', str(auth))
    auth.unlink()
    task = root / 'task.md'
    task.write_text('격리 hook 확인.', encoding='utf8')
    thread = run('thread', 'start', 'probe', '--title', '계정 hook 확인', '--profile', 'fixture', '--account', 'fixture', '--machine', 'mac-a', '--kind', 'worktree', '--repo', str(repo), '--base', 'main', '--task-file', str(task))
    pane = thread['pane']
    run('pane', 'wait-output', pane, '--match', 'AAA_HOOK_STARTUP_OK', '--timeout', '20000')
    assert any((r['phase'] == 'startup' and r['startup_contexts'] > 0 for r in requests)), requests
    # 본문 표시는 턴 종료보다 빠르다. 입력 가능한 실제 완료 화면까지 기다린다.
    run('pane', 'wait-output', pane, '--match', 'Worked for', '--timeout', '5000')
    # host가 저장한 재독 문맥을 흉내 내 실제 compact hook의 재독 경로를 확인한다.
    specfile = root / 'state/agent/sessions' / pane / 'spec.json'
    spec = json.loads(specfile.read_text(encoding='utf8'))
    spec['spec']['env'].append(['AAA_AUTOMATION_REENTRY', 'AAA_REENTRY_PROOF: 현재 brief와 memory를 다시 읽는다.'])
    specfile.write_text(json.dumps(spec, ensure_ascii=False), encoding='utf8')
    phase = 'compact'
    run('pane', 'send-text', pane, '/compact')
    # ConPTY의 쓰기 ACK와 TUI가 입력을 읽은 사건은 다르다.
    run('pane', 'wait-output', pane, '--source', 'visible', '--match', '/compact', '--timeout', '5000')
    run('pane', 'send-keys', pane, 'enter')
    run('pane', 'wait-output', pane, '--match', 'Context compacted', '--timeout', '20000')
    phase = 'after'
    run('pane', 'send-text', pane, 'Continue fixture.')
    run('pane', 'wait-output', pane, '--source', 'visible', '--match', 'Continue fixture.', '--timeout', '5000')
    run('pane', 'send-keys', pane, 'enter')
    run('pane', 'wait-output', pane, '--match', 'AAA_HOOK_AFTER_COMPACT_OK', '--timeout', '20000')
    assert any((r['phase'] == 'after' and r['startup_contexts'] > 0 and r['memory_context'] for r in requests)), requests
    home = root / 'state/agent/account-runs/probe' / thread['id'] / 'codex-home'
    hooks = json.loads((home / 'hooks.json').read_text(encoding='utf8'))
    assert set(hooks['hooks']) == {'SessionStart', 'UserPromptSubmit', 'PostToolUse'}
    assert 'hooks = true' in (home / 'config.toml').read_text(encoding='utf8')
    (root / 'result.json').write_text(json.dumps(dict(result='passed', codex=subprocess.check_output([codex, '--version']).decode().strip(), startup=True, compact=True, requests=requests), indent=2), encoding='utf8')
    print((root / 'result.json').read_text())
except BaseException as error:
    (root / 'failure.txt').write_text(str(error), encoding='utf8')
    if pane:
        try:
            (root / 'failure-screen.json').write_text(json.dumps(run('pane', 'read', pane, '--lines', '1000'), ensure_ascii=False, indent=2), encoding='utf8')
        except Exception:
            pass
    raise
finally:
    if pane:
        try:
            run('pane', 'close', pane)
        except Exception:
            pass
    for p in reversed(children):
        if p.poll() is None:
            p.terminate()
            p.wait(timeout=10)
    server.shutdown()
    server.server_close()
