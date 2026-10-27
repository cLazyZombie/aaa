"""선택 계측 원문을 읽는다. 성능 판정은 ci-local보다 먼저 별도로 실행한다."""
import argparse
import json
import math
from pathlib import Path
import sys


def summarize(directory):
    cases = []
    for path in sorted(directory.glob("*.jsonl")):
        rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
        if not rows:
            raise ValueError(f"빈 계측 파일: {path.name}")
        name = rows[0]["case"]
        if any(row["case"] != name for row in rows):
            raise ValueError(f"서로 다른 시험이 섞인 계측: {path.name}")
        complete = rows[0]["event"] == "start" and rows[-1]["event"] == "end"
        success = complete and not rows[-1].get("panicking", False) and rows[-1].get("ok", True)
        phases, labels, spans, samples = {}, {}, {}, {}
        pending = set()
        for row in rows:
            event = row["event"]
            if event in ("phase", "end"):
                bucket, key = phases, row["category"]
                label = labels.setdefault(f"{key}:{row['label']}", {"count": 0, "ms": 0})
                label["count"] += 1
                label["ms"] += row["duration_ms"]
            elif event == "span":
                pending.discard(row["span"])
                bucket, key = spans, row["label"]
            elif event == "sample":
                bucket, key = samples, row["label"]
            else:
                if event == "span-start":
                    pending.add(row["span"])
                continue
            value = bucket.setdefault(key, {"count": 0, "ms": 0})
            value["count"] += 1
            value["ms"] += row["duration_ms"]
        cases.append(dict(file=path.name, case=name, complete=complete, ok=success,
                          wall_ms=rows[-1]["elapsed_ms"], phases=phases, phase_labels=labels,
                          inclusive_spans=spans, browser_samples=samples, unfinished_spans=len(pending)))
    return cases


def verdict(cases, required, max_seconds):
    errors = []
    for name in required:
        if not any(case["case"] == name for case in cases):
            errors.append(f"필수 시험 계측 누락: {name}")
    if not cases:
        errors.append("계측 결과가 없다")
    for case in cases:
        if not case["ok"]:
            errors.append(f"실패 또는 미완료: {case['case']} ({case['file']})")
        if max_seconds is not None and case["wall_ms"] >= max_seconds * 1000:
            errors.append(f"성능 상한 초과: {case['case']} {case['wall_ms'] / 1000:.3f}초")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--require", action="append", default=[], help="실제로 실행돼야 하는 원래 시험 이름")
    parser.add_argument("--max-seconds", type=float, help="준비·계약·정리를 모두 포함한 개별 상한")
    args = parser.parse_args()
    if args.max_seconds is not None and (not math.isfinite(args.max_seconds) or args.max_seconds <= 0):
        parser.error("--max-seconds는 양수다")
    try:
        cases = summarize(args.directory)
        errors = verdict(cases, args.require, args.max_seconds)
        print(json.dumps(dict(cases=cases, errors=errors, ok=not errors), ensure_ascii=False, indent=2))
        return bool(errors)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"계측 읽기 실패: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
