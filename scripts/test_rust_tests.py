"""분할의 완전성과 실제 자식의 병렬 실행·실패 보존을 검사한다."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import json
import os
import queue
import signal
import socket
import sys
import tempfile
import threading
import time
import unittest
import subprocess
from unittest.mock import patch

from scripts.rust_tests import Job, RunRecord, execute, partitions, plan, process_tree


class RustTests(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "Unix 전체 gate의 브라우저와 SSH 실험실 잠금을 검사한다")
    def test_ssh_owner_waits_for_browser_while_other_jobs_run_and_keeps_its_exit(self):
        import fcntl

        waiting = threading.Event()
        other_done = threading.Event()

        class ObservedRecord(RunRecord):
            def run(self, key, *args, **kwargs):
                if key.endswith("-resource"):
                    waiting.set()
                code = super().run(key, *args, **kwargs)
                if key.endswith("-other"):
                    other_done.set()
                return code

        with tempfile.TemporaryDirectory() as tmp, ThreadPoolExecutor(max_workers=1) as pool:
            root = Path(tmp)
            lock = root / "ssh-browser.lock"
            owner = """import fcntl, os
from pathlib import Path
assert Path("browser-done").exists()
with open(os.environ["AAA_CI_SSH_BROWSER_LOCK"], "rb") as guard:
    try:
        fcntl.flock(guard, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        pass
    else:
        raise AssertionError("SSH 시험이 실행 중인데 브라우저가 들어갈 수 있다")
print("running 1 test")
raise SystemExit(7)
"""
            jobs = [Job("aaa_host-2", [sys.executable, "-c", owner], root, ["machine_setup::tests::launcher"]),
                    Job("other", [sys.executable, "-c", "print('other done')"], root)]
            with lock.open("a+b") as browser, patch.dict(os.environ, AAA_CI_SSH_BROWSER_LOCK=str(lock)):
                fcntl.flock(browser, fcntl.LOCK_EX)
                result = pool.submit(execute, jobs, root, 2, 1, 2, record=ObservedRecord(root))
                try:
                    self.assertTrue(waiting.wait(10))
                    self.assertTrue(other_done.wait(10))
                    self.assertFalse(result.done())
                    (root / "browser-done").touch()
                finally:
                    fcntl.flock(browser, fcntl.LOCK_UN)
                self.assertEqual({j["name"]: j["exit"] for j in result.result(timeout=10)},
                                 {"other": 0, "aaa_host-2": 7})

    @unittest.skipUnless(os.name == "posix", "실제 Unix SIGINT/TERM과 shebang runner를 검사한다")
    def interrupt_runner(self, phase, signum, ignore_term=False, snapshot_error=False):
        with tempfile.TemporaryDirectory(prefix="aaa-rust-interrupt-") as tmp, socket.socket() as listener:
            root = Path(tmp)
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(3)
            epoch = time.monotonic()
            connection_log = root / "connection.log"
            common = f"""import json, os, signal, socket, sys, time
from pathlib import Path
def note(message):
    with Path({str(connection_log)!r}).open("a") as log:
        print(f'{{time.monotonic() - {epoch!r}:012.6f}} child pid={{os.getpid()}} {{message}}', file=log)

def gate(phase):
    if phase != {phase!r}: return
    # TERM을 받은 도구가 정상 종료해도 바깥 runner는 중단을 실패로 보존해야 한다.
    signal.signal(signal.SIGTERM, signal.SIG_IGN if {ignore_term!r} else lambda *_: sys.exit(0))
    Path({str(root / 'snapshot-pid')!r}).write_text(str(os.getpid()))
    print('partial-' + phase, flush=True)
    note(f'{{phase}} 접속 시도')
    try:
        link = socket.create_connection(('127.0.0.1', {listener.getsockname()[1]}), timeout=3)
    except OSError as error:
        note(f'{{phase}} 접속 실패: {{error!r}}')
        raise
    with link:
        note(f'{{phase}} 접속 성공')
        link.sendall(str(os.getpid()).encode())
        link.settimeout(None)
        link.recv(1)
"""
            artifacts = []
            for name in (["m13"] if phase == "list" else ["first", "queued"]):
                binary = root / name
                binary.write_text(f"#!{sys.executable}\n" + common + """
if '--list' in sys.argv:
    gate('list')
    print('owner: test')
else:
    gate('execute')
    print('running 1 test')
    print('test owner ... FAILED')
    sys.exit(7)
""")
                binary.chmod(0o755)
                artifacts.append(dict(reason="compiler-artifact", profile=dict(test=True), executable=str(binary),
                                      manifest_path=str(root / "Cargo.toml"), target=dict(name=name, kind=["test"])))
            cargo = root / "cargo"
            cargo.write_text(f"#!{sys.executable}\n" + common + f"artifacts = {artifacts!r}\n" + """
if '--no-run' in sys.argv:
    gate('prepare')
    for artifact in artifacts: print(json.dumps(artifact))
elif '--doc' in sys.argv:
    gate('doctests')
else: sys.exit(9)
""")
            cargo.chmod(0o755)
            # 실제 신호·종료·회수의 3초 계약에 머신 전체 ps의 부하를 섞지 않는다.
            # 조회 결과/실패의 해석은 별도 process_tree owner가 검사한다.
            ps = root / "ps"
            ps.write_text(f"#!{sys.executable}\nfrom pathlib import Path\nimport sys\n"
                          f"pid = Path({str(root / 'snapshot-pid')!r}).read_text()\n"
                          f"sys.exit(7) if {snapshot_error!r} else None\n"
                          "print(f'{pid} 1 S - fixture-child')\n")
            ps.chmod(0o755)
            logs = root / "logs"
            runner = Path(__file__).with_name("rust_tests.py").resolve()
            # runner의 자식이 아닌 프로세스를 함께 두고 생존을 확인한다.
            outsider = subprocess.Popen([sys.executable, "-c", "import sys; sys.stdin.buffer.read()"],
                                        stdin=subprocess.PIPE)
            timeline = []

            def note(message):
                timeline.append(f"{time.monotonic() - epoch:012.6f} {message}")

            note("시험: runner 실행 요청")
            process = subprocess.Popen([sys.executable, str(runner), "--logs", str(logs), "--workers", "1"],
                                       cwd=root, env={**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"],
                                                      "RUST_TEST_THREADS": "1"},
                                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            ready_job = {"prepare": "prepare", "list": "list-m13", "execute": "00-first",
                         "doctests": "02-doctests"}[phase]
            lines = queue.Queue()
            def read():
                for line in process.stdout:
                    if line.strip():
                        note(f"runner: {line.rstrip()}")
                    lines.put(line)
                note(f"runner: stdout EOF (exit={process.poll()})")
                lines.put(None)
            reader = threading.Thread(target=read)
            reader.start()
            waiting_for = f"{ready_job} START"
            try:
                # 앞 단계의 START·완료 출력을 먼저 읽는다. 접속 시한은 대상 자식이
                # 시작되었다는 기록이 나온 뒤에만 적용한다.
                while True:
                    line = lines.get(timeout=3)
                    if line is None:
                        self.fail(f"{waiting_for} 전에 runner가 끝났다 (exit={process.wait(timeout=3)})")
                    if line.startswith(f"START {ready_job} pid="):
                        started_pid = int(line.split()[2].removeprefix("pid="))
                        break
                waiting_for = f"{ready_job} 준비 접속"
                with listener.accept()[0] as peer:
                    note(f"시험: {ready_job} 접속 수락")
                    peer.settimeout(3)
                    waiting_for = f"{ready_job} 준비 PID"
                    child_pid = int(peer.recv(100))
                    self.assertEqual(child_pid, started_pid)
                    waiting_for = f"{ready_job} 시작 기록 검증"
                    before = json.loads((logs / "state.json").read_text())
                    self.assertTrue(any(child["pid"] == child_pid for child in before["active"].values()))
                    # 시작 사건과 완전한 상태가 자식 종료보다 먼저 디스크에 있어야 한다.
                    events = [json.loads(line) for line in (logs / "events.jsonl").read_text().splitlines()]
                    self.assertTrue(any(event["event"] == "child_started" and event["pid"] == child_pid for event in events))
                    note(f"시험: signal={signum} 전송 (runner pid={process.pid})")
                    process.send_signal(signum)
                    waiting_for = f"signal={signum} 뒤 runner 종료"
                    exit_code = process.wait(timeout=3)
                    note(f"시험: runner 종료 (exit={exit_code})")
                    self.assertEqual(exit_code, 128 + signum)
                    waiting_for = f"{ready_job} 소켓 EOF"
                    self.assertEqual(peer.recv(1), b"", "직접 자식이 실제로 종료되어야 한다")
                    note(f"시험: {ready_job} 소켓 EOF (pid={child_pid})")
                waiting_for = "중단 결과 검증"
                state = json.loads((logs / "state.json").read_text())
                summary = json.loads((logs / "summary.json").read_text())
                self.assertEqual(state["status"], "interrupted")
                self.assertEqual(summary["signal"], signum)
                self.assertEqual(state["active"], {})
                self.assertEqual(state["interrupted_waits"][0]["pid"], child_pid)
                snapshot = state["interrupted_waits"][0]["processes"]
                if snapshot_error:
                    self.assertIn("CalledProcessError", snapshot["error"])
                else:
                    self.assertTrue(any(row["pid"] == child_pid for row in snapshot))
                events = [json.loads(line) for line in (logs / "events.jsonl").read_text().splitlines()]
                if ignore_term:
                    finished = next(event for event in events if event["event"] == "child_finished"
                                    and event["pid"] == child_pid)
                    self.assertEqual(finished["exit"], -signal.SIGKILL)
                if phase == "execute":
                    self.assertEqual([job["status"] for job in state["jobs"]],
                                     ["interrupted", "not_started", "not_started"])
                if phase == "doctests":
                    self.assertEqual([job["exit"] for job in summary["jobs"]], [7, 7, 128 + signum])
                self.assertIsNone(outsider.poll(), "runner 밖 프로세스는 살아 있어야 한다")
            except (AssertionError, ValueError, queue.Empty, TimeoutError, subprocess.TimeoutExpired) as error:
                note(f"시험: {waiting_for} 중 실패 ({error!r})")
                child_trace = connection_log.read_text().splitlines() if connection_log.exists() else []
                trace = "\n".join(sorted([*timeline, *child_trace]))
                state_path = logs / "state.json"
                state_text = state_path.read_text() if state_path.exists() else "기록 없음"
                events_path = logs / "events.jsonl"
                events_text = events_path.read_text() if events_path.exists() else "기록 없음"
                self.fail(f"{waiting_for} 중 실패 ({error!r})\n{trace}\n"
                          f"정리 전 runner 상태:\n{state_text}\nrunner 사건:\n{events_text}")
            finally:
                if process.poll() is None:
                    # 실패 경로에서도 runner가 직접 띄운 자식을 회수하게 한다.
                    process.terminate()
                    try:
                        process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        process.kill()
                process.wait(timeout=3)
                reader.join(timeout=3)
                process.stdout.close()
                outsider.stdin.close()
                outsider.wait(timeout=3)

    def test_sigint_during_prepare_preserves_the_current_child(self):
        self.interrupt_runner("prepare", signal.SIGINT)

    def test_sigterm_during_listing_preserves_the_current_child(self):
        self.interrupt_runner("list", signal.SIGTERM)

    def test_sigterm_preserves_running_and_unstarted_shards_without_touching_another_process(self):
        self.interrupt_runner("execute", signal.SIGTERM)

    def test_sigint_during_doctests_keeps_preceding_binary_failures(self):
        self.interrupt_runner("doctests", signal.SIGINT)

    def test_sigterm_ignoring_child_is_killed_and_reaped(self):
        self.interrupt_runner("execute", signal.SIGTERM, ignore_term=True)

    def test_snapshot_lookup_failure_still_reaps_the_interrupted_child(self):
        self.interrupt_runner("execute", signal.SIGTERM, snapshot_error=True)

    @unittest.skipUnless(os.name == "posix", "Unix ps 진단의 결과·오류 계약을 검사한다")
    def test_process_snapshot_filters_descendants_and_preserves_lookup_errors(self):
        rows = "100 1 S wait /fixture/parent\n101 100 S wait /fixture/child with spaces\n200 1 R - /fixture/outsider\n"
        with patch("scripts.rust_tests.subprocess.check_output", return_value=rows) as ps:
            snapshot = process_tree(100)
        self.assertEqual([row["pid"] for row in snapshot], [100, 101])
        self.assertEqual(snapshot[1]["executable"], "/fixture/child with spaces")
        self.assertEqual(ps.call_args.args[0], ["ps", "-axo", "pid=,ppid=,stat=,wchan=,comm="])
        with patch("scripts.rust_tests.subprocess.check_output",
                   side_effect=subprocess.TimeoutExpired(["ps"], 1)):
            snapshot = process_tree(100)
        self.assertIn("TimeoutExpired", snapshot["error"])

    def test_individual_times_keep_failures_and_reject_missing_completed_cases(self):
        with tempfile.TemporaryDirectory(prefix="aaa-rust-times-") as tmp:
            root = Path(tmp)
            fixture = root / "child.py"
            fixture.write_text("""import os, sys
assert os.environ['RUSTC_BOOTSTRAP'] == '1'
assert '--report-time' in sys.argv
print('running 3 tests')
print('test module::빠른_시험 ... ok <0.125s>')
print('test module::ignored ... ignored')
if sys.argv[1] == 'complete':
    print('test module::failed ... FAILED <5.001s>')
print('test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 5.126s')
sys.exit(7 if sys.argv[1] == 'complete' else 0)
""")
            jobs = [Job(mode, [sys.executable, str(fixture), mode], root) for mode in ["complete", "missing"]]
            results = execute(jobs, root, 1, 1, cpu_budget=1, report_time=True)
            self.assertEqual(results[0]["exit"], 7)
            self.assertEqual(results[0]["test_times"], [
                dict(name="module::빠른_시험", outcome="ok", seconds=0.125),
                dict(name="module::failed", outcome="FAILED", seconds=5.001),
            ])
            self.assertEqual(results[1]["exit"], 1)
            self.assertIn("개별 시험 시간의 수", (root / results[1]["log"]).read_text())
            for result in results:
                for key in ["load_before", "load_after"]:
                    self.assertTrue(result[key] is None or len(result[key]) == 3)

    def test_long_jobs_start_first_without_losing_unknown_or_short_jobs(self):
        artifacts = [dict(executable=name, manifest_path=f"/workspace/{name}/Cargo.toml",
                          target=dict(name=name, kind=["test"]))
                     for name in ("short", "slow", "new", "m13")]
        names = ["one", "two", "three", "a", "b", "c"]
        # 시간 원장에 개별 시간이 있는 owner는 그 값(60)으로, 없는 owner는 binary 기본값을 나눈 값(6)으로 줄을 선다.
        durations = {"short": 1, "slow": 40, "m13::one": 60, "m13::two": 60, "m13::three": 60}
        with patch("scripts.rust_tests.subprocess.check_output",
                   return_value="\n".join(name + ": test" for name in names)):
            jobs = plan(artifacts, 6, durations)
        self.assertEqual([job.name for job in jobs],
                         ["new", "m13-1", "m13-2", "m13-3", "slow", "m13-4", "m13-5", "m13-6", "short"])
        self.assertCountEqual([name for job in jobs for name in job.tests or []], names)
        for job in jobs:
            self.assertEqual(job.cwd.name, "m13" if job.tests else job.name)

    def test_waiting_large_job_does_not_block_a_fitting_job(self):
        with tempfile.TemporaryDirectory(prefix="aaa-rust-admission-") as tmp, socket.socket() as listener:
            root = Path(tmp)
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(10)
            port = listener.getsockname()[1]
            fixture = root / "child.py"
            fixture.write_text("""import socket, sys
with socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=10) as link:
    link.sendall(sys.argv[2].encode())
    assert link.recv(1) == b'!'
print(f'running {sys.argv[3]} tests')
""")
            errors = []
            seen = []

            def release():
                links = []
                try:
                    # A가 CPU 2자리를 쓰는 동안 B·C(각2)는 기다리고 D(1)는 바로 들어와야 한다.
                    for _ in range(2):
                        child, _ = listener.accept()
                        child.settimeout(10)
                        links.append(child)
                        seen.append(child.recv(1))
                    self.assertCountEqual(seen, [b"A", b"D"])
                    for child in links:
                        child.sendall(b"!")
                    for _ in range(2):
                        child, _ = listener.accept()
                        links.append(child)
                        child.settimeout(10)
                        seen.append(child.recv(1))
                        child.sendall(b"!")
                except Exception as error:
                    errors.append(error)
                finally:
                    for child in links:
                        child.close()

            gate = threading.Thread(target=release)
            gate.start()
            jobs = [Job(name, [sys.executable, str(fixture), str(port), name, str(size)],
                        root, [f"case-{n}" for n in range(size)])
                    for name, size in (("A", 2), ("B", 2), ("C", 2), ("D", 1))]
            results = execute(jobs, root, 3, 2, cpu_budget=3)
            gate.join()
            self.assertEqual(errors, [])
            self.assertCountEqual(seen, [b"A", b"B", b"C", b"D"])
            self.assertTrue(all(result["exit"] == 0 for result in results))

    def test_every_test_is_selected_exactly_once(self):
        names = [f"module::case_{n}" for n in range(100)]
        groups = partitions(names)
        self.assertEqual(sorted(name for group in groups for name in group), sorted(names))
        self.assertTrue(all(len(group) == 1 for group in groups))
        with self.assertRaises(ValueError):
            partitions(["same", "same"])

    def test_module_and_install_splits_keep_each_original_owner_once(self):
        names = ["backup::tests::one", "backup::tests::two", "router::tests::three"]
        artifacts = [dict(executable=name, manifest_path=f"/workspace/{name}/Cargo.toml",
                          target=dict(name=name, kind=[kind]))
                     for name, kind in (("aaa_host", "lib"), ("install", "test"))]
        with patch("scripts.rust_tests.subprocess.check_output",
                   return_value="\n".join(name + ": test" for name in names)):
            jobs = plan(artifacts, 4)
        for target in ("aaa_host", "install"):
            selected = [job for job in jobs if job.name.startswith(target + "-")]
            self.assertCountEqual([name for job in selected for name in job.tests], names)
            self.assertTrue(all("--exact" in job.argv for job in selected))
            if target == "install":
                self.assertTrue(all(len(job.tests) == 1 for job in selected))

    def test_completed_browser_returns_slots_without_restarting_any_job(self):
        with tempfile.TemporaryDirectory(prefix="aaa-rust-returned-cpu-") as tmp, socket.socket() as listener:
            root = Path(tmp)
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(3)
            fixture = root / "child.py"
            fixture.write_text("""import socket, sys
with socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=3) as peer:
    peer.sendall(sys.argv[2].encode())
    assert peer.recv(1) == b'!'
print('running 1 test')
""")
            completion = root / "e2e.result"
            errors, seen = [], []

            def finish_browser():
                peers = []
                try:
                    peer, _ = listener.accept()
                    peers.append(peer)
                    seen.append(peer.recv(10))
                    self.assertEqual(seen, [b"first"])
                    # 처음 Rust 한 자리가 아직 점유 중이다. 브라우저 완료 뒤 두 자리를 반환한다.
                    completion.write_text("e2e|ok\n")
                    for _ in range(2):
                        peer, _ = listener.accept()
                        peers.append(peer)
                        seen.append(peer.recv(10))
                except Exception as error:
                    errors.append(error)
                finally:
                    for peer in peers:
                        peer.sendall(b"!")
                        peer.close()

            browser = threading.Thread(target=finish_browser)
            browser.start()
            jobs = [Job(name, [sys.executable, str(fixture), str(listener.getsockname()[1]), name], root, ["owner"])
                    for name in ("first", "second", "third")]
            results = execute(jobs, root, 3, 1, cpu_budget=1, release_budget=(completion, 3))
            browser.join()
            self.assertEqual(errors, [])
            self.assertCountEqual(seen, [b"first", b"second", b"third"])
            self.assertEqual(len(results), 3)
            self.assertTrue(all(result["exit"] == 0 for result in results))

    def test_missing_selected_tests_is_a_failure_even_when_child_exits_zero(self):
        with tempfile.TemporaryDirectory(prefix="aaa-rust-count-") as tmp:
            root = Path(tmp)
            job = Job("empty", [sys.executable, "-c", "print('running 0 tests')"], root, ["owner"])
            self.assertEqual(execute([job], root, 1, 1, cpu_budget=1)[0]["exit"], 1)

    def test_six_m13_jobs_share_cpu_budget_and_release_it_after_failure(self):
        with tempfile.TemporaryDirectory(prefix="aaa-rust-cpu-") as tmp, socket.socket() as listener:
            root = Path(tmp)
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(10)
            port = listener.getsockname()[1]
            fixture = root / "child.py"
            fixture.write_text("""import os, socket, sys
assert os.environ['RUST_TEST_THREADS'] == '4'
with socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=10) as link:
    link.sendall(b'ready')
    assert link.recv(1) == b'!'
print(f'running {sys.argv[2]} tests')
raise SystemExit(int(sys.argv[3]))
""")
            errors = []

            def release():
                links = []
                try:
                    # 여섯 실제 자식이 함께 와야 연다. singleton에 4자리를 예약하면 도달할 수 없다.
                    for _ in range(6):
                        child, _ = listener.accept()
                        child.settimeout(10)
                        links.append(child)
                    for child in links:
                        self.assertEqual(child.recv(5), b"ready")
                        child.sendall(b"!")
                    following, _ = listener.accept()
                    with following:
                        following.settimeout(10)
                        self.assertEqual(following.recv(5), b"ready")
                        following.sendall(b"!")
                except Exception as error:
                    errors.append(error)
                finally:
                    for child in links:
                        child.close()

            gate = threading.Thread(target=release)
            gate.start()
            sizes = [1, 1, 1, 4, 4, 4, 4]
            jobs = [Job(f"m13-{n}", [sys.executable, str(fixture), str(port), str(size), "7" if n == 0 else "0"],
                        root, [f"case-{i}" for i in range(size)]) for n, size in enumerate(sizes)]
            results = execute(jobs, root, 6, 4, cpu_budget=15)
            gate.join()
            self.assertEqual(errors, [])
            self.assertEqual(sorted(result["exit"] for result in results), [0] * 6 + [7])
            first = [result for result in results if result["name"] != "m13-6"]
            self.assertLess(max(result["start_seconds"] for result in first),
                            min(result["start_seconds"] + result["seconds"] for result in first))
            events = [(result["start_seconds"], result["cpu_slots"]) for result in results]
            events += [(result["start_seconds"] + result["seconds"], -result["cpu_slots"]) for result in results]
            active = 0
            for _, slots in sorted(events):
                active += slots
                self.assertLessEqual(active, 15)
            self.assertEqual(active, 0)

    @unittest.skipUnless(os.name == "posix", "가짜 Cargo·시험 실행 파일의 Unix shebang이 필요하다")
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
if 'failed_owner' in selected:
    print("thread 'failed_owner' (123) panicked at tests/lab.rs:1:1:")
    print('30s 안에 labA 연결이 되지 않았다')
    print('진단 꼬리' * 4000)
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
            failed = next(job for job in summary["jobs"] if job["exit"])
            self.assertEqual(failed["failures"], [dict(test="failed_owner", message="30s 안에 labA 연결이 되지 않았다")])
            self.assertIn("FAIL other failed_owner: 30s 안에 labA 연결이 되지 않았다", result.stdout[-2000:])


if __name__ == "__main__":
    unittest.main()
