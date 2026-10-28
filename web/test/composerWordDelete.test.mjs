import assert from "node:assert/strict";
import { test } from "node:test";
import { wordDeleteRange } from "../src/composerWordDelete.ts";

test("단어 삭제: 한글·영문·문장부호·공백·줄바꿈의 공백 경계", () => {
  for (const [value, expected] of [
    ["안녕 world", "안녕 "], ["hello 세계", "hello "], ["한글English", ""],
    ["hello foo/bar.txt!", "hello "], ["hello world   ", "hello "],
    ["hello\t세계\n  ", "hello\t"], ["hello\u00a0세계", "hello\u00a0"],
    ["hello 👋🏽", "hello "], ["   \n\t", ""], ["", ""],
  ]) {
    const [start, end] = wordDeleteRange(value, value.length, value.length);
    assert.equal(value.slice(0, start) + value.slice(end), expected, JSON.stringify(value));
  }
});

test("단어 삭제: 중간 커서·선택·맨 앞은 뒤쪽 글을 보존", () => {
  assert.deepEqual(wordDeleteRange("hello 세계 next", 8, 8), [6, 8]);
  assert.deepEqual(wordDeleteRange("hello 세계 next", 2, 8), [2, 8]);
  assert.deepEqual(wordDeleteRange("hello", 0, 0), [0, 0]);
});
