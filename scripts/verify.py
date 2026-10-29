#!/usr/bin/env python3
"""개발의 관련 검사와 착지 gate를 구분한다. 기본 ci-local 전체 gate는 바꾸지 않는다."""

import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parent.parent


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def changed_paths(base):
    # merge-base로 줄이지 않는다. 착지 시작점 이후의 통합 변경 전부와 미커밋·새 파일을 본다.
    revision = git("rev-parse", "--verify", base + "^{commit}").decode().strip()
    subprocess.run(["git", "merge-base", "--is-ancestor", revision, "HEAD"], cwd=ROOT, check=True)
    tracked = git("diff", "--name-only", "--no-renames", "-z", revision, "--")
    untracked = git("ls-files", "--others", "--exclude-standard", "-z")
    return revision, sorted({os.fsdecode(p) for p in (tracked + untracked).split(b"\0") if p})


def scope_for(paths):
    def doc(p):
        return p.startswith("docs/") and p.endswith(".md")

    def web(p):
        # 의존성·번들러·proto 생성·harness·CI·공유 fixture는 축소 대상으로 보지 않는다.
        return p.startswith(("web/src/", "web/public/", "web/test/", "web/e2e/specs/"))

    if paths and all(doc(p) for p in paths):
        return "docs"
    if paths and any(web(p) for p in paths) and all(web(p) or doc(p) for p in paths):
        return "web"
    return "full"


def commands(phase, scope, specs, packages):
    if phase == "landing":
        if specs or packages:
            raise ValueError("착지 gate에는 --spec/--package로 검사를 줄일 수 없다")
        if scope == "docs":
            return [["bash", "scripts/ci-local.sh", "--only", "public"]]
        if scope == "web":
            return [["bash", "scripts/ci-local.sh", "--only", "web,e2e,public"]]
        return [["bash", "scripts/ci-local.sh"]]
    if packages:
        if any(not p.startswith("aaa") or not all(c.isalnum() or c == "-" for c in p) for p in packages):
            raise ValueError("--package에는 aaa workspace crate 이름을 쓴다")
        selected = [arg for p in packages for arg in ("-p", p)]
        result = [["cargo", "test", "--profile", "verify", "--locked", *selected],
                  ["cargo", "clippy", "--profile", "verify", "--locked", *selected,
                   "--all-targets", "--", "-D", "warnings"]]
        if specs:
            result.append(["bash", "scripts/ci-local.sh", "--only", "web,e2e,public", "--spec", specs])
        else:
            result.append(["bash", "scripts/check-public.sh"])
        return result
    if scope == "docs":
        return [["bash", "scripts/ci-local.sh", "--only", "public"]]
    if scope == "web":
        result = ["bash", "scripts/ci-local.sh", "--only", "web,e2e,public"]
        if specs:
            result += ["--spec", specs]
        return [result]
    raise ValueError("개발은 관련 owner를 --package로 지정하거나 ci-local --only로 검사한다. 전체 gate는 착지에서 실행한다")


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("develop", "landing"))
    parser.add_argument("--base", required=True, help="개발/착지를 시작한 기준 commit 또는 ref")
    parser.add_argument("--spec", help="개발 중 관련 E2E 번호, 쉼표 구분; 생략하면 웹 전체")
    parser.add_argument("--package", action="append", default=[], help="개발 중 관련 Rust crate; 반복 가능")
    parser.add_argument("--logs", type=Path)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    try:
        base, paths = changed_paths(args.base)
        scope = scope_for(paths)
        plan = commands(args.phase, scope, args.spec, args.package)
        head = git("rev-parse", "HEAD").decode().strip()
        if args.phase == "landing" and git("status", "--porcelain"):
            raise ValueError("착지는 변경을 커밋한 뒤 최종 통합 SHA에서 실행한다")
        if args.phase == "landing" and os.environ.get("AAA_E2E_ONLY"):
            raise ValueError("착지에서는 AAA_E2E_ONLY로 E2E를 줄일 수 없다")
        data = dict(phase=args.phase, scope=scope, base=base, commit=head, paths=paths, commands=plan)
        print(json.dumps(data, ensure_ascii=False, indent=2), flush=True)
        if args.dry_run:
            return 0
        logs = args.logs or ROOT / "target/verification" / f"{args.phase}-{time.time_ns()}"
        logs.mkdir(parents=True, exist_ok=False)
        (logs / "plan.json").write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
        started = time.monotonic()
        code = 0
        for index, command in enumerate(plan):
            if command[:2] == ["bash", "scripts/ci-local.sh"]:
                command = [*command, "--logs", str(logs.resolve() / str(index)), "--tee"]
            print("실행: " + shlex.join(command), flush=True)
            code = subprocess.run(command, cwd=ROOT).returncode
            if code:
                break
        if args.phase == "landing" and (git("rev-parse", "HEAD").decode().strip() != head
                                         or git("status", "--porcelain")):
            print("착지 검사 중 HEAD 또는 작업 파일이 바뀌었다", file=sys.stderr)
            code = 1
        data.update(exit_code=code, elapsed_s=round(time.monotonic() - started, 3))
        (logs / "result.json").write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
        return code
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"검증 계획 실패: {error}\n")


if __name__ == "__main__":
    sys.exit(main())
