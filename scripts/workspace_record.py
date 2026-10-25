"""검증 종료를 Git 공용 폴더에 원자적으로 보관한다. aaa 없이도 실행된다.

rc·no2-engine·pt가 같은 파일을 사용한다. 기존 결과에서 집계 필드만 복사하며
로그·명령·경로·origin 원문은 보관하지 않는다. 수집 시 main 첫 부모 계보를 확인한다.
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
RUNNERS = {"rc": ("default-tests", "workspace-tests"),
           "engine": ("workspace-tests", "automation-tests", "2d-tests", "3d-tests"), "pt": ()}


def git(*args):
    return subprocess.check_output(["git", "-c", "gc.auto=0", *args], text=True,
                                   stderr=subprocess.PIPE, timeout=10).strip()


def remote_id(url):
    if "://" in url:
        parsed = urlsplit(url)
        if parsed.scheme not in ("https", "http", "ssh", "git"):
            raise ValueError("unsupported origin scheme")
        authority, path = parsed.netloc, parsed.path
    else:
        authority, path = url.split(":", 1)
        if "@" not in authority:
            raise ValueError("origin is not remote")
    host = authority.rsplit("@", 1)[-1].lower()
    path = path.strip("/").removesuffix(".git")
    if not host or not path or "?" in path or "#" in path:
        raise ValueError("invalid origin identity")
    return hashlib.sha256(f"{host}/{path}".encode()).hexdigest()


def snapshot():
    return dict(commit=git("rev-parse", "HEAD"), tree=git("rev-parse", "HEAD^{tree}"),
                remote=remote_id(git("remote", "get-url", "origin")),
                clean=not git("status", "--porcelain", "--untracked-files=no"))


def start(runner, mode, scope):
    if runner not in RUNNERS or mode not in ("default", "coverage") or scope not in ("desktop", "full"):
        raise ValueError("unsupported verification runner/mode/scope")
    # 새 출처 확인이 실패한 실행을 agent의 구형 mtime 추정으로 우회하지 않는다.
    marker = Path("target/verification/workspace-source.json")
    marker.parent.mkdir(parents=True, exist_ok=True)
    marker.write_text("{}\n")
    data = snapshot()
    data.update(runner=runner, mode=mode, scope=scope, started_at_ms=time.time_ns() // 1_000_000)
    path = Path("target/workspace-metrics") / (uuid.uuid4().hex + ".json")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data) + "\n")
    return str(path)


def read(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_INPUT:
        raise ValueError(f"invalid metrics input: {path.name}")
    return path.read_text()


def fresh(path, before):
    if not path.exists():
        return False
    at = path.stat().st_mtime_ns // 1_000_000
    return before["started_at_ms"] <= at <= before["ended_at_ms"]


def fields(value, names):
    return {key: value[key] for key in names if key in value}


def compact_tests(value):
    return [dict(target=Path(b.get("target", "binary")).name, exit=b.get("exit"),
                 tests=[fields(t, ("name", "event", "exec_time")) for t in b.get("tests", [])],
                 suites=[fields(s, ("passed", "failed", "ignored", "filtered_out")) for s in b.get("suites", [])])
            for b in value]


def optional_json(src, dst, before, transform):
    try:
        if fresh(src, before):
            dst.parent.mkdir(parents=True, exist_ok=True)
            dst.write_text(json.dumps(transform(json.loads(read(src)))))
            # agent의 기존 실행 범위 검사에 원본 시각을 보존한다.
            os.utime(dst, ns=(src.stat().st_atime_ns, src.stat().st_mtime_ns))
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"workspace optional summary skipped ({src.name}): {error}", file=sys.stderr)


def stages(before, source):
    runner = before["runner"]
    if runner == "pt":
        path = source.with_suffix(".stages")
        return read(path).splitlines() if path.exists() else []
    root = Path("target/verification")
    if runner == "rc":
        path = root / "summary.json"
        if not fresh(path, before):
            return []
        value = json.loads(read(path))
        if isinstance(value.get("cargoProfile"), str):
            before["profile"] = value["cargoProfile"]
        return [f'{g["name"]}|{"ok" if g["status"] == "passed" else "FAIL"}|0|0|{g["seconds"]}'
                for g in value["gates"] if g.get("seconds") is not None and g["status"] in ("passed", "failed")]
    rows = []
    for path in sorted((root / "gates").glob("*.status")):
        if fresh(path, before):
            key, name, code, seconds = read(path).strip().split("\t")
            rows.append(f'{name}|{"ok" if code == "0" else "FAIL"}|0|0|{seconds}')
    return rows


def finish(token, code):
    source = Path(token)
    if not re.fullmatch(r"[0-9a-f]{32}\.json", source.name) or source.parent != Path("target/workspace-metrics"):
        raise ValueError("invalid verification token")
    before = json.loads(read(source))
    after = snapshot()
    if not before["clean"] or not after["clean"] or any(before[k] != after[k] for k in after):
        raise ValueError("verification source changed or contains uncommitted changes")
    before.update(format=2, ended_at_ms=time.time_ns() // 1_000_000)
    common = Path(git("rev-parse", "--path-format=absolute", "--git-common-dir"))
    store = common / "aaa-workspace-runs"
    store.mkdir(exist_ok=True)
    try:
        rows = stages(before, source)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"workspace stage summary skipped: {error}", file=sys.stderr)
        rows = []
    seconds = (before["ended_at_ms"] - before["started_at_ms"]) / 1000
    outcome = "succeeded" if int(code) == 0 else "failed"
    with tempfile.TemporaryDirectory(prefix=".writing-", dir=store) as tmp:
        out = Path(tmp)
        (out / "workspace-summary.tsv").write_text(
            f'v1|{before["started_at_ms"] // 1000}|{seconds}|{outcome}\n' + "\n".join(rows) + "\n")
        for name in RUNNERS[before["runner"]]:
            optional_json(Path("target/verification") / name / "report.json",
                          out / "verification" / name / "report.json", before, compact_tests)
        e2e = {"rc": "target/rc-e2e/report.json", "engine": "target/no2-e2e/all-headless.json"}.get(before["runner"])
        if e2e:
            optional_json(Path(e2e), out / "e2e.json", before, lambda v: dict(
                status=v.get("status"), reports=[fields(r, ("scenario", "status", "duration_ms")) for r in v.get("reports", [])]))
        (out / "provenance.json").write_text(json.dumps(before) + "\n")
        os.rename(out, store / f'run-{before["started_at_ms"]}-{source.stem}')
    source.unlink()
    source.with_suffix(".stages").unlink(missing_ok=True)


if __name__ == "__main__":
    try:
        if sys.argv[1] == "start":
            print(start(*sys.argv[2:]))
        elif sys.argv[1] == "finish":
            finish(*sys.argv[2:])
        else:
            raise ValueError("expected start or finish")
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"workspace record failed: {error}", file=sys.stderr)
        sys.exit(1)
