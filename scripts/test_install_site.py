"""실제 셸·HTTP·해시·자식 실행 경계. 서비스는 만들지 않고 임시 HOME에 격리한다."""

import copy
import hashlib
import http.server
import json
import os
import platform
import select
import shutil
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
import unittest
from unittest import mock
from pathlib import Path

from scripts.build_install_site import main, render, verify_site
from scripts.release import ASSETS, LAUNCHERS, RELEASES, ROOT, render_windows_entry


class SiteFixture(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="aaa-install-site-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.site = self.root / 'site'
        self.home = self.root / 'home space'
        self.home.mkdir()
        self.temp = self.root / 'temp space'
        self.temp.mkdir()
        self.record = self.root / 'setup-record'
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('AAA_')}
        self.env.update(HOME=str(self.home), USERPROFILE=str(self.home), TMPDIR=str(self.temp),
                        TMP=str(self.temp), TEMP=str(self.temp),
                        SETUP_RECORD=str(self.record), SETUP_EXIT='23', SETUP_VERSION='0.1.6',
                        AAA_CONFIG_DIR=str(self.home / 'config'), AAA_STATE_DIR=str(self.home / 'state'))
        self.payloads = {name: b'fixture' for name in ASSETS.values()}
        self.payloads.update({name: b'launcher fixture' for name in LAUNCHERS.values()})
        self.requests = []

    def build(self):
        self.manifest = {'version': '0.1.6', 'assets': {}, 'launchers': {}}
        for group, names in [('assets', ASSETS), ('launchers', LAUNCHERS)]:
            for target, name in names.items():
                self.manifest[group][target] = {
                    'url': f'{RELEASES}/v0.1.6/{name}',
                    'sha256': hashlib.sha256(self.payloads[name]).hexdigest(),
                }
        self.sums = ''.join(f'{hashlib.sha256(body).hexdigest()}  {name}\n'
                            for name, body in self.payloads.items())
        render(self.manifest, self.sums, self.site)
        self.payloads['SHA256SUMS'] = self.sums.encode()
        self.payloads['manifest.json'] = json.dumps(self.manifest).encode()
        for name in ('install.sh', 'install.ps1'):
            self.payloads[name] = (self.site / name).read_bytes()

    def serve(self):
        fixture = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                fixture.requests.append(self.path)
                body = fixture.payloads.get(self.path.removeprefix('/'))
                if body is None:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header('Content-Type', 'application/octet-stream')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args):
                pass

        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        worker = threading.Thread(target=server.serve_forever)
        worker.start()

        def stop():
            server.shutdown()
            worker.join()
            server.server_close()

        self.addCleanup(stop)
        self.url = f'http://127.0.0.1:{server.server_port}'


class SiteContract(SiteFixture):
    def test_published_urls_match_artifact_and_reject_old_or_missing_entry(self):
        self.build()
        self.serve()
        verify_site(self.site, self.url)
        self.assertEqual(self.requests, ['/install.sh', '/install.ps1'])
        for name in ('install.sh', 'install.ps1'):
            original = self.payloads[name]
            self.payloads[name] = b'old release'
            with self.assertRaisesRegex(ValueError, name):
                verify_site(self.site, self.url)
            del self.payloads[name]
            with self.assertRaisesRegex(ValueError, 'http=404'):
                verify_site(self.site, self.url)
            self.payloads[name] = original

    def test_changed_latest_fails_before_downloading_assets_or_writing_output(self):
        latest = json.dumps(dict(tag_name='v0.1.7', draft=False, prerelease=False)).encode()
        with mock.patch('scripts.build_install_site.read_url', return_value=latest) as read:
            with mock.patch.object(sys, 'argv', ['build_install_site.py', '--output', str(self.site),
                                               '--expected-tag', 'v0.1.6']):
                with self.assertRaises(SystemExit) as error, mock.patch('sys.stderr'):
                    main()
        self.assertEqual(error.exception.code, 1)
        read.assert_called_once_with('https://api.github.com/repos/cLazyZombie/aaa/releases/latest')
        self.assertFalse(self.site.exists())

    def test_generated_windows_entry_is_the_existing_release_owner(self):
        self.build()
        digest = hashlib.sha256(self.payloads[ASSETS['windows-x86_64']]).hexdigest()
        self.assertEqual((self.site / 'install.ps1').read_bytes(), render_windows_entry('0.1.6', digest))
        self.assertNotIn('@AAA_', (self.site / 'install.sh').read_text(encoding='utf-8'))
        self.assertEqual(sorted(p.name for p in self.site.iterdir()),
                         ['.nojekyll', 'install.ps1', 'install.sh', 'release.json'])

    def test_rejects_missing_duplicate_changed_and_foreign_release_metadata(self):
        self.build()
        for case in ('missing', 'duplicate', 'sha', 'url', 'old', 'invalid-version'):
            with self.subTest(case=case):
                manifest = copy.deepcopy(self.manifest)
                sums = self.sums
                if case == 'missing':
                    sums = '\n'.join(sums.splitlines()[1:])
                elif case == 'duplicate':
                    sums += sums.splitlines()[0] + '\n'
                elif case == 'sha':
                    manifest['assets']['linux-x86_64']['sha256'] = '0' * 64
                elif case == 'url':
                    manifest['assets']['linux-x86_64']['url'] = 'https://example.invalid/aaa'
                elif case == 'old':
                    manifest['version'] = '0.1.5'
                else:
                    manifest['version'] = '0.1.6; echo unsafe'
                output = self.root / case
                with self.assertRaises(ValueError):
                    render(manifest, sums, output)
                self.assertFalse(output.exists(), '검증 전에 Pages 파일을 만들었다')


class PagesWorkflow(unittest.TestCase):
    def setUp(self):
        self.workflow = (ROOT / '.github/workflows/install-pages.yml').read_text(encoding='utf-8')

    def test_trigger_trust_main_and_deploy_contract(self):
        for required in (
            'workflow_dispatch:', 'workflow_run:', 'workflows: [release]', 'types: [completed]',
            "github.repository == 'cLazyZombie/aaa'", "github.ref == 'refs/heads/main'",
            "github.event_name == 'workflow_dispatch'", "github.event_name == 'workflow_run'",
            "github.event.workflow_run.conclusion == 'success'",
            "github.event.workflow_run.event == 'push'",
            'github.event.workflow_run.head_repository.full_name == github.repository',
            "startsWith(github.event.workflow_run.head_branch, 'v')",
            "if: needs.build.outputs.publish == 'true'", 'name: github-pages',
            '--expected-tag "$EXPECTED_TAG"', '--verify-url "$PAGE_URL"',
            'name: github-pages\n          path: target/pages-artifact',
        ):
            self.assertIn(required, self.workflow)
        self.assertNotIn('\n  release:', self.workflow)
        # 완료한 release의 코드·artifact를 실행하지 않고 main의 게시 도구만 실행한다.
        self.assertNotIn('ref:', self.workflow)
        self.assertEqual(self.workflow.count("if: steps.release.outputs.publish == 'true'"), 3)
        deploy = self.workflow.split('\n  deploy:', 1)[1]
        self.assertIn('contents: read', deploy)
        self.assertLess(deploy.index('Pages 게시'), deploy.index('두 공개 설치 URL'))

    @unittest.skipUnless(os.name == 'posix' and shutil.which('jq'), '실제 bash·jq로 게시 조건을 검사한다')
    def test_actual_gate_handles_manual_stable_prerelease_draft_old_and_api_error(self):
        gate = textwrap.dedent(self.workflow.split('        run: |\n', 1)[1]
                               .split('      - name:', 1)[0])
        with tempfile.TemporaryDirectory(prefix='aaa-pages-gate-') as temporary:
            root = Path(temporary)
            gh = root / 'gh'
            gh.write_text(f'#!{sys.executable}\n' + '''import json, os, subprocess, sys
if os.environ['FAIL_API'] == '1':
    print('release API: HTTP 503', file=sys.stderr)
    sys.exit(1)
path = sys.argv[2]
release = dict(tag_name=os.environ['TAG'], draft=os.environ['DRAFT'] == '1',
               prerelease=os.environ['PRERELEASE'] == '1')
if path.endswith('/latest'):
    release['tag_name'] = os.environ['LATEST']
sys.exit(subprocess.run(['jq', '-r', sys.argv[4]], input=json.dumps(release), text=True).returncode)
''', encoding='utf-8')
            gh.chmod(0o755)
            for case, updates, publish, code in (
                ('stable', {}, True, 0),
                ('manual', dict(EVENT_NAME='workflow_dispatch', FAIL_API='1'), True, 0),
                ('prerelease-tag', dict(RELEASE_TAG='v0.1.6-rc.1'), False, 0),
                ('prerelease-flag', dict(PRERELEASE='1'), False, 0),
                ('draft', dict(DRAFT='1'), False, 0),
                ('old', dict(LATEST='v0.1.7'), False, 0),
                ('foreign-tag', dict(TAG='v0.1.7'), False, 0),
                ('api-error', dict(FAIL_API='1'), False, 1),
            ):
                with self.subTest(case=case):
                    output = root / case
                    env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'],
                               GITHUB_OUTPUT=str(output), GITHUB_REPOSITORY='example/aaa',
                               EVENT_NAME='workflow_run', RELEASE_TAG='v0.1.6', TAG='v0.1.6',
                               LATEST='v0.1.6', DRAFT='0', PRERELEASE='0', FAIL_API='0')
                    env.update(updates)
                    result = subprocess.run(['bash', '-c', gate], env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, code, result.stderr)
                    actual = output.read_text() if output.exists() else ''
                    self.assertEqual('publish=true\n' in actual, publish, result.stdout)
                    if case == 'stable':
                        self.assertIn('tag=v0.1.6\n', actual)
                    if case == 'api-error':
                        self.assertIn('HTTP 503', result.stderr)


UNIX_PAYLOAD = '''import json, os, pathlib, sys
if sys.argv[1:] == ['--version']:
    print('aaa ' + os.environ['SETUP_VERSION'])
    sys.exit(0)
pathlib.Path(os.environ['SETUP_RECORD']).write_text(json.dumps({
    'args': sys.argv[1:], 'tty': os.isatty(0), 'executable': sys.argv[0],
    'launchers': [p.name for p in pathlib.Path(sys.argv[0]).parent.glob('aaa-launcher-*.json')],
}))
sys.exit(int(os.environ['SETUP_EXIT']))
'''


@unittest.skipUnless(os.name == 'posix', 'Unix sh·제어 터미널은 macOS/Linux에서 실행한다')
class UnixEntry(SiteFixture):
    def setUp(self):
        super().setUp()
        self.tools = self.root / 'tools'
        self.tools.mkdir()
        self.curl = shutil.which('curl')
        self.assertIsNotNone(self.curl)
        body = f'#!{sys.executable}\n{UNIX_PAYLOAD}'.encode()
        for target, name in ASSETS.items():
            if not target.startswith('windows'):
                self.payloads[name] = body
        self.build()
        self.serve()
        self.tool('curl', '''import os, sys
args = sys.argv[1:]
for i, value in enumerate(args):
    if value.startswith('https://github.com/cLazyZombie/aaa/releases/download/v0.1.6/'):
        args[i] = os.environ['TEST_SERVER'] + '/' + value.rsplit('/', 1)[1]
    elif value == '=https':
        args[i] = '=http'
os.execv(os.environ['REAL_CURL'], [os.environ['REAL_CURL'], *args])
''')
        self.tool('uname', '''import os, sys
print({'-s': os.environ['TEST_OS'], '-m': os.environ['TEST_ARCH'], '-o': 'GNU/Linux'}[sys.argv[1]])
''')
        self.env.update(PATH=str(self.tools) + os.pathsep + self.env['PATH'],
                        TEST_SERVER=self.url, REAL_CURL=self.curl, TEST_OS=platform.system(),
                        TEST_ARCH=platform.machine())
        self.existing = self.home / '.local/bin/aaa'
        self.existing.parent.mkdir(parents=True)
        self.existing.write_text('existing installation stays')

    def tool(self, name, source):
        path = self.tools / name
        path.write_text(f'#!{sys.executable}\n{source}')
        path.chmod(0o755)

    def run_entry(self, *args, pipe=True):
        import pty
        # HTTP thread가 있는 부모에서 fork 코드를 실행하지 않는다. 새 Python이 제어 터미널을 연다.
        command = (['sh', '-c', 'url=$1; shift; curl -fsSL "$url/install.sh" | sh -s -- "$@"', 'entry', self.url, *args]
                   if pipe else ['sh', str(self.site / 'install.sh'), *args])
        master, slave = pty.openpty()
        owner = ('import os, fcntl, termios, sys; os.setsid(); '
                 'fcntl.ioctl(0, termios.TIOCSCTTY, 0); os.execvpe(sys.argv[1], sys.argv[1:], os.environ)')
        process = subprocess.Popen([sys.executable, '-c', owner, *command], env=self.env,
                                   stdin=slave, stdout=slave, stderr=slave, close_fds=True)
        os.close(slave)
        output = bytearray()
        deadline = time.monotonic() + 30
        try:
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not select.select([master], [], [], max(remaining, 0))[0]:
                    process.kill()
                    self.fail('설치 입구가 종료되지 않았다: ' + output.decode(errors='replace'))
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno != 5:  # Linux PTY EOF는 EIO다.
                        raise
                    break
                if not chunk:
                    break
                output.extend(chunk)
        finally:
            os.close(master)
            code = process.wait(timeout=5)
        self.assertEqual(self.existing.read_text(), 'existing installation stays')
        self.assertEqual(list(self.temp.iterdir()), [], '다운로드 임시 폴더가 남았다')
        return code, output.decode(errors='replace')

    def test_pipe_preserves_setup_arguments_terminal_exit_and_macos_launcher(self):
        for system, machine, target in [('Darwin', 'arm64', 'macos-aarch64'),
                                        ('Darwin', 'x86_64', 'macos-x86_64'),
                                        ('Linux', 'aarch64', 'linux-aarch64'),
                                        ('Linux', 'x86_64', 'linux-x86_64')]:
            with self.subTest(target=target):
                self.env.update(TEST_OS=system, TEST_ARCH=machine)
                self.requests.clear()
                code, output = self.run_entry('two words', '', '한글', 'literal;$()')
                self.assertEqual(code, 23, output)
                record = json.loads(self.record.read_text())
                self.assertEqual(record['args'], ['setup', 'two words', '', '한글', 'literal;$()'])
                self.assertTrue(record['tty'])
                self.assertFalse(Path(record['executable']).exists())
                expected = [LAUNCHERS[target]] if system == 'Darwin' else []
                self.assertEqual(record['launchers'], expected)
                self.assertEqual(self.requests, ['/install.sh', '/SHA256SUMS', '/' + ASSETS[target],
                                                *['/' + name for name in expected]])

    def test_saved_script_propagates_success(self):
        self.env['SETUP_EXIT'] = '0'
        code, output = self.run_entry('--help', pipe=False)
        self.assertEqual(code, 0, output)
        self.assertEqual(json.loads(self.record.read_text())['args'], ['setup', '--help'])

    def test_refuses_tampered_binary_launcher_checksums_and_version(self):
        self.env.update(TEST_OS='Darwin', TEST_ARCH='arm64')
        original = self.payloads.copy()
        for case in ('binary', 'launcher', 'sum', 'duplicate', 'missing', 'download', 'version'):
            with self.subTest(case=case):
                self.payloads = original.copy()
                self.env['SETUP_VERSION'] = '0.1.6'
                if case in ('binary', 'launcher'):
                    name = 'aaa-macos-aarch64' if case == 'binary' else LAUNCHERS['macos-aarch64']
                    self.payloads[name] += b'tampered'
                elif case == 'sum':
                    self.payloads['SHA256SUMS'] = self.sums.replace(
                        self.manifest['assets']['macos-aarch64']['sha256'], '0' * 64).encode()
                elif case == 'duplicate':
                    self.payloads['SHA256SUMS'] += next(line for line in self.sums.splitlines(keepends=True)
                                                      if line.endswith('  aaa-macos-aarch64\n')).encode()
                elif case == 'missing':
                    self.payloads['SHA256SUMS'] = b''
                elif case == 'download':
                    del self.payloads['aaa-macos-aarch64']
                else:
                    self.env['SETUP_VERSION'] = '0.1.7'
                code, output = self.run_entry()
                self.assertEqual(code, 2, output)
                self.assertFalse(self.record.exists(), output)
                self.assertIn('aaa bootstrap:', output)
                if case == 'download':
                    self.assertIn('curl 종료 코드 22', output)

    def test_unsupported_platform_and_no_terminal_fail_without_downloading_payload(self):
        for system, machine in [('FreeBSD', 'amd64'), ('Linux', 'riscv64')]:
            self.env.update(TEST_OS=system, TEST_ARCH=machine)
            self.requests.clear()
            code, output = self.run_entry()
            self.assertEqual(code, 2, output)
            self.assertEqual(self.requests, ['/install.sh'])
            self.assertFalse(self.record.exists())
        self.env.update(TEST_OS='Linux', TEST_ARCH='x86_64')
        self.requests.clear()
        result = subprocess.run(['sh', str(self.site / 'install.sh')], env=self.env,
                                stdin=subprocess.DEVNULL, capture_output=True, text=True,
                                start_new_session=True, timeout=30)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn('대화형 터미널', result.stderr)
        self.assertEqual(self.requests, [])


@unittest.skipUnless(os.name == 'nt', 'Windows PowerShell 5.1/7·PE 실행은 Windows 실기에서 검사한다')
class WindowsEntry(SiteFixture):
    def setUp(self):
        super().setUp()
        source = self.root / 'fixture.rs'
        source.write_text('''fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] { println!("aaa 0.1.6"); return; }
    std::fs::write(std::env::var_os("SETUP_RECORD").unwrap(), args.join("\\n")).unwrap();
    std::process::exit(23);
}''')
        binary = self.root / 'fixture.exe'
        subprocess.run(['rustc', str(source), '--edition=2024', '-C', 'target-feature=+crt-static',
                        '-o', str(binary)], check=True, capture_output=True, timeout=60)
        self.payloads[ASSETS['windows-x86_64']] = binary.read_bytes()
        self.build()
        self.serve()
        runner = self.root / 'owner.ps1'
        runner.write_text('''param([string]$BaseUrl)
$env:PSModulePath = [IO.Path]::Combine($PSHOME, 'Modules')
$ErrorActionPreference = 'Stop'
function Invoke-WebRequest {
    param([switch]$UseBasicParsing, [string]$Uri, [string]$OutFile)
    $prefix = 'https://github.com/cLazyZombie/aaa/releases/download/v0.1.6/'
    if (-not $Uri.StartsWith($prefix)) { throw 'unexpected URL' }
    Microsoft.PowerShell.Utility\\Invoke-WebRequest -UseBasicParsing -Uri ($BaseUrl + '/' + $Uri.Substring($prefix.Length)) -OutFile $OutFile
}
$before = $ErrorActionPreference
Invoke-RestMethod ($BaseUrl + '/install.ps1') | Invoke-Expression
if ($before -cne $ErrorActionPreference) { throw 'caller preference changed' }
exit $global:LASTEXITCODE
''', encoding='utf-8-sig')
        self.runner = runner

    def test_actual_irm_iex_preserves_existing_install_and_rejects_bad_hash(self):
        for shell in ('powershell.exe', 'pwsh.exe'):
            with self.subTest(shell=shell):
                home = self.home / shell
                target = home / '.local/bin/aaa.exe'
                target.parent.mkdir(parents=True)
                target.write_bytes(b'old installation')
                env = dict(self.env, USERPROFILE=str(home))
                result = subprocess.run([shell, '-NoLogo', '-NoProfile', '-NonInteractive',
                                         '-File', str(self.runner), '-BaseUrl', self.url],
                                        env=env, capture_output=True, timeout=60)
                self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
                self.assertEqual(self.record.read_text(), 'setup')
                self.assertEqual(target.read_bytes(), b'old installation')
                self.assertEqual(list(self.temp.iterdir()), [])
                self.record.unlink()
                clean = self.home / (shell + '-reject')
                clean.mkdir()
                name = ASSETS['windows-x86_64']
                original = self.payloads[name]
                self.payloads[name] += b'tamper'
                env['USERPROFILE'] = str(clean)
                try:
                    result = subprocess.run([shell, '-NoLogo', '-NoProfile', '-NonInteractive',
                                             '-File', str(self.runner), '-BaseUrl', self.url],
                                            env=env, capture_output=True, timeout=60)
                finally:
                    self.payloads[name] = original
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn(b'bootstrap_checksum_mismatch', result.stderr)
                self.assertFalse(self.record.exists())
                self.assertEqual(list(self.temp.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
