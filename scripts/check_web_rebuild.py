#!/usr/bin/env python3
"""실제 웹 빌드와 Cargo로 동일 출력 생략·변경 반영을 검사한다(다른 빌드와 함께 실행하지 않는다)."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import uuid


ROOT = Path(__file__).resolve().parent.parent


def snapshot():
    root = ROOT / "web/dist"
    return {
        path.relative_to(root).as_posix(): (
            path.stat().st_mtime_ns,
            hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None,
        )
        for path in [root, *sorted(root.rglob("*"))]
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--logs", type=Path, default=ROOT / "target/web-rebuild-check")
    args = parser.parse_args()
    args.logs.mkdir(parents=True, exist_ok=True)
    npm = shutil.which("npm")
    if not npm:
        parser.error("npm 실행 파일이 필요하다")

    def run(label, command, cwd=ROOT):
        started = time.monotonic()
        output = args.logs / f"{label}.stdout.log"
        errors = args.logs / f"{label}.stderr.log"
        with output.open("w", encoding="utf-8") as stdout, errors.open("w", encoding="utf-8") as stderr:
            result = subprocess.run(command, cwd=cwd, stdout=stdout, stderr=stderr, check=False)
        print(f"{label}: exit={result.returncode}, {time.monotonic() - started:.2f}초", flush=True)
        if result.returncode:
            raise RuntimeError(f"{label} 실패: exit={result.returncode}, stdout={output}, stderr={errors}")
        return output

    def web(label):
        run(f"{label}-web", [npm, "run", "build"], ROOT / "web")

    def cargo(label, expected):
        output = run(f"{label}-cargo", ["cargo", "build", "--locked", "-p", "aaa", "--bin", "aaa", "--message-format=json"])
        fresh = {}
        for line in output.read_text(encoding="utf-8").splitlines():
            event = json.loads(line)
            if event.get("reason") != "compiler-artifact":
                continue
            name = event["target"]["name"].replace("_", "-")
            if name in ("aaa", "aaa-host"):
                fresh[name] = event["fresh"]
        print(f"{label}: fresh={fresh}", flush=True)
        if set(fresh) != {"aaa", "aaa-host"} or (expected is not None and any(value != expected for value in fresh.values())):
            raise AssertionError(f"{label}: 기대 fresh={expected}, 실제={fresh}, 로그={output}")

    def embedded(label):
        run(f"{label}-embedded", ["cargo", "test", "--locked", "-p", "aaa-host", "--lib",
                                  "web::assets::tests::embedded_assets_restore_exact_build_input", "--", "--exact"])

    probe = ROOT / "web/public" / f"aaa-rebuild-check-{uuid.uuid4().hex}.txt"
    try:
        web("warm")
        cargo("warm", None)
        before = snapshot()
        web("same")
        if snapshot() != before:
            raise AssertionError("같은 웹 빌드가 dist의 내용이나 파일·폴더 mtime을 바꿨다")
        cargo("same", True)

        probe.write_text("first bytes", encoding="utf-8")
        web("added")
        cargo("added", False)
        stamp = probe.stat()
        probe.write_text("after bytes", encoding="utf-8")
        os.utime(probe, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
        web("changed")
        cargo("changed", False)
        if (ROOT / "web/dist" / probe.name).read_text(encoding="utf-8") != "after bytes":
            raise AssertionError("같은 길이·mtime인 웹 파일 변경을 빠뜨렸다")
        embedded("changed")

        # test 호출의 feature/profile 차이가 삭제 감지의 증거를 대신하지 않도록 다시 기준을 만든다.
        cargo("before-delete-warm", None)
        cargo("before-delete-fresh", True)
        probe.unlink()
        web("deleted")
        cargo("deleted", False)
        if (ROOT / "web/dist" / probe.name).exists():
            raise AssertionError("삭제한 웹 파일이 dist에 남았다")
        embedded("deleted")
        print("동일 출력 생략·추가·같은 길이/mtime 변경·삭제·내장 bytes 대조 통과", flush=True)
    finally:
        probe.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
