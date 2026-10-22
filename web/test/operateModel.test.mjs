// 운영 화면 규칙(src/operateModel.ts, 관리 GUI W4, plan §8.343). 거부·검사의 정본은 host다(m13 `resolve_keeps_dirty_worktree_and_unmerged_branch`,
// project.rs `repo_edit_replaces_in_place_and_settings_view_reads_it`).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { canOperate, canPark, checkSetting, inboxOpen, needsAnswer, parkPending, parseRepoArg, repoArg, repoGroups, resolveDefaults, resolveRequest, threadActions, unparkDefault, unparkWay } from "../src/operateModel.ts";

describe("운영 화면", () => {
  it("설정 읽기·끝내기 미리보기·정리 실행은 1.77 이상 host만", () => {
    for (const v of [undefined, null, "1.76", "2.77"]) assert.equal(canOperate(v), false, String(v));
    for (const v of ["1.77", "1.90"]) assert.equal(canOperate(v), true, v);
  });
  it("thread 동작: 도는 세션은 다시 시작하지 않고, brief는 받기 전에만 다시 보내며, 데려온 thread는 다시 띄우지 않으며, 끝낸 thread는 다시 열기만", () => {
    const open = { status: "open", thread_kind: "worktree", cleanup_pending: undefined, brief: "pending", role: "worker", automation: undefined };
    assert.deepEqual(threadActions(open, true), { resolve: true, reopen: false, restart: false, brief: true, park: true, unpark: false });
    // 받은 brief는 다시 보내지 않는다
    assert.equal(threadActions({ ...open, brief: "delivered" }, true).brief, false);
    assert.deepEqual(threadActions(open, false), { resolve: true, reopen: false, restart: true, brief: false, park: true, unpark: false });
    assert.equal(threadActions({ ...open, thread_kind: "adopted" }, false).restart, false);
    assert.deepEqual(threadActions({ ...open, status: "starting" }, false), { resolve: false, reopen: false, restart: false, brief: false, park: false, unpark: false });
    assert.deepEqual(threadActions({ ...open, status: "resolved" }, false), { resolve: false, reopen: true, restart: false, brief: false, park: false, unpark: false });
    // 정리가 끊긴 thread는 끝내기로 이어 간다
    assert.equal(threadActions({ ...open, status: "resolved", cleanup_pending: true }, false).resolve, true);
  });
  it("보류(plan §8.421): 사람이 띄운 열린 일반 thread만 보류하고, 보류한 thread는 다시 시작 대신 이어 하기·끝내기", () => {
    for (const v of [undefined, null, "1.103"]) assert.equal(canPark(v), false, String(v));
    assert.equal(canPark("1.104"), true);
    const open = { status: "open", thread_kind: "worktree", cleanup_pending: undefined, brief: "delivered", role: "worker", automation: undefined };
    for (const t of [{ ...open, role: "evaluator" }, { ...open, automation: "nightly" }, { ...open, thread_kind: "adopted" }, { ...open, status: "failed" }]) {
      assert.equal(threadActions(t, true).park, false, JSON.stringify(t));
    }
    const parked = { ...open, status: "parked", park: { parked_at_ms: 5 } };
    assert.deepEqual(threadActions(parked, false), { resolve: true, reopen: false, restart: false, brief: false, park: false, unpark: true });
    assert.equal(parkPending({ status: "open", park: { parked_at_ms: null } }), true);
    assert.equal(parkPending({ status: "open", park: undefined }), false);
    assert.equal(parkPending(parked), false);
  });
  it("이어 하기 창: 같은 머신은 같은 대화(잇지 못하면 새 대화), 다른 머신은 새 worktree와 요약 brief. 막히지 않은 보류 머신이 기본", () => {
    assert.equal(unparkWay({ same: true, conversation: "resume" }, true), "같은 대화를 그대로 잇는다");
    assert.match(unparkWay({ same: true, conversation: "new" }, true), /같은 자리에서 새 대화/);
    assert.match(unparkWay({ same: false, conversation: "new", repo: "/r" }, true), /push된 branch로 새 worktree/);
    assert.match(unparkWay({ same: false, conversation: "new" }, false), /새 작업 폴더/);
    const a = { machine: "mac-a", same: true, blocked: undefined };
    const b = { machine: "mac-b", same: false, blocked: undefined };
    assert.equal(unparkDefault([b, a]), "mac-a");
    assert.equal(unparkDefault([b, { ...a, blocked: "끊김" }]), "mac-b");
    assert.equal(unparkDefault([{ ...b, blocked: "push" }, { ...a, blocked: "끊김" }]), null);
  });
  it("끝내기: 위험하거나 미리보기를 모르면 남기기가 기본이고, 남기면 버리기는 보내지 않는다", () => {
    assert.deepEqual(resolveDefaults(null), { keep: true });
    assert.deepEqual(resolveDefaults({ keep_default: false }), { keep: false });
    const t = { project: "demo", id: "t-0001" };
    assert.deepEqual(resolveRequest(t, { keep: true, skipCopy: false, discard: true }), {
      op: "thread_resolve", project: "demo", thread: "t-0001", keep_worktree: true, reopen: false, skip_copy: false, discard_uncopied: false,
    });
    assert.equal(resolveRequest(t, { keep: false, skipCopy: true, discard: true }).discard_uncopied, true);
  });
  it("저장소 인자는 host `parse_repo_arg`와 같다: `@` 뒤가 머신 이름 꼴일 때만 머신", () => {
    assert.deepEqual(parseRepoArg(" /work/app@mac-a "), { path: "/work/app", machine: "mac-a" });
    assert.deepEqual(parseRepoArg("git@github.com:o/r.git"), { path: "git@github.com:o/r.git", machine: null });
    assert.deepEqual(parseRepoArg("/work/app"), { path: "/work/app", machine: null });
    assert.equal(repoArg({ path: "/work/app", machine: "mac-a" }), "/work/app@mac-a");
    assert.equal(repoArg({ path: "/work/app", machine: null }), "/work/app");
  });
  it("저장소 묶음은 할 일 저장소 칸 이름(경로 마지막 폴더)마다 머신별 자리, 머신 없는 것은 host 머신", () => {
    const g = repoGroups([{ path: "/work/app", machine: null }, { path: "D:\\src\\lib", machine: "win-a" }, { path: "/w/app/", machine: "mac-b" }], "mac-a");
    assert.deepEqual(g.map((x) => [x.name, x.rows.map((r) => r.machine)]), [["app", ["mac-a", "mac-b"]], ["lib", ["win-a"]]]);
  });
  it("숫자 설정은 host와 같은 범위다", () => {
    assert.equal(checkSetting("max_parallel_threads", "0"), "1~1000의 수다");
    assert.equal(checkSetting("max_parallel_threads", "1"), null);
    assert.equal(checkSetting("auto_resolve_days", "0"), null);
    assert.equal(checkSetting("auto_resolve_days", "1001"), "0~1000의 수다");
    assert.equal(checkSetting("auto_resolve_days", "2.5"), "0~1000의 수다");
  });
  it("알림함 배지는 끝내지 않은 항목과 싣지 못한 항목, post 탭은 답할 요청을 「답할 것」에 맡긴다", () => {
    assert.equal(inboxOpen([{ done: false }, { done: true }, { done: false }], 3), 5);
    // 사용자가 답할 요청 post 항목은 「답할 것」 배지가 센다(두 배지가 같은 항목을 세지 않는다)
    assert.equal(inboxOpen([{ done: false, item_kind: "post", event: "post request for user" }, { done: false, item_kind: "post", event: "post notice for user" }], 0), 1);
    assert.equal(needsAnswer({ to: "user", post_type: "request", needs_user: true }), true);
    assert.equal(needsAnswer({ to: "user", post_type: "notice", needs_user: false }), false);
    assert.equal(needsAnswer({ to: "t-0001", post_type: "request", needs_user: true }), false);
  });
});
