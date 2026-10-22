"""실제 빌드 스크립트의 Git 격리·출처·빌드 위치·정리 계약. 컴파일러만 기록하는 가짜다."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().with_name("deploy-build.sh")


@unittest.skipUnless(os.name == "posix", "bash 빌드 경로는 Unix에서 확인한다")
class DeployBuildContract(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="aaa-build-contract-")
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name).resolve()
        self.repo, origin, tools = [self.root / n for n in ("repo", "origin.git", "tools")]
        self.repo.mkdir()
        tools.mkdir()
        self.env = {**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"], "BUILD_LOG": str(self.root / "calls")}
        self.env.pop("AAA_DEPLOY_BUILD_ROOT", None)
        self.git("init", "-b", "main")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")
        (self.repo / "web").mkdir()
        (self.repo / "web/package.json").write_text("{}")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        self.commit = self.git("rev-parse", "HEAD")
        subprocess.run(["git", "init", "--bare", str(origin)], check=True, capture_output=True)
        self.git("remote", "add", "origin", str(origin))
        self.git("push", "-u", "origin", "main")
        for tool, body in {
            "npm": '''#!/usr/bin/env python3
import json, os, sys
if sys.argv[1:] == ['--version']: print('12.0.0')
else:
 with open(os.environ['BUILD_LOG'], 'a') as f: f.write(json.dumps(dict(tool='npm', args=sys.argv[1:], cwd=os.getcwd()))+'\\n')
 if os.environ.get('FAIL_STAGE') == 'npm': sys.exit(41)
''',
            "cargo": '''#!/usr/bin/env python3
import json, os, pathlib, sys
if sys.argv[1:] == ['--version']: print('cargo 1.95.0')
else:
 with open(os.environ['BUILD_LOG'], 'a') as f: f.write(json.dumps(dict(tool='cargo', args=sys.argv[1:], salt=os.environ['AAA_BUILD_SALT'], cwd=os.getcwd(), target=os.environ['CARGO_TARGET_DIR'], split=os.environ.get('CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO')))+'\\n')
 if os.environ.get('FAIL_STAGE') == 'cargo': sys.exit(42)
 binary = pathlib.Path(os.environ['CARGO_TARGET_DIR'])/'release/aaa'
 binary.parent.mkdir(parents=True)
 salt = 'wrong' if os.environ.get('FAIL_STAGE') == 'version' else os.environ['AAA_BUILD_SALT']
 binary.write_text('#!/bin/sh\\nprintf "aaa 0.1.2+'+salt+'\\\\n"\\n')
 binary.chmod(0o755)
 if os.environ.get('FAIL_STAGE') != 'symbols' and not os.environ.get('NO_CACHE_SYMBOLS'):
  symbols = binary.with_name('aaa.dSYM')/'Contents/Resources/DWARF'
  symbols.mkdir(parents=True)
  (symbols/'aaa').write_bytes(b'matching-symbol-fixture')
''',
            "dsymutil": '''#!/usr/bin/env python3
import json, os, pathlib, sys
with open(os.environ['BUILD_LOG'], 'a') as f: f.write(json.dumps(dict(tool='dsymutil', args=sys.argv[1:]))+'\\n')
if os.environ.get('FAIL_STAGE') == 'symbols': sys.exit(43)
symbols = pathlib.Path(sys.argv[1]+'.dSYM')/'Contents/Resources/DWARF'
symbols.mkdir(parents=True)
(symbols/'aaa').write_bytes(b'matching-symbol-fixture')
''',
        }.items():
            file = tools / tool
            file.write_text(body)
            file.chmod(0o755)

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], text=True, stderr=subprocess.PIPE).strip()

    def build(self, output=None, revision=None, env=None, cwd=None):
        return subprocess.run(
            ["bash", str(SCRIPT), revision or self.commit, "--out", str(output or self.root / "candidate")],
            cwd=cwd or self.repo, env={**self.env, **(env or {})}, capture_output=True, text=True,
        )

    def calls(self):
        return [json.loads(line) for line in (self.root / "calls").read_text().splitlines()]

    def assert_cleaned(self, parent):
        self.assertEqual(self.git("worktree", "list", "--porcelain").count("worktree "), 1)
        self.assertFalse(list(parent.glob("aaa-deploy-build.*")), "이번 빌드의 source·target은 실패해도 지운다")

    def test_main_ancestor_is_built_once_with_salt_and_provenance(self):
        (self.repo / "dirty").write_text("caller changes")
        output = self.root / "candidate"
        result = self.build(output)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        manifest = json.loads((output / "provenance.json").read_text())
        self.assertEqual(manifest["commit"], self.commit)
        self.assertEqual(manifest["origin_main"], self.commit)
        self.assertEqual(manifest["sha256"], hashlib.sha256((output / "aaa").read_bytes()).hexdigest())
        self.assertEqual(manifest["software"], "aaa 0.1.2+" + self.commit[:12])
        self.assertEqual((output / "aaa.dSYM/Contents/Resources/DWARF/aaa").read_bytes(), b"matching-symbol-fixture")
        self.assertFalse((output / "aaa.dSYM").is_symlink())
        self.assertEqual(set(manifest), {"commit", "origin_main", "sha256", "software", "built_at", "tools"})
        self.assertNotIn(str(self.root), (output / "provenance.json").read_text())
        calls = self.calls()
        self.assertEqual([(c['tool'], c['args']) for c in calls], [("npm", ["ci", "--no-audit", "--no-fund"]), ("npm", ["run", "build"]), ("cargo", ["build", "--release", "--locked", "-p", "aaa", "--bin", "aaa"])])
        self.assertEqual(calls[-1]["salt"], self.commit[:12])
        if os.uname().sysname == "Darwin":
            self.assertEqual(calls[-1]["split"], "unpacked")
        self.assertNotEqual(calls[-1]["cwd"], str(self.repo))
        self.assertEqual((self.repo / "dirty").read_text(), "caller changes")
        self.assertEqual(self.git("rev-parse", "HEAD"), self.commit)
        self.assert_cleaned(self.repo / "target/deploy-build")
        self.assertNotEqual(self.build(output).returncode, 0, "기존 후보는 덮어쓰지 않는다")
        (self.repo / "unlanded").write_text("candidate")
        self.git("add", "unlanded")
        self.git("commit", "-qm", "unlanded")
        rejected = self.build(self.root / "rejected", self.git("rev-parse", "HEAD"))
        self.assertNotEqual(rejected.returncode, 0, rejected.stdout + rejected.stderr)
        self.assertIn("origin/main", rejected.stderr)
        self.assertFalse((self.root / "rejected").exists())
        self.assertEqual(len(self.calls()), 3, "거부한 커밋을 빌드하면 안 된다")

    def test_default_root_ignores_tmpdir_and_shared_cargo_target(self):
        system_temp = self.root / "system temp"
        system_temp.mkdir()
        shared_target = self.root / "shared target"
        shared_target.mkdir()
        (shared_target / "keep").write_text("other build")
        roots = []
        for index, override in enumerate((None, "")):
            with self.subTest(override=override):
                env = {"TMPDIR": str(system_temp), "CARGO_TARGET_DIR": str(shared_target)}
                if override is not None:
                    env["AAA_DEPLOY_BUILD_ROOT"] = override
                result = self.build(self.root / f"candidate-{index}", env=env, cwd=self.repo / "web")
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                scratch = Path(self.calls()[-1]["cwd"]).parent
                roots.append(scratch)
                self.assertEqual(scratch.parent, self.repo / "target/deploy-build")
                self.assertEqual(self.calls()[-1]["target"], str(scratch / "target"))
                self.assert_cleaned(scratch.parent)
        self.assertNotEqual(*roots, "빌드마다 source와 target을 새로 만든다")
        self.assertEqual(list(system_temp.iterdir()), [])
        self.assertEqual((shared_target / "keep").read_text(), "other build")
        self.assertEqual(len(list(shared_target.iterdir())), 1)

    @unittest.skipUnless(os.name == "posix" and os.uname().sysname == "Darwin", "macOS는 dSYM이 필수다")
    def test_macos_missing_symbols_does_not_emit_provenance(self):
        result = self.build(env={"FAIL_STAGE": "symbols"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("dSYM", result.stderr)
        self.assertFalse((self.root / "candidate/provenance.json").exists())
        self.assert_cleaned(self.repo / "target/deploy-build")

    @unittest.skipUnless(os.name == "posix" and os.uname().sysname == "Darwin", "macOS는 dSYM을 수집한다")
    def test_macos_collects_symbols_when_compiler_cache_did_not(self):
        result = self.build(env={"NO_CACHE_SYMBOLS": "1"})
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.calls()[-1]["tool"], "dsymutil")
        self.assertEqual((self.root / "candidate/aaa.dSYM/Contents/Resources/DWARF/aaa").read_bytes(), b"matching-symbol-fixture")
        self.assertTrue((self.root / "candidate/provenance.json").is_file())
        self.assert_cleaned(self.repo / "target/deploy-build")

    def test_custom_parent_handles_absolute_and_caller_relative_paths(self):
        for index, configured in enumerate((str(self.root / "absolute build root"), "relative build root")):
            with self.subTest(configured=configured):
                cwd = self.repo / "web"
                parent = (cwd / configured).resolve()
                parent.mkdir()
                (parent / "keep").write_text("other build")
                result = self.build(self.root / f"candidate-{index}", env={"AAA_DEPLOY_BUILD_ROOT": configured}, cwd=cwd)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                scratch = Path(self.calls()[-1]["cwd"]).parent
                self.assertEqual(scratch.parent, parent)
                self.assertEqual(self.calls()[-1]["target"], str(scratch / "target"))
                self.assert_cleaned(parent)
                self.assertEqual(list(parent.iterdir()), [parent / "keep"])
                self.assertEqual((parent / "keep").read_text(), "other build")

    def test_failures_clean_only_owned_scratch_and_preserve_partial_output(self):
        parent = self.root / "build root"
        parent.mkdir()
        (parent / "keep").write_text("other build")
        for stage, code, count in (("npm", 41, 1), ("cargo", 42, 3), ("version", 1, 3)):
            with self.subTest(stage=stage):
                output = self.root / stage
                before = len(self.calls()) if (self.root / "calls").exists() else 0
                result = self.build(output, env={"AAA_DEPLOY_BUILD_ROOT": str(parent), "FAIL_STAGE": stage})
                self.assertEqual(result.returncode, code, result.stdout + result.stderr)
                self.assertEqual(len(self.calls()) - before, count, "실패를 다시 빌드하지 않는다")
                self.assert_cleaned(parent)
                self.assertEqual((parent / "keep").read_text(), "other build")
                self.assertEqual(output.exists(), stage == "version")
                if stage == "version":
                    self.assertTrue((output / "aaa").is_file(), "기존처럼 부분 출력은 보존한다")
                    self.assertFalse((output / "provenance.json").exists())

    def test_unusable_parent_fails_without_falling_back_to_system_temp(self):
        parent = self.root / "file"
        parent.write_text("keep")
        result = self.build(env={"AAA_DEPLOY_BUILD_ROOT": str(parent)})
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "calls").exists())
        self.assertFalse((self.root / "candidate").exists())
        self.assertEqual(parent.read_text(), "keep")


if __name__ == "__main__":
    unittest.main()
