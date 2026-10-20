// 입력칸 「/」 목록 규칙(src/slashCommands.ts, plan §8.323): 맨 앞에서만 열기, 순위, 숨김, 확인창·터미널 알림 판정
import assert from "node:assert/strict";
import { it } from "node:test";
import { commandOf, groups, listed, rank, sendAction, slashQuery } from "../src/slashCommands.ts";

const cmd = (name, more = {}) => ({ name, description: "", source: "builtin", kind: "send", hint: null, aliases: [], idle_only: false, insert: `/${name} `, ...more });
const list = {
  harness: "claude",
  builtin_version: "2.1.304",
  commands: [
    cmd("compact", { idle_only: true, description: "대화를 요약해 컨텍스트를 비운다" }),
    cmd("context"),
    cmd("model", { kind: "picker", hint: "/model sonnet" }),
    cmd("clear", { kind: "confirm", aliases: ["reset", "new"] }),
    cmd("exit", { kind: "danger", aliases: ["quit"] }),
    cmd("frontend:review", { source: "user" }),
    cmd("finish", { source: "skill", description: "리뷰하고 마무리한다" }),
  ],
};

it("메시지 맨 앞의 「/이름」에서만 연다(줄 중간·경로·인자 뒤는 아니다)", () => {
  assert.equal(slashQuery("/", 1), "");
  assert.equal(slashQuery("/co", 3), "co");
  assert.equal(slashQuery("/co 지시", 3), "co");
  for (const [text, caret] of [["a /co", 5], ["/Users/me/a", 12], ["/tmp/x", 4], ["/compact 지시", 12], ["/coX", 3], ["", 0]])
    assert.equal(slashQuery(text, caret), null, `${text}@${caret}`);
});

it("순위: 앞 < 단어 경계 < 부분 < 글자 순서 < 설명, 같은 순위는 쓴 횟수", () => {
  const names = (q, usage) => rank(listed(list, false), q, usage).map((c) => c.name);
  assert.deepEqual(names("co"), ["compact", "context"]);
  assert.deepEqual(names("co", { context: 3 }), ["context", "compact"]);
  assert.deepEqual(names("rev"), ["frontend:review"]);
  assert.deepEqual(names("cmpt"), ["compact"]);
  assert.deepEqual(names("요약"), ["compact"]);
  // 다른 이름으로도 찾는다
  assert.deepEqual(names("reset"), ["clear"]);
});

it("숨김: 끝내는 명령은 목록에 없고, 작업 중이면 쉴 때만 쓰는 명령도 없다. 빈 「/」는 출처별 묶음", () => {
  assert.ok(!listed(list, false).some((c) => c.name === "exit"));
  assert.ok(!listed(list, true).some((c) => c.name === "compact"));
  assert.deepEqual(groups(listed(list, false)).map((g) => [g.source, g.items.length]), [["builtin", 4], ["skill", 1], ["user", 1]]);
});

it("보낼 때: 비우는·끝내는 명령(다른 이름 포함)은 확인창, 선택창은 인자 없을 때만 알림, 모르는 것·경로는 그냥", () => {
  const action = (text) => {
    const found = commandOf(list, text);
    return found ? sendAction(found.command.kind, found.args) : null;
  };
  assert.deepEqual(
    ["/clear", " /reset ", "/new", "/exit", "/quit", "/model", "/model sonnet", "/context", "/unknown", "/Users/me/a.txt", "clear", "/clear 그리고"].map(action),
    ["confirm", "confirm", "confirm", "confirm", "confirm", "picker", "send", "send", null, null, null, "confirm"],
  );
  assert.equal(sendAction("screen", false), "screen");
  assert.equal(commandOf(null, "/clear"), null);
});
