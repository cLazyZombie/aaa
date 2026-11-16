// 옛 아이디어 화면(t-1260, 읽기 전용 t-1894 4단계): 휴대폰 페이지 쓸기(결정 24), 노트 인용 꼴(결정 9·17), 노트 그림 주소의 거름,
// 옮기기 상태와 옮긴 곳.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { ideaFileUrl, ideaLegacy, ideaLinkTarget, ideaModelChoices, ideaSwipe, movedTarget } from "../src/ideasModel.ts";
import { formatNotes } from "../src/reviewNoteStore.ts";

describe("아이디어", () => {


  // 고르기 목록은 host 머신이 맡을 수 있는 profile뿐이고, 지금 값은 목록에 없어도 남는다
  it("진행 모델 목록", () => {
    const owners = [{ machine: "mac-a", profiles: ["opus-high", "codex-high"] }, { machine: "mac-b", profiles: ["sonnet-high"] }];
    assert.deepEqual(ideaModelChoices(owners, "mac-a", "opus-high"), ["opus-high", "codex-high"]);
    assert.deepEqual(ideaModelChoices(owners, "mac-a", "old-max"), ["old-max", "opus-high", "codex-high"]);
    assert.deepEqual(ideaModelChoices(owners, null, "opus-high"), ["opus-high"]);
  });

  // 「목록 | 노트 | 대화」가 한 줄: 노트 ← 대화, 대화 → 노트. 노트 →는 drawer(목록) 몫, 왼쪽 끝 24px은 브라우저 뒤로다
  it("페이지처럼 쓸기", () => {
    assert.equal(ideaSwipe("note", 200, -80, 5), "chat");
    assert.equal(ideaSwipe("chat", 200, 80, 5), "note");
    assert.equal(ideaSwipe("note", 200, -30, 2), "claim", "아직 짧은 쓸기는 붙잡기만");
    assert.equal(ideaSwipe("note", 200, 80, 5), "ignore", "목록은 drawer가 연다");
    assert.equal(ideaSwipe("chat", 200, -80, 5), "ignore");
    assert.equal(ideaSwipe("chat", 20, 80, 5), "ignore", "왼쪽 끝은 뒤로 쓸기");
    assert.equal(ideaSwipe("note", 200, -40, 60), "ignore", "세로는 스크롤");
    assert.equal(ideaSwipe("note", 200, -4, 3), "pending");
  });


  // thread가 받는 인용: 아이디어·절·고른 글(그림·영상은 통째로 이름과 설명)과 의견
  it("노트 인용 꼴", () => {
    const anchor = { text: "의견은 모아 두었다가\n한 번에 보낸다", start: 0, end: 10, prefix: "", suffix: "" };
    const text = formatNotes("idea:n-0001", [
      { id: "a", at: 2, comment: "그림에 휴대폰도", source: { kind: "media", idea: "n-0001", section: "초안", name: "0123456789abcdef.png", label: "화면 짜임" } },
      { id: "b", at: 1, comment: "보내기 전에 한 번 더", source: { kind: "note", idea: "n-0001", section: "결정", anchor } },
    ]);
    assert.equal(text, [
      "인용: n-0001 노트 「결정」", "> 의견은 모아 두었다가", "> 한 번에 보낸다", "의견: 보내기 전에 한 번 더", "",
      "인용: n-0001 노트 「초안」", "> 그림 화면 짜임 (0123456789abcdef.png)", "의견: 그림에 휴대폰도",
    ].join("\n"));
  });

  // 노트 그림은 그 아이디어 폴더의 내용 hash 이름만 주소로 바꾼다(바깥 경로·주소는 그리지 않는다)
  it("노트 그림 주소와 할 일 링크", () => {
    assert.equal(ideaFileUrl("demo", "01JD0000000000000000000000", "0123456789abcdef.mp4"), "/api/v1/projects/demo/ideas/01JD0000000000000000000000/files/0123456789abcdef.mp4");
    for (const bad of ["../TASKS.md", "https://example.com/a.png", "0123456789ABCDEF.png", "uploads/0123456789abcdef.png"]) {
      assert.equal(ideaFileUrl("demo", "x", bad), null, bad);
    }
    assert.equal(ideaLinkTarget("ideas/n-0001"), "n-0001");
    assert.equal(ideaLinkTarget("ideas/n-0001/note.md"), null);
  });
  // 4단계: 옮긴 곳이 상태를 정한다(완료보다 앞선다)
  it("옛 아이디어 상태", () => {
    const mv = (done) => ({ repo: "pt", branch: "idea-n-0003", path: "docs/camp/", done, at_ms: 0, rev: 2, branch_ms: 1 });
    assert.deepEqual(ideaLegacy({ status: "open" }), { kind: "open", label: "다듬는 중" });
    assert.equal(ideaLegacy({ status: "done" }).kind, "done");
    assert.equal(ideaLegacy({ status: "open", moved: mv(false) }).label, "옮기는 중 → pt · design/idea-n-0003");
    assert.equal(ideaLegacy({ status: "open", moved: mv(true) }).label, "옮김 → pt docs/camp/");
  });

  // 옮긴 곳 열기: 문서면 그대로, 폴더면 옮기는 branch가 고친 첫 문서 → 그 폴더 README → 그 폴더 첫 문서 → 기획 폴더 README
  it("옮긴 곳 문서", () => {
    const repo = (changed, docs) => ({
      repo: "pt", folder: "docs", tree: { docs: docs.map((path) => ({ path })) },
      branches: [{ slug: "idea-n-0003", changed: changed.map(([path, status]) => ({ path, status })) }],
    });
    const m = (path, done = false) => ({ repo: "pt", branch: "idea-n-0003", path, done, at_ms: 0, rev: 1, branch_ms: 1 });
    assert.equal(movedTarget(m("docs/camp/board.md", true), []), "pt/docs/camp/board.md");
    const r = repo([["docs/camp/x.png", "A"], ["docs/camp/board.md", "A"], ["docs/old.md", "D"]], ["docs/README.md", "docs/camp/a.md", "docs/camp/README.md"]);
    assert.equal(movedTarget(m("docs/camp/"), [r]), "pt/docs/camp/board.md");
    assert.equal(movedTarget(m("docs/camp/", true), [r]), "pt/docs/camp/README.md", "옮긴 뒤에는 branch를 보지 않는다");
    assert.equal(movedTarget(m("docs/camp/"), [repo([], ["docs/camp/a.md"])]), "pt/docs/camp/a.md");
    assert.equal(movedTarget(m("docs/new/"), [repo([], ["docs/camp/a.md"])]), "pt/docs/README.md");
  });
});
