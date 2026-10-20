"""공개 검사 CLI가 실제 Git 파일명·크기 경계를 지키는지 임시 저장소에서 본다."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
MIB = 1024 * 1024


class PublicCheckTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="aaa-public-")
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.env = dict(os.environ, CHECK_PUBLIC_SESSION_DIRS="", GIT_CONFIG_NOSYSTEM="1",
                        GIT_CONFIG_GLOBAL=os.devnull, GIT_AUTHOR_NAME="PublicCheck",
                        GIT_AUTHOR_EMAIL="noreply@github.com",
                        GIT_COMMITTER_NAME="PublicCheck", GIT_COMMITTER_EMAIL="noreply@github.com")
        self.git("init", "-q")
        (self.repo / "scripts").mkdir()
        shutil.copy2(ROOT / "scripts/check-public.sh", self.repo / "scripts/check-public.sh")
        self.commit("fixture")

    def git(self, *args):
        return subprocess.run(["git", "-c", "commit.gpgsign=false", *args], cwd=self.repo,
                              env=self.env, check=True, capture_output=True)

    def commit(self, message):
        self.git("add", "--all")
        self.git("commit", "-qm", message)

    def file(self, name, size=0):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("wb") as output:
            output.truncate(size)
        self.git("add", "--", name)

    def check(self, *args):
        return subprocess.run(["bash", "scripts/check-public.sh", *(args or ("HEAD",))],
                              cwd=self.repo, env=self.env, capture_output=True, text=True)

    def test_current_index_names_and_size_boundary(self):
        self.file("space dir/at boundary.bin", MIB)
        passed = self.check()
        self.assertEqual(passed.returncode, 0, passed.stdout + passed.stderr)
        # 공백·개행 경로와 마지막 파일도 각각 검사되어야 한다. 가짜 키는 빈 파일이다.
        forbidden = ["space dir/empty.key", "line\nbreak.pem", "z-last.env"]
        for name in forbidden:
            self.file(name)
        self.file("space dir/over boundary.bin", MIB + 1)
        blocked = self.check()
        self.assertNotEqual(blocked.returncode, 0, blocked.stdout + blocked.stderr)
        for name in forbidden:
            self.assertIn("금지 확장자: " + name, blocked.stdout)
        self.assertIn("1MiB 넘는 파일: space dir/over boundary.bin", blocked.stdout)

    def test_history_includes_deleted_forbidden_file(self):
        clean = self.check("--history", "HEAD")
        self.assertEqual(clean.returncode, 0, clean.stdout + clean.stderr)
        name = "old dir/line\nbreak.key"
        self.file(name)
        self.commit("old fixture")
        self.git("rm", "--", name)
        self.commit("remove fixture")
        current = self.check()
        self.assertEqual(current.returncode, 0, current.stdout + current.stderr)
        history = self.check("--history", "HEAD")
        self.assertNotEqual(history.returncode, 0, history.stdout + history.stderr)
        self.assertIn("금지 확장자: " + name, history.stdout)

    def test_size_exceptions_are_bounded_and_exact(self):
        # plan은 누적 문서에 2MiB, 버전 고정 글꼴은 현재 크기까지만 허용한다.
        limits = {
            "docs/plan.md": 2 * MIB,
            "web/public/fonts/meslotendard-0.3.1/Meslotendard-Bold.woff2": 2163908,
            "web/public/fonts/meslotendard-0.3.1/Meslotendard-Regular.woff2": 2137228,
        }
        for name, limit in limits.items():
            self.file(name, limit)
        passed = self.check()
        self.assertEqual(passed.returncode, 0, passed.stdout + passed.stderr)
        for name, limit in limits.items():
            self.file(name, limit + 1)
        others = ["docs/other.md", "web/public/fonts/meslotendard-0.3.1/Other.woff2"]
        for name in others:
            self.file(name, MIB + 1)
        blocked = self.check()
        self.assertNotEqual(blocked.returncode, 0, blocked.stdout + blocked.stderr)
        for name in [*limits, *others]:
            self.assertIn(name, blocked.stdout)


if __name__ == "__main__":
    unittest.main()
