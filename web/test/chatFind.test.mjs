import { test } from "node:test";
import assert from "node:assert/strict";
import { findOccurrences, foldFindCase, formatFindCount, indexOf, markdownFindSegments, matchAt, startIndex, stepFindIndex } from "../src/chatFindText.ts";

test("대소문자를 접어 겹치지 않게 찾고, 길이가 바뀌는 글자는 offset을 지킨다(참고 threadSearch)", () => {
  assert.deepEqual(findOccurrences("ZQ-needle zq-NEEDLE", "zq-needle"), [0, 10]);
  assert.deepEqual(findOccurrences("aaaa", "aa"), [0, 2]);
  assert.deepEqual(findOccurrences("빌드 오류와 빌드 오류", "빌드 오류"), [0, 7]);
  assert.equal(foldFindCase("İx").length, 2);
  assert.deepEqual(findOccurrences("ΣΟΦΟΣ", "σοφος"), [0]);
  assert.deepEqual(findOccurrences("", "a"), []);
});

test("차례는 끝에서 처음으로 돌고 개수는 n/전체다(참고 threadFind)", () => {
  assert.equal(stepFindIndex(6, 7, 1), 0);
  assert.equal(stepFindIndex(0, 7, -1), 6);
  assert.equal(stepFindIndex(99, 7, 1), 0);
  assert.equal(stepFindIndex(0, 0, 1), 0);
  assert.equal(formatFindCount(2, 7), "3/7");
  assert.equal(formatFindCount(0, 0), "0/0");
});

test("그린 글처럼 뽑는다: 굵은 글·code는 잇고, 줄·목록·표 칸·code 블록은 끊는다", () => {
  const md = "답 **빌드 오류**는 `cargo test`에서\n둘째 줄\n\n- 원인: [링크 글](https://x.y)\n  - 안쪽\n\n| a | b |\n|---|---|\n| c | **d**e |\n\n```rust\nfn main() {}\n```\n\n\\[x\\]";
  assert.deepEqual(markdownFindSegments(md, true), ["답 빌드 오류는 cargo test에서", "둘째 줄", "원인: 링크 글", "안쪽", "a", "b", "c", "de", "fn main() {}", "\\[x\\]"]);
});

test("파일 이름은 열 곳이 없으면 뒤에 경로가 붙고, file:// code는 열 곳이 있으면 경로로 그려진다", () => {
  assert.deepEqual(markdownFindSegments("보라 [이름](docs/x.md)", false), ["보라 이름 (docs/x.md)"]);
  assert.deepEqual(markdownFindSegments("보라 [이름](docs/x.md) `file:///tmp/a.txt`", true), ["보라 이름 /tmp/a.txt"]);
});

test("접힌 code는 보이는 머리와 가린 꼬리가 따로다(찾는 말은 줄을 넘지 않는다)", () => {
  const lines = Array.from({ length: 40 }, (_, i) => `줄 ${i}`);
  const [head, tail] = markdownFindSegments(["```", ...lines, "```"].join("\n"), true);
  assert.equal(head, lines.slice(0, 20).join("\n"));
  assert.equal(tail, lines.slice(20).join("\n"));
});

test("줄별 개수에서 자리와 차례, 보이는 곳부터 시작", () => {
  const rows = [{ id: "a", count: 2 }, { id: "b", count: 0 }, { id: "c", count: 3 }];
  assert.deepEqual(matchAt(rows, 3), { rowId: "c", occurrence: 1 });
  assert.equal(matchAt(rows, 5), null);
  assert.equal(indexOf(rows, { rowId: "c", occurrence: 9 }), 4);
  assert.equal(indexOf(rows, { rowId: "b", occurrence: 0 }), null);
  assert.equal(indexOf(rows, { rowId: "z", occurrence: 0 }), null);
  assert.equal(startIndex(rows, { rowId: "b", passed: 0 }), 2);
  assert.equal(startIndex(rows, { rowId: "a", passed: 1 }), 1);
  assert.equal(startIndex(rows, { rowId: "c", passed: 3 }), 0);
  assert.equal(startIndex(rows, null), 0);
});
