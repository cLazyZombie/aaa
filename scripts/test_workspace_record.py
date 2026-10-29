"""세 저장소의 실제 shell 종료와 Git 공용 원장의 계약을 검사한다."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class WorkspaceRecord(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="aaa-record-")
        self.addCleanup(temp.cleanup)
        self.repo = Path(temp.name)
        self.git("init", "-qb", "main")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")
        self.git("remote", "add", "origin", "https://example.com/team/game.git")
        (self.repo / "scripts").mkdir()
        shutil.copyfile(ROOT / "scripts/workspace_record.py", self.repo / "scripts/workspace_record.py")
        (self.repo / ".gitignore").write_text("target/\n")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], text=True).strip()

    def helper(self, *args):
        return subprocess.run(["python3", "scripts/workspace_record.py", *args], cwd=self.repo,
                              text=True, capture_output=True, timeout=10)

    def put(self, name, value):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value))
        return path

    def archives(self):
        return list((self.repo / ".git/aaa-workspace-runs").glob("run-*"))

    def test_finish_when_main_repeats_then_keeps_failures_and_only_fresh_compact_results(self):
        for runner in ("rc", "engine", "pt"):
            for code in ("0", "7"):
                start = self.helper("start", runner, "coverage", "full")
                self.assertEqual(start.returncode, 0, start.stderr)
                token = start.stdout.strip()
                if runner == "rc":
                    self.put("target/verification/summary.json", dict(gates=[dict(name="runtime", status="passed", seconds=2)], private="secret"))
                if runner == "engine":
                    path = self.repo / "target/verification/gates/runtime.status"
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text("runtime\truntime tests\t0\t2\n")
                if runner == "pt":
                    (self.repo / token).with_suffix(".stages").write_text("build|ok|0|0|2\n")
                report = [dict(target="/private/build/game", exit=0, stdout="secret", tests=[dict(name="unit", event="ok", exec_time=.25, stdout="secret")], suites=[dict(passed=1, failed=0, ignored=0)])]
                self.put("target/verification/workspace-tests/report.json", report)
                self.put("target/verification/pt-tests/report.json", report)
                stale = self.put("target/verification/default-tests/report.json", report)
                os.utime(stale, ns=(1, 1))
                for name in ("rc-e2e/report.json", "no2-e2e/all-headless.json", "pt-e2e/report.json"):
                    self.put("target/" + name, dict(status="passed", timing_unit="scenario-wall", error="secret", reports=[dict(scenario="smoke", duration_ms=3, status="passed", steps=[dict(error="secret")])]))
                result = self.helper("finish", token, code)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertFalse((self.repo / token).exists())
        self.assertEqual(len(self.archives()), 6)
        for path in self.archives():
            text = "\n".join(p.read_text() for p in path.rglob("*") if p.is_file())
            self.assertNotIn("secret", text)
            self.assertNotIn("/private", text)
            self.assertNotIn("https://", text)
            self.assertFalse((path / "verification/default-tests/report.json").exists())
            source = json.loads((path / "provenance.json").read_text())
            self.assertEqual(source["mode"], "coverage")
            self.assertEqual(source["scope"], "full")
            self.assertEqual(source["commit"], self.git("rev-parse", "HEAD"))
            self.assertIn("|2", (path / "workspace-summary.tsv").read_text())
            if source["runner"] == "pt":
                tests = json.loads((path / "verification/pt-tests/report.json").read_text())
                self.assertEqual(tests[0]["tests"][0]["exec_time"], .25)
                e2e = json.loads((path / "e2e.json").read_text())
                self.assertEqual(e2e["timing_unit"], "scenario-wall")
                self.assertEqual(e2e["reports"][0]["duration_ms"], 3)

    def test_finish_when_source_changes_then_rejects_without_changing_original_failure(self):
        token = self.helper("start", "rc", "default", "desktop").stdout.strip()
        (self.repo / "scripts/workspace_record.py").write_text("# changed\n")
        # helper 자체가 바뀌어도 원래 구현으로 출처를 검증한다.
        result = subprocess.run(["python3", str(ROOT / "scripts/workspace_record.py"), "finish", token, "0"],
                                cwd=self.repo, text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("source changed", result.stderr)
        self.assertFalse(self.archives())
        result = subprocess.run(["bash", "-c", 'trap \'code=$?; python3 /missing-helper finish token "$code" || :; exit "$code"\' EXIT; exit 7'],
                                cwd=self.repo, capture_output=True)
        self.assertEqual(result.returncode, 7)


if __name__ == "__main__":
    unittest.main()
