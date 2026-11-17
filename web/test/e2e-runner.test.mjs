// 실제 runner를 가짜 child와 실행해 종료 코드·재확인·최초 산출물 보존을 검사한다.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

const exec = promisify(execFile);
const root = new URL("../e2e/", import.meta.url);
const cleanEnv = () => Object.fromEntries(Object.entries(process.env).filter(([key]) =>
  !key.startsWith("AAA_E2E_") && !["GITHUB_ACTIONS", "GITHUB_STEP_SUMMARY"].includes(key)));

function fixture(t, names) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-e2e-runner-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  fs.mkdirSync(path.join(dir, "lib"));
  fs.mkdirSync(path.join(dir, "specs"));
  fs.copyFileSync(new URL("web.e2e.mjs", root), path.join(dir, "runner.mjs"));
  fs.copyFileSync(new URL("lib/jobs.mjs", root), path.join(dir, "lib/jobs.mjs"));
  fs.writeFileSync(path.join(dir, "durations.json"), "{}");
  for (const name of names) fs.writeFileSync(path.join(dir, "specs", `${name}.mjs`), '  await step("fixture", async () => {});\n');
  fs.writeFileSync(path.join(dir, "lib/child.mjs"), `
import fs from "node:fs";
import path from "node:path";
const name = path.basename(process.argv[2], ".mjs");
const state = new URL("../" + name + ".json", import.meta.url);
const ids = fs.existsSync(state) ? JSON.parse(fs.readFileSync(state)) : [];
ids.push(process.pid);
fs.writeFileSync(state, JSON.stringify(ids));
fs.mkdirSync(process.env.AAA_E2E_ARTIFACTS, { recursive: true });
fs.writeFileSync(path.join(process.env.AAA_E2E_ARTIFACTS, name + "-" + ids.length + ".txt"), String(process.pid));
const failed = name.includes("always") || (name.includes("once") && ids.length === 1);
console.log((failed ? "FAIL " : "ok   ") + name);
process.exitCode = failed ? 1 : 0;
`);
  const artifacts = path.join(dir, "artifacts");
  const summary = path.join(dir, "summary.md");
  return {
    dir, artifacts, summary,
    run: async (args = [], extra = {}) => {
      try {
        return { ...(await exec(process.execPath, [path.join(dir, "runner.mjs"), ...args], {
          env: { ...cleanEnv(), AAA_E2E_ARTIFACTS: artifacts, GITHUB_STEP_SUMMARY: summary, ...extra },
        })), code: 0 };
      } catch (error) { return error; }
    },
    result: () => JSON.parse(fs.readFileSync(path.join(artifacts, "run-summary.json"))),
  };
}

test("CI는 실패 작업만 새 자식으로 한 번 재확인하고 최초 산출물을 보존한다", async t => {
  const f = fixture(t, ["01-pass", "02-once"]);
  const result = await f.run(["--workers", "2", "--retries", "1"], { GITHUB_ACTIONS: "true" });
  assert.equal(result.code, 0, result.stderr);
  const summary = f.result();
  assert.equal(summary.recovered, 1);
  assert.equal(summary.failed, 0);
  assert.deepEqual(summary.jobs.find(j => j.name === "01-pass").attempts.map(a => a.ok), [true]);
  assert.deepEqual(summary.jobs.find(j => j.name === "02-once").attempts.map(a => a.ok), [false, true]);
  assert.equal(summary.jobs.find(j => j.name === "02-once").code, 0);
  const ids = JSON.parse(fs.readFileSync(path.join(f.dir, "02-once.json")));
  assert.equal(new Set(ids).size, 2, "새 프로세스가 새 실험실을 만든다");
  assert.ok(fs.existsSync(path.join(f.artifacts, "02-once-1.txt")), "최초 실패 산출물을 보존한다");
  assert.match(result.stdout, /::warning.*02-once/);
  assert.match(fs.readFileSync(f.summary, "utf8"), /실패 → 통과/);
});

test("로컬 기본 실패는 한 번, 두 번 실패한 CI 작업은 최종 실패다", async t => {
  const local = fixture(t, ["01-once"]);
  assert.equal((await local.run()).code, 1);
  assert.equal(local.result().jobs[0].attempts.length, 1);
  const ci = fixture(t, ["01-always", "02-pass"]);
  assert.equal((await ci.run(["--retries", "1", "--workers", "2"])).code, 1);
  assert.equal(ci.result().failed, 1);
  assert.equal(ci.result().jobs.find(j => j.name === "01-always").attempts.length, 2);
  assert.equal(ci.result().jobs.find(j => j.name === "02-pass").attempts.length, 1);
});

test("실제 spec 목록은 shard 셋에 빠짐·중복 없이 모두 남는다", async () => {
  const list = async args => (await exec(process.execPath, [fileURLToPath(new URL("web.e2e.mjs", root)), "--list", ...args], { env: cleanEnv() }))
    .stdout.split("\n").filter(line => line.startsWith("specs/"));
  const all = await list([]);
  const shards = await Promise.all([1, 2, 3].map(i => list(["--shard", `${i}/3`])));
  assert.deepEqual(shards.flat().sort(), all.sort());
  assert.equal(new Set(shards.flat()).size, all.length);
});

test("잘못된 재확인 설정은 실행 전에 실패한다", async t => {
  const f = fixture(t, ["01-pass"]);
  for (const args of [["--retries", "2"], ["--shard", "0/3"], ["--retries", "1", "--write-durations"]]) {
    assert.notEqual((await f.run(args)).code, 0);
  }
  assert.ok(!fs.existsSync(path.join(f.dir, "01-pass.json")));
});
