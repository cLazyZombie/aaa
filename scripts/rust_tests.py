"""Cargo가 준비한 모든 시험 binary를 격리된 묶음으로 실행한다. doctest도 별도로 유지한다."""
import argparse
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


@dataclass
class Job:
    name: str
    argv: list[str]
    cwd: Path
    tests: list[str] | None = None


def panic_summaries(output):
    """긴 진단/역추적 뒤에도 panic의 첫 줄(대기 단계)을 CI 요약에서 찾게 한다."""
    lines = output.splitlines()
    summaries = []
    for index, line in enumerate(lines[:-1]):
        match = re.match(r"^thread '(.+)'(?: \(\d+\))? panicked at .+:$", line)
        if match and lines[index + 1].strip():
            summaries.append(dict(test=match[1], message=lines[index + 1].strip()[:500]))
    return summaries


def partitions(names, count):
    """긴 rollout은 먼저 시작하고 나머지는 정확한 이름으로 균등 분할한다."""
    if len(set(names)) != len(names):
        raise ValueError("시험 목록에 중복 이름이 있다")
    slow = [name for name in names if name.startswith("rollout::")]
    rest = [name for name in names if not name.startswith("rollout::")]
    groups = [[name] for name in slow]
    groups += [rest[i::count] for i in range(min(count, len(rest)))]
    if sorted(name for group in groups for name in group) != sorted(names):
        raise ValueError("시험 분할이 원래 목록과 다르다")
    return groups


def prepare(logs):
    command = ["cargo", "test", "--workspace", "--locked", "--no-fail-fast", "--no-run", "--message-format=json-render-diagnostics"]
    artifacts = []
    malformed = False
    with (logs / "build.jsonl").open("w") as record:
        child = subprocess.Popen(command, stdout=subprocess.PIPE, text=True)
        for line in child.stdout:
            record.write(line)
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                malformed = True
                continue
            if message["reason"] == "compiler-message":
                print(message["message"].get("rendered", ""), end="", flush=True)
            if message["reason"] == "compiler-artifact" and message["profile"]["test"] and message.get("executable"):
                artifacts.append(message)
        if child.wait() or malformed:
            raise RuntimeError("Cargo 시험 준비 실패")
    if not artifacts:
        raise RuntimeError("Cargo가 시험 실행 파일을 하나도 내지 않았다")
    if len({artifact["executable"] for artifact in artifacts}) != len(artifacts):
        raise RuntimeError("Cargo 시험 실행 파일 목록이 중복됐다")
    return artifacts


def plan(artifacts, workers, durations=None):
    jobs = []
    for artifact in artifacts:
        executable = artifact["executable"]
        target = artifact["target"]
        cwd = Path(artifact["manifest_path"]).parent
        name = target["name"]
        if name == "m13" and "test" in target["kind"]:
            listed = subprocess.check_output([executable, "--list", "--format", "terse"], cwd=cwd, text=True)
            names = [line.removesuffix(": test") for line in listed.splitlines() if line.endswith(": test")]
            if not names:
                raise RuntimeError("m13 시험 목록이 비었다")
            for index, group in enumerate(partitions(names, max(1, min(3, workers - 1)))):
                jobs.append(Job(f"m13-{index + 1}", [executable, "--exact", *group], cwd, group))
        else:
            jobs.append(Job(name, [executable], cwd))
    # m13 여섯 묶음을 먼저 시작하고, 빈 자리에는 실측상 긴 binary부터 넣는다.
    # 새 binary는 시간이 없으므로 먼저 실행한다. 시간은 순서에만 쓰고 선택·시한에는 쓰지 않는다.
    durations = {} if durations is None else durations
    jobs.sort(key=lambda job: (not job.name.startswith("m13-"), -durations.get(job.name, float("inf"))))
    return jobs


def execute(jobs, logs, workers, threads, cpu_budget=None):
    cpu_budget = max(1, os.cpu_count() or 1) if cpu_budget is None else cpu_budget
    if cpu_budget < 1:
        raise ValueError("CPU 예산은 1 이상이어야 한다")
    available = cpu_budget
    epoch = time.monotonic()

    def slots_for(job):
        # exact 한 시험뿐인 rollout이 libtest 기본 thread 수만큼 자리를 예약하지 않는다.
        return max(1, min(cpu_budget, threads, len(job.tests) if job.tests is not None else threads))

    def run(index, job, slots):
        started = time.monotonic()
        log = logs / f"{index:02d}-{job.name}.log"
        env = {**os.environ, "RUST_TEST_THREADS": str(threads)}
        try:
            with log.open("w") as output:
                try:
                    code = subprocess.run(job.argv, cwd=job.cwd, env=env, stdout=output, stderr=subprocess.STDOUT).returncode
                except OSError as error:
                    output.write(f"시험 기동 실패: {error}\n")
                    code = 1
        finally:
            finished = time.monotonic()
        text = log.read_text(errors="replace")
        if job.tests is not None:
            match = re.search(r"^running (\d+) tests?$", text, re.MULTILINE)
            if not match or int(match[1]) != len(job.tests):
                text += "\n시험 실행 수가 분할 목록과 다르다\n"
                code = 1
                log.write_text(text)
        return dict(name=job.name, exit=code, seconds=finished - started,
                    start_seconds=started - epoch, queue_seconds=started - epoch,
                    cpu_slots=slots, log=log.name, failures=panic_summaries(text), output=text)

    results = []
    with ThreadPoolExecutor(max_workers=workers) as pool:
        pending = list(enumerate(jobs))
        running = {}
        while pending or running:
            # CPU 자리가 있는 작업만 worker에 보낸다. 큰 작업의 자리 대기가 worker를
            # 차지해, 뒤의 실행 가능한 작은 작업까지 막거나 긴 작업의 순서를 뒤집지 않는다.
            for index, job in pending.copy():
                if len(running) == workers:
                    break
                slots = slots_for(job)
                if slots > available:
                    continue
                available -= slots
                pending.remove((index, job))
                running[pool.submit(run, index, job, slots)] = slots
            done, _ = wait(running, return_when=FIRST_COMPLETED)
            for future in done:
                available += running.pop(future)
                result = future.result()
                print(f"\nRunning {result['name']} ({result['seconds']:.2f}초, exit={result['exit']})", flush=True)
                print(result.pop("output"), end="", flush=True)
                results.append(result)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--logs", type=Path, required=True)
    parser.add_argument("--workers", type=int, default=int(os.environ.get("AAA_RUST_TEST_WORKERS", "6")))
    args = parser.parse_args()
    if args.workers < 1:
        parser.error("workers는 1 이상이어야 한다")
    args.logs.mkdir(parents=True, exist_ok=True)
    args.logs = args.logs.resolve()
    started = time.monotonic()
    artifacts = prepare(args.logs)
    durations = json.loads(Path(__file__).with_name("rust-test-durations.json").read_text())
    jobs = plan(artifacts, args.workers, durations)
    # doctest를 binary 실행으로 대신하지 않는다. 기존 Cargo의 전체 workspace doc 범위를 그대로 실행한다.
    jobs.append(Job("doctests", ["cargo", "test", "--workspace", "--doc", "--locked", "--no-fail-fast"], Path.cwd()))
    # 여섯 m13 프로세스는 1+1+1+4+4+4=15자리다(16 CPU). 나머지는 CPU 예산 안에서 들어온다.
    threads = int(os.environ.get("RUST_TEST_THREADS", max(1, (os.cpu_count() or 1) // min(args.workers, 4))))
    if threads < 1:
        raise ValueError("RUST_TEST_THREADS는 1 이상이어야 한다")
    manifest = [dict(name=job.name, argv=job.argv, cwd=str(job.cwd), tests=job.tests) for job in jobs]
    (args.logs / "plan.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    results = execute(jobs, args.logs, args.workers, threads)
    summary = dict(seconds=time.monotonic() - started, workers=args.workers, threads=threads,
                   cpu_budget=max(1, os.cpu_count() or 1), jobs=results)
    (args.logs / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
    # ci-local annotation은 로그 꼬리를 고른다. 전체 출력 뒤 실패 원문 첫 줄을 다시 남겨
    # panic 위치만 보이고 what/diag가 잘리는 일을 막는다. 성공/종료 코드 판정은 그대로다.
    for job in results:
        if job["exit"]:
            print(f"FAIL {job['name']}: exit={job['exit']}, 로그 {job['log']}")
            for failure in job["failures"]:
                print(f"FAIL {job['name']} {failure['test']}: {failure['message']}")
    return int(any(result["exit"] for result in results))


if __name__ == "__main__":
    raise SystemExit(main())
