// 할 일 화면의 행 상태·거르기·검색·배지 셈(src/tasksModel.ts, 할 일 보기 T1, plan §8.306).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { agentOwner, agentParts, anchorOf, canonicalLine, defaultMachine, descendants, draftOf, draftOps, REPO_DIR, repoMachines, repoName, dropOp, emptyDraft, findTask, itemToTask, lineDiff, moveOp, noteItems, ownerKindOf, ownerLabel, parentAnchor, restorable, taskBadge, handOffText, ownerFixes, startable, linkCandidates, newAutoCleans, lastLogId, mineKeys, taskDepth, taskMatches, taskList, taskState, unassigned, pendingQueue, queueDropPosition, queueStep, queueWhy, repoChoices, unreserveHome } from "../src/tasksModel.ts";

const task = (over) => ({ list: "Backlog", title: "일", owner: null, thread: null, notes: [], done: false, line: 1, raw: "- [ ] 일", block_sha256: "", parent: null, problems: [], ...over });
const MISREAD = "owner 칸이 owner 꼴이 아님: t-0001 S4 뒤";
const groups = new Map([["t-0001", "working"], ["t-0002", "resolved"], ["t-0003", "waiting_on_you"], ["t-0004", "parked"]]);

describe("할 일 화면", () => {
  it("고정 ID로 검색하고 coordinator에게 전달한다", () => {
    const t = task({ short_id: "d-0007", title: "바뀐 제목" });
    assert.equal(taskMatches(t, "all", "d-0007", groups), true);
    assert.equal(handOffText(t), "d-0007 task를 thread로 맡겨 줘");
    assert.equal(handOffText({ ...t, title: "다시 바뀐 제목" }), handOffText(t));
  });

  it("행 상태: 형식 어긋남 > 열린 thread와 이어짐 > 대기. 끝난·모르는 thread는 대기다", () => {
    assert.equal(taskState(task({ thread: "t-0001", problems: [MISREAD] }), groups), "problem");
    assert.equal(taskState(task({ thread: "t-0001" }), groups), "run");
    assert.equal(taskState(task({ thread: "t-0003" }), groups), "run");
    assert.equal(taskState(task({ thread: "t-0002" }), groups), "wait");
    assert.equal(taskState(task({ thread: "t-0099" }), groups), "wait");
    assert.equal(taskState(task({}), groups), "wait");
    // 보류한 thread(plan §8.421)와 이어진 task는 진행 중이 아니라 보류다. 새로 맡기지 않는다(그 thread를 이어 한다)
    assert.equal(taskState(task({ thread: "t-0004" }), groups), "parked");
    assert.equal(startable(task({ thread: "t-0004" }), groups, []), false);
    assert.equal(startable(task({ thread: "t-0002" }), groups, []), true);
  });

  it("owner: 비움·agent는 미할당, me는 나. owner를 읽지 못한 형식 어긋남은 미할당으로 세지 않는다", () => {
    assert.equal(ownerLabel(task({})), "미할당");
    assert.equal(ownerLabel(task({ owner: "agent" })), "미할당");
    assert.equal(ownerLabel(task({ owner: "me" })), "나");
    // agent 일은 담당 칩이 없고 모델·머신 칩이 보인다
    assert.equal(ownerLabel(task({ owner: "codex@mac-b" })), null);
    assert.equal(ownerLabel(task({ owner: "Priya Rao", profile: null, machine: null })), "Priya Rao");
    assert.equal(unassigned(task({ problems: [MISREAD] })), false);
    assert.equal(ownerLabel(task({ problems: [MISREAD] })), null);
    // `(미할당)`은 미할당으로 읽혔다(정규형 안내만 단다)
    assert.equal(unassigned(task({ problems: ["owner `미할당`는 미할당으로 읽음(정규형은 비움)"] })), true);
  });

  it("거르기: 전체·내 일·미할당·진행 중", () => {
    const ts = [task({ title: "a", owner: "me" }), task({ title: "b" }), task({ title: "c", thread: "t-0001", owner: "claude" }), task({ title: "d", problems: [MISREAD] })];
    const pick = (f) => ts.filter((t) => taskMatches(t, f, "", groups)).map((t) => t.title);
    assert.deepEqual(pick("all"), ["a", "b", "c", "d"]);
    assert.deepEqual(pick("mine"), ["a"]);
    assert.deepEqual(pick("unassigned"), ["b"]);
    assert.deepEqual(pick("running"), ["c"]);
    const parked = task({ title: "e", thread: "t-0004" });
    assert.deepEqual([...ts, parked].filter((t) => taskMatches(t, "parked", "", groups)).map((t) => t.title), ["e"]);
    assert.equal(taskMatches(parked, "running", "", groups), false);
  });

  it("검색은 제목·메모·owner에서 대소문자를 가리지 않는다", () => {
    const t = task({ title: "Login 고치기", owner: "Priya Rao", notes: ["- 테스트 먼저"] });
    for (const q of ["login", "LOGIN", "테스트", "priya", "  고치기  "]) assert.equal(taskMatches(t, "all", q, groups), true, q);
    assert.equal(taskMatches(t, "all", "배포", groups), false);
    assert.equal(taskMatches(t, "mine", "login", groups), false);
  });

  it("배지는 형식 어긋남 task 수다(진행 중·대기는 세지 않는다)", () => {
    assert.equal(taskBadge(null), 0);
    const board = { project: "p", sha256: "", lists: [], unlinked_threads: [], notes_files: [], tasks: [task({ problems: [MISREAD] }), task({ problems: ["제목이 김(메모로 옮길 후보)"] }), task({ thread: "t-0001" }), task({})] };
    assert.equal(taskBadge(board), 2);
  });

  it("하위 task 깊이와 한 목록: `##` 목록을 가리지 않고 파일 차례대로, 거르기 칩마다 수를 센다", () => {
    const ts = [task({ line: 3, title: "상위" }), task({ line: 4, title: "하위", parent: 3 }), task({ line: 5, title: "손자", parent: 4 }), task({ line: 9, title: "끊긴 상위", parent: 7 }), task({ line: 12, list: "Later", title: "나중", owner: "me" })];
    const byLine = new Map(ts.map((t) => [t.line, t]));
    assert.deepEqual(ts.map((t) => taskDepth(t, byLine)), [0, 1, 2, 0, 0]);
    const board = { project: "p", sha256: "", lists: [{ name: "Backlog", notes: ["설명"] }, { name: "빈 목록", notes: [] }, { name: "Later", notes: [] }], tasks: ts, unlinked_threads: [], notes_files: [] };
    const all = taskList(board, "all", "", null, groups);
    assert.deepEqual(all.top, []);
    assert.deepEqual(all.rest.map((r) => [r.t.title, r.depth]), [["상위", 0], ["하위", 1], ["손자", 2], ["끊긴 상위", 0], ["나중", 0]]);
    const mine = taskList(board, "mine", "", null, groups);
    assert.deepEqual(mine.rest.map((r) => r.t.title), ["나중"]);
    assert.deepEqual(mine.counts, { all: 5, mine: 1, unassigned: 4, running: 0, parked: 0, reserved: 0, check: 0 });
  });

  it("상세가 가리키는 task: 제목으로 찾고, 같은 제목이면 줄이 같은 것", () => {
    const board = { project: "p", sha256: "", lists: [], unlinked_threads: [], notes_files: [], tasks: [task({ line: 2, title: "같음" }), task({ line: 8, title: "같음" }), task({ line: 9, title: "다른" })] };
    assert.equal(findTask(board, { title: "같음", line: 8 }).line, 8);
    // 줄이 밀려도(위에 줄이 더해짐) 제목으로 따라간다
    assert.equal(findTask(board, { title: "다른", line: 3 }).line, 9);
    assert.equal(findTask(board, { title: "없음", line: 1 }), null);
    assert.equal(findTask(null, { title: "같음", line: 2 }), null);
  });
});

// 편집(T2, plan §8.311): 화면 동작 → host 동작
describe("할 일 편집", () => {
  // ## A: 하나(1) [하위 하나-1(2), 하위 하나-2(3)], 둘(4), 셋(5) / ## B: 넷(7)
  const t = (line, title, list, parent = null) => task({ line, title, list, parent, block_sha256: `sha-${line}` });
  const board = {
    project: "demo", sha256: "", text: "", unlinked_threads: [], notes_files: [],
    lists: [{ name: "A", notes: [] }, { name: "B", notes: [] }],
    tasks: [t(1, "하나", "A"), t(2, "하위 하나-1", "A", 1), t(3, "하위 하나-2", "A", 1), t(4, "둘", "A"), t(5, "셋", "A"), t(7, "넷", "B")],
  };
  const by = (line) => board.tasks.find((x) => x.line === line);
  const place = (op) => op && { before: op.before?.title ?? null, parent: op.parent?.title ?? null, to_list: op.to_list };

  it("위로·아래로·맨 위로는 같은 상위 안에서, 끝으로 내리면 목록 끝·상위의 하위 끝", () => {
    assert.deepEqual(place(moveOp(board, by(4), "up")), { before: "하나", parent: null, to_list: "" });
    assert.deepEqual(place(moveOp(board, by(5), "top")), { before: "하나", parent: null, to_list: "" });
    assert.deepEqual(place(moveOp(board, by(1), "down")), { before: "셋", parent: null, to_list: "" });
    assert.deepEqual(place(moveOp(board, by(4), "down")), { before: null, parent: null, to_list: "A" });
    assert.deepEqual(place(moveOp(board, by(2), "down")), { before: null, parent: "하나", to_list: "" });
    assert.equal(moveOp(board, by(1), "up"), null);
    assert.equal(moveOp(board, by(7), "down"), null);
    assert.equal(moveOp(board, by(1), "top"), null);
    // 한 목록(§8.393): 맨 위 단계는 `##` 목록을 넘어 움직이고, 목록이 갈리는 틈에서는 앞 이웃의 목록 끝에 붙는다
    assert.deepEqual(place(moveOp(board, by(5), "down")), { before: null, parent: null, to_list: "B" });
    assert.deepEqual(place(moveOp(board, by(7), "up")), { before: "셋", parent: null, to_list: "" });
    assert.deepEqual(place(moveOp(board, by(7), "top")), { before: "하나", parent: null, to_list: "" });
    // 거른 채면 보이는 이웃과 자리를 바꾸고 숨은 task(둘)의 차례는 그대로다
    assert.deepEqual(place(moveOp(board, by(1), "down", (x) => x.line !== 4)), { before: null, parent: null, to_list: "A" });
    assert.deepEqual(place(moveOp(board, by(5), "up", (x) => x.line !== 4)), { before: "하나", parent: null, to_list: "" });
    assert.equal(moveOp(board, by(4), "up").at.block_sha256, "sha-4", "대상은 블록 hash로 찾는다");
  });

  it("끌어 놓기: 앞·뒤에 놓으면 그 task와 같은 상위가 되고, 자기 하위·같은 자리는 없음", () => {
    assert.deepEqual(place(dropOp(board, by(5), by(1), "before")), { before: "하나", parent: null, to_list: "" });
    assert.deepEqual(place(dropOp(board, by(5), by(2), "after")), { before: "하위 하나-2", parent: null, to_list: "" });
    assert.deepEqual(place(dropOp(board, by(5), by(3), "after")), { before: null, parent: "하나", to_list: "" });
    assert.deepEqual(place(dropOp(board, by(1), by(7), "after")), { before: null, parent: null, to_list: "B" });
    assert.equal(dropOp(board, by(1), by(2), "before"), null, "자기 하위");
    assert.equal(dropOp(board, by(4), by(4), "before"), null);
    assert.equal(dropOp(board, by(4), by(1), "after"), null, "이미 하나 뒤");
    assert.equal(dropOp(board, by(4), by(5), "before"), null, "이미 셋 앞");
    assert.deepEqual([...descendants(board, by(1))].sort(), [2, 3]);
    assert.deepEqual(place(dropOp(board, by(1), by(7), "before")), { before: null, parent: null, to_list: "A" }, "목록 사이 틈은 앞 목록 끝");
    assert.deepEqual(place(dropOp(board, by(7), by(4), "before")), { before: "둘", parent: null, to_list: "" }, "다른 목록 task 앞");
  });

  it("폼 → 동작: 바뀐 칸만 고치고, 목록·상위가 바뀌면 옮긴다. 안 바뀌면 없음", () => {
    const one = by(4);
    const d = draftOf({ ...one, owner: "codex@mac-b", notes: ["메모"] }, [], null);
    assert.deepEqual([d.who, d.profile, d.machine], ["agent", "codex", "mac-b"]);
    assert.deepEqual(draftOps({ ...one, owner: "codex@mac-b", notes: ["메모"] }, d), []);
    const ops = draftOps(one, { ...draftOf(one, [], null), title: " 둘 고침 ", who: "person", person: "Priya", notes: "a\n\n  b  \n\n", list: "B" });
    assert.deepEqual(ops.map((o) => o.op), ["update", "move"]);
    assert.deepEqual([ops[0].title, ops[0].owner, ops[0].person, ops[0].notes], ["둘 고침", "Priya", true, ["a", "", "  b"]]);
    assert.equal(ops[1].to_list, "B");
    const child = draftOps(one, { ...draftOf(one, [], null), parent: anchorOf(by(7)) });
    assert.deepEqual(place(child[0]), { before: null, parent: "넷", to_list: "" });
    const add = draftOps(null, { ...emptyDraft("새 목록"), title: "새 일", who: "me" });
    assert.deepEqual([add[0].op, add[0].list, add[0].owner, add[0].parent], ["add", "새 목록", "me", null]);
    // owner 목록 전에 만든 폼(`claude`를 사람으로 봄)도 보낼 동작이 없다(손대지 않은 폼)
    assert.deepEqual(draftOps({ ...one, owner: "claude" }, draftOf({ ...one, owner: "claude" }, [], null)), []);
    assert.equal(ownerKindOf("claude", ["claude"]), "agent");
    assert.equal(ownerKindOf("Priya Rao", ["claude"]), "person");
    assert.equal(ownerKindOf("agent", []), "none");
    assert.equal(canonicalLine("일", "me", null, "t-0001"), "- [ ] 일 (me) · t-0001");
    assert.equal(canonicalLine("일", "me", "aaa", "t-0001"), "- [ ] 일 (me) [aaa] · t-0001");
  });

  it("저장소 칸(plan §8.330): 폼은 바뀐 때만 보내고 빈 글은 비움, 「바로 시작」은 이름마다 머신을 묶고 덜 붐비는 머신이 기본", () => {
    const one = { ...by(4), repo: "aaa" };
    assert.deepEqual(draftOps(one, draftOf(one, [], null)), [], "그대로면 동작 없음");
    const cleared = draftOps(one, { ...draftOf(one, [], null), repo: "" });
    assert.deepEqual([cleared[0].repo, cleared[0].owner, cleared[0].title], ["", null, null]);
    const notesOnly = draftOps(one, { ...draftOf(one, [], null), notes: "새 메모" });
    assert.equal(notesOnly[0].repo, null, "칸을 안 고치면 None(그대로)");
    const add = draftOps(null, { ...emptyDraft("A"), title: "새 일", repo: REPO_DIR });
    assert.equal(add[0].repo, REPO_DIR);
    assert.equal(draftOps(null, { ...emptyDraft("A"), title: "새 일" })[0].repo, null);
    assert.deepEqual([repoName("/work/aaa/"), repoName("C:\\src\\rc")], ["aaa", "rc"]);
    const by_name = repoMachines({
      repos: [
        { path: "/work/aaa", machine: null },
        { path: "/srv/b/aaa", machine: "mac-b" },
        { path: "/work/rc", machine: null },
      ],
      local_machine: "mac-a",
    });
    assert.deepEqual([...by_name.keys()], ["aaa", "rc"]);
    assert.deepEqual(by_name.get("aaa"), [{ machine: "mac-a", path: "/work/aaa" }, { machine: "mac-b", path: "/srv/b/aaa" }]);
    assert.deepEqual([...repoMachines({ repos: [{ path: "/work/aaa", machine: null }], local_machine: null }).keys()], [], "로컬 agent가 없으면 host 줄은 뺀다");
    assert.equal(defaultMachine(["mac-a", "mac-b"], ["mac-c", "mac-b", "mac-a"]), "mac-b", "덜 붐비는 차례에서 후보에 든 첫 머신");
    assert.equal(defaultMachine(["mac-a"], []), "mac-a", "한 곳뿐이면 그곳");
    assert.equal(defaultMachine(["mac-a", "mac-b"], []), "", "고를 수 있는 머신이 없으면 host가 고른다");
  });

  it("상위 task는 고른 판의 블록 고정점으로 든다: 그대로면 옮기지 않고, 더할 때 그 블록을 보낸다", () => {
    const sub = by(2);
    const was = parentAnchor(board, sub);
    assert.deepEqual([was.title, was.block_sha256], ["하나", "sha-1"]);
    assert.deepEqual(draftOps(sub, draftOf(sub, [], was), was), [], "상위를 그대로 두면 옮기지 않는다");
    const add = draftOps(null, { ...emptyDraft("A", anchorOf(by(4))), title: "하위 새 일" });
    assert.deepEqual([add[0].parent.title, add[0].parent.block_sha256], ["둘", "sha-4"], "줄 번호가 아니라 고른 판의 블록을 보낸다(줄이 밀려도 host가 블록 hash로 찾는다)");
  });

  it("「나중」 항목: 들여쓰지 않은 글머리마다 하나, 들여쓴 줄은 붙고 빈 줄은 끝에서 뗀다", () => {
    const items = noteItems(["소개", "- 하나 `code`", "  이어진 줄", "", "- 둘", "1. 셋", ""]);
    assert.deepEqual(items, [["소개"], ["- 하나 `code`", "  이어진 줄"], ["- 둘"], ["1. 셋"]]);
    assert.deepEqual(itemToTask(items[1]), { title: "하나 `code`", notes: ["이어진 줄"] });
    const long = itemToTask([`- ${"가".repeat(130)}`]);
    assert.equal([...long.title].length, 120);
    assert.equal(long.notes[0], "가".repeat(130));
  });

  it("기록: 줄 차이와 되살릴 수 있는 항목", () => {
    assert.deepEqual(lineDiff("- [ ] a\n  메모", "- [ ] a (me)\n  메모"), [
      { kind: "add", text: "- [ ] a (me)" },
      { kind: "del", text: "- [ ] a" },
      { kind: "same", text: "  메모" },
    ]);
    assert.deepEqual(lineDiff(null, "x"), [{ kind: "add", text: "x" }]);
    const e = { action: "delete", restored_by: null };
    assert.equal(restorable(e), true);
    assert.equal(restorable({ ...e, restored_by: 3 }), false);
    assert.equal(restorable({ ...e, action: "update" }), false);
  });
});

describe("할 일 thread 연결(T3, plan §8.316)", () => {
  it("확인 필요: 형식 어긋남 다음이고 열린 thread보다 앞선다. 거르기 「확인 필요」", () => {
    const check = task({ thread: "t-0002", attention: "t-0002 끝남: 보고가 없다" });
    assert.equal(taskState(check, groups), "check");
    assert.equal(taskState({ ...check, problems: [MISREAD] }, groups), "problem");
    assert.equal(taskMatches(check, "check", "", groups), true);
    assert.equal(taskMatches(task({ thread: "t-0001" }), "check", "", groups), false);
  });

  it("배지: 형식 어긋남·확인 필요·이 기기가 보지 못한 me task를 센다", () => {
    const board = { project: "p", sha256: "", lists: [], unlinked_threads: [], notes_files: [], tasks: [task({ title: "a", attention: "t-0002 끝남: 보고가 없다" }), task({ title: "B", owner: "me" }), task({ title: "c", owner: "me" }), task({ title: "d" })] };
    assert.equal(taskBadge(board), 1, "본 목록이 아직 없으면 me task를 세지 않는다");
    assert.equal(taskBadge(board, new Set()), 3);
    assert.equal(taskBadge(board, new Set(mineKeys(board))), 1);
    assert.deepEqual(mineKeys(board), ["b", "c"]);
  });

  it("맡기기 글과 맡길 수 있는 task", () => {
    assert.equal(handOffText(task({ title: "로그인" })), "「로그인」 task를 thread로 맡겨 줘");
    assert.equal(handOffText(task({ title: "로그인", thread: "t-0002", attention: "t-0002 끝남: 보고가 없다" }), true), "「로그인」 task를 thread로 다시 맡겨 줘(앞 t-0002: t-0002 끝남: 보고가 없다)");
    const agents = ["claude"];
    assert.equal(startable(task({}), groups, agents), true);
    assert.equal(startable(task({ owner: "claude" }), groups, agents), true);
    assert.equal(startable(task({ owner: "codex@mac-b" }), groups, agents), true);
    assert.equal(startable(task({ owner: "me" }), groups, agents), false);
    assert.equal(startable(task({ owner: "Priya" }), groups, agents), false);
    assert.equal(startable(task({ thread: "t-0001" }), groups, agents), false, "열린 thread와 이어졌다");
    assert.equal(startable(task({ thread: "t-0002" }), groups, agents), true, "끝난 thread는 다시 맡긴다");
    assert.equal(startable(task({ problems: [MISREAD] }), groups, agents), false);
  });

  it("task의 모델·머신 칸이 정한 것은 바로 시작 창에서 잠근다(host `--from-task`가 다른 값을 거부한다)", () => {
    for (const o of [null, "", "agent", "me"]) assert.deepEqual(ownerFixes(task({ owner: o })), { profile: false, machine: false }, String(o));
    // 옛 host(칸 없음)는 owner 글을 나눈다
    assert.deepEqual(ownerFixes(task({ owner: "claude" })), { profile: true, machine: false });
    assert.deepEqual(ownerFixes(task({ owner: "codex@mac-a" })), { profile: true, machine: true });
    assert.deepEqual(ownerFixes(task({ owner: "@mac-a" })), { profile: false, machine: true });
    // 새 host(FRONT_API 1.75)는 나눠 준 칸을 쓴다: 모르는 맨 이름은 사람이라 잠그지 않는다
    assert.deepEqual(ownerFixes(task({ owner: "codex-fast", profile: "codex-fast", machine: null })), { profile: true, machine: false });
    assert.deepEqual(ownerFixes(task({ owner: "Priya", profile: null, machine: null })), { profile: false, machine: false });
  });

  it("모델·머신 칸(plan §8.335): host가 나눈 값이 먼저, 옛 host는 owner 글을 나누고, 폼은 칸마다 고쳐 `모델@머신`으로 보낸다", () => {
    // 끊긴 머신의 profile도 host가 모델로 읽어 준다(맡길 수 있는 목록이 비어도)
    assert.deepEqual(agentParts(task({ owner: "codex-fast", profile: "codex-fast", machine: null }), []), { profile: "codex-fast", machine: null });
    assert.equal(agentParts(task({ owner: "me", profile: null, machine: null }), ["me"]), null);
    assert.deepEqual(agentParts(task({ owner: "@mac-b" })), { profile: null, machine: "mac-b" });
    assert.equal(agentParts(task({ owner: "codex-fast" }), []), null, "옛 host: 목록에 없는 맨 이름은 사람");
    assert.deepEqual(agentParts(task({ owner: "codex-fast" }), ["codex-fast"]), { profile: "codex-fast", machine: null });
    assert.deepEqual([agentOwner("claude", "mac-b"), agentOwner("claude", ""), agentOwner("", "mac-b"), agentOwner(" ", " ")], ["claude@mac-b", "claude", "@mac-b", ""]);
    const t = task({ owner: "codex-fast@mac-a", profile: "codex-fast", machine: "mac-a", raw: "- [ ] 일 (codex-fast@mac-a)" });
    const d = draftOf(t, [], null);
    assert.deepEqual([d.who, d.profile, d.machine], ["agent", "codex-fast", "mac-a"]);
    assert.deepEqual(draftOps(t, d), [], "그대로면 동작 없음");
    const owner = (patch) => draftOps(t, { ...d, ...patch })[0].owner;
    assert.equal(owner({ machine: "" }), "codex-fast", "머신만 비움");
    assert.equal(owner({ profile: "claude" }), "claude@mac-a", "모델만 바꿈");
    assert.equal(owner({ profile: "", machine: "" }), "", "둘 다 비면 미할당");
    assert.equal(owner({ who: "me" }), "me");
    // 미할당 task의 폼은 agent에 두 칸이 빈 것이다
    const none = draftOf(task({ profile: null, machine: null }), [], null);
    assert.deepEqual([none.who, none.profile, none.machine], ["agent", "", ""]);
    assert.deepEqual(draftOps(task({ profile: null, machine: null }), none), []);
    assert.equal(startable(task({ owner: "codex-fast", profile: "codex-fast", machine: null }), groups, []), true);
  });

  it("잇기 후보: 형식 어긋남·열린 thread 연결을 빼고 제목·메모로 찾는다", () => {
    const board = { project: "p", sha256: "", lists: [], unlinked_threads: [], notes_files: [], tasks: [task({ title: "로그인", notes: ["쿠키"] }), task({ title: "배포", thread: "t-0001" }), task({ title: "문서", problems: [MISREAD] }), task({ title: "정리", thread: "t-0002" })] };
    assert.deepEqual(linkCandidates(board, groups, "").map((t) => t.title), ["로그인", "정리"]);
    assert.deepEqual(linkCandidates(board, groups, "쿠키").map((t) => t.title), ["로그인"]);
  });

  it("자동 정리 알림: 본 id보다 새, 되살리지 않은 auto_clean만", () => {
    const e = (id, over) => ({ id, at_ms: 1, by: "host 자동 정리", action: "auto_clean", list: "Backlog", title: `일${id}`, before: "- [ ] 일", after: null, restored_by: null, ...over });
    const log = [e(9), e(8, { restored_by: 10 }), e(7, { action: "delete" }), e(5)];
    assert.deepEqual(newAutoCleans(log, 5).map((x) => x.id), [9]);
    assert.deepEqual(newAutoCleans(log, 0).map((x) => x.id), [9, 5]);
    assert.equal(lastLogId(log), 9);
    assert.equal(lastLogId([]), 0);
  });
});

// 한 목록의 예약(plan §8.393, t-0611): 맨 위 host 예약 차례, 하위 함께(2A), 저장소 거르기, 차례 순번
describe("할 일 한 목록과 예약", () => {
  const t = (line, title, over = {}) => task({ line, title, id: `ID${line}`, short_id: `d-${String(line).padStart(4, "0")}`, block_sha256: `sha-${line}`, repo: "aaa", ...over });
  const q = (line, position, over = {}) => ({ task_id: `ID${line}`, task: t(line, `옛 ${line}`), position, state: "queued", reason: "자리 대기", ...over });
  // 1 상위[2 하위, 3 하위], 4 rc 일, 5 진행 중(thread), 6 둘째 목록 일, 7 저장소 없음
  const tasks = [t(1, "상위"), t(2, "하위 하나", { parent: 1 }), t(3, "하위 둘", { parent: 1 }), t(4, "rc 일", { repo: "rc" }), t(5, "진행 일", { thread: "t-0001" }), t(6, "둘째 목록", { list: "Later" }), t(7, "정하지 않음", { repo: null })];
  const board = (queue) => ({ project: "p", sha256: "", lists: [{ name: "Backlog", notes: [] }, { name: "Later", notes: [] }], tasks, unlinked_threads: [], notes_files: [], repos: ["aaa", "rc"], queue });
  const rows = (rs) => rs.map((r) => (r.kind === "gap" ? `gap:${r.n}:${r.repos.join("·")}` : `${r.kind}:${r.t.title}${r.kind === "under" ? `@${r.depth}` : ""}${r.kind === "task" && r.reservedKids ? `+${r.reservedKids}` : ""}${r.kind === "reserved" && r.crumb ? `<${r.crumb.title}` : ""}`));

  it("예약은 host 차례대로 맨 위, 시작했거나 끝·취소한 예약은 원래 자리. 상위 예약은 하위를 데리고 올라간다(2A)", () => {
    const b = board([q(4, 2), q(1, 1), q(5, 3, { state: "running" }), q(6, 4, { state: "cancelled" }), q(7, 5, { state: "check", reason: "알려지지 않은 모델" })]);
    assert.deepEqual(pendingQueue(b).map((x) => x.task_id), ["ID1", "ID4", "ID7"]);
    const m = taskList(b, "all", "", null, groups);
    assert.deepEqual(rows(m.top), ["reserved:상위", "under:하위 하나@1", "under:하위 둘@1", "reserved:rc 일", "reserved:정하지 않음"]);
    assert.deepEqual(rows(m.rest), ["task:진행 일", "task:둘째 목록"]);
    assert.equal(m.counts.reserved, 3);
    assert.equal(queueWhy(pendingQueue(b)[0]), null, "그냥 자리 대기는 행에도 상세에도 까닭이 없다");
    assert.equal(queueWhy(pendingQueue(b)[2]), "알려지지 않은 모델");
    assert.equal(queueWhy({ ...pendingQueue(b)[0], reason: "지정 조건의 머신 연결·모델 가용성 대기" }), "지정 조건의 머신 연결·모델 가용성 대기");
  });

  it("하위만 예약하면 상위 경로가 붙고 상위 행에 「하위 N 예약됨」. 「예약」 거르기는 예약 행만", () => {
    const b = board([q(3, 1)]);
    const m = taskList(b, "all", "", null, groups);
    assert.deepEqual(rows(m.top), ["reserved:하위 둘<상위"]);
    assert.deepEqual(rows(m.rest).slice(0, 2), ["task:상위+1", "task:하위 하나"]);
    const r = taskList(board([q(1, 1)]), "reserved", "", null, groups);
    assert.deepEqual(rows(r.top), ["reserved:상위"]);
    assert.deepEqual(r.rest, []);
    assert.deepEqual(taskList(board([]), "reserved", "", null, groups).top, []);
  });

  it("저장소 거르기는 상태 거르기와 함께 걸리고, 숨은 예약은 그 자리에 흐린 한 줄로 남는다", () => {
    const b = board([q(1, 1), q(4, 2), q(7, 3), q(6, 4)]);
    const m = taskList(b, "reserved", "", "rc", groups);
    assert.deepEqual(rows(m.top), ["gap:1:aaa", "reserved:rc 일", "gap:2:정하지 않음·aaa"]);
    assert.equal(m.counts.reserved, 1);
    assert.equal(m.counts.all, 1);
    const none = taskList(b, "all", "", "", groups);
    assert.deepEqual(rows(none.top), ["gap:2:aaa·rc", "reserved:정하지 않음", "gap:1:aaa"]);
    assert.deepEqual(taskList(board([q(1, 1)]), "all", "", "rc", groups).top, [], "보이는 예약이 없으면 숨은 줄도 없다");
    assert.deepEqual(repoChoices(b).map((c) => [c.key, c.n]), [["aaa", 5], ["rc", 1], ["", 1]]);
  });

  it("예약 차례 순번: 놓은 자리 앞·뒤, 같은 자리는 없음, 일반 → 예약은 그 자리에", () => {
    const order = ["A", "B", "C", "D"];
    assert.equal(queueDropPosition(order, "D", "A", "before"), 1);
    assert.equal(queueDropPosition(order, "A", "C", "after"), 3);
    assert.equal(queueDropPosition(order, "A", "B", "before"), null);
    assert.equal(queueDropPosition(order, "B", "A", "after"), null);
    assert.equal(queueDropPosition(order, "A", "A", "after"), null);
    assert.equal(queueDropPosition(order, "N", "B", "before"), 2, "새 예약은 B 앞");
    assert.equal(queueDropPosition(order, "N", "D", "after"), 5);
  });

  it("예약 위로·아래로는 보이는 예약과 자리를 바꾸고, 사이 숨은 예약은 그대로. 맨 앞으로는 전체 맨 앞", () => {
    const order = ["A", "H", "B", "C"];
    const vis = (x) => x !== "H";
    assert.equal(queueStep(order, "B", "up", vis), 1, "A 앞으로(H는 그대로 B 뒤로 밀린다)");
    assert.equal(queueStep(order, "B", "up"), 2);
    assert.equal(queueStep(order, "A", "down", vis), 3, "B 바로 뒤");
    assert.equal(queueStep(order, "A", "up", vis), null);
    assert.equal(queueStep(order, "C", "down", vis), null);
    assert.equal(queueStep(order, "C", "top", vis), 1);
    assert.equal(queueStep(order, "A", "top", vis), null);
    assert.equal(queueStep(order, "Z", "up", vis), null);
  });

  it("예약을 풀면 돌아갈 자리: 같은 상위의 다음 일반 할 일 위, 없으면 상위 하위 끝·목록 맨 끝", () => {
    const b = board([q(4, 1), q(5, 2)]);
    assert.equal(unreserveHome(b, tasks[3]), "d-0006 둘째 목록 바로 위", "다음 예약(진행 일)은 건너뛴다");
    assert.equal(unreserveHome(b, tasks[2]), "d-0001 상위 하위 끝");
    assert.equal(unreserveHome(b, tasks[6]), "목록 맨 끝");
  });
});
