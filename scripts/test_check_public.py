"""공개 검사 CLI가 실제 Git 파일명·크기 경계를 지키는지 임시 저장소에서 본다."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
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

    def test_size_uses_worktree_bytes_and_preserves_unusual_names(self):
        names = ["line\nbreak.bin", "tab\tname.bin", "-leading.bin", "한글.bin"]
        for name in names:
            self.file(name, MIB)
        passed = self.check()
        self.assertEqual(passed.returncode, 0, passed.stdout + passed.stderr)
        # index를 갱신하지 않은 실제 파일의 크기를 본다.
        for name in names:
            with (self.repo / name).open("ab") as output:
                output.write(b"x")
        blocked = self.check()
        self.assertNotEqual(blocked.returncode, 0, blocked.stdout + blocked.stderr)
        for name in names:
            self.assertIn("1MiB 넘는 파일: " + name, blocked.stdout)

    @unittest.skipUnless(os.name == "posix", "실제 symlink는 Unix에서 확인한다")
    def test_size_follows_symlinks_and_skips_removed_paths(self):
        target = self.repo / "untracked-target"
        target.write_bytes(b"x" * MIB)
        (self.repo / "linked.bin").symlink_to(target.name)
        self.git("add", "linked.bin")
        self.file("removed.bin", MIB + 1)
        (self.repo / "removed.bin").unlink()
        (self.repo / "dangling.bin").symlink_to("missing-target")
        self.git("add", "dangling.bin")
        passed = self.check()
        self.assertEqual(passed.returncode, 0, passed.stdout + passed.stderr)
        with target.open("ab") as output:
            output.write(b"x")
        blocked = self.check()
        self.assertNotEqual(blocked.returncode, 0, blocked.stdout + blocked.stderr)
        self.assertIn("1MiB 넘는 파일: linked.bin", blocked.stdout)

    @unittest.skipUnless(sys.platform.startswith("linux"), "macOS 파일시스템은 잘못된 UTF-8 파일명을 거부한다")
    def test_size_preserves_non_utf8_filename_bytes(self):
        raw_name = b"non-utf8-\xff.bin"
        raw_path = os.fsencode(self.repo) + b"/" + raw_name
        with open(raw_path, "wb") as output:
            output.truncate(MIB)
        self.git("add", os.fsdecode(raw_name))
        passed = self.check()
        self.assertEqual(passed.returncode, 0, passed.stdout + passed.stderr)
        with open(raw_path, "ab") as output:
            output.write(b"x")
        blocked = subprocess.run(["bash", "scripts/check-public.sh", "HEAD"], cwd=self.repo,
                                 env=self.env, capture_output=True)
        self.assertNotEqual(blocked.returncode, 0, blocked.stdout + blocked.stderr)
        self.assertIn(b"1MiB " + "넘는 파일: ".encode() + raw_name, blocked.stdout)

    @unittest.skipUnless(os.name == "posix", "실제 symlink 오류는 Unix에서 확인한다")
    def test_metadata_error_cannot_pass_as_an_empty_size_list(self):
        (self.repo / "loop.bin").symlink_to("loop.bin")
        self.git("add", "loop.bin")
        blocked = self.check()
        self.assertNotEqual(blocked.returncode, 0, blocked.stdout + blocked.stderr)
        self.assertIn("추적 파일 크기를 읽지 못했다", blocked.stdout)

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
