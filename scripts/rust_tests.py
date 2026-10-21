"""Cargo가 준비한 모든 시험 binary를 격리된 묶음으로 실행한다. doctest도 별도로 유지한다."""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
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


def plan(artifacts, workers):
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
    # 독립 프로세스로 긴 m13을 먼저 시작한다. 다른 binary는 빈 자리부터 이어 실행한다.
    jobs.sort(key=lambda job: not job.name.startswith("m13-"))
    return jobs


def execute(jobs, logs, workers, threads, cpu_budget=None):
    cpu_budget = max(1, os.cpu_count() or 1) if cpu_budget is None else cpu_budget
    if cpu_budget < 1:
        raise ValueError("CPU 예산은 1 이상이어야 한다")
    available = cpu_budget
    changed = threading.Condition()
    epoch = time.monotonic()

    def run(index, job, submitted):
        nonlocal available
        # exact 한 시험뿐인 rollout이 libtest 기본 thread 수만큼 자리를 예약하지 않는다.
        slots = min(cpu_budget, threads, len(job.tests) if job.tests is not None else threads)
        slots = max(1, slots)
        with changed:
            changed.wait_for(lambda: available >= slots)
            available -= slots
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
            with changed:
                available += slots
                changed.notify_all()
        text = log.read_text(errors="replace")
        if job.tests is not None:
            match = re.search(r"^running (\d+) tests?$", text, re.MULTILINE)
            if not match or int(match[1]) != len(job.tests):
                text += "\n시험 실행 수가 분할 목록과 다르다\n"
                code = 1
                log.write_text(text)
        return dict(name=job.name, exit=code, seconds=finished - started,
                    start_seconds=started - epoch, queue_seconds=started - submitted,
                    cpu_slots=slots, log=log.name, output=text)

    results = []
    with ThreadPoolExecutor(max_workers=workers) as pool:
        pending = [pool.submit(run, index, job, time.monotonic()) for index, job in enumerate(jobs)]
        for future in as_completed(pending):
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
    jobs = plan(artifacts, args.workers)
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
    return int(any(result["exit"] for result in results))


if __name__ == "__main__":
    raise SystemExit(main())
