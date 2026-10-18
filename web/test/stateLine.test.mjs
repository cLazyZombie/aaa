// thread 줄 상태 말(src/sidebarModel.ts stateLine): herdr-projects sidebar.rs `state_line` 시험
// (`assert_eq!(state_line(Group::Landing, …), "landing · PR #4")`)을 aaa 말로.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { prNumber, stateLine } from "../src/sidebarModel.ts";

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
