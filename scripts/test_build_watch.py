"""실제 Cargo 캐시와 Git 판·소스 목록 갱신을 함께 지킨다. workspace는 빌드하지 않는다."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent


class BuildWatch(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="aaa-build-watch-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.common_built = False
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.git("init", "-qb", "main")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")
        package = self.repo / "crates/probe"
        (package / "src").mkdir(parents=True)
        common = self.repo / "crates/common"
        (common / "src").mkdir(parents=True)
        (common / "Cargo.toml").write_text(
            '[package]\nname="aaa-watch-common"\nversion="0.0.0"\nedition="2021"\n')
        (common / "src/lib.rs").write_text('pub fn value() -> u8 { 1 }\n')
        (package / "Cargo.toml").write_text(
            '[package]\nname="aaa-build-watch-probe"\nversion="0.0.0"\nedition="2021"\n'
            '[dependencies]\naaa-watch-common={path="../common"}\n[workspace]\n')
        (package / "src/lib.rs").write_text(
            'include!(concat!(env!("OUT_DIR"), "/panic_build.rs"));\n'
            'pub fn source_count() -> usize { SOURCE_FILES.len() + usize::from(aaa_watch_common::value()) }\n')
        shutil.copyfile(ROOT / "crates/aaa/build.rs", package / "build.rs")
        (self.repo / ".gitignore").write_text("target/\n")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args], text=True,
                                       stderr=subprocess.PIPE).strip()

    def check(self, fresh):
        package = self.repo / "crates/probe"
        result = subprocess.run(["cargo", "check", "--offline", "--message-format=json"],
                                cwd=package, text=True, capture_output=True,
                                env={**os.environ, "CARGO_TARGET_DIR": str(package / "target")})
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        artifacts = [message for message in map(json.loads, result.stdout.splitlines())
                     if message["reason"] == "compiler-artifact" and message["target"]["kind"] == ["lib"]]
        self.assertEqual(len(artifacts), 2)
        by_name = {a["target"]["name"]: a for a in artifacts}
        self.assertEqual(by_name["aaa_build_watch_probe"]["fresh"], fresh, result.stderr)
        # Git 판·index가 바뀌어도 공통 backend 코드의 캐시는 보존되어야 한다.
        self.assertEqual(by_name["aaa_watch_common"]["fresh"], self.common_built, result.stderr)
        self.common_built = True
        generated, = (package / "target/debug/build").glob("*/out/panic_build.rs")
        source = generated.read_text()
        self.assertIn(self.git("rev-parse", "HEAD"), source)
        return source

    def test_fresh_checkout_reuses_build_but_tracks_index_and_commit(self):
        self.assertFalse((self.repo / ".git/packed-refs").exists())
        self.check(False)
        self.check(True)
        (self.repo / "new.rs").write_text("// newly tracked source\n")
        self.git("add", "new.rs")
        self.assertIn('"new.rs"', self.check(False))
        self.check(True)
        self.git("commit", "-qm", "next source")
        self.check(False)
        self.check(True)

    def test_packed_linked_worktree_tracks_new_loose_ref_and_detached_head(self):
        linked = self.root / "linked"
        self.git("worktree", "add", "-qb", "nested/watch", str(linked))
        self.git("pack-refs", "--all")
        self.repo = linked
        self.check(False)
        self.check(True)
        # HEAD 자체는 그대로인 packed branch에서 새 loose ref가 생긴다.
        self.git("commit", "--allow-empty", "-qm", "new loose ref")
        self.check(False)
        self.check(True)
        self.git("checkout", "--detach", "HEAD~1")
        self.check(False)
        self.check(True)


if __name__ == "__main__":
    unittest.main()
