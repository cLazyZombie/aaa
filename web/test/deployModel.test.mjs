// 웹 「배포」 규칙(src/deployModel.ts, 관리 GUI W6a, plan §8.359). 권한·후보 대조의 정본은 host·agent다
// (crates/aaa-host/src/rollout/web_tests.rs, crates/aaa-agent/src/deploy/stage.rs). 여기는 화면이 보낼 요청을 정하는 규칙이다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { canDeploy, canStart, candidatePathOk, newDeployId, refused, rolloutSpec, rolloutTargets } from "../src/deployModel.ts";

const view = (over = {}) => ({
  candidate: { machine: "mac-b", path: "~/aaa-candidate/aaa", sha256: "a".repeat(64), commit: "c".repeat(40), version: "0.1.2+cccccccccccc", total_bytes: 9, built_at: null },
  targets: [
    { machine: "mac-a", host: true, problem: null },
    { machine: "mac-b", host: false, problem: null },
  ],
  busy: false,
  ...over,
});
const input = { source: "mac-b", path: "~/aaa-candidate/aaa", machines: ["mac-b"] };

describe("관리 W6a", () => {
  it("웹 배포는 1.82 이상 host만", () => {
    for (const v of [undefined, "1.81", "2.82"]) assert.equal(canDeploy(v), false, String(v));
    assert.equal(canDeploy("1.82"), true);
  });
  it("ID는 host valid_id(영숫자 26자)를 지난다", () => {
    const ids = new Set(Array.from({ length: 50 }, newDeployId));
    for (const id of ids) assert.match(id, /^[0-9A-Z]{26}$/);
    assert.equal(ids.size, 50);
  });
  it("후보는 빌드 머신의 절대 경로나 ~/ 아래 …/aaa", () => {
    for (const ok of ["/opt/aaa/candidate/aaa", "~/aaa-candidate/aaa", " ~/c/aaa "]) assert.equal(candidatePathOk(ok), true, ok);
    for (const bad of ["", "aaa", "c/aaa", "/opt/aaa/candidate", "/opt/aaa-x", "~/../x/aaa", "mac-b:/opt/aaa"]) assert.equal(candidatePathOk(bad), false, bad);
  });
  it("시작은 점검한 그 후보·대상이 모두 준비됐고 진행 중 rollout이 없을 때만", () => {
    assert.equal(canStart(null, input), false);
    assert.equal(canStart({ input, view: view() }, input), true);
    assert.equal(canStart({ input, view: view() }, { ...input, path: " ~/aaa-candidate/aaa " }), true);
    for (const changed of [
      { ...input, source: "mac-a" },
      { ...input, path: "~/other/aaa" },
      { ...input, machines: [] },
    ])
      assert.equal(canStart({ input, view: view() }, changed), false, JSON.stringify(changed));
    assert.equal(canStart({ input, view: view({ busy: true }) }, input), false);
    assert.equal(canStart({ input, view: view({ targets: [{ machine: "mac-a", host: true, problem: "agent와 연결이 끊겨 있다" }] }) }, input), false);
  });
  it("대상은 host 머신이 늘 맨 앞: 다른 머신을 모두 풀면 host만 간다(빈 목록은 host가 「모든 머신」으로 읽는다)", () => {
    assert.deepEqual(rolloutTargets("mac-a", []), ["mac-a"]);
    assert.deepEqual(rolloutTargets("mac-a", ["mac-b", "mac-a"]), ["mac-a", "mac-b"]);
  });
  it("확정 거절일 때만 보인 rollout ID를 지운다(연결·내부 오류는 닿았는지 모른다)", () => {
    for (const code of ["bad_request", "forbidden", "unsupported_version"]) assert.equal(refused(code), true, code);
    for (const code of ["internal", "timeout", "unavailable"]) assert.equal(refused(code), false, code);
  });
  it("보낼 rollout의 SHA·커밋은 점검 결과 그대로(사람이 치지 않는다)", () => {
    assert.deepEqual(rolloutSpec(view(), ["mac-b"]), { from: "~/aaa-candidate/aaa", source_machine: "mac-b", sha256: "a".repeat(64), commit: "c".repeat(40), machines: ["mac-b"] });
  });
});
