// 자동화 편집 폼 고르기 칸과 설정 보기 흐름 카드의 규칙(src/automationChoices.ts, t-1538). 목록의 정본은 host assignable·profiles다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { effortsOf, flowOf, matching, modelsOf, moveEarlier, profileChip, profileChoices, repoNames } from "../src/automationChoices.ts";

const view = (name, model, effort, harness = "claude") => ({ name, harness, model, effort, description: "", args: [], builtin: model === null });

describe("자동화 고르기 칸", () => {
  it("profile은 assignable 이름에 host 모델·생각 수준을 붙이고, 모델이 없는 기본 profile은 harness·「기본」이다", () => {
    const host = [view("opus-high", "opus", "high"), view("opus-low", "opus", "low"), view("claude", null, null), view("hidden", "opus", "max")];
    // safety가 막은 hidden은 assignable에 없다: 고르기 목록에도 없다. 다른 머신에만 있는 이름은 모델을 모른다
    const owners = [{ machine: "mac-a", connected: true, profiles: ["opus-high", "opus-low", "claude"] }, { machine: "mac-b", connected: true, profiles: ["codex-fast"] }];
    const c = profileChoices(host, owners);
    assert.deepEqual(c.map((p) => [p.name, p.model, p.effort]), [["claude", "claude", "기본"], ["opus-high", "opus", "high"], ["opus-low", "opus", "low"], ["codex-fast", "다른 머신", "기본"]]);
    assert.deepEqual(modelsOf(c), ["claude", "opus", "다른 머신"]);
    assert.deepEqual(effortsOf(c, "opus"), ["high", "low"]);
    assert.deepEqual(matching(c, "opus", "low").map((p) => p.name), ["opus-low"]);
    // assignable을 못 읽으면(옛 host) host 머신 profile 전체다
    assert.equal(profileChoices(host, null).length, 4);
  });
  it("같은 모델·생각 수준의 profile이 여럿이면 모두 고를 후보로 남는다", () => {
    const c = profileChoices([view("a-high", "opus", "high"), view("b-high", "opus", "high")], null);
    assert.deepEqual(matching(c, "opus", "high").map((p) => p.name), ["a-high", "b-high"]);
  });
  it("목록에 없는 기존 값은 지우지 않고 표지를 단 칩이다", () => {
    const chip = profileChip("typo-high", profileChoices([view("opus-high", "opus", "high")], null));
    assert.deepEqual(chip, { name: "typo-high", model: "", effort: "", known: false });
  });
  it("차례 바꾸기는 한 칸 앞으로만, 맨 앞은 그대로다", () => {
    assert.deepEqual(moveEarlier(["a", "b", "c"], 2), ["a", "c", "b"]);
    assert.deepEqual(moveEarlier(["a", "b"], 0), ["a", "b"]);
  });
  it("저장소 이름은 경로의 마지막 이름이다(host matches_repo와 같다), 겹치면 한 번", () => {
    assert.deepEqual(repoNames([{ path: "/w/app", machine: null }, { path: "/x/app/", machine: "mac-b" }, { path: "C:\\work\\tool", machine: "win" }]), ["app", "tool"]);
  });
});

describe("설정 보기 흐름 카드", () => {
  it("일정 → 새 thread 시작: 언제·무엇을·어디서와 한도 알약", () => {
    const f = flowOf({
      on: { schedule: { daily: "09:00" } },
      do: { start: { title: "점검 {date}", profiles: ["sol-high", "opus-high"], machines: ["mac-a"], reports: true } },
      limits: { usage_session_below: 5, usage_week_below: 10, max_per_day: 1 },
    });
    assert.equal(f.when.title, "매일 09:00");
    assert.deepEqual(f.when.notes, ["놓친 차례는 한 번으로"]);
    assert.equal(f.what.title, "새 thread 시작");
    assert.deepEqual(f.what.notes, ["「점검 {date}」", "사건마다 새 thread", "사건 thread 보고 붙임"]);
    assert.deepEqual(f.where.rows.map((r) => [r.label, r.chips, r.empty]), [["profile", ["sol-high", "opus-high"], "없음"], ["머신", ["mac-a"], "자동"], ["저장소", [], "작업 폴더"]]);
    assert.deepEqual(f.limits, ["5시간 창 5% 아래면 쉼", "주간 10% 아래면 쉼", "동시 1", "하루 1번"]);
  });
  it("CI 조건과 명령 동작은 저장소·branch·사건과 실행 머신을 보인다", () => {
    const f = flowOf({ on: { ci: { repo: "app", branches: ["main"], events: ["failed"] } }, do: { command: { command: "make", machine: "mac-b" } }, limits: {} });
    assert.equal(f.when.title, "CI 결과");
    assert.deepEqual(f.when.rows.map((r) => [r.label, r.chips]), [["저장소", ["app"]], ["branch", ["main"]], ["workflow", []], ["사건", ["failed"]]]);
    assert.deepEqual(f.where.rows.map((r) => [r.label, r.chips]), [["명령", ["make"]], ["실행 머신", ["mac-b"]]]);
    assert.deepEqual(f.limits, ["하루 상한 없음"]);
  });
  it("폼으로 모르는 조건·동작은 흐름 카드가 없다(host 낱말 줄로 돌아간다)", () => {
    assert.equal(flowOf({ on: { future: {} }, do: { inbox: { text: "" } } }), null);
  });
});
