"""실제 CI 셸의 병렬 진입·실시간 로그·실패 전파·선행 조건을 사건으로 검사한다."""

import os
import json
import queue
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent

TOOL = r'''
import os, pathlib, socket, sys
root = pathlib.Path(os.environ["FIXTURE_ROOT"])
tool = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
if tool == "python3":
    if args[:2] == ["-m", "unittest"]:
        if not os.environ.get("PYTHON_STAGE"):
            sys.exit(0)
        stage = "python"
    elif args[0] != "scripts/rust_tests.py":
        os.execv(sys.executable, [sys.executable, *args])
    else:
        stage = "test"
elif tool == "cargo":
    stage = args[0]
    if stage == "build":
        binary = root / "target/e2e/aaa"
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_text("fixture binary")
    if stage != "clippy":
        sys.exit(9 if os.environ.get("FAIL_STAGE") == stage else 0)
elif tool == "npm":
    if args[:2] == ["root", "-g"]:
        print(root)
    sys.exit(0)
elif tool == "node":
    if args[0] == "-e":
        sys.exit(0)
    assert pathlib.Path(os.environ["AAA_BIN"]).read_text() == "fixture binary"
    stage = "e2e"
else:
    stage = "public"
    for expected in os.environ.get("EXPECT_STAGES", "").split():
        assert (root / (expected + ".finished")).exists(), expected
    print("public after all stages", flush=True)
    sys.exit(0)
print(f"LIVE-{stage}-한글", end="", flush=True)
print(f"stderr-{stage}", file=sys.stderr, flush=True)
if os.environ.get("GATE"):
    with socket.create_connection(("127.0.0.1", int(os.environ["GATE"])), timeout=10) as link:
        link.sendall(stage.encode())
        assert link.recv(1) == b"!"
print("x" * 20000)
print(f"TAIL-{stage}", end="", flush=True)
(root / (stage + ".finished")).touch()
sys.exit(7 if os.environ.get("FAIL_STAGE") == stage else 0)
'''


@unittest.skipUnless(os.name == "posix", "Unix shebang 도구와 Unix CI 단계 분기를 검사한다")
class CiLocal(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="aaa-ci-contract-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "scripts").mkdir()
        (self.root / "tools").mkdir()
        (self.root / "web/dist").mkdir(parents=True)
        # e2e의 기존 번들 최신성 검사도 그대로 실행한다.
        (self.root / "web/dist/index.html").touch()
        for name in ("ci-local.sh", "ci_stream.py", "workspace_metrics.py", "check_fixture_secrets.py"):
            shutil.copyfile(ROOT / "scripts" / name, self.root / "scripts" / name)
        for name in ("cargo", "npm", "node", "python3", "public"):
            self.write_tool(self.root / "tools" / name)
        (self.root / "scripts/fixtures").mkdir()
        (self.root / "scripts/fixtures/secret-exceptions.json").write_text("[]")
        (self.root / "scripts/check-public.sh").write_text('exec "$FIXTURE_ROOT/tools/public"\n')
        self.env = dict(os.environ, FIXTURE_ROOT=str(self.root),
                        PATH=str(self.root / "tools") + os.pathsep + os.environ["PATH"])
        self.env.pop("GITHUB_ACTIONS", None)

    def write_tool(self, path):
        path.write_text(f"#!{sys.executable}\n" + TOOL)
        path.chmod(0o755)

    def run_ci(self, *args, gate=False, stages=("test", "e2e", "clippy"), live_output=True):
        command = ["bash", "scripts/ci-local.sh", "--logs", "logs", *args]
        if not gate:
            return subprocess.run(command, cwd=self.root, env=self.env,
                                  capture_output=True, text=True, timeout=20)
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(10)
            self.env["GATE"] = str(listener.getsockname()[1])
            process = subprocess.Popen(command, cwd=self.root, env=self.env,
                                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            lines = []
            events = queue.Queue()

            def read():
                for line in process.stdout:
                    lines.append(line)
                    events.put(line)

            reader = threading.Thread(target=read)
            reader.start()
            peers = []
            try:
                names = []
                for _ in stages:
                    peer, _ = listener.accept()
                    peers.append(peer)
                    peer.settimeout(10)
                    names.append(peer.recv(100).decode())
                self.assertCountEqual(names, stages)
                # 자식이 종료를 허락받기 전에 화면에 출력이 도착해야 한다.
                live = set()
                while live_output and len(live) < len(stages):
                    line = events.get(timeout=10)
                    for stage in names:
                        if f"[{stage}] LIVE-{stage}-한글" in line:
                            live.add(stage)
                for peer in peers:
                    peer.sendall(b"!")
            finally:
                for peer in peers:
                    peer.close()
                listener.close()
                process.wait(timeout=20)
                reader.join(timeout=10)
                process.stdout.close()
            return subprocess.CompletedProcess(command, process.returncode, "".join(lines), "")

    def assert_python_and_rust(self, failure=None):
        self.env.update(PYTHON_STAGE="1", EXPECT_STAGES="python test")
        if failure:
            self.env["FAIL_STAGE"] = failure
        result = self.run_ci("--only", "test,public", "--tee", gate=True,
                             stages=("python", "test"), live_output=False)
        self.assertEqual(result.returncode, 1 if failure else 0, result.stdout)
        summary = (self.root / "logs/workspace-summary.tsv").read_text()
        self.assertIn("test|FAIL|" if failure else "test|ok|", summary)
        self.assertIn("public|ok|", summary)
        log = (self.root / "logs/test.log").read_text()
        for stage in ("python", "test"):
            self.assertEqual(log.count(f"LIVE-{stage}-한글"), 1)
            self.assertEqual(log.count(f"TAIL-{stage}"), 1)
        self.assertIn("TAIL-python", (self.root / "logs/python.log").read_text())
        python_code = 7 if failure == "python" else 0
        for line in (self.root / "logs/python.log").read_text().splitlines():
            self.assertIn(f"[python exit={python_code}] {line}\n", log)
        self.assertNotIn("[python exit=", (self.root / "logs/python.log").read_text())
        if failure:
            self.assertIn(f"FAIL {failure}", log)

    def test_auth_fixture_guard_fails_before_python_and_rust_execution(self):
        source = "let {} = {};".format("secret", json.dumps("automation"))
        (self.root / "scripts/unsafe.rs").write_text(source, encoding="utf-8")
        result = self.run_ci("--only", "test")
        self.assertEqual(result.returncode, 1, result.stdout)
        log = (self.root / "logs/test.log").read_text()
        self.assertIn("scripts/unsafe.rs:1:", log)
        self.assertNotIn("automation", log)
        for stage in ("python", "test"):
            self.assertFalse((self.root / f"{stage}.finished").exists())

    def test_python_and_rust_start_together(self):
        self.assert_python_and_rust()

    def test_python_failure_waits_for_rust_and_fails_the_stage(self):
        self.assert_python_and_rust("python")

    def test_rust_failure_waits_for_python_and_fails_the_stage(self):
        self.assert_python_and_rust("test")

    def test_tee_runs_all_three_together_and_keeps_failure_and_final_bytes(self):
        self.env.update(FAIL_STAGE="clippy", EXPECT_STAGES="test e2e clippy")
        # 같은 --logs 폴더를 다시 써도 옛 완료 표지로 일찍 끝나면 안 된다.
        (self.root / "logs").mkdir()
        (self.root / "logs/.stream-done").touch()
        for stage in ("test", "e2e", "clippy"):
            (self.root / f"logs/{stage}.log").write_text("STALE\n")
        result = self.run_ci("--only", "test,e2e,clippy,public", "--tee", gate=True)
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertNotIn("STALE", result.stdout)
        self.assertIn("public after all stages", result.stdout)
        for stage in ("test", "e2e", "clippy"):
            log = (self.root / f"logs/{stage}.log").read_text()
            self.assertEqual(log, f"LIVE-{stage}-한글stderr-{stage}\n" + "x" * 20000 + f"\nTAIL-{stage}")
            self.assertIn(f"[{stage}] TAIL-{stage}", result.stdout)
        summary = (self.root / "logs/workspace-summary.tsv").read_text()
        self.assertIn("clippy|FAIL|", summary)
        for stage in ("test", "e2e", "public"):
            self.assertIn(f"{stage}|ok|", summary)

    def test_single_tee_stage_keeps_its_status(self):
        self.env["FAIL_STAGE"] = "clippy"
        result = self.run_ci("--only", "clippy", "--tee")
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("TAIL-clippy", result.stdout)
        self.assertNotIn("── 함께:", result.stdout)

    def test_unchanged_dist_uses_successful_web_build_time(self):
        source = self.root / "web/src/main.ts"
        source.parent.mkdir()
        source.write_text("same emitted bytes")
        stamp = self.root / "web/node_modules/.cache/aaa-web-build.stamp"
        stamp.parent.mkdir(parents=True)
        stamp.touch()
        os.utime(self.root / "web/dist/index.html", (1, 1))
        os.utime(source.parent, (1, 1))
        os.utime(source, (2, 2))
        os.utime(stamp, (3, 3))
        result = self.run_ci("--only", "e2e")
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertTrue((self.root / "e2e.finished").exists())

    def test_newer_build_script_still_rejects_stale_web(self):
        source = self.root / "web/scripts/build.mjs"
        source.parent.mkdir()
        source.write_text("changed build script")
        stamp = self.root / "web/node_modules/.cache/aaa-web-build.stamp"
        stamp.parent.mkdir(parents=True)
        stamp.touch()
        os.utime(source.parent, (1, 1))
        os.utime(source, (3, 3))
        os.utime(stamp, (2, 2))
        result = self.run_ci("--only", "e2e")
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("web/scripts/build.mjs", (self.root / "logs/e2e.log").read_text())
        self.assertFalse((self.root / "e2e.finished").exists())

    def test_build_failure_skips_dependents_but_runs_public(self):
        for failing in ("build", "test"):
            with self.subTest(failing=failing):
                self.env["FAIL_STAGE"] = failing
                result = self.run_ci("--tee")
                self.assertEqual(result.returncode, 1, result.stdout)
                summary = (self.root / "logs/workspace-summary.tsv").read_text()
                for stage in ("test", "e2e", "clippy"):
                    self.assertIn(f"{stage}|건너뜀:", summary)
                    self.assertFalse((self.root / f"{stage}.finished").exists())
                self.assertIn("public|ok|", summary)

    def test_without_tee_keeps_logs_and_summary(self):
        self.env["EXPECT_STAGES"] = "test e2e clippy"
        result = self.run_ci("--only", "test,e2e,clippy,public")
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertNotIn("LIVE-", result.stdout)
        for stage in ("test", "e2e", "clippy"):
            self.assertIn(f"TAIL-{stage}", (self.root / f"logs/{stage}.log").read_text())
        self.assertIn("public after all stages", (self.root / "logs/public.log").read_text())

    def test_reader_failure_does_not_stop_checks_and_is_reported(self):
        (self.root / "scripts/ci_stream.py").write_text("raise SystemExit(8)\n")
        self.env["EXPECT_STAGES"] = "test e2e clippy"
        result = self.run_ci("--only", "test,e2e,clippy,public", "--tee")
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("실패: 출력", result.stdout)
        summary = (self.root / "logs/workspace-summary.tsv").read_text()
        for stage in ("test", "e2e", "clippy", "public"):
            self.assertIn(f"{stage}|ok|", summary)


class CiStream(unittest.TestCase):
    def test_reader_preserves_unicode_across_chunks_and_drains_after_done(self):
        temporary = tempfile.TemporaryDirectory(prefix="aaa-ci-stream-")
        self.addCleanup(temporary.cleanup)
        logs = Path(temporary.name) / "logs"
        logs.mkdir()
        (logs / "test.log").write_text("x" * 8191 + "한글\n" + "y" * 20000 + "끝")
        done = logs / "done"
        done.touch()
        result = subprocess.run([sys.executable, str(ROOT / "scripts/ci_stream.py"),
                                 str(logs), str(done), "test"],
                                env=dict(os.environ, PYTHONIOENCODING="ascii"),
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("[test] 한글\n", result.stdout)
        self.assertIn("끝", result.stdout)
        self.assertEqual(result.stdout.count("x"), 8191)
        self.assertEqual(result.stdout.count("y"), 20000)
        self.assertNotIn("\ufffd", result.stdout)


if __name__ == "__main__":
    unittest.main()
