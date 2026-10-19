// 보내는 글의 보이지 않는 문자(src/hiddenChars.ts, plan §8.266): t3code `promptLinks.test.ts` sanitizeLinkedPrompt 시험을 친 글 규칙으로.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { stripHidden } from "../src/hiddenChars.ts";

describe("보이지 않는 문자 빼기", () => {
  it("zero-width·bidi 제어·tag 문자를 빼고 센다", () => {
    const tag = String.fromCodePoint(0xe0041, 0xe0042);
    assert.deepEqual(stripHidden(`고쳐 줘​‮abc‬${tag}`), { text: "고쳐 줘abc", removed: 5 });
  });
  it("ESC 같은 제어 문자도 빼지만 탭·줄바꿈·CR과 앞뒤 공백은 그대로다", () => {
    assert.deepEqual(stripHidden(" a\tb\r\nc\x1b[31m \n"), { text: " a\tb\r\nc[31m \n", removed: 1 });
  });
  it("보통 한글·영문·그림 문자는 그대로다", () => {
    for (const text of ["안녕하세요 commit and push", "❤️ 좋아요", "👩‍💻 개발", "👍🏽", "é"]) assert.deepEqual(stripHidden(text), { text, removed: 0 });
  });
  it("그림 문자 사이가 아닌 ZWJ는 뺀다", () => {
    assert.deepEqual(stripHidden("a‍b 👩‍"), { text: "ab 👩", removed: 2 });
  });
  it("variation selector는 그림 표시(U+FE0F)만 남긴다", () => {
    assert.deepEqual(stripHidden("x︀y️"), { text: "xy️", removed: 1 });
  });
});
