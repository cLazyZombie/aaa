"""검사 범위를 잘못 줄이거나 개발 결과를 착지 전체로 오인하지 않도록 실제 Git/CLI 경계를 본다."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from scripts import verify


class Verification(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="aaa-verify-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.addCleanup(patch.stopall)
        patch.object(verify, "ROOT", self.root).start()
        for args in (("init", "-qb", "main"), ("config", "user.name", "Test"),
                     ("config", "user.email", "test@example.com"), ("config", "commit.gpgsign", "false")):
            self.git(*args)
        (self.root / "scripts").mkdir()
        shutil.copyfile(Path(__file__).with_name("verify.py"), self.root / "scripts/verify.py")
        (self.root / "scripts/ci-local.sh").write_text('echo called >> calls\nexit "${CHECK_EXIT:-0}"\n')
        (self.root / "web/src").mkdir(parents=True)
        (self.root / "web/src/a.ts").write_text("old")
        (self.root / ".gitignore").write_text("target/\ncalls\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True, stderr=subprocess.PIPE)

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")

    def cli(self, *args, **env):
        context = {k: v for k, v in os.environ.items() if k != "AAA_E2E_ONLY"}
        context["PYTHONIOENCODING"] = "cp1252"
        return subprocess.run([sys.executable, "scripts/verify.py", *args, "--base", self.base],
                              cwd=self.root, env={**context, **env}, capture_output=True, text=True)

    def test_web_landing_uses_all_web_without_rust_precompile_or_os_repeats(self):
        (self.root / "web/src/a.ts").write_text("new")
        self.commit()
        result = self.cli("landing", "--dry-run")
        self.assertEqual(result.returncode, 0, result.stderr)
        data = json.loads(result.stdout)
        self.assertEqual(data["scope"], "web")
        self.assertEqual(data["base"], self.base)
        self.assertEqual(data["commands"], [["bash", "scripts/ci-local.sh", "--only", "web,e2e,public"]])
        for option in (("--spec", "23"), ("--package", "aaa-host")):
            self.assertNotEqual(self.cli("landing", "--dry-run", *option).returncode, 0)
        self.assertNotEqual(self.cli("landing", "--dry-run", AAA_E2E_ONLY="one").returncode, 0)

    def test_unknown_shared_inputs_and_rename_from_backend_keep_full_gate(self):
        for path in ("Cargo.lock", "web/package-lock.json", "web/vite.config.ts", "web/patches/x.patch",
                     "web/e2e/lib/harness.mjs", "scripts/ci-local.sh", "crates/aaa-proto/src/lib.rs"):
            with self.subTest(path=path):
                self.assertEqual(verify.scope_for(["web/src/a.ts", path]), "full")
        backend = self.root / "crates/aaa-host/a.rs"
        backend.parent.mkdir(parents=True)
        backend.write_text("code")
        self.commit()
        base = self.git("rev-parse", "HEAD").strip()
        backend.rename(self.root / "web/src/moved.ts")
        self.commit()
        _, paths = verify.changed_paths(base)
        self.assertIn("crates/aaa-host/a.rs", paths, "rename 원본 경계를 숨기지 않는다")
        self.assertEqual(verify.scope_for(paths), "full")
        self.assertEqual(verify.commands("landing", "full", None, []), [["bash", "scripts/ci-local.sh"]])

    def test_dirty_and_untracked_paths_are_counted_and_cannot_be_landed(self):
        (self.root / "web/src/a.ts").write_text("changed")
        (self.root / "new backend.rs").write_text("untracked")
        _, paths = verify.changed_paths(self.base)
        self.assertEqual(paths, ["new backend.rs", "web/src/a.ts"])
        self.assertEqual(verify.scope_for(paths), "full")
        self.assertNotEqual(self.cli("landing").returncode, 0)
        self.assertFalse((self.root / "calls").exists())
        self.assertEqual(verify.scope_for([]), "full", "변경 없음으로 범위를 축소하지 않는다")

    def test_development_requires_owner_and_keeps_partial_selection_explicit(self):
        self.assertEqual(verify.commands("develop", "web", "23,109", []),
                         [["bash", "scripts/ci-local.sh", "--only", "web,e2e,public", "--spec", "23,109"]])
        with self.assertRaises(ValueError):
            verify.commands("develop", "full", None, [])
        plan = verify.commands("develop", "full", None, ["aaa-host"])
        self.assertEqual(plan[0], ["cargo", "test", "--profile", "verify", "--locked", "-p", "aaa-host"])
        self.assertIn("--all-targets", plan[1])

    def test_child_failure_is_recorded_once_and_stops_without_retry(self):
        (self.root / "web/src/a.ts").write_text("new")
        result = self.cli("develop", "--spec", "23", CHECK_EXIT="7")
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assertEqual((self.root / "calls").read_text(), "called\n")
        records = list(self.root.glob("target/verification/*/result.json"))
        self.assertEqual(len(records), 1)
        data = json.loads(records[0].read_text())
        self.assertEqual((data["phase"], data["exit_code"]), ("develop", 7))
        self.assertEqual(data["commands"][0][-2:], ["--spec", "23"])


if __name__ == "__main__":
    unittest.main()
