// 자동화 편집 초안 규칙(src/automationForm.ts, 관리 GUI W2, plan §8.332). 합치기·검사의 정본은 host 유닛 automation::edit이다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { actionKind, bodyOf, canEditAutomations, conditionKind, lineDiff, newForm, parseList, parsePercent, setAction, setCondition, setField, USAGE_DEFAULTS, validName } from "../src/automationForm.ts";

describe("자동화 편집 초안", () => {
  it("편집은 1.74 이상 host만: 옛 host는 새 요청을 모른다", () => {
    for (const v of [undefined, null, "", "1.73", "2.74"]) assert.equal(canEditAutomations(v), false, String(v));
    for (const v of ["1.74", "1.80"]) assert.equal(canEditAutomations(v), true, v);
  });
  it("새 초안은 사용량 문턱 기본값(5시간 5%·주간 10%)을 채우고 횟수 제한은 두지 않는다", () => {
    const f = newForm();
    assert.deepEqual(f.limits, USAGE_DEFAULTS);
    assert.deepEqual(USAGE_DEFAULTS, { usage_session_below: 5, usage_week_below: 10 });
    assert.equal(conditionKind(f), "schedule");
    assert.equal(actionKind(f), "inbox");
  });
  it("조건 종류를 바꾸면 겹치는 칸(일정·저장소·branch)을 잇고, 다른 조건의 칸은 남기지 않는다", () => {
    let f = setField(newForm(), "on", "daily", "07:30");
    f = setCondition(f, "command");
    assert.deepEqual(f.on, { command: { daily: "07:30", command: "" } });
    f = setCondition(f, "git");
    assert.deepEqual(Object.keys(f.on), ["git"]);
    f = setField(f, "on", "repo", "app");
    f = setCondition(f, "ci");
    assert.equal(f.on.ci.repo, "app");
    assert.deepEqual(f.on.ci.events, ["failed"]);
  });
  it("동작 종류를 바꿔도 본문 글은 잇는다. 배포 제안은 글 칸이 없다", () => {
    let f = setField(newForm(), "do", "text", "본다");
    f = setAction(f, "start");
    assert.equal(f.do.start.text, "본다");
    assert.deepEqual(bodyOf(f), { inline: "본다", file: null });
    f = setAction(f, "deploy");
    assert.equal(f.do, "deploy");
    assert.equal(bodyOf(f).inline, null);
  });
  it("따로 둔 본문 파일(text_file·brief_file)은 파일로 고친다", () => {
    const f = { on: { schedule: { every: "1h" } }, do: { start: { title: "t", profiles: [], machines: [], brief_file: "automations/a.md" } } };
    assert.deepEqual(bodyOf(f), { inline: null, file: "automations/a.md" });
  });
  it("빈 칸은 칸을 지워 host 기본값을 쓴다. 후보 목록은 빈 채로 남겨 host가 오류를 알린다", () => {
    let f = setAction(newForm(), "start");
    f = setField(f, "do", "profiles", []);
    assert.deepEqual(f.do.start.profiles, []);
    f = setField(f, "limits", "usage_week_below", null);
    assert.equal("usage_week_below" in f.limits, false);
    assert.deepEqual(parseList(" a, b,,\nc "), ["a", "b", "c"]);
    assert.equal(parsePercent(""), null);
    assert.equal(parsePercent("101"), "bad");
    assert.equal(parsePercent("7.5"), 7.5);
  });
  it("이름은 host 파일 이름 규칙과 같다", () => {
    for (const n of ["ci-main", "a_1", "x"]) assert.equal(validName(n), true, n);
    for (const n of ["", "-a", "A", "a b", "a/b", "x".repeat(49)]) assert.equal(validName(n), false, n);
  });
  it("충돌 차이: 내 초안에만 있는 줄은 −, 지금 파일에만 있는 줄은 +", () => {
    const d = lineDiff("a\nb\nc", "a\nx\nc");
    assert.deepEqual(d.map((l) => `${l.kind}:${l.text}`), ["same:a", "add:x", "del:b", "same:c"]);
    assert.deepEqual(lineDiff("same", "same").map((l) => l.kind), ["same"]);
  });
});
