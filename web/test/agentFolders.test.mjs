// 에이전트 패널 저장소 폴더(src/agentFolders.ts, plan §8.260): 무엇을 한 폴더로 묶고 어떤 차례로 보이는가.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { agentFolders, threadRepo } from "../src/agentFolders.ts";

// 줄: [이름, 저장소 경로|null, coordinator?]
const row = (name, repo = null, coordinator = false) => ({ name, t: repo === undefined ? undefined : { repo }, coordinator });
const group = (rows) => {
  const g = agentFolders(rows, (r) => threadRepo(r.t), (r) => r.coordinator);
  return {
    coordinators: g.coordinators.map((r) => r.name),
    folders: g.folders.map((f) => [f.name, f.rows.map((r) => r.name)]),
    flat: g.flat.map((r) => r.name),
  };
};

describe("저장소 폴더", () => {
  it("머신마다 경로가 달라도 마지막 폴더 이름이 같으면 한 폴더다(Windows 경로·끝 구분자 포함)", () => {
    const g = group([row("a", "/srv/game/rc"), row("b", "/Users/me/work/rc/"), row("c", "C:\\src\\rc")]);
    assert.deepEqual(g.folders, [["rc", ["a", "b", "c"]]]);
  });

  it("coordinator는 폴더 밖 맨 위, 폴더는 이름순이고 「기타」(저장소 없는 thread·thread 아닌 agent)는 끝, 폴더 안은 받은 차례", () => {
    const g = group([
      row("free", undefined),
      row("t-rc2", "/w/rc"),
      row("coord", undefined, true),
      row("t-aaa", "/w/aaa"),
      row("dir", null),
      row("t-rc1", "/x/rc"),
      row("t-eng", "/w/no2-engine"),
    ]);
    assert.deepEqual(g.coordinators, ["coord"]);
    assert.deepEqual(g.folders, [
      ["aaa", ["t-aaa"]],
      ["no2-engine", ["t-eng"]],
      ["rc", ["t-rc2", "t-rc1"]],
      ["기타", ["free", "dir"]],
    ]);
    assert.deepEqual(g.flat, []);
  });

  it("저장소 폴더가 하나뿐이어도 폴더로 그린다(머리가 저장소 이름을 말한다)", () => {
    const g = group([row("coord", undefined, true), row("t1", "/w/aaa"), row("t2", "/w/aaa")]);
    assert.deepEqual(g.folders, [["aaa", ["t1", "t2"]]]);
  });

  it("저장소를 아는 줄이 없으면(옛 host·thread 없는 agent만) 폴더 없이 받은 차례 그대로", () => {
    const g = group([row("free", undefined), row("coord", undefined, true), row("dir", null)]);
    assert.deepEqual(g, { coordinators: [], folders: [], flat: ["free", "coord", "dir"] });
  });
});
