import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { splitAppIds, taskTarget, appIdHash } from "../src/appIds.ts";
import { parseMarkdown } from "../src/herdr/markdown.ts";
const exists = ({ target }) => new Set(["t-0001", "d-0002", "q-1000", "r-0003", "d-10000"]).has(target);
const ref = (kind, target) => ({ kind, target });
describe("대화·보고서의 짧은 ID", () => {
  it("존재하는 네 종류를 원문 그대로 나누며 조사·괄호·문장 끝을 보존한다", () => {
    assert.deepEqual(splitAppIds("t-0001과 (d-0002), q-1000에 답한 r-0003. d-9999", exists), [ref("thread", "t-0001"), "과 (", ref("task", "d-0002"), "), ", ref("question", "q-1000"), "에 답한 ", ref("report", "r-0003"), ". d-9999"]);
    assert.deepEqual(splitAppIds("d-10000", exists), [ref("task", "d-10000")]);
    assert.deepEqual(splitAppIds("d-0002", () => false), ["d-0002"]);
  });
  it("다른 낱말·주소·경로·파일 이름에 든 ID를 링크하지 않는다", () => {
    for (const text of ["", "d-002", "xd-0002", "d-0002x", "d-00020", "foo_d-0002", "https://example.ts.net/d-0002", "#task=d-0002", "#thread=t-0001", "demo/d-0002", "./d-0002", "d-0002/file", "d-0002.md", "mail@t-0001", "%2Fd-0002"])
      assert.deepEqual(splitAppIds(text, exists), [text], text);
  });
  it("Markdown 파서가 URL·명시 링크·코드 블록을 텍스트와 구분한다", () => {
    const blocks = parseMarkdown("d-0002\n\n```text\nd-0002\n```\n\nhttps://example.ts.net/d-0002 [d-0002](https://example.ts.net/)\n");
    assert.equal(blocks[0].type, "paragraph");
    assert.equal(blocks[1].type, "code");
    assert.deepEqual(blocks[2].lines[0].filter(n => n.type === "link").map(n => n.href), ["https://example.ts.net/d-0002", "https://example.ts.net/"]);
  });
});
describe("할 일 주소", () => {
  it("프로젝트 없는 주소·프로젝트 있는 주소와 5자리 번호를 받는다", () => {
    assert.deepEqual(taskTarget("d-0002"), { project: null, id: "d-0002" });
    assert.deepEqual(taskTarget("demo/d-10000"), { project: "demo", id: "d-10000" });
    assert.equal(appIdHash(ref("task", "d-0002")), "#task=d-0002");
    assert.equal(appIdHash(ref("report", "r-0003")), "#report=r-0003");
    for (const value of ["", "d-2", "demo/other/d-0002", "/d-0002", "demo/t-0001", "d-0002x"])
      assert.equal(taskTarget(value), null, value);
  });
});
