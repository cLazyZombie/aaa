"""Cargo가 준비한 모든 시험 binary를 격리된 묶음으로 실행한다. doctest도 별도로 유지한다."""
import argparse
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import threading
import time


@dataclass
class Job:
    name: str
    argv: list[str]
    cwd: Path
    tests: list[str] | None = None
    weight: float = 0


class Interrupted(RuntimeError):
    pass


def process_tree(pid):
    """대기 중인 시험이 띄운 CLI/cp/codesign 등의 상태만 보존한다. 인자·환경은 읽지 않는다."""
    if os.name != "posix":
        return dict(error="process snapshot은 Unix에서만 지원한다")
    try:
        output = subprocess.check_output(["ps", "-axo", "pid=,ppid=,stat=,wchan=,comm="],
                                         text=True, stderr=subprocess.STDOUT, timeout=1)
    except (OSError, subprocess.SubprocessError) as error:
        return dict(error=f"{type(error).__name__}: {error}")
    rows = []
    for line in output.splitlines():
        parts = line.split(None, 4)
        if len(parts) == 5:
            rows.append(dict(pid=int(parts[0]), ppid=int(parts[1]), state=parts[2], wait=parts[3],
                             executable=parts[4]))
    owned = {pid}
    while True:
        found = owned | {row["pid"] for row in rows if row["ppid"] in owned}
        if found == owned:
            return [row for row in rows if row["pid"] in owned]
        owned = found


class RunRecord:
    """종료를 기다리기 전에 기록한다. signal handler는 잠금을 잡거나 자식을 죽이지 않는다."""
    def __init__(self, logs):
        self.logs = logs
        self.epoch = time.monotonic()
        self.lock = threading.Lock()
        self.abort_error = None
        self.signum = None
        self.state = dict(phase="prepare", status="running", jobs=[], active={}, results=[])
        (logs / "events.jsonl").write_text("")
        self.update("runner_started")

    def update(self, event, **fields):
        with self.lock:
            self.state.update(fields)
            self._write(event)

    def _write(self, event, **detail):
        elapsed = time.monotonic() - self.epoch
        with (self.logs / "events.jsonl").open("a") as output:
            output.write(json.dumps(dict(event=event, seconds=elapsed, **detail), ensure_ascii=False) + "\n")
        snapshot = dict(self.state, seconds=elapsed, signal=self.signum)
        # 같은 디렉터리 rename으로 중단 직전에도 완전한 JSON 하나를 읽는다.
        for name, value in [("state.json", snapshot), ("summary.json", dict(
                seconds=elapsed, status=self.state["status"], signal=self.signum,
                workers=self.state.get("workers"), threads=self.state.get("threads"),
                cpu_budget=self.state.get("cpu_budget"), exit=self.state.get("exit"), jobs=self.state["results"]))]:
            temporary = self.logs / (name + ".tmp")
            temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
            temporary.replace(self.logs / name)

    @property
    def stopping(self):
        return self.signum is not None or self.abort_error is not None

    @property
    def stop_code(self):
        return 128 + self.signum if self.signum is not None else 1

    def signal(self, signum, _frame):
        self.signum = signum

    def run(self, key, argv, cwd, output, env=None, stderr=subprocess.STDOUT):
        if self.stopping:
            raise Interrupted("자식 기동 전 중단")
        child = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                 stdout=output, stderr=stderr)
        try:
            with self.lock:
                self.state["active"][key] = dict(pid=child.pid, argv=argv, cwd=str(cwd),
                                                 wait="process exit", log=Path(output.name).name)
                self._write("child_started", name=key, **self.state["active"][key])
            print(f"START {key} pid={child.pid} log={output.name}", flush=True)
            while child.poll() is None:
                try:
                    child.wait(timeout=0.1)
                except subprocess.TimeoutExpired:
                    pass
                if self.stopping:
                    # Popen이 소유한 직접 자식만 정리한다. group/pattern 종료는 쓰지 않는다.
                    processes = process_tree(child.pid)
                    with self.lock:
                        current = dict(name=key, **self.state["active"][key], processes=processes)
                        self.state.setdefault("interrupted_waits", []).append(current)
                        self._write("interrupt_wait", **current)
                    child.terminate()
                    try:
                        child.wait(timeout=1)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait()
                    break
            code = child.wait()
            return self.stop_code if self.stopping else code
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
            with self.lock:
                self.state["active"].pop(key, None)
                self._write("child_finished", name=key, pid=child.pid, exit=child.returncode)

    def completed(self, index, result):
        with self.lock:
            self.state["jobs"][index]["status"] = "interrupted" if self.stopping else "completed"
            self.state["results"].append(result)
            self._write("job_finished", name=result["name"], exit=result["exit"])


def panic_summaries(output):
    """긴 진단/역추적 뒤에도 panic의 첫 줄(대기 단계)을 CI 요약에서 찾게 한다."""
    lines = output.splitlines()
    summaries = []
    for index, line in enumerate(lines[:-1]):
        match = re.match(r"^thread '(.+)'(?: \(\d+\))? panicked at .+:$", line)
        if match and lines[index + 1].strip():
            summaries.append(dict(test=match[1], message=lines[index + 1].strip()[:500]))
    return summaries


def test_times(output):
    """libtest --report-time의 실제 완료 case만 읽는다. 합계/ignored는 개별 시간으로 세지 않는다."""
    return [dict(name=match[1], outcome=match[2], seconds=float(match[3]))
            for match in re.finditer(r"^test (.+?) \.\.\. (ok|FAILED) <([\d.]+)s>$", output, re.MULTILINE)]


def load_average():
    return list(os.getloadavg()) if hasattr(os, "getloadavg") else None


def partitions(names):
    """각 독립 integration owner를 슬롯에 넣는다. 긴 고정 shard가 뒤 시험을 붙잡지 않는다."""
    if len(set(names)) != len(names):
        raise ValueError("시험 목록에 중복 이름이 있다")
    slow = [name for name in names if name.startswith("rollout::")]
    rest = [name for name in names if not name.startswith("rollout::")]
    groups = [[name] for name in [*slow, *rest]]
    if sorted(name for group in groups for name in group) != sorted(names):
        raise ValueError("시험 분할이 원래 목록과 다르다")
    return groups


def prepare(logs, record=None):
    command = ["cargo", "test", "--profile", "verify", "--workspace", "--locked", "--no-fail-fast", "--no-run", "--message-format=json-render-diagnostics"]
    artifacts = []
    malformed = False
    # pipe EOF를 기다리지 않고 직접 소유한 Cargo의 종료를 기다린다. 중단돼도 원문은 파일에 남는다.
    with (logs / "build.jsonl").open("w") as output, (logs / "build.stderr.log").open("w") as errors:
        if record is None:
            code = subprocess.run(command, stdout=output, stderr=errors).returncode
        else:
            code = record.run("prepare", command, Path.cwd(), output, stderr=errors)
    print((logs / "build.stderr.log").read_text(errors="replace"), end="", flush=True)
    if record is not None and record.stopping:
        raise Interrupted("Cargo 시험 준비 중단")
    with (logs / "build.jsonl").open() as output:
        for line in output:
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                malformed = True
                continue
            if message["reason"] == "compiler-message":
                print(message["message"].get("rendered", ""), end="", flush=True)
            if message["reason"] == "compiler-artifact" and message["profile"]["test"] and message.get("executable"):
                artifacts.append(message)
        if code or malformed:
            raise RuntimeError(f"Cargo 시험 준비 실패: exit={code}, malformed={malformed}, 로그 build.jsonl")
    if not artifacts:
        raise RuntimeError("Cargo가 시험 실행 파일을 하나도 내지 않았다")
    if len({artifact["executable"] for artifact in artifacts}) != len(artifacts):
        raise RuntimeError("Cargo 시험 실행 파일 목록이 중복됐다")
    return artifacts


def plan(artifacts, workers, durations=None, record=None):
    durations = {} if durations is None else durations
    jobs = []
    for artifact in artifacts:
        executable = artifact["executable"]
        target = artifact["target"]
        cwd = Path(artifact["manifest_path"]).parent
        name = target["name"]
        independent = (name in {"m13", "install", "e2e", "m12", "deploy_start"}
                       or name == "restart" and sys.platform == "darwin") and "test" in target["kind"]
        modules = name in {"aaa", "aaa_host", "aaa_agent"}
        if independent or modules:
            argv = [executable, "--list", "--format", "terse"]
            if record is None:
                listed = subprocess.check_output(argv, cwd=cwd, text=True)
            else:
                record.update("listing_started", phase="list")
                log = record.logs / f"list-{name}.log"
                with log.open("w") as output:
                    code = record.run(f"list-{name}", argv, cwd, output)
                if record.stopping:
                    raise Interrupted(f"{name} 시험 목록 읽기 중단")
                if code:
                    raise RuntimeError(f"{name} 시험 목록 읽기 실패: exit={code}, 로그 {log}")
                listed = log.read_text()
            names = [line.removesuffix(": test") for line in listed.splitlines() if line.endswith(": test")]
            if not names:
                raise RuntimeError(f"{name} 시험 목록이 비었다")
            groups = partitions(names)
            if modules:
                by_module = {}
                for test in names:
                    parts = test.split("::")
                    module = "::".join(parts[:4]) if name == "aaa" and parts[0] == "install" else parts[0]
                    by_module.setdefault(module, []).append(test)
                groups = list(by_module.values())
            for index, group in enumerate(groups):
                weight = durations.get(f"{name}::{group[0]}",
                                       durations.get(name, 6 * workers) * len(group) / len(names))
                if name == "m13" and group[0].startswith("rollout::"):
                    weight = durations.get(f"{name}::{group[0]}", 60)
                jobs.append(Job(f"{name}-{index + 1}", [executable, "--exact", *group], cwd, group, weight))
        else:
            jobs.append(Job(name, [executable], cwd, weight=durations.get(name, float("inf"))))
    # 시간은 순서에만 쓴다. 전부 실행하며 선택·시한·실패 판정을 바꾸지 않는다.
    jobs.sort(key=lambda job: -job.weight)
    return jobs


def execute(jobs, logs, workers, threads, cpu_budget=None, report_time=False, record=None, release_budget=None):
    cpu_budget = max(1, os.cpu_count() or 1) if cpu_budget is None else cpu_budget
    if cpu_budget < 1:
        raise ValueError("CPU 예산은 1 이상이어야 한다")
    threads = min(threads, cpu_budget)
    available = cpu_budget
    record = RunRecord(logs) if record is None else record
    epoch = time.monotonic()
    record.update("execution_started", phase="execute", workers=workers, threads=threads, cpu_budget=cpu_budget,
                  initial_cpu_budget=cpu_budget,
                  jobs=[dict(name=job.name, tests=job.tests, status="queued") for job in jobs])

    def slots_for(job):
        return max(1, min(cpu_budget, threads, len(job.tests) if job.tests is not None else threads))

    def run(index, job, slots):
        started = time.monotonic()
        load_before = load_average()
        log = logs / f"{index:02d}-{job.name}.log"
        env = {**os.environ, "RUST_TEST_THREADS": str(threads)}
        argv = job.argv
        if report_time and job.name != "doctests":
            # 이미 준비한 libtest의 출력만 켠다. Cargo/rustc는 이 환경으로 실행하지 않는다.
            env["RUSTC_BOOTSTRAP"] = "1"
            argv = [*argv, "-Z", "unstable-options", "--report-time", "--format", "pretty", "--color", "never"]
        try:
            with log.open("w") as output:
                try:
                    code = record.run(f"{index:02d}-{job.name}", argv, job.cwd, output, env)
                except Interrupted as error:
                    output.write(f"시험 중단: {error}\n")
                    code = record.stop_code
                except OSError as error:
                    output.write(f"시험 기동 실패: {error}\n")
                    code = 1
        finally:
            finished = time.monotonic()
        text = log.read_text(errors="replace")
        times = test_times(text) if report_time else []
        if report_time and job.name != "doctests" and not record.stopping:
            totals = re.search(r"^test result: .*? (\d+) passed; (\d+) failed;", text, re.MULTILINE)
            if not totals or len(times) != int(totals[1]) + int(totals[2]):
                text += "\n개별 시험 시간의 수가 libtest 완료 수와 다르다\n"
                code = code or 1
                log.write_text(text)
        if job.tests is not None and not record.stopping:
            match = re.search(r"^running (\d+) tests?$", text, re.MULTILINE)
            if not match or int(match[1]) != len(job.tests):
                text += "\n시험 실행 수가 분할 목록과 다르다\n"
                code = 1
                log.write_text(text)
        result = dict(name=job.name, exit=code, seconds=finished - started,
                    start_seconds=started - epoch, queue_seconds=started - epoch,
                    cpu_slots=slots, load_before=load_before, load_after=load_average(), test_times=times,
                    log=log.name, failures=panic_summaries(text))
        record.completed(index, result)
        return dict(result, output=text)

    results = []
    pool = ThreadPoolExecutor(max_workers=workers)
    try:
        pending = list(enumerate(jobs))
        running = {}
        while pending or running:
            if release_budget is not None and release_budget[0].is_file():
                following = max(cpu_budget, release_budget[1])
                available += following - cpu_budget
                cpu_budget = following
                record.update("cpu_budget_returned", cpu_budget=cpu_budget)
                release_budget = None
            if record.stopping:
                with record.lock:
                    for index, _ in pending:
                        record.state["jobs"][index]["status"] = "not_started"
                    record._write("queue_interrupted")
                pending.clear()
            # CPU 자리가 있는 작업만 worker에 보낸다. 큰 작업의 자리 대기가 worker를
            # 차지해, 뒤의 실행 가능한 작은 작업까지 막거나 긴 작업의 순서를 뒤집지 않는다.
            for index, job in pending.copy():
                if record.stopping or len(running) == workers:
                    break
                slots = slots_for(job)
                if slots > available:
                    continue
                available -= slots
                pending.remove((index, job))
                with record.lock:
                    record.state["jobs"][index].update(status="running", cpu_slots=slots)
                    record._write("job_started", name=job.name)
                running[pool.submit(run, index, job, slots)] = slots
            if not running:
                break
            done, _ = wait(running, timeout=0.1, return_when=FIRST_COMPLETED)
            for future in done:
                available += running.pop(future)
                result = future.result()
                print(f"\nRunning {result['name']} ({result['seconds']:.2f}초, exit={result['exit']})", flush=True)
                print(result.pop("output"), end="", flush=True)
                results.append(result)
    except BaseException as error:
        # executor의 암묵적 join보다 먼저 중단시켜 다른 worker의 무기한 대기를 막는다.
        record.abort_error = f"{type(error).__name__}: {error}"
        raise
    finally:
        pool.shutdown(wait=True, cancel_futures=True)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--logs", type=Path, required=True)
    parser.add_argument("--workers", type=int, default=int(os.environ.get("AAA_RUST_TEST_WORKERS", "6")))
    parser.add_argument("--report-time", action="store_true", help="개별 libtest 시간과 시작·끝 load를 summary.json에 보존한다")
    args = parser.parse_args()
    if args.workers < 1:
        parser.error("workers는 1 이상이어야 한다")
    args.logs.mkdir(parents=True, exist_ok=True)
    args.logs = args.logs.resolve()
    record = RunRecord(args.logs)
    handlers = {signum: signal.signal(signum, record.signal) for signum in (signal.SIGINT, signal.SIGTERM)}
    try:
        return run_suite(args, record)
    except Exception as error:
        code = record.stop_code if record.stopping else 1
        record.update("runner_failed", status="interrupted" if record.signum is not None else "failed",
                      error=f"{type(error).__name__}: {error}", exit=code)
        print(f"FAIL runner: {type(error).__name__}: {error}, exit={code}", flush=True)
        return code
    finally:
        for signum, handler in handlers.items():
            signal.signal(signum, handler)


def run_suite(args, record):
    artifacts = prepare(args.logs, record)
    durations = json.loads(Path(__file__).with_name("rust-test-durations.json").read_text())
    jobs = plan(artifacts, args.workers, durations, record)
    # doctest를 binary 실행으로 대신하지 않는다. 기존 Cargo의 전체 workspace doc 범위를 그대로 실행한다.
    jobs.append(Job("doctests", ["cargo", "test", "--profile", "verify", "--workspace", "--doc", "--locked", "--no-fail-fast"], Path.cwd(),
                    weight=durations.get("doctests", 5)))
    # 준비/Clippy가 끝났으므로 Cargo 잠금 경합 없이 doctest도 같은 실행 예산에 넣는다.
    jobs.sort(key=lambda job: -job.weight)
    # 큰 unit binary 안의 병렬도와 integration owner의 한 자리를 같은 예산에서 계산한다.
    threads = int(os.environ.get("RUST_TEST_THREADS", max(1, (os.cpu_count() or 1) // min(args.workers, 4))))
    if threads < 1:
        raise ValueError("RUST_TEST_THREADS는 1 이상이어야 한다")
    manifest = [dict(name=job.name, argv=job.argv, cwd=str(job.cwd), tests=job.tests) for job in jobs]
    (args.logs / "plan.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    cpu_budget = int(os.environ.get("AAA_RUST_TEST_CPU_BUDGET", os.cpu_count() or 1))
    release_file = os.environ.get("AAA_RUST_TEST_RELEASE_FILE")
    cpus = getattr(os, "process_cpu_count", os.cpu_count)() or 1
    release_budget = (Path(release_file), cpus) if release_file else None
    results = execute(jobs, args.logs, args.workers, threads, cpu_budget=cpu_budget,
                      report_time=args.report_time, record=record, release_budget=release_budget)
    code = record.stop_code if record.stopping else int(any(result["exit"] for result in results))
    record.update("runner_finished", status="interrupted" if record.stopping else "failed" if code else "completed",
                  exit=code)
    # ci-local annotation은 로그 꼬리를 고른다. 전체 출력 뒤 실패 원문 첫 줄을 다시 남겨
    # panic 위치만 보이고 what/diag가 잘리는 일을 막는다. 성공/종료 코드 판정은 그대로다.
    for job in results:
        if job["exit"]:
            print(f"FAIL {job['name']}: exit={job['exit']}, 로그 {job['log']}")
            for failure in job["failures"]:
                print(f"FAIL {job['name']} {failure['test']}: {failure['message']}")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
