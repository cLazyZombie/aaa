"""분할의 완전성과 실제 자식의 병렬 실행·실패 보존을 검사한다."""
from pathlib import Path
import json
import os
import socket
import sys
import tempfile
import threading
import unittest
import subprocess

from scripts.rust_tests import Job, execute, partitions


class RustTests(unittest.TestCase):
    def test_every_test_is_selected_exactly_once_including_rollout(self):
        names = [f"module::case_{n}" for n in range(99)] + ["rollout::restart"]
        for count in (1, 2, 3, 8):
            groups = partitions(names, count)
            self.assertEqual(sorted(name for group in groups for name in group), sorted(names))
            self.assertEqual(groups[0], ["rollout::restart"])
            self.assertTrue(all(groups))
        with self.assertRaises(ValueError):
            partitions(["same", "same"], 2)

    def test_parallel_jobs_finish_without_dropping_a_failed_or_following_job(self):
        with tempfile.TemporaryDirectory(prefix="aaa-rust-runner-") as tmp, socket.socket() as listener:
            root = Path(tmp)
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(10)
            port = listener.getsockname()[1]
            fixture = root / "child.py"
            fixture.write_text("""import socket, sys
with socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=10) as link:
    link.sendall(b'ready')
    assert link.recv(1) == b'!'
print('running 1 test')
print('test owner ... ' + ('ok' if sys.argv[2] == '0' else 'FAILED'))
raise SystemExit(int(sys.argv[2]))
""")
            seen = []
            errors = []

            def release():
                try:
                    # 앞 두 실제 자식이 모두 도달해야 연다. 직렬 runner는 이 사건을 만족하지 못한다.
                    first, _ = listener.accept()
                    second, _ = listener.accept()
                    for child in (first, second):
                        with child:
                            seen.append(child.recv(5))
                            child.sendall(b"!")
                    third, _ = listener.accept()
                    with third:
                        seen.append(third.recv(5))
                        third.sendall(b"!")
                except Exception as error:
                    errors.append(error)

            gate = threading.Thread(target=release)
            gate.start()
            jobs = [Job(f"job-{n}", [sys.executable, str(fixture), str(port), str(code)], root, ["owner"])
                    for n, code in enumerate((0, 7, 0))]
            results = execute(jobs, root, 2, 1)
            gate.join()
            self.assertEqual(errors, [])
            self.assertEqual(seen, [b"ready"] * 3)
            self.assertEqual(sorted(result["exit"] for result in results), [0, 0, 7])

    def test_missing_selected_tests_is_a_failure_even_when_child_exits_zero(self):
        with tempfile.TemporaryDirectory(prefix="aaa-rust-count-") as tmp:
            root = Path(tmp)
            job = Job("empty", [sys.executable, "-c", "print('running 0 tests')"], root, ["owner"])
            self.assertEqual(execute([job], root, 1, 1)[0]["exit"], 1)

    def test_real_runner_keeps_all_artifacts_and_doctests_after_a_binary_failure(self):
        with tempfile.TemporaryDirectory(prefix="aaa-rust-pipeline-") as tmp:
            root = Path(tmp)
            calls = root / "calls"
            artifacts = []
            for target, names in (("m13", ["rollout::owner", "module::one", "module::two"]),
                                  ("other", ["failed_owner"])):
                package = root / target
                package.mkdir()
                executable = package / "tests"
                executable.write_text(f"#!{sys.executable}\n" + f"names = {names!r}\n" + """import json, sys
from pathlib import Path
if '--list' in sys.argv:
    print('\\n'.join(name + ': test' for name in names))
    raise SystemExit(0)
selected = [name for name in names if '--exact' not in sys.argv or name in sys.argv]
with Path('../calls').open('a') as output:
    for name in selected: output.write(name + '\\n')
print(f'running {len(selected)} tests')
raise SystemExit(7 if 'failed_owner' in selected else 0)
""")
                executable.chmod(0o755)
                artifacts.append(dict(reason="compiler-artifact", profile=dict(test=True), executable=str(executable),
                                      manifest_path=str(package / "Cargo.toml"), target=dict(name=target, kind=["test"])))
            cargo = root / "cargo"
            cargo.write_text(f"#!{sys.executable}\n" + f"artifacts = {artifacts!r}\n" + """import json, sys
from pathlib import Path
if '--no-run' in sys.argv:
    for artifact in artifacts: print(json.dumps(artifact))
elif '--doc' in sys.argv:
    with Path('calls').open('a') as output: output.write('doctests\\n')
else: raise SystemExit('알 수 없는 Cargo 호출')
""")
            cargo.chmod(0o755)
            runner = Path(__file__).with_name("rust_tests.py").resolve()
            result = subprocess.run([sys.executable, str(runner), "--logs", str(root / "logs"), "--workers", "2"],
                                    cwd=root, env={**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"]},
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertCountEqual(calls.read_text().splitlines(),
                                  ["rollout::owner", "module::one", "module::two", "failed_owner", "doctests"])
            summary = json.loads((root / "logs/summary.json").read_text())
            self.assertEqual(sum(job["exit"] != 0 for job in summary["jobs"]), 1)


if __name__ == "__main__":
    unittest.main()
