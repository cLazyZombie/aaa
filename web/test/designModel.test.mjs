// 기획 트리(t-1894): 문서 목록을 폴더 트리로 묶는 차례와 문서 안 상대 경로 풀기. 트리 차례는 사이드바 줄 차례이고,
// 경로 풀기는 그림 주소·앱 안 문서 열기가 저장소 밖을 가리키지 않게 하는 계약이다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { describe, it } from "node:test";
import { ancestors, blockDiff, calloutOf, idLineOf, withoutIdLines, chatImageDir, designFileUrl, designMarks, designTree, docLabel, docsUnder, headingOf, ideaLinkId, markCounts, marksOf, mdBlocks, modelChoices, parseRepoFilter, pickRepo, resolveDocPath, savesOf, sectionsOf, shownRepos, SLUG, slugSuggest, splitContext, splitTarget, takeImages, taskState } from "../src/designModel.ts";

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
    ]);
    assert.deepEqual(marks.map((m) => [m.kind, m.after]), [["open", null], ["save", "보스 도감에"], ["push", "보스 도감에"], ["edit", "보스 도감에"], ["save", "다음 말"]]);
    assert.equal(marks[0].text, "새 대화 · main 8ebc07f");
    assert.match(marks[1].text, /^main에 올림 · 문서 1 고침 · 1 새로 · 4e1a2bf · \d{2}:\d{2}$/);
    assert.match(marks[3].text, /^사람이 a\.md 고침 · main 7c9d01f · \d{2}:\d{2}$/);
    // 구현 맡기기(표지를 main에, t-2002)·넘긴 블록 고침 알림·되돌리기
    const more = designMarks([ev("handoff", "d-0005 · 블록 2", "x", "f068151fff"), ev("relay", "d-0005 → t-0007", "x", "35678e3fff"), ev("edit", "docs/a.md 되돌림 ← 1a2b3c4", "x", "99aa")]);
    assert.deepEqual(more.map((m) => m.text), ["구현 맡기기 d-0005 · 블록 2 · main에 표지 f068151", "넘긴 블록 고침 d-0005 → t-0007 · 35678e3", "docs/a.md 되돌림 ← 1a2b3c4 · 99aa"]);
    assert.equal(savesOf([ev("open", ""), ev("save", "a"), ev("edit", "b"), ev("handoff", "d-1"), ev("relay", "d-1")]), 3);
    // 「main에 반영」 예약(t-1955): 예약·취소·시작 못 함(까닭은 host 글)
    // 옛 기록(t-2002 전)의 「main에 반영」 예약 사건도 그대로 그린다
    const waits = designMarks([ev("apply_wait", "", "x"), ev("apply_cancel", "", "x"), ev("apply_fail", "이미 반영 중이다: t-0009", "x")]);
    assert.match(waits[0].text, /^main에 반영 예약 · 자리 나면 시작 · \d{2}:\d{2}$/);
    assert.match(waits[1].text, /^반영 예약 취소 · \d{2}:\d{2}$/);
    assert.equal(waits[2].text, "예약한 반영을 시작하지 못함 · 이미 반영 중이다: t-0009");
    // 그림·영상 붙임(고치기의 「그림」·대화 입력창, t-1950)은 고침이 아니라 붙임이다
    const pics = designMarks([ev("edit", "docs/design/89ea9511ae150ab2.png", "x", "a60733efff"), ev("edit", "docs/a/clip.MP4", "x", "b1")]);
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
  it("이력 차이(t-2002): 옛 판과 지금 판의 덩어리를 차례대로 맞춰 생김·사라짐을 가른다", () => {
    const d = blockDiff(["# 보스", "옛 문단", "같은 끝"], ["# 보스", "새 문단", "더한 문단", "같은 끝"]);
    assert.deepEqual(d.map((x) => [x.kind, x.text]), [["same", "# 보스"], ["back", "옛 문단"], ["gone", "새 문단"], ["gone", "더한 문단"], ["same", "같은 끝"]]);
    assert.deepEqual(blockDiff([], ["a"]), [{ text: "a", kind: "gone" }]);
    assert.deepEqual(blockDiff(["a"], ["a"]), [{ text: "a", kind: "same" }]);
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
    assert.deepEqual(markCounts([want]), { work: 4, plan: 2, open: 2 }, "같은 번호는 진행 하나");
  });

  it("ID 표지 줄(t-1957)은 홀로 한 덩어리이고 그리지 않는다(Rust design_id_line과 같은 꼴)", () => {
    assert.equal(idLineOf("<!-- aaa-id: k-12 -->"), "k-0012");
    assert.equal(idLineOf("  <!--aaa-id:k-0012-->  "), "k-0012");
    for (const bad of ["<!-- aaa-id: k-0 -->", "<!-- aaa-id: d-12 -->", "<!-- aaa-id: k-12 x -->", "`<!-- aaa-id: k-12 -->`", "<!-- aaa-id: k-12 --->"])
      assert.equal(idLineOf(bad), null, bad);
    assert.deepEqual(mdBlocks("# 루프\n<!-- aaa-id: k-0012 -->\n> [!계획]\n> 보스"), ["# 루프", "<!-- aaa-id: k-0012 -->", "> [!계획]\n> 보스"]);
    assert.equal(withoutIdLines("# 루프\n<!-- aaa-id: k-0012 -->\n\n본문"), "# 루프\n\n본문");
    const fenced = "```\n<!-- aaa-id: k-0012 -->\n```";
    assert.equal(withoutIdLines(fenced), fenced, "울타리 안 예시는 그대로");
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

  it("긴 줄의 해석 결과를 확인하고 과도한 되돌기는 별도 프로세스에서 차단한다", () => {
    // 동기 정규식은 node:test의 JS timer도 막는다. 소유한 자식의 종료 사건을 기다리고,
    // 진짜 되돌기 정체 때만 자식을 끝낸다. 러너의 1초 벽시계 성능을 단언하지 않는다.
    const fixture = fileURLToPath(new URL("./fixtures/design-long-lines.mjs", import.meta.url));
    const result = execFileSync(process.execPath, ["--experimental-strip-types", fixture], {
      timeout: 30_000, killSignal: "SIGKILL", encoding: "utf8",
    });
    assert.equal(result.trim(), "긴 줄 4개 해석 확인");
  });
});

// 트리 저장소 필터(t-1946): 이 브라우저에 남긴 값은 믿지 않고 읽는다. 저장소가 빠지면 무시하고, 다 빠지면 전부 보인다
describe("기획 트리 저장소 필터", () => {
  const names = ["game", "engine", "tool", "puzzle"];
  const seen = (picked) => [...shownRepos(names, picked)];

  it("남긴 값: 없음·깨짐·배열 아님·빈 배열은 전부(null), 문자열만 겹침 없이", () => {
    for (const raw of [null, "", "{", "\"game\"", "{}", "[]", "[1, null, \"\"]"]) assert.equal(parseRepoFilter(raw), null, String(raw));
    assert.deepEqual(parseRepoFilter('["game", 3, "puzzle", "game"]'), ["game", "puzzle"]);
  });

  it("보일 저장소: 고른 것 중 지금 있는 것만, 하나도 없으면 전부", () => {
    assert.deepEqual(seen(null), names);
    assert.deepEqual(seen(["puzzle", "game"]), ["game", "puzzle"], "트리 차례");
    assert.deepEqual(seen(["game", "gone"]), ["game"], "없는 저장소는 무시");
    assert.deepEqual(seen(["gone"]), names, "고른 것이 다 빠지면 전부");
    assert.deepEqual([...shownRepos([], ["game"])], [], "목록이 비면 비어 있다");
  });

  it("누름: 전체에서 누르면 그것만, 그 뒤 더하고 빼기, 다 빼거나 다 고르면 전체", () => {
    assert.deepEqual(pickRepo(names, null, "game"), ["game"]);
    assert.deepEqual(pickRepo(names, ["gone"], "tool"), ["tool"], "남긴 것이 다 빠진 전체에서도 그것만");
    assert.deepEqual(pickRepo(names, ["game"], "puzzle"), ["game", "puzzle"]);
    assert.deepEqual(pickRepo(names, ["game", "puzzle"], "game"), ["puzzle"]);
    assert.equal(pickRepo(names, ["game"], "game"), null, "다 빼면 전체");
    assert.equal(pickRepo(names, ["game", "engine", "tool"], "puzzle"), null, "다 고르면 전체(새 저장소도 보인다)");
    assert.equal(pickRepo(names, ["game"], null), null, "「전체」");
    assert.deepEqual(pickRepo(names, ["game", "gone"], "tool"), ["game", "gone", "tool"], "잠시 빠진 저장소는 누름으로 지우지 않는다");
  });
});

describe("대화 모델 고르기와 옛 아이디어 링크", () => {
  it("모델은 그 머신이 맡을 수 있는 profile이고, 지금 값이 목록에 없으면 앞에 둔다", () => {
    const owners = [{ machine: "mac-a", profiles: ["opus-high", "codex-high"] }, { machine: "mac-b", profiles: ["other"] }];
    assert.deepEqual(modelChoices(owners, "mac-a", "opus-high"), ["opus-high", "codex-high"]);
    assert.deepEqual(modelChoices(owners, "mac-a", "old-max"), ["old-max", "opus-high", "codex-high"]);
    assert.deepEqual(modelChoices(owners, null, "opus-high"), ["opus-high"]);
  });
  it("옛 할 일 메모의 아이디어 링크(`ideas/n-0001`)만 n-ID로 읽는다(t-1959)", () => {
    assert.equal(ideaLinkId("ideas/n-0001"), "n-0001");
    for (const path of ["ideas/n-0001/note.md", "uploads/n-0001", "ideas/d-0001", "n-0001"]) assert.equal(ideaLinkId(path), null, path);
  });
});
