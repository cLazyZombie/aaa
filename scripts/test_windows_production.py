"""실제 CLI의 한 번 빌드·산출물 불변·SHA/심볼/native 결속을 가짜 컴파일러로 확인한다."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("windows-production.py")
spec = importlib.util.spec_from_file_location("windows_production", SCRIPT)
production = importlib.util.module_from_spec(spec)
spec.loader.exec_module(production)

GUID = "{01234567-89AB-CDEF-0123-456789ABCDEF}"
TOOLS = r'''
import json, os, pathlib, sys
tool = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
if args == ['--version']:
    print('fixture tool')
elif tool == 'npm':
    pass
elif tool == 'cargo':
    with open(os.environ['BUILD_CALLS'], 'a') as f:
        f.write(json.dumps(dict(args=args, salt=os.environ['AAA_BUILD_SALT'], target=os.environ['CARGO_TARGET_DIR']))+'\n')
    if os.environ.get('FAIL_COMPILE'): sys.exit(42)
    target = pathlib.Path(os.environ['CARGO_TARGET_DIR'])/'x86_64-pc-windows-msvc/release'
    target.mkdir(parents=True)
    for name in ['aaa.exe', 'aaa.pdb']: (target/name).write_text('matching fixture')
elif tool == 'llvm-readobj':
    print('Format: COFF-x86-64\nPDBGUID: {01234567-89AB-CDEF-0123-456789ABCDEF}\nPDBAge: 1')
elif tool == 'llvm-pdbutil':
    guid = '{11234567-89AB-CDEF-0123-456789ABCDEF}' if os.environ.get('WRONG_SYMBOLS') else '{01234567-89AB-CDEF-0123-456789ABCDEF}'
    print('GUID: '+guid+'\nAge: 1\nHas Debug Info: true\nIs stripped: false')
    if not os.environ.get('NO_LINES'): print('line/addr entries = 4')
'''


@unittest.skipUnless(os.name == "posix", "실제 Unix CLI와 가짜 cross 컴파일러를 실행한다")
class WindowsProduction(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="aaa-production-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        (self.root / "scripts").mkdir()
        (self.root / "web").mkdir()
        shutil.copyfile(SCRIPT, self.root / "scripts/windows-production.py")
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.2"\n')
        (self.root / ".gitignore").write_text("target/\nartifact/\ncalls\n")
        self.tools = self.root / "tools"
        self.tools.mkdir()
        for name in ("cargo", "npm", "llvm-readobj", "llvm-pdbutil"):
            path = self.tools / name
            path.write_text(f"#!{sys.executable}\n" + TOOLS)
            path.chmod(0o755)
        for args in (("init", "-qb", "main"), ("config", "user.name", "Test"),
                     ("config", "user.email", "test@example.com"), ("config", "commit.gpgsign", "false"),
                     ("add", "."), ("commit", "-qm", "fixture")):
            self.git(*args)
        self.commit = self.git("rev-parse", "HEAD").strip()
        self.artifact = self.root / "artifact"
        self.env = {**os.environ, "PATH": str(self.tools) + os.pathsep + os.environ["PATH"],
                    "BUILD_CALLS": str(self.root / "calls")}

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True, stderr=subprocess.PIPE)

    def cli(self, action, commit=None, **env):
        return subprocess.run([sys.executable, "scripts/windows-production.py", action,
                               "--commit", commit or self.commit, "--artifact", str(self.artifact)],
                              cwd=self.root, env={**self.env, **env}, capture_output=True, text=True)

    def build(self):
        result = self.cli("build")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return json.loads((self.artifact / "provenance.json").read_text())

    def native_receipt(self, data):
        (self.artifact / "native-check.json").write_text(json.dumps(dict(
            commit=self.commit, sha256=data["sha256"], software=data["software"], config_check_exit=0)))

    def test_build_once_and_verify_same_copied_pair_without_compiler(self):
        data = self.build()
        calls = (self.root / "calls").read_text().splitlines()
        self.assertEqual(len(calls), 1)
        call = json.loads(calls[0])
        self.assertEqual(call["args"], ["xwin", "build", "--release", "--locked", "-p", "aaa", "--bin", "aaa",
                                        "--target", "x86_64-pc-windows-msvc"])
        self.assertEqual(call["salt"], self.commit[:12])
        self.assertEqual(call["target"], str(self.root / "target"))
        self.assertEqual(data["symbols"], dict(guid=GUID, age=1, line_tables=True))
        self.assertNotIn(str(self.root), (self.artifact / "provenance.json").read_text())
        before = {p.name: p.read_bytes() for p in self.artifact.iterdir()}
        self.assertNotEqual(self.cli("verify").returncode, 0, "native 영수증 없는 후보는 전달하지 않는다")
        self.native_receipt(data)
        result = self.cli("verify")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for name, contents in before.items():
            self.assertEqual((self.artifact / name).read_bytes(), contents)
        self.assertNotEqual(self.cli("build").returncode, 0)
        self.assertEqual((self.root / "calls").read_text().splitlines(), calls)

    def test_different_sha_changed_bytes_symbols_and_missing_lines_are_rejected(self):
        data = self.build()
        self.native_receipt(data)
        self.assertNotEqual(self.cli("verify", commit="a" * 40).returncode, 0)
        self.assertNotEqual(self.cli("verify", WRONG_SYMBOLS="1").returncode, 0)
        self.assertNotEqual(self.cli("verify", NO_LINES="1").returncode, 0)
        (self.artifact / "aaa.pdb").write_text("different build")
        result = self.cli("verify")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("aaa.pdb", result.stderr)

    def test_dirty_source_fails_before_compile_and_compile_failure_is_not_retried(self):
        (self.root / "untracked.rs").write_text("change")
        rejected = self.cli("build")
        self.assertNotEqual(rejected.returncode, 0)
        self.assertFalse((self.root / "calls").exists())
        (self.root / "untracked.rs").unlink()
        failed = self.cli("build", FAIL_COMPILE="1")
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("42", failed.stderr)
        self.assertEqual(len((self.root / "calls").read_text().splitlines()), 1)
        self.assertFalse(self.artifact.exists())


if __name__ == "__main__":
    unittest.main()
