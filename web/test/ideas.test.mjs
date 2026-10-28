// 아이디어 화면(t-1260): 휴대폰 페이지 쓸기(결정 24), 고친 노트 머리(결정 16), 노트 인용 꼴(결정 9·17), 노트 그림 주소의 거름.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { ideaFileUrl, ideaLinkTarget, ideaSwipe, withEditNote } from "../src/ideasModel.ts";
import { formatNotes } from "../src/reviewNoteStore.ts";

describe("아이디어", () => {
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

  // 사람이 고친 노트는 다음 말 앞에 한 번만 붙는다. harness 명령에는 붙이지 않는다
  it("고친 노트 머리", () => {
    const note = { edited: "판 9 「미정」 1줄 더함" };
    assert.equal(withEditNote(note, "보내기 전에 보여 줘"), "(노트 고침: 판 9 「미정」 1줄 더함)\n보내기 전에 보여 줘");
    assert.equal(withEditNote(note, "(노트 고침: x)\n말"), "(노트 고침: x)\n말");
    assert.equal(withEditNote(note, "/compact"), "/compact");
    assert.equal(withEditNote({ edited: null }, "말"), "말");
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
});
