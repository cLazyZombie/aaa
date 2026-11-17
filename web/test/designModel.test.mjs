// 기획 트리(t-1894): 문서 목록을 폴더 트리로 묶는 차례와 문서 안 상대 경로 풀기. 트리 차례는 사이드바 줄 차례이고,
// 경로 풀기는 그림 주소·앱 안 문서 열기가 저장소 밖을 가리키지 않게 하는 계약이다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { describe, it } from "node:test";
import { addedDocs, ancestors, branchesByPath, calloutOf, changedBlocks, chatImageDir, designFileUrl, designMarks, designTree, docLabel, docsUnder, headingOf, markCounts, marksOf, mdBlocks, mergedToMain, resolveDocPath, sectionsOf, slugSuggest, SLUG, splitContext, splitTarget, takeImages, taskState } from "../src/designModel.ts";

const doc = (path, title = "") => ({ path, title, bytes: 1 });
const names = (nodes) => nodes.map((n) => (n.kind === "dir" ? `${n.name}/` : n.name));

describe("기획 트리", () => {
  it("폴더 먼저, README 먼저, 폴더 제목은 그 README", () => {
    const nodes = designTree("docs", [
      doc("docs/zeta.md"),
      doc("docs/README.md", "개요"),
      doc("docs/alpha.md", "알파"),
      doc("docs/loop/skill-tree.md", "스킬 트리"),
      doc("docs/loop/README.md", "게임 루프"),
      doc("docs/art/a/deep.md"),
      doc("other/out.md"),
    ]);
    assert.deepEqual(names(nodes), ["art/", "loop/", "README.md", "alpha.md", "zeta.md"]);
    const loop = nodes[1];
    assert.equal(loop.title, "게임 루프");
    assert.deepEqual(names(loop.children), ["README.md", "skill-tree.md"]);
    assert.deepEqual(names(nodes[0].children), ["a/"], "중간 폴더도 만든다");
    assert.equal(nodes[0].title, "");
    assert.equal(docLabel(doc("docs/loop/plan-v2.md")), "plan-v2", "제목 없는 문서는 파일 이름");
    assert.deepEqual(ancestors("docs/loop/skill-tree.md"), ["docs", "docs/loop"]);
  });

  it("문서 안 상대 경로", () => {
    const at = "docs/loop/README.md";
    assert.equal(resolveDocPath(at, "media/a.png"), "docs/loop/media/a.png");
    assert.equal(resolveDocPath(at, "./skill-tree.md#보상"), "docs/loop/skill-tree.md");
    assert.equal(resolveDocPath(at, "../art/%EA%B7%B8%EB%A6%BC.png?x=1"), "docs/art/그림.png");
    assert.equal(resolveDocPath(at, "../../README.md"), "README.md");
    for (const bad of ["../../../x.md", "a%2F..%2F..%2F..%2F..%2Fx", "/etc/passwd", "https://example.com/a.png", "data:image/png;base64,AA", "#절", " "]) {
      assert.equal(resolveDocPath(at, bad), null, bad);
    }
  });

  it("그림 주소와 앱 주소", () => {
    assert.equal(
      designFileUrl("de mo", "app", "docs/loop/그림 1.png", "0123456789abcdef0123456789abcdef01234567"),
      "/api/v1/projects/de%20mo/design/app/docs/loop/%EA%B7%B8%EB%A6%BC%201.png?c=0123456789abcdef0123456789abcdef01234567",
    );
    assert.deepEqual(splitTarget("app/docs/a.md"), { repo: "app", path: "docs/a.md" });
    assert.equal(splitTarget("app/"), null);
    assert.equal(splitTarget("/docs/a.md"), null);
  });
});

// 2-3 기획 화면(t-1894): 트리의 branch 표시, branch 판의 main과 다른 덩어리, branch 이름 짓기, 대화 사이 사건 줄
describe("기획 branch 화면", () => {
  const branch = (slug, changed, extra = {}) => ({ slug, changed, events: [], state: "open", ...extra });
  const repo = {
    tree: { docs: [doc("docs/a.md"), doc("docs/b.md")] },
    branches: [
      branch("one", [{ path: "docs/a.md", status: "M" }, { path: "docs/new.md", status: "A" }, { path: "docs/pic.png", status: "A" }]),
      branch("two", [{ path: "docs/a.md", status: "M" }, { path: "docs/new.md", status: "A" }]),
    ],
  };
  it("문서마다 고친 branch(만든 차례), main에 없는 새 문서는 먼저 만든 branch 하나로(그림은 문서가 아니다)", () => {
    assert.deepEqual(branchesByPath(repo).get("docs/a.md").map((b) => b.slug), ["one", "two"]);
    assert.equal(branchesByPath(repo).get("docs/b.md"), undefined);
    assert.deepEqual(addedDocs(repo).map((a) => [a.path, a.branch.slug]), [["docs/new.md", "one"]]);
  });
  it("branch 이름: 영문·숫자만 남기고, 꼴은 소문자·숫자·-(끝은 -가 아님)", () => {
    assert.equal(slugSuggest("Boss Dex 2!"), "boss-dex-2");
    assert.equal(slugSuggest("보스 도감"), "");
    for (const ok of ["a", "boss-dex", "floors-100"]) assert.ok(SLUG.test(ok), ok);
    for (const bad of ["", "-a", "a-", "A", "a_b", "a--b", "main", "x".repeat(49)]) assert.ok(!SLUG.test(bad), bad);
    assert.equal(slugSuggest("Main"), "main-1", "판 고르기의 main과 겹치지 않게");
  });
  it("덩어리는 빈 줄로 나누고 제목 줄은 홀로, 코드 울타리 안 빈 줄은 나누지 않는다. main에 없는 덩어리만 표시", () => {
    const text = "# 제목\n본문 하나\n\n```\n코드\n\n끝\n```\n\n## 절\n새 글\n";
    const blocks = mdBlocks(text);
    assert.deepEqual(blocks, ["# 제목", "본문 하나", "```\n코드\n\n끝\n```", "## 절", "새 글"]);
    assert.deepEqual(changedBlocks("# 제목\n본문 하나\n\n## 절\n옛 글\n", blocks), [false, false, true, false, true]);
    assert.deepEqual(changedBlocks(null, blocks), blocks.map(() => true), "main에 없는 새 문서는 모두");
    assert.deepEqual(mdBlocks("````\n```\n안\n```\n````\n\n끝\n"), ["````\n```\n안\n```\n````", "끝"], "긴 울타리 안의 짧은 울타리는 글이다");
  });
  it("대화 사건: 같은 말 뒤 thread 저장은 한 줄로 묶고, 새 대화·올림·사람 고침은 따로", () => {
    const ev = (kind, text, after, commit = "abcdef1234") => ({ kind, text, after, commit, at_ms: 0 });
    const marks = designMarks([
      ev("open", "", null, "8ebc07ffff"),
      ev("save", "docs/a.md", "보스 도감에"),
      ev("create", "docs/new.md", "보스 도감에", "4e1a2bffff"),
      ev("push", "", "보스 도감에"),
      ev("edit", "docs/a.md", "보스 도감에", "7c9d01ffff"),
      ev("save", "docs/b.md", "다음 말"),
    ], "boss-dex");
    assert.deepEqual(marks.map((m) => [m.kind, m.after]), [["open", null], ["save", "보스 도감에"], ["push", "보스 도감에"], ["edit", "보스 도감에"], ["save", "다음 말"]]);
    assert.equal(marks[0].text, "새 대화 · design/boss-dex · main 8ebc07f 에서");
    assert.equal(marks[1].text, "문서 1 고침 · 1 새로 · 4e1a2bf");
    assert.equal(marks[3].text, "사람이 a.md 고침 · 7c9d01f");
    // 구현 맡김(3-2)과 넘긴 블록 고침 알림(3-3)
    const more = designMarks([ev("handoff", "d-0005 · 블록 2", "x", "f068151fff"), ev("relay", "d-0005 → t-0007", "x", "35678e3fff")], "loop-plan");
    assert.deepEqual(more.map((m) => m.text), ["구현 맡김 d-0005 · 블록 2 · f068151", "넘긴 블록 고침 d-0005 → t-0007 · 35678e3"]);
    // 그림·영상 붙임(고치기의 「그림」·대화 입력창, t-1950)은 고침이 아니라 붙임이다
    const pics = designMarks([ev("edit", "docs/design/89ea9511ae150ab2.png", "x", "a60733efff"), ev("edit", "docs/a/clip.MP4", "x", "b1")], "s");
    assert.deepEqual(pics.map((m) => m.text), ["그림 89ea9511ae150ab2.png 붙임 · a60733e", "그림 clip.MP4 붙임 · b1"]);
  });
  it("대화 입력창 그림(t-1950): 초안의 @경로를 떼어 「(그림: …)」 줄로, 폴더는 보는 문서 폴더(기획 폴더 밖이면 기획 폴더)", () => {
    assert.deepEqual(takeImages("@docs/design/a1.png @docs/design/b2.webp 이 그림으로 넣어 줘"), { paths: ["docs/design/a1.png", "docs/design/b2.webp"], text: "이 그림으로 넣어 줘" });
    assert.deepEqual(takeImages("그림 없음 가@나.png"), { paths: [], text: "그림 없음 가@나.png" }, "낱말 가운데 @는 그림이 아니다");
    assert.deepEqual(takeImages("a  b\n    code"), { paths: [], text: "a  b\n    code" }, "그림이 없으면 빈칸·들여쓰기를 건드리지 않는다");
    assert.deepEqual(takeImages("@docs/a.png 표\n| x  |  y |\n    들여 쓴 줄"), { paths: ["docs/a.png"], text: "표\n| x  |  y |\n    들여 쓴 줄" }, "멘션만 뗀다");
    assert.equal(chatImageDir("docs/design", "docs/design/loop/game-loop.md"), "docs/design/loop");
    assert.equal(chatImageDir("docs/design", "docs/design/game-loop.md"), "docs/design");
    assert.equal(chatImageDir("docs/design", "docs/attack-two.md"), "docs/design", "기획 폴더 밖 문서(폴더 재편 전 branch)");
    assert.equal(chatImageDir("docs/design", "docs/designer/x.md"), "docs/design", "이름만 앞이 같은 폴더");
    assert.equal(chatImageDir("docs/design", null), "docs/design");
    const chips = splitContext("(보는 문서: docs/design/game-loop.md)\n(그림: docs/design/a1.png)\n넣어 줘").chips;
    assert.deepEqual(chips[1], { label: "그림", value: "a1.png", path: true });
  });
  it("main에 들어감(t-1950): 제 commit이 있었고 main보다 앞선 것이 없는 열린 대화", () => {
    const b = { state: "open", ahead: 0, head: "e188", base: "10aa", read: true };
    assert.equal(mergedToMain(b), true);
    assert.equal(mergedToMain({ ...b, ahead: 6 }), false);
    assert.equal(mergedToMain({ ...b, head: "10aa" }), false, "갓 연 대화(끝이 갈라진 자리)");
    assert.equal(mergedToMain({ ...b, state: "rest" }), false);
    assert.equal(mergedToMain({ ...b, base: null }), false, "옛 host는 갈라진 판이 없다");
    assert.equal(mergedToMain({ ...b, read: false }), false, "agent가 그 branch를 읽지 못한 0(머신 끊김·시한)은 들어간 것이 아니다");
  });
  it("그림 주소: branch 판이면 slug를 싣는다", () => {
    assert.equal(designFileUrl("p", "r", "docs/x y.png", "abc", "boss-dex"), "/api/v1/projects/p/design/r/docs/x%20y.png?c=abc&b=boss-dex");
    assert.equal(designFileUrl("p", "r", "docs/x.png", "abc"), "/api/v1/projects/p/design/r/docs/x.png?c=abc");
  });
});

describe("블록 표지(3-1)", () => {
  it("인용 줄로만 된 덩어리의 첫 줄 표지만 표지 블록이다", () => {
    const blocks = mdBlocks("# 게임 루프\n\n## 탑\n\n> [!계획]\n> **난이도** — 층마다 오른다.\n\n> [!계획] d-0012\n> 처음 1칸.\n>\n> ![시안](a.png)\n\n> [!미정]\n> 확률\n\n> 앞 인용\n> [!계획] 중간\n\n> [!계획]\n섞임\n\n```\n> [!계획] d-0099\n```\n");
    const sections = sectionsOf(blocks, "게임 루프");
    const got = blocks.map((b, i) => calloutOf(b, sections[i]));
    assert.deepEqual(got.map((c) => c && [c.kind, c.task, c.title, c.section, c.image]), [
      null, null,
      ["plan", null, "난이도", "탑", false],
      ["plan", "d-0012", "처음 1칸.", "탑", true],
      ["open", null, "확률", "탑", false],
      null, null, null,
    ]);
    // 본문이 없으면 절 이름이 줄 이름이다. 그림만 있는 첫 줄은 그림을 뺀 글이다
    assert.equal(calloutOf("> [!계획]", "나가기").title, "나가기");
    assert.equal(calloutOf("> [!계획] ![지도](m.png) 캠프 지도", "캠프").title, "캠프 지도");
  });

  it("절은 가장 가까운 앞 제목, 없으면 문서 제목", () => {
    assert.deepEqual(sectionsOf(["머리말", "## 탑 ##", "글", "### 층", "글"], "문서"), ["문서", "탑", "탑", "층", "층"]);
  });

  it("넘긴 블록의 상태는 할 일과 thread에서 읽는다", () => {
    const tasks = [
      { short_id: "d-0001", done: false, thread: "t-0001" },
      { short_id: "d-0002", done: false, thread: null },
      { short_id: "d-0003", done: true, thread: null },
      { short_id: "d-0004", done: false, thread: "t-0004" },
      { short_id: "d-0005", done: false, thread: "t-0005" },
      { short_id: "d-0006", done: false, thread: "t-0006" },
      { short_id: "d-0007", done: false, thread: "t-0007" },
    ];
    const threads = [
      { id: "t-0001", status: "open", stage: "work", group: "working" },
      { id: "t-0004", status: "open", stage: "landing", group: "working" },
      { id: "t-0005", status: "open", stage: "merge_ready", group: "ready_for_review" },
      { id: "t-0006", status: "open", stage: "work", group: "waiting_on_you" },
      { id: "t-0007", status: "resolved", stage: "work", group: "done" },
    ];
    const none = { archived: new Set(), missing: new Set() };
    const state = (id, lookup = none) => taskState(id, tasks, threads, lookup);
    assert.equal(taskState("d-0001", null, threads), null, "할 일을 아직 못 읽음");
    assert.deepEqual(state("d-0001"), { tone: "work", label: "작업 중 d-0001 · t-0001" });
    assert.deepEqual(state("d-0002"), { tone: "wait", label: "대기 d-0002" });
    assert.deepEqual(state("d-0003"), { tone: "stale", label: "표지 남음 d-0003" });
    assert.deepEqual(state("d-0004"), { tone: "landing", label: "착지 중 d-0004 · t-0004" });
    assert.deepEqual(state("d-0005"), { tone: "ready", label: "병합 준비됨 d-0005 · t-0005" });
    assert.deepEqual(state("d-0006"), { tone: "review", label: "검수 대기 d-0006 · t-0006" });
    assert.deepEqual(state("d-0007"), { tone: "wait", label: "대기 d-0007" }, "끝난 thread는 다시 대기");
    assert.deepEqual(state("d-0008", { archived: new Set(["d-0008"]), missing: new Set() }), { tone: "stale", label: "표지 남음 d-0008" }, "보관된 할 일");
    assert.deepEqual(state("d-9999", { archived: new Set(), missing: new Set(["d-9999"]) }), { tone: "missing", label: "할 일 없음 d-9999" });
    assert.equal(state("d-9999"), null, "보관을 아직 못 찾았으면(읽는 중·읽지 못함) 「할 일 없음」이 아니다");
  });

  it("폴더·저장소 합계는 같은 d-ID를 한 번만 센다", () => {
    const docs = [
      { path: "docs/a.md", title: "", bytes: 1, plans: 3, open: 1, tasks: ["d-0001", "d-0002"] },
      { path: "docs/loop/b.md", title: "", bytes: 1, plans: 2, open: 0, tasks: ["d-0001"] },
      { path: "docs/loop/c.md", title: "", bytes: 1, plans: 0, open: 2, tasks: [] },
      { path: "docs/old.md", title: "", bytes: 1 },
    ];
    assert.deepEqual(markCounts(docs), { work: 2, plan: 2, open: 3 });
    assert.deepEqual(markCounts([docs[3], undefined]), { work: 0, plan: 0, open: 0 }, "옛 host의 문서 줄");
    const loop = designTree("docs", docs).find((n) => n.kind === "dir");
    assert.deepEqual(docsUnder(loop).map((d) => d.path), ["docs/loop/b.md", "docs/loop/c.md"]);
    assert.deepEqual(markCounts(docsUnder(loop)), { work: 1, plan: 1, open: 2 });
  });

  it("보낸 말 앞의 맥락 줄은 칩으로 떼고 말만 남긴다", () => {
    const sent = "(보는 문서: docs/bosses/README.md)\n(문서 고침: docs/bosses/README.md, docs/a/b.md)\n(고른 블록: 쓰러지면과 가호 칸 · 확률)\n이 블록 같이 정하자";
    assert.deepEqual(splitContext(sent), {
      chips: [
        { label: "보는 문서", value: "bosses/README.md", path: true },
        { label: "사람 고침", value: "README.md, b.md", path: true },
        { label: "고른 블록", value: "쓰러지면과 가호 칸 · 확률", path: false },
      ],
      text: "이 블록 같이 정하자",
    });
    assert.deepEqual(splitContext("(보는 문서: README.md)"), { chips: [], text: "(보는 문서: README.md)" }, "말이 없으면 그대로");
    assert.deepEqual(splitContext("그냥 말\n(보는 문서: a.md)").chips, [], "맨 앞 줄만 맥락이다");
  });

  it("agent와 같은 공용 견본으로 센다(LF·CRLF)", () => {
    const raw = fs.readFileSync(new URL("./fixtures/design-marks.md", import.meta.url), "utf8").replace(/\r\n/g, "\n");
    const m = /expect plans=(\d+) open=(\d+) tasks=([\w,-]*)/.exec(raw);
    const want = { plans: Number(m[1]), open: Number(m[2]), tasks: m[3] ? m[3].split(",") : [] };
    for (const text of [raw, raw.replace(/\n/g, "\r\n")]) {
      const blocks = mdBlocks(text);
      const sections = sectionsOf(blocks, "");
      assert.deepEqual(marksOf(blocks.map((b, i) => calloutOf(b, sections[i]))), want);
    }
    assert.deepEqual(markCounts([want]), { work: 3, plan: 2, open: 2 }, "같은 번호는 진행 하나");
  });

  it("미정 뒤 번호는 할 일이 아니고, 번호는 정규형이다", () => {
    assert.deepEqual([calloutOf("> [!미정] d-0012", "s").kind, calloutOf("> [!미정] d-0012", "s").task], ["open", null]);
    assert.equal(calloutOf("> [!계획] d-12 줄", "s").task, "d-0012");
    assert.equal(calloutOf("> [!계획] d-12 줄", "s").title, "줄");
    assert.equal(calloutOf("> [!계획] d-0", "s").task, null);
    assert.equal(headingOf("## 탑 ##"), "탑");
    assert.equal(headingOf("# C#"), "C#");
    assert.equal(headingOf("####### 일곱"), null);
  });

  it("긴 줄에서도 되돌기 없이 바로 끝난다", () => {
    const started = performance.now();
    calloutOf(`> [!계획]${" ".repeat(200_000)}x`, "s");
    calloutOf(`> [!계획] ${"![".repeat(100_000)}](a)`, "s");
    sectionsOf([`# a${" ".repeat(200_000)}x`], "s");
    mdBlocks(`> [!계획]${" ".repeat(200_000)}x\r\n> 끝`);
    assert.ok(performance.now() - started < 1000, `${performance.now() - started}ms`);
  });
});
