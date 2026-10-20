// 「답할 것」 모델(src/asksModel.ts, plan §8.337): 질문 본문이 여는 파일 주소는 그 질문의 `library/…`·`report.md`뿐이고, 주소의
// 질문·답할 것 열쇠는 답한 뒤에도 그때 남긴 기록으로 이어진다(새 보고 뒤의 칩·「답함으로 표시」 뒤의 카드).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { askFileUrl, choicePicture, doneOf, openAsks, questionKey } from "../src/asksModel.ts";

const BASE = "/api/v1/projects/demo/questions/t-1/abc/";
const thread = (id, questions) => ({ id, title: id, updated_at_ms: 1, questions });
const open = { report_hash: "abc", items: [{ text: "자리", choices: [] }], answer: null };
const rec = (id, kind, subject, done_ms) => ({ id, kind, subject, title: subject, asked_ms: 0, done_ms, by: "Mac · Chrome", summary: "1A", items: [] });

describe("「답할 것」 파일 주소", () => {
  it("그 질문의 library와 보고 사본만 연다", () => {
    assert.equal(askFileUrl(BASE, "library/a b.png"), `${BASE}library/a%20b.png`);
    assert.equal(askFileUrl(BASE, "./library/시안/a.png?x=1#y"), `${BASE}library/%EC%8B%9C%EC%95%88/a.png`);
    assert.equal(askFileUrl(BASE, "report.md"), `${BASE}report.md`);
    for (const bad of ["../secret.png", "library/../brief.md", "library/.env", "library/%2e%2e/x", "src/app.ts", "https://example.ts.net/a.png", "/etc/passwd", "library/"])
      assert.equal(askFileUrl(BASE, bad), null, bad);
    assert.equal(askFileUrl(null, "library/a.png"), null);
  });

  it("선택지 본문 맨 앞 그림만 큰 그림이다", () => {
    assert.deepEqual(choicePicture("![B안](library/b.png)\n더 쓴 글"), { alt: "B안", src: "library/b.png", rest: "더 쓴 글" });
    assert.equal(choicePicture("글 다음 ![B안](library/b.png)"), null);
  });
});

describe("「답할 것」 열쇠", () => {
  it("질문 주소는 열린 질문, 답했으면 마지막 답한 기록, 둘 다 없으면 없음", () => {
    const done = [rec("old", "question", "t-1", 1), rec("new", "question", "t-1", 5), rec("x", "request", "t-1", 9)];
    assert.equal(questionKey("t-1", [thread("t-1", open)], done), "q:t-1");
    assert.equal(questionKey("t-1", [thread("t-1", null)], done), "d:new");
    assert.equal(questionKey("t-2", [], done), null);
  });

  it("답해서 목록에서 빠진 것은 그 종류·대상의 기록으로 이어진다", () => {
    const done = [rec("q1", "question", "t-1", 1), rec("r1", "request", "item-7", 2), rec("k1", "task", "로그인", 3)];
    assert.equal(doneOf("q:t-1", done)?.id, "q1");
    assert.equal(doneOf("r:item-7", done)?.id, "r1");
    assert.equal(doneOf("t:로그인", done)?.id, "k1");
    assert.equal(doneOf("r:t-1", done), null);
    assert.equal(doneOf("d:q1", done), null);
  });

  it("목록은 질문, 확인 필요 post, 할 일 확인 필요 순", () => {
    const data = { requests: [{ id: "i2", title: "늦음", body: "", created_ms: 9 }, { id: "i1", title: "이름", body: "", created_ms: 3 }], done: [], keepDays: 14 };
    const tasks = [{ title: "볼 일", attention: "확인 필요" }, { title: "그냥", attention: null }];
    const keys = openAsks([thread("t-1", open), thread("t-2", { ...open, answer: { summary: "1A" } })], data, tasks).map((a) => a.key);
    assert.deepEqual(keys, ["q:t-1", "r:i1", "r:i2", "t:볼 일"]);
  });
});
