"""실제 Git·CI 종료 보관의 출처·개수·worktree 수명을 검사한다."""

import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from scripts import workspace_metrics as metrics

ROOT = Path(__file__).resolve().parent.parent


class WorkspaceMetrics(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="aaa-workspace-ci-")
        self.addCleanup(temp.cleanup)
        self.repo = Path(temp.name) / "repo"
        self.repo.mkdir()
        self.logs = Path(temp.name) / "outside-logs"
        self.logs.mkdir()
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")
        self.git("remote", "add", "origin", "https://example.com/team/app.git")
        (self.repo / "source").write_text("source\n")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        scripts = self.repo / "scripts"
        scripts.mkdir()
        for name in ("ci-local.sh", "workspace_metrics.py"):
            shutil.copyfile(ROOT / "scripts" / name, scripts / name)
        (scripts / "check-public.sh").write_text("exit 0\n")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], text=True).strip()

    def helper(self, action):
        return subprocess.run(["python3", "scripts/workspace_metrics.py", action, str(self.logs)],
                              cwd=self.repo, capture_output=True, text=True, timeout=10)

    def archives(self):
        return list((self.repo / ".git/aaa-workspace-runs").glob("run-*"))

    def test_ci_local_archives_success_failure_and_external_logs_without_source_paths(self):
        for code in [0, 7]:
            (self.repo / "scripts/check-public.sh").write_text(f"exit {code}\n")
            result = subprocess.run(["bash", "scripts/ci-local.sh", "--only", "public", "--logs", str(self.logs)],
                                    cwd=self.repo, capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0 if code == 0 else 1, result.stderr)
        archives = self.archives()
        self.assertEqual(len(archives), 2)
        outcomes = set()
        for archive in archives:
            data = json.loads((archive / "provenance.json").read_text())
            self.assertEqual(data["commit"], self.git("rev-parse", "HEAD"))
            self.assertEqual(data["remote"], metrics.remote_id("git@example.com:team/app.git"))
            self.assertLessEqual(data["started_at_ms"], data["ended_at_ms"])
            outcomes.add((archive / "workspace-summary.tsv").read_text().splitlines()[0].split("|")[-1])
            self.assertNotIn(str(self.repo), (archive / "provenance.json").read_text())
            self.assertNotIn(str(self.logs), (archive / "workspace-summary.tsv").read_text())
        self.assertEqual(outcomes, {"succeeded", "failed"})
        shutil.rmtree(self.logs)
        self.assertTrue(all((p / "workspace-summary.tsv").is_file() for p in archives))

    def test_dirty_or_changed_source_is_rejected_without_changing_ci_exit(self):
        for mode in ["dirty", "changed"]:
            (self.repo / "source").write_text("source\n")
            self.assertEqual(self.helper("start").returncode, 0)
            (self.repo / "source").write_text(mode + "\n")
            if mode == "changed":
                self.git("add", "source")
                self.git("commit", "-qm", "changed")
            (self.logs / "workspace-summary.tsv").write_text("v1|100|1|succeeded\n")
            result = self.helper("finish")
            self.assertEqual(result.returncode, 1)
            self.assertIn("출처 미확인", result.stderr)
            self.assertFalse(self.archives())
        (self.repo / "source").write_text("dirty again\n")
        result = subprocess.run(["bash", "scripts/ci-local.sh", "--only", "public", "--logs", str(self.logs)],
                                cwd=self.repo, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0)
        self.assertIn("출처 미확인", result.stderr)
        self.assertFalse(self.archives())

    def test_compact_counts_preserve_missing_logs_and_exclude_terminal_output(self):
        self.assertEqual(self.helper("start").returncode, 0)
        (self.logs / "workspace-summary.tsv").write_text("v1|100|164|failed\nbuild|ok|0|1|12|unused\n")
        (self.logs / "rust").mkdir()
        (self.logs / "rust/summary.json").write_text(json.dumps(dict(jobs=[dict(log="unit.log"), dict(log="missing.log")])))
        (self.logs / "rust/unit.log").write_text("private stdout\ntest result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 2 filtered out; finished in 1s\n")
        (self.logs / "e2e.log").write_text("private stdout\n## 요약: 파일 1개 × 1번, worker 1개, 모두 1.0초\n## 작업: 1개\n##   01-case 통과 1/1 1.0초\n")
        result = self.helper("finish")
        self.assertEqual(result.returncode, 0, result.stderr)
        archive, = self.archives()
        self.assertNotIn("private", (archive / "rust/unit.log").read_text())
        self.assertNotIn("private", (archive / "e2e.log").read_text())
        self.assertEqual(len(json.loads((archive / "rust/summary.json").read_text())["jobs"]), 2)
        self.assertFalse((archive / "rust/missing.log").exists())

    def test_split_owner_results_are_all_archived(self):
        self.assertEqual(self.helper("start").returncode, 0)
        (self.logs / "workspace-summary.tsv").write_text("v1|100|164|succeeded\n")
        (self.logs / "rust").mkdir()
        jobs = [dict(log=f"{n}-owner.log") for n in range(350)]
        (self.logs / "rust/summary.json").write_text(json.dumps(dict(jobs=jobs)))
        for job in jobs:
            (self.logs / "rust" / job["log"]).write_text(
                "private stdout\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n")
        result = self.helper("finish")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "")
        archive, = self.archives()
        self.assertEqual(json.loads((archive / "rust/summary.json").read_text())["jobs"], jobs)
        self.assertEqual(len(list((archive / "rust").glob("*-owner.log"))), len(jobs))
        for job in jobs:
            self.assertNotIn("private", (archive / "rust" / job["log"]).read_text())


if __name__ == "__main__":
    unittest.main()
