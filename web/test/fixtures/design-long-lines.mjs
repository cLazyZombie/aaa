// 동기 정규식의 무한 되돌기는 부모의 자식 프로세스 시한으로 막는다.
import assert from "node:assert/strict";
import { calloutOf, mdBlocks, sectionsOf } from "../../src/designModel.ts";

const spaces = " ".repeat(200_000);
assert.deepEqual(calloutOf(`> [!계획]${spaces}x`, "s"), {
  kind: "plan", task: null, title: "x", body: "x", section: "s", image: false,
});
const imageBody = `${"![".repeat(100_000)}](a)`;
assert.deepEqual(calloutOf(`> [!계획] ${imageBody}`, "s"), {
  kind: "plan", task: null, title: "![".repeat(40), body: imageBody, section: "s", image: true,
});
assert.deepEqual(sectionsOf([`# a${spaces}x`], "s"), [`a${spaces}x`]);
assert.deepEqual(mdBlocks(`> [!계획]${spaces}x\r\n> 끝`), [`> [!계획]${spaces}x\n> 끝`]);
console.log("긴 줄 4개 해석 확인");
