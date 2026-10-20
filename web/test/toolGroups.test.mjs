// 대화의 도구 호출 묶음(src/toolGroups.ts, plan §8.304). t3code `presentation.ts` summarizeToolGroup·toolGroupAction과
// `MessagesTimeline.logic.ts`의 이어진 work 묶기를 aaa 기록(「이름: 인자」 도구 줄)으로.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { groupChatRows, groupExpanded, rowIndexById, summarizeToolGroup, toggleExpanded, toolAction, toolGroupKind } from "../src/toolGroups.ts";
import { chatRowIds } from "../src/chatScroll.ts";

const tool = (text) => ({ role: "tool", text });
const msg = (role, text) => ({ role, text });

describe("동작 분류(참고 toolGroupAction·toolNameToken)", () => {
  it("Claude와 Codex 도구 이름을 같은 동작으로", () => {
    assert.deepEqual(
      ["Bash: ls", "exec_command: ls", "shell: ls", "Edit: a", "apply_patch", "Read: a", "view_image: a.png", "Grep: x", "Glob: *.rs", "WebSearch: q", "WebFetch: u", "Agent: 조사", "TodoWrite"].map(toolAction),
      ["command", "command", "command", "edit", "edit", "read", "read", "code-search", "code-search", "search", "search", "other", "other"],
    );
  });
  it("MCP·이름공간 도구는 이름이 read여도 그 밖의 도구", () => {
    assert.equal(toolAction("mcp__docs__read: x"), "other");
    assert.equal(toolAction("docs.read"), "other");
  });
});

describe("요약 문구(참고 summarizeToolGroup)", () => {
  it("나온 차례대로 「명령 2개 실행, 파일 3개 읽음」", () => {
    assert.equal(summarizeToolGroup(["Bash: a", "Read: x", "Bash: b", "Read: y", "Read: z"].map(tool)), "명령 2개 실행, 파일 3개 읽음");
  });
  it("두 가지까지 고르고(명령·수정 먼저, 그 밖의 도구 나중) 나머지는 「그 밖의 동작 N개」", () => {
    const texts = ["Grep: a", "Edit: a.rs", "Edit: a.rs", "Edit: b.rs", "WebSearch: q", "mcp__x__y: z"];
    assert.equal(summarizeToolGroup(texts.map(tool)), "코드 1번 검색, 파일 2개 수정, 그 밖의 동작 2개");
    assert.equal(summarizeToolGroup(["Agent: a", "Read: x", "Bash: b"].map(tool)), "파일 1개 읽음, 명령 1개 실행, 그 밖의 동작 1개");
  });
  it("수정은 서로 다른 파일 수, 파일 없는 수정은 하나씩", () => {
    assert.equal(summarizeToolGroup(["Edit: a", "Write: a", "apply_patch", "apply_patch"].map(tool)), "파일 3개 수정");
  });
  it("생각은 세지 않고, 생각만이면 「생각 (×N)」", () => {
    assert.equal(summarizeToolGroup([msg("thinking", "음"), tool("Bash: a"), msg("thinking", "음"), tool("Bash: b")]), "명령 2개 실행");
    assert.equal(summarizeToolGroup([msg("thinking", "a"), msg("thinking", "b")]), "생각 (×2)");
  });
  it("머리 그림 종류", () => {
    assert.equal(toolGroupKind(["Bash: a", "Bash: b"].map(tool)), "command");
    assert.equal(toolGroupKind(["Bash: a", "Read: b"].map(tool)), "mixed");
    assert.equal(toolGroupKind([msg("thinking", "a"), msg("thinking", "b")]), "reasoning");
  });
});

describe("묶기", () => {
  const rows = (messages) => groupChatRows(messages, messages.map((_, i) => `m${i}`));
  it("이어진 도구·생각 줄 둘 이상만 묶고, 사람·답 말이 끊는다. 하나는 그대로", () => {
    const messages = [msg("user", "q"), tool("Bash: a"), msg("thinking", "t"), tool("Read: b"), msg("assistant", "a"), tool("Write: c"), msg("assistant", "b")];
    assert.deepEqual(rows(messages), [
      { kind: "message", id: "m0", index: 0 },
      { kind: "tools", id: "m1", indices: [1, 2, 3], memberIds: ["m1", "m2", "m3"] },
      { kind: "message", id: "m4", index: 4 },
      { kind: "message", id: "m5", index: 5 },
      { kind: "message", id: "m6", index: 6 },
    ]);
  });
  it("앞 쪽을 읽어 묶음 앞에 도구 줄이 붙어도 옛 첫 줄 id로 같은 묶음을 찾고 펼침이 남는다", () => {
    const newest = [tool("Read: b"), tool("Read: c"), msg("assistant", "a")].map((m, i) => ({ ...m, id: `f:${10 + i}` }));
    const older = [msg("user", "q"), tool("Bash: a")].map((m, i) => ({ ...m, id: `f:0${i}` }));
    const before = groupChatRows(newest, chatRowIds(newest));
    const opened = toggleExpanded(before[0], new Set());
    const all = [...older, ...newest];
    const after = groupChatRows(all, chatRowIds(all));
    assert.equal(after[1].id, "f:01");
    assert.equal(rowIndexById(after).get(before[0].id), 1);
    assert.equal(groupExpanded(after[1], opened), true);
    assert.equal(groupExpanded(after[1], toggleExpanded(after[1], opened)), false);
  });
});
