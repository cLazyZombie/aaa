"""실제 자식 실행으로 최초 원문·좁은 분류·새 프로세스 1회·실패 전파를 확인한다."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import socket
import threading
import unittest
from unittest import mock

from scripts import windows_install_recheck as owner

NAME = 'worker_lifetime::approved_worker_outlives_its_cli_and_records_the_same_id_after_private_display_disappears'


def stalled(name=NAME):
    return f'''running 1 test
test {name} ... FAILED

failures:

---- {name} stdout ----
thread '{name}' panicked at crates/aaa/tests/install/authorization.rs:328:
install 승인 PTY(17) 사건 상한(elapsed=60s)
PTY stalled: answered=6/6, waiting=child_exit
stack={{"symbol":"ReadConsoleW"}}
state={{"reader_io_pending":{{"status":"ok","value":true}}}}
console={{"prefix_enter_down":true}}

failures:
    {name}
test result: FAILED. 0 passed; 1 failed
error: 1 target failed:
    `-p aaa --test install`
'''


FAKE = '''import json, os, pathlib, sys
sys.stdout.reconfigure(encoding='utf-8')
root = pathlib.Path.cwd()
config = json.loads((root / 'config.json').read_text(encoding='utf-8'))
with (root / 'calls.jsonl').open('a') as f: f.write(json.dumps({'pid':os.getpid(),'args':sys.argv[1:]})+'\\n')
first = '--workspace' in sys.argv
print(config['first'] if first else 'test exact owner ... ' + ('ok' if config['retry_exit']==0 else 'FAILED'), flush=True)
sys.exit(config['first_exit'] if first else config['retry_exit'])
'''


class WindowsInstallRecheck(unittest.TestCase):
    def invoke(self, text, first_exit=101, retry_exit=0):
        lab = tempfile.TemporaryDirectory(prefix='aaa recheck ')
        self.addCleanup(lab.cleanup)
        root = Path(lab.name)
        (root / 'cargo fixture.py').write_text(FAKE, encoding='utf-8')
        (root / 'config.json').write_text(json.dumps({'first': text, 'first_exit': first_exit, 'retry_exit': retry_exit}), encoding='utf-8')
        output = io.StringIO()
        with contextlib.redirect_stdout(output), mock.patch.dict(os.environ, {'GITHUB_STEP_SUMMARY': str(root / 'step-summary.md')}):
            result = owner.run(root, root / 'logs', (sys.executable, str(root / 'cargo fixture.py')))
        calls = [json.loads(line) for line in (root / 'calls.jsonl').read_text().splitlines()]
        summary = json.loads((root / 'logs/summary.json').read_text())
        return result, calls, summary, root, output.getvalue()

    def test_confirmed_stall_has_one_fresh_owner_and_preserves_first_failure(self):
        result, calls, summary, root, output = self.invoke(stalled())
        self.assertEqual(result, 0)
        self.assertEqual(len(calls), 2)
        self.assertNotEqual(calls[0]['pid'], calls[1]['pid'])
        self.assertEqual(calls[1]['args'], ['test', '--profile', 'verify', '--locked', '-p', 'aaa', '--test', 'install', NAME, '--', '--exact'])
        self.assertEqual(summary['first_exit'], 101)
        self.assertEqual(summary['rechecks'], [{'owner': NAME, 'exit': 0, 'log': 'owner-1.log'}])
        self.assertIn('::warning', output)
        self.assertIn('사건 상한', (root / 'logs/workspace-first.log').read_text())
        self.assertIn('ok', (root / 'logs/owner-1.log').read_text())
        self.assertIn(NAME, (root / 'step-summary.md').read_text(encoding='utf-8'))

    def test_failed_recheck_is_not_repeated_and_remains_failure(self):
        result, calls, summary, _, _ = self.invoke(stalled(), retry_exit=101)
        self.assertEqual((result, len(calls), summary['exit']), (101, 2, 101))

    def test_clean_workspace_is_executed_once(self):
        result, calls, summary, _, _ = self.invoke('test result: ok. 43 passed', first_exit=0)
        self.assertEqual((result, len(calls), summary['rechecks']), (0, 1, []))

    def test_other_panic_owner_target_or_missing_evidence_remains_first_failure(self):
        for text in [stalled('unknown::test'), stalled().replace('사건 상한', '다른 단언'),
                     stalled().replace('ReadConsoleW', 'other'), stalled().replace('"value":true', '"value":false'),
                     stalled().replace('"prefix_enter_down":true', '"prefix_enter_down":false'),
                     stalled().replace('error: 1 target failed:', 'error: 2 targets failed:'),
                     stalled() + '\ntest unrelated::also_failed ... FAILED\n']:
            with self.subTest(text=text[:60]):
                result, calls, summary, _, _ = self.invoke(text)
                self.assertEqual((result, len(calls), summary['rechecks']), (101, 1, []))

    def test_non_cargo_failure_code_is_not_rechecked(self):
        result, calls, _, _, _ = self.invoke(stalled(), first_exit=7)
        self.assertEqual((result, len(calls)), (7, 1))

    def test_only_the_three_documented_owners_have_the_stall_exception(self):
        documented = [NAME,
                      'wizard::both_wizards_back_from_host_choices_then_submit_the_same_agent_plan_as_explicit_cli',
                      'recovery::closed_enrollment_window_keeps_the_id_and_action_after_files_are_applied']
        # 사용자가 승인한 재확인 예외의 범위다. 목록이 늘어도 실패해야 한다.
        self.assertEqual(owner.OWNERS, set(documented))
        for name in documented:
            with self.subTest(name=name):
                self.assertEqual(owner.stalled_owners(stalled(name)), [name])
        self.assertEqual(owner.stalled_owners(stalled() + '\nerror: 2 targets failed:\n'), [])

    def test_direct_cli_on_a_local_machine_is_rejected_before_cargo(self):
        # 이 시험이 도는 Unix에서는 OS 조건만으로 거절된다. CI 인가 값 둘의 판정은 여기서 가리지 못한다.
        with tempfile.TemporaryDirectory() as root:
            env = dict(os.environ)
            env.pop('GITHUB_ACTIONS', None)
            env.pop('AAA_CI_WINDOWS_INSTALL_RECHECK', None)
            result = subprocess.run([sys.executable, str(Path(owner.__file__)), '--logs', root + '/logs'],
                                    env=env, capture_output=True, encoding='utf-8', errors='replace')
            self.assertEqual(result.returncode, 2)
            self.assertFalse((Path(root) / 'logs').exists())

    def test_partial_stdout_is_saved_before_child_finishes(self):
        with tempfile.TemporaryDirectory() as folder, socket.socket() as server:
            root = Path(folder)
            server.bind(('127.0.0.1', 0))
            server.listen()
            server.settimeout(5)
            source = root / 'live.py'
            source.write_text("import socket,sys\nprint('first bytes',end='',flush=True)\nwith socket.create_connection(('127.0.0.1',int(sys.argv[1]))) as s: s.recv(1)\n")
            ready = threading.Event()
            class Stream(io.StringIO):
                def write(self, value):
                    result = super().write(value)
                    if 'first bytes' in self.getvalue():
                        ready.set()
                    return result
            codes = []
            log = root / 'live.log'
            with contextlib.redirect_stdout(Stream()):
                thread = threading.Thread(target=lambda: codes.append(owner.execute([sys.executable, str(source), str(server.getsockname()[1])], root, log)))
                thread.start()
                with server.accept()[0] as connection:
                    try:
                        self.assertTrue(ready.wait(5))
                        self.assertTrue(thread.is_alive())
                        self.assertEqual(log.read_text(), 'first bytes')
                    finally:
                        connection.sendall(b'!')
                thread.join(5)
            self.assertFalse(thread.is_alive())
            self.assertEqual(codes, [0])


if __name__ == '__main__':
    unittest.main()
