"""실제 빌드 스크립트의 Git 격리·출처·빌드 호출 계약. 컴파일러만 기록하는 가짜다."""
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
    def test_main_ancestor_is_built_once_with_salt_and_provenance(self):
        with tempfile.TemporaryDirectory(prefix="aaa-build-contract-") as tmp:
            root = Path(tmp)
            repo, origin, tools = [root / n for n in ("repo", "origin.git", "tools")]
            repo.mkdir()
            tools.mkdir()
            env = {**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"], "BUILD_LOG": str(root / "calls")}

            def git(*args):
                return subprocess.check_output(["git", "-C", str(repo), *args], text=True, stderr=subprocess.PIPE).strip()

            git("init", "-b", "main")
            git("config", "commit.gpgsign", "false")
            git("config", "user.name", "Test")
            git("config", "user.email", "test@example.com")
            (repo / "web").mkdir()
            (repo / "web/package.json").write_text("{}")
            git("add", ".")
            git("commit", "-qm", "fixture")
            commit = git("rev-parse", "HEAD")
            subprocess.run(["git", "init", "--bare", str(origin)], check=True, capture_output=True)
            git("remote", "add", "origin", str(origin))
            git("push", "-u", "origin", "main")
            for tool, body in {
                "npm": '''#!/usr/bin/env python3
import json, os, sys
if sys.argv[1:] == ['--version']: print('12.0.0')
else:
 with open(os.environ['BUILD_LOG'], 'a') as f: f.write(json.dumps(dict(tool='npm', args=sys.argv[1:], cwd=os.getcwd()))+'\\n')
''',
                "cargo": '''#!/usr/bin/env python3
import json, os, pathlib, sys
if sys.argv[1:] == ['--version']: print('cargo 1.95.0')
else:
 with open(os.environ['BUILD_LOG'], 'a') as f: f.write(json.dumps(dict(tool='cargo', args=sys.argv[1:], salt=os.environ['AAA_BUILD_SALT'], cwd=os.getcwd()))+'\\n')
 binary = pathlib.Path(os.environ['CARGO_TARGET_DIR'])/'release/aaa'
 binary.parent.mkdir(parents=True)
 binary.write_text('#!/bin/sh\\nprintf "aaa 0.1.2+'+os.environ['AAA_BUILD_SALT']+'\\\\n"\\n')
 binary.chmod(0o755)
''',
            }.items():
                file = tools / tool
                file.write_text(body)
                file.chmod(0o755)
            (repo / "dirty").write_text("caller changes")

            def run(output, revision):
                return subprocess.run(["bash", str(SCRIPT), revision, "--out", str(output)], cwd=repo, env=env, capture_output=True, text=True)

            output = root / "candidate"
            result = run(output, commit)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            manifest = json.loads((output / "provenance.json").read_text())
            self.assertEqual(manifest["commit"], commit)
            self.assertEqual(manifest["origin_main"], commit)
            self.assertEqual(manifest["sha256"], hashlib.sha256((output / "aaa").read_bytes()).hexdigest())
            self.assertEqual(manifest["software"], "aaa 0.1.2+" + commit[:12])
            calls = [json.loads(line) for line in (root / "calls").read_text().splitlines()]
            self.assertEqual([(c['tool'], c['args']) for c in calls], [("npm", ["ci", "--no-audit", "--no-fund"]), ("npm", ["run", "build"]), ("cargo", ["build", "--release", "--locked", "-p", "aaa", "--bin", "aaa"])])
            self.assertEqual(calls[-1]["salt"], commit[:12])
            self.assertNotEqual(calls[-1]["cwd"], str(repo))
            self.assertEqual((repo / "dirty").read_text(), "caller changes")
            self.assertEqual(git("rev-parse", "HEAD"), commit)
            self.assertEqual(git("worktree", "list", "--porcelain").count("worktree "), 1)
            self.assertNotEqual(run(output, commit).returncode, 0, "기존 후보는 덮어쓰지 않는다")
            (repo / "unlanded").write_text("candidate")
            git("add", "unlanded")
            git("commit", "-qm", "unlanded")
            unlanded = git("rev-parse", "HEAD")
            rejected = run(root / "rejected", unlanded)
            self.assertNotEqual(rejected.returncode, 0, rejected.stdout + rejected.stderr)
            self.assertIn("origin/main", rejected.stderr)
            self.assertFalse((root / "rejected").exists())
            self.assertEqual(len((root / "calls").read_text().splitlines()), 3, "거부한 커밋을 빌드하면 안 된다")


if __name__ == "__main__":
    unittest.main()
