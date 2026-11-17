import assert from "node:assert/strict";
import { it } from "node:test";
import { loadDesignRepos } from "../src/designLoad.ts";

const repo = (name, folder = "docs") => ({ repo: name, folder, machine: "mac-a", tree: null, error: null, branches: [], handoffs: [] });
const reply = (...repos) => ({ kind: "design_tree", repos });
const deferred = () => { let resolve; const promise = new Promise((r) => { resolve = r; }); return { promise, resolve }; };

it("느린 저장소를 기다리는 동안 다른 트리가 오며 전송 실패도 그 저장소에만 남는다", async () => {
  const slow = deferred(), good = deferred(), gotGood = deferred();
  const calls = [], received = [];
  const old = { ...repo("bad"), tree: { commit: "old", docs: [] } };
  const run = loadDesignRepos(async (req) => {
    calls.push(req);
    if (req.cached) return reply(repo("good"), old, repo("slow"), repo("tool", null));
    if (req.repo === "bad") throw new TypeError("Load failed");
    return req.repo === "good" ? good.promise : slow.promise;
  }, (repos) => { assert.equal(repos.length, 4); }, (r) => { received.push(r); if (r.repo === "good") gotGood.resolve(); }, () => true);
  good.resolve(reply({ ...repo("good"), tree: { commit: "new", docs: [] } }));
  await gotGood.promise;
  assert.equal(received.find((r) => r.repo === "good").tree.commit, "new");
  assert.equal(received.find((r) => r.repo === "bad").tree.commit, "old");
  assert.match(received.find((r) => r.repo === "bad").error, /Load failed/);
  assert.ok(!received.some((r) => r.repo === "slow"));
  slow.resolve(reply(repo("slow")));
  await run;
  assert.deepEqual(calls, [{ op: "design_tree", cached: true }, ...["good", "bad", "slow"].map((repo) => ({ op: "design_tree", repo }))]);
});

it("끝난 조회는 그리지 않고 목록 실패는 빈 목록 성공으로 바꾸지 않는다", async () => {
  let active = true;
  const pending = deferred(), started = deferred();
  const run = loadDesignRepos(async (req) => {
    if (req.cached) return reply(repo("app"));
    started.resolve();
    return pending.promise;
  }, () => {}, () => assert.fail("끝난 조회가 그렸다"), () => active);
  await started.promise;
  active = false;
  pending.resolve(reply(repo("app")));
  await run;
  await assert.rejects(loadDesignRepos(async () => { throw new TypeError("Load failed"); }, () => assert.fail("실패를 빈 목록으로 그렸다"), () => {}, () => true), /Load failed/);
});
