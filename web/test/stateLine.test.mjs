// thread 줄 상태 말(src/sidebarModel.ts stateLine): herdr-projects sidebar.rs `state_line` 시험
// (`assert_eq!(state_line(Group::Landing, …), "landing · PR #4")`)을 aaa 말로.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { needsYou, prNumber, projectLine, stateLine } from "../src/sidebarModel.ts";

const thread = (group, pr = null) => ({ group, pr, status: "open", agent: null, updated_at_ms: 0 });
const pr = (n) => ({ url: `https://github.com/o/app/pull/${n}`, state: "OPEN", review: "APPROVED" });

describe("검토·병합 중 줄의 사실", () => {
  it("따라가는 PR이 있으면 PR 번호, 없으면 보고", () => {
    assert.equal(stateLine(thread("landing", pr(4)), true, 0), "병합 중 · PR #4");
    assert.equal(stateLine(thread("ready_for_review", pr(12)), true, 0), "검토 대기 · PR #12");
    assert.equal(stateLine(thread("ready_for_review"), true, 0), "검토 대기 · 보고");
    assert.equal(stateLine(thread("landing"), true, 0), "병합 중 · 보고");
  });
  it("번호가 아닌 주소 끝은 쓰지 않는다", () => {
    assert.equal(prNumber("https://github.com/o/app/pull/7"), "7");
    assert.equal(prNumber("https://github.com/o/app/pull/"), null);
    assert.equal(prNumber("https://github.com/o/app/pull/7x"), null);
    assert.equal(prNumber(null), null);
  });
});

// 프로젝트 머리 셈(src/sidebarModel.ts projectLine, plan §8.203): 사람이 볼 thread(herdr-projects needs you: 확인 필요·검토)를
// 한 수로 세지 않고 줄 글(「확인 필요」·「검토 대기」)과 같은 말로 나눠 보인다(QA t-0362 O1). 0인 쪽은 뺀다. 세는 규칙(needsYou)은 그대로다
describe("프로젝트 머리 셈", () => {
  it("확인 필요와 검토 대기를 나눠 세고, 0인 쪽은 뺀다", () => {
    assert.equal(projectLine(["waiting_on_you", "ready_for_review"]), "확인 필요 1 · 검토 대기 1");
    assert.equal(projectLine(["ready_for_review"]), "검토 대기 1");
    assert.equal(projectLine(["waiting_on_you", "waiting_on_you", "working"]), "확인 필요 2 · 작업 중 1");
    assert.equal(
      projectLine(["idle", "working", "working", "landing", "ready_for_review", "waiting_on_you"]),
      "확인 필요 1 · 검토 대기 1 · 작업 중 2 · 병합 중 1",
    );
  });
  it("셀 것이 없으면 쉼, 멈춘 프로젝트는 멈춤", () => {
    assert.equal(projectLine(["idle"]), "쉼");
    assert.equal(projectLine([]), "쉼");
    assert.equal(projectLine(["ready_for_review"], true), "멈춤");
  });
  it("사람이 볼 thread는 여전히 둘 다(머리 강조·탭 막대 수)", () => {
    assert.deepEqual(["waiting_on_you", "ready_for_review", "landing", "working"].map(needsYou), [true, true, false, false]);
  });
});
