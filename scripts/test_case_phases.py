"""부분 원문/미실행을 성공으로 세거나 보조 span을 전체에서 빼는 오류를 막는다."""
import json
from pathlib import Path
import tempfile
import unittest

from scripts.case_phases import summarize, verdict


class CasePhasesTest(unittest.TestCase):
    def test_overlapping_samples_never_reduce_wall_and_partial_or_missing_runs_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rows = [
                dict(event="start", category="prepare", label="start", elapsed_ms=0, duration_ms=0),
                dict(event="span-start", category="observe", label="body", span=1, elapsed_ms=10, duration_ms=0),
                dict(event="span", category="observe", label="body", span=1, elapsed_ms=2000, duration_ms=1990),
                dict(event="sample", category="contract", label="frame", elapsed_ms=2000, duration_ms=1000),
                dict(event="phase", category="prepare", label="fixture", elapsed_ms=3000, duration_ms=3000),
                dict(event="end", category="cleanup", label="files", elapsed_ms=5100, duration_ms=2100, ok=True),
            ]
            def put(name, source):
                (root / name).write_text("\n".join(json.dumps(dict(case="owner", **row)) for row in source), encoding="utf-8")
            put("complete.jsonl", rows)
            cases = summarize(root)
            self.assertEqual(cases[0]["wall_ms"], 5100)
            self.assertEqual(sum(x["ms"] for x in cases[0]["phases"].values()), 5100)
            self.assertEqual(cases[0]["inclusive_spans"]["body"]["ms"], 1990)
            self.assertTrue(verdict(cases, ["owner"], 5))
            self.assertFalse(verdict(cases, ["owner"], None))
            put("partial.jsonl", rows[:-1])
            cases = summarize(root)
            self.assertEqual(len(cases), 2, "원래 실패 회차를 다음 성공으로 덮지 않는다")
            self.assertTrue(verdict(cases, ["owner"], None))
            self.assertTrue(verdict(cases[:1], ["not-executed"], None))
            self.assertTrue(verdict([], [], None))


if __name__ == "__main__":
    unittest.main()
