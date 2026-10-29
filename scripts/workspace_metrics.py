"""CI가 이미 만든 종료 요약을 worktree 정리 뒤에도 회수할 수 있게 보관한다.

검증을 실행하지 않는다. 원문 경로·origin·환경·시험 출력은 원장에 복사하지 않는다.
"""

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit
import uuid

MAX_INPUT = 8 * 1024 * 1024
MAX_RUST_JOBS = 4096  # 모듈/독립 owner 분할 결과도 보존한다. 원문 입력의 8 MiB 상한은 별도다.


def git(*args):
    result = subprocess.run(["git", "-c", "gc.auto=0", *args], capture_output=True,
                            text=True, check=True, timeout=10)
    return result.stdout.strip()


def remote_id(url):
    if "://" in url:
        parsed = urlsplit(url)
        if parsed.scheme not in ("https", "http", "ssh", "git"):
            raise ValueError("지원하지 않는 origin 형식")
        authority, path = parsed.netloc, parsed.path
    else:
        authority, path = url.split(":", 1)
        if "@" not in authority:
            raise ValueError("원격 origin 아님")
    host = authority.rsplit("@", 1)[-1].lower()
    path = path.strip("/").removesuffix(".git")
    if not host or not path or "?" in path or "#" in path:
        raise ValueError("origin 신원 미확인")
    return hashlib.sha256(f"{host}/{path}".encode()).hexdigest()


def snapshot():
    return dict(commit=git("rev-parse", "HEAD"), tree=git("rev-parse", "HEAD^{tree}"),
                remote=remote_id(git("remote", "get-url", "origin")),
                clean=not git("status", "--porcelain", "--untracked-files=no"))


def read(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_INPUT:
        raise ValueError(f"지표 입력 크기 상한 또는 일반 파일 아님: {path.name}")
    return path.read_text()


def start(logs):
    # 같은 --logs를 재사용해도 예전 시작 자료를 새 실행에 붙이지 않는다.
    source = logs / "workspace-source.json"
    source.write_text("{}\n")
    data = snapshot()
    data["started_at_ms"] = time.time_ns() // 1_000_000
    data["run_id"] = uuid.uuid4().hex
    source.write_text(json.dumps(data) + "\n")


def finish(logs):
    before = json.loads(read(logs / "workspace-source.json"))
    if not re.fullmatch(r"[a-f0-9]{32}", before.get("run_id", "")) or not isinstance(before.get("started_at_ms"), int):
        raise ValueError("CI 시작 출처 형식 오류")
    after = snapshot()
    if not before["clean"] or not after["clean"] or any(before[k] != after[k] for k in after):
        raise ValueError("실행 중 커밋·origin 변경 또는 미커밋 변경으로 출처 미확인")
    before.update(format=1, ended_at_ms=time.time_ns() // 1_000_000)
    binary = Path("target/debug/aaa")
    evidence = logs / "e2e-binary.log"
    if (evidence.is_file() and "이번 빌드의 웹 E2E 실행 사본:" in read(evidence)
            and binary.is_file() and binary.stat().st_mtime_ns // 1_000_000 <= before["ended_at_ms"]):
        before["artifact_bytes"] = binary.stat().st_size
    common = Path(git("rev-parse", "--path-format=absolute", "--git-common-dir"))
    store = common / "aaa-workspace-runs"
    store.mkdir(exist_ok=True)
    # 임시 폴더는 수집 목록에서 제외하며 완성 뒤 원자적으로 공개한다.
    with tempfile.TemporaryDirectory(prefix=".writing-", dir=store) as tmp:
        out = Path(tmp)
        rows = read(logs / "workspace-summary.tsv").splitlines()
        # 여섯째 칸의 원본 로그 절대 경로는 parser에 필요하지 않다.
        (out / "workspace-summary.tsv").write_text("\n".join(
            "|".join(row.split("|")[:5]) for row in rows) + "\n")
        rust = logs / "rust/summary.json"
        if rust.is_file():
            try:
                jobs = json.loads(read(rust))["jobs"]
                if len(jobs) > MAX_RUST_JOBS:
                    raise ValueError("Rust 결과 개수 상한")
                (out / "rust").mkdir()
                compact = []
                for job in jobs:
                    name = job["log"]
                    if not isinstance(name, str) or name.startswith(".") or "/" in name or "\\" in name:
                        raise ValueError("Rust 로그 이름 형식 오류")
                    compact.append(dict(log=name))
                    # 개별 stdout 대신 libtest가 이미 출력한 개수 요약만 남긴다.
                    try:
                        text = read(logs / "rust" / name)
                        (out / "rust" / name).write_text("\n".join(
                            line for line in text.splitlines() if line.startswith("test result: ")) + "\n")
                    except (OSError, ValueError) as error:
                        print(f"workspace Rust 부속 요약 미수집: {error}", file=sys.stderr)
                (out / "rust/summary.json").write_text(json.dumps(dict(jobs=compact)))
            except (OSError, ValueError, KeyError, TypeError) as error:
                print(f"workspace Rust 요약 미수집: {error}", file=sys.stderr)
        e2e = logs / "e2e.log"
        if e2e.is_file():
            try:
                text = read(e2e)
                # runner의 집계 행만 보존하며 터미널·오류 원문·주소는 복사하지 않는다.
                pattern = r"^## (?:요약: 파일 \d+개 × \d+번.*|작업: \d+개.*|  [\w-]+\s+(?:통과|실패)\s+\d+/\d+\s+[\d.]+초(?: \(독립 작업 합계\))?\s*)$"
                (out / "e2e.log").write_text("\n".join(
                    line for line in text.splitlines() if re.fullmatch(pattern, line)) + "\n")
            except (OSError, ValueError) as error:
                print(f"workspace E2E 요약 미수집: {error}", file=sys.stderr)
        (out / "provenance.json").write_text(json.dumps(before) + "\n")
        os.rename(out, store / f"run-{before['started_at_ms']}-{before['run_id']}")
        # TemporaryDirectory는 rename된 경로의 존재를 요구하지 않는다.


if __name__ == "__main__":
    try:
        {"start": start, "finish": finish}[sys.argv[1]](Path(sys.argv[2]))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"workspace {sys.argv[1]} 요약 보관 실패: {error}", file=sys.stderr)
        sys.exit(1)
