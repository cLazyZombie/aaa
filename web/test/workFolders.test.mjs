// 「작업」 트리의 작업 공간 폴더(src/workFolders.ts, plan §8.260·§8.280): 무엇을 한 폴더로 묶고 어떤 차례로 보이는가.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { threadRepo, workFolders } from "../src/workFolders.ts";

// 줄: [이름, 저장소 경로|null]
const row = (name, repo = null) => ({ name, t: { repo } });
const group = (rows) => {
  const g = workFolders(rows, (r) => threadRepo(r.t));
  return { folders: g.folders.map((f) => [f.name, f.rows.map((r) => r.name)]), flat: g.flat.map((r) => r.name) };
};

describe("작업 공간 폴더", () => {
  it("머신마다 경로가 달라도 마지막 폴더 이름이 같으면 한 폴더다(Windows 경로·끝 구분자 포함)", () => {
    const g = group([row("a", "/srv/game/rc"), row("b", "/Users/me/work/rc/"), row("c", "C:\\src\\rc")]);
    assert.deepEqual(g.folders, [["rc", ["a", "b", "c"]]]);
  });

  it("폴더는 이름순이고 「작업 폴더」(저장소 없는 thread)는 끝, 폴더 안은 받은 차례", () => {
    const g = group([row("t-rc2", "/w/rc"), row("t-aaa", "/w/aaa"), row("dir", null), row("t-rc1", "/x/rc"), row("t-eng", "/w/no2-engine")]);
    assert.deepEqual(g.folders, [
      ["aaa", ["t-aaa"]],
      ["no2-engine", ["t-eng"]],
      ["rc", ["t-rc2", "t-rc1"]],
      ["작업 폴더", ["dir"]],
    ]);
    assert.deepEqual(g.flat, []);
  });

  it("저장소 폴더가 하나뿐이어도 폴더로 그린다(머리가 저장소 이름을 말한다)", () => {
    const g = group([row("t1", "/w/aaa"), row("t2", "/w/aaa")]);
    assert.deepEqual(g.folders, [["aaa", ["t1", "t2"]]]);
  });

  it("저장소를 아는 줄이 없으면(옛 host) 폴더 없이 받은 차례 그대로", () => {
    const g = group([row("dir"), row("old")]);
    assert.deepEqual(g, { folders: [], flat: ["dir", "old"] });
  });
});
