import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { splitAppIds, taskTarget, appIdHash, canonicalShortId, normalizeAppTarget, uniqueAppTargets } from "../src/appIds.ts";
import { parseMarkdown } from "../src/herdr/markdown.ts";
const exists = ({ target }) => new Set(["t-0001", "d-0002", "q-1000", "r-0003", "d-10000"]).has(target);
const ref = (kind, target) => ({ kind, target });
describe("대화·보고서의 짧은 ID", () => {
  it("존재하는 네 종류를 원문 그대로 나누며 조사·괄호·문장 끝을 보존한다", () => {
    assert.deepEqual(splitAppIds("t-0001과 (d-0002), q-1000에 답한 r-0003. d-9999", exists), [ref("thread", "t-0001"), "과 (", ref("task", "d-0002"), "), ", ref("question", "q-1000"), "에 답한 ", ref("report", "r-0003"), ". d-9999"]);
    assert.deepEqual(splitAppIds("d-10000", exists), [ref("task", "d-10000")]);
    assert.deepEqual(splitAppIds("d-0002", () => false), ["d-0002"]);
  });
  it("맨 q-ID가 문장·괄호·붙은 구분자 안에서도 각각 인식된다", () => {
    for (const separator of ["·", ",", ", ", " 및 ", "\n"]) {
      assert.deepEqual(splitAppIds(`질문(q-1000${separator}q-1001)에 답한다.`, () => true),
        ["질문(", ref("question", "q-1000"), separator, ref("question", "q-1001"), ")에 답한다."]);
    }
  });
  it("다른 낱말·주소·경로·파일 이름에 든 ID를 링크하지 않는다", () => {
    for (const text of ["", "d-0", "xd-0002", "d-0002x", "d-00020", "foo_d-0002", "https://example.ts.net/d-0002", "#task=d-0002", "#thread=t-0001", "demo/d-0002", "./d-0002", "d-0002/file", "d-0002.md", "mail@t-0001", "%2Fd-0002"])
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
    for (const value of ["", "d-0", "demo/other/d-0002", "/d-0002", "demo/t-0001", "d-0002x"])
      assert.equal(taskTarget(value), null, value);
  });
});

describe("숫자로 같은 ID", () => {
  it("짧게 쓰거나 0을 더 붙여도 같은 정본에 연결하며 원문 표시는 보존한다", () => {
    for (const [kind, prefix, n] of [["thread", "t", "965"], ["question", "q", "1295"], ["report", "r", "10000"], ["automation", "a", "10000"]]) {
      for (const digits of [n, `0${n}`, `00${n}`]) {
        const text = `${prefix}-${digits}`, target = `${prefix}-${n.padStart(4, "0")}`;
        assert.deepEqual(splitAppIds(text, id => id.target === target), [{ kind, target, ...(text === target ? {} : { label: text }) }]);
      }
    }
    assert.deepEqual(taskTarget("other/d-00002"), { project: "other", id: "d-0002" });
    assert.equal(normalizeAppTarget("other/t-965", "t"), "other/t-0965");
    assert.equal(normalizeAppTarget("other/nested/t-965", "t"), "other/nested/t-965");
    assert.equal(normalizeAppTarget("other/d-965", "t"), "other/d-965");
  });
  it("큰 번호의 정밀도·0·범위 초과·무작위 ID를 구별한다", () => {
    assert.equal(canonicalShortId("r-9007199254740993", "r"), "r-9007199254740993");
    assert.equal(canonicalShortId("p-10000", "p"), "p-00010000");
    for (const text of ["q-0", "q-0000", "q-+1", "q-１", "q-9223372036854775808", "q-12abc34567"])
      assert.equal(canonicalShortId(text, "q"), null);
    for (const text of ["t-965x", "xq-1295", "r-10000.md", "other/t-965", "https://example.ts.net/q-01295", "#question=q-01295"])
      assert.deepEqual(splitAppIds(text, () => true), [text]);
  });
  it("다른 프로젝트의 같은 번호는 명시한 프로젝트로만 고른다", () => {
    const rows = [{ project: "first", id: "r-10000" }, { project: "second", id: "r-10000" }, { project: "second", id: "r-9999" }];
    const ids = uniqueAppTargets(rows, r => r.project, r => r.id);
    assert.equal(ids.has("r-10000"), false);
    assert.equal(ids.get("first/r-10000"), rows[0]);
    assert.equal(ids.get("second/r-10000"), rows[1]);
    assert.equal(ids.get("r-9999"), rows[2]);
  });
});
