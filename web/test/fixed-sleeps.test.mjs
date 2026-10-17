// 고정 sleep ratchet(herdr-web-ui scripts/conventions.test.ts 「fixed sleeps」 그대로): 파일마다 fixed-sleeps.json의 수를
// 넘지 못하고, 줄면 수를 낮춰야 하며, origin/main의 수보다 올릴 수 없다. 세는 법은 fixed-sleeps.mjs 머리에 있다.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fixedSleepCounts, fixedSleeps, read, repo, sleepFiles } from "./fixed-sleeps.mjs";

const BASELINE = "web/test/fixed-sleeps.json";
const baseline = JSON.parse(read(BASELINE));

function fail(offenders, instruction) {
  if (offenders.length) assert.fail(`${instruction}\n  ${offenders.join("\n  ")}`);
}

const HOW =
  "정한 시간이 아니라 기다리는 것을 기다린다(docs/testing.md 「기다리기」): 관찰할 수 있는 사건(host 흐름 사건·`pane wait-output`·" +
  "`aaa ticker tick`·화면 표지·fake 기록)을 기다리거나, 시한이 있는 반복으로 상태를 본다. 일부러 고정한 기다림(아무것도 오지 않음 증명)은 " +
  "그 줄이나 윗줄에 `// fixed wait: <이유>`를 단다. fixed-sleeps.json은 올리지 않는다. 파일 이름을 바꿨으면 같은 수로 새 경로에 옮긴다.";

test("고정 sleep을 더하지 않고, 줄면 기준을 낮춘다", () => {
  const counts = fixedSleepCounts();
  const grown = Object.entries(counts).filter(([file, n]) => n > (baseline[file] ?? 0));
  fail(
    grown.flatMap(([file, n]) => [`${file}: 고정 sleep ${n}개, 기준 ${baseline[file] ?? 0}`, ...fixedSleeps(file).map((l) => `  ${l}`)]),
    HOW,
  );
  const shrunk = Object.entries(baseline).filter(([file, n]) => (counts[file] ?? 0) < n);
  fail(
    shrunk.map(([file, n]) => `${file}: 지금 ${counts[file] ?? 0}, 기준 ${n}`),
    `고정 sleep이 줄었다: ${BASELINE}의 이 줄을 보인 수로 낮춘다(0이면 줄을 지운다, \`node web/test/fixed-sleeps.mjs\`가 모두 보인다). 다시 늘지 않게 한다.`,
  );
});

test("기준은 origin/main보다 올리지 않는다", (t) => {
  const git = (...args) => spawnSync("git", args, { cwd: repo, encoding: "utf8" });
  // CI(ubuntu, 이력을 모두 받음)는 AAA_SLEEP_BASE로 기준을 준다: main push면 그 전 main, 그 밖에는 origin/main(ci.yml).
  // 주었으면 건너뛰지 않는다. 얕은 checkout에서는 늘 건너뛰어 CI가 이 검사를 한 번도 하지 않았다(10-18 감사)
  const given = process.env.AAA_SLEEP_BASE ?? "";
  const base = git("merge-base", "HEAD", given || "origin/main");
  if (base.status !== 0 && given) assert.fail(`AAA_SLEEP_BASE=${given}와 HEAD가 갈라진 곳을 찾지 못했다(이력을 받지 않은 checkout?): ${base.stderr.trim()}`);
  // 기준을 주지 않은 얕은 checkout(macOS·Windows CI job)에는 origin/main이 없다: 그때는 위 시험(기준 파일)만 본다
  if (base.status !== 0) return t.skip("origin/main이 없다(얕은 checkout)");
  const json = git("show", `${base.stdout.trim()}:${BASELINE}`);
  if (json.status !== 0) return t.skip("이 branch가 갈라진 곳에는 기준 파일이 아직 없다");
  const before = JSON.parse(json.stdout);
  const total = (c) => Object.values(c).reduce((s, n) => s + n, 0);
  fail(
    [
      // 파일마다는 main에 있던 파일만 본다. 새 파일(나누기·이름 바꾸기로 옮긴 sleep)은 합계가 막는다
      ...Object.entries(baseline)
        .filter(([file, n]) => file in before && n > before[file])
        .map(([file, n]) => `${file}: ${n}, origin/main ${before[file]}`),
      ...(total(baseline) > total(before) ? [`합계 ${total(baseline)}, origin/main ${total(before)}`] : []),
    ],
    `${BASELINE}은 줄기만 한다: 새 고정 sleep을 코드에서 뺀다. 파일 이름을 바꿨으면 같은 수로 옮겨 합계를 지킨다.`,
  );
});

test("기준에는 있는 파일만", () => {
  const covered = new Set(sleepFiles());
  fail(
    Object.keys(baseline).filter((f) => !fs.existsSync(path.join(repo, f)) || !covered.has(f)),
    `${BASELINE}에서 이 줄을 지운다: 파일이 없거나 더는 보지 않는다. 이름을 바꾼 파일은 같은 수로 새 경로에 둔다.`,
  );
});

test("JS: 고정 sleep과 시한 있는 반복을 가른다(herdr-web-ui 사례)", () => {
  const n = (code) => fixedSleeps("example.mjs", code).length;
  const sleep = "const sleep = (ms) => new Promise((r) => setTimeout(r, ms));\n";
  assert.equal(n(`${sleep}await sleep(300);`), 1);
  assert.equal(n("await new Promise((resolve) => setTimeout(resolve, 1500));"), 1);
  assert.equal(n(`${sleep}while (!ready) await sleep(20);`), 1);
  assert.equal(n("await new Promise((resolve) => setTimeout(resolve, 0));"), 0);
  assert.equal(n(`${sleep}while (!done() && Date.now() < deadline) await sleep(25);`), 0);
  assert.equal(n(`${sleep}for (let i = 0; i < 40 && !seen(); i++) await sleep(50);`), 0);
  assert.equal(n(`${sleep}const deadline = Date.now() + 5000;\nwhile (!await check()) {\n  assert.ok(Date.now() < deadline, "late");\n  await sleep(25);\n}`), 0);
  assert.equal(n(`${sleep}for (;;) {\n  if (seen()) break;\n  if (Date.now() > deadline) throw new Error("late");\n  await sleep(25);\n}`), 0);
  assert.equal(n(`${sleep}await Promise.race([event, sleep(1000)]);`), 0);
  assert.equal(n(`${sleep}// fixed wait: 이 시간 안에 아무것도 오지 않아야 한다\nawait sleep(NO_SEND_WAIT_MS);`), 0);
  assert.equal(n(`${sleep}async function f() {\n  while (Date.now() < deadline) {\n    const g = async () => { await sleep(5); };\n  }\n}`), 1);
  assert.equal(n(`${sleep}await sleep(1000); // fixed wait:`), 1);
  assert.equal(n(`${sleep}for (let i = 0; i < panes.length; i++) { await sleep(1000); }`), 1);
  assert.equal(n("await new Promise((r) => setTimeout(() => r(), 500));"), 1);
  assert.equal(n("await new Promise(function (r) { setTimeout(r, 500); });"), 1);
  // 참고가 세지 않던 Playwright 고정 기다림
  assert.equal(n("await page.waitForTimeout(500);"), 1);
  assert.equal(n("await page.waitForTimeout(0);"), 0);
  // 문자열(시험 프로그램 원문) 안은 코드가 아니다
  assert.equal(n('fs.writeFileSync("a.js", `await new Promise((r) => setTimeout(r, 900));`);'), 0);
});

test("JS: 다른 파일에서 import한 sleep helper도 센다(웹 E2E spec은 lib/harness.mjs의 sleep을 쓴다)", () => {
  const n = (code) => fixedSleeps("web/e2e/specs/example.mjs", code).length;
  assert.equal(n('import { aaa, sleep } from "../lib/harness.mjs";\nawait sleep(300);\nawait sleep(300);'), 2);
  assert.equal(n('import { sleep as nap } from "../lib/harness.mjs";\nawait nap(300);'), 1);
  assert.equal(n('import { until } from "../lib/harness.mjs";\nawait until("x", () => true);'), 0);
  assert.equal(n('import { sleep } from "../lib/harness.mjs";\nawait Promise.race([event, sleep(1000)]);'), 0);
});

test("JS: sleep helper는 정의가 아니라 부를 때마다 센다", () => {
  const n = (code) => fixedSleeps("example.mjs", code).length;
  assert.equal(n("const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));\nawait sleep(10);\nawait sleep(20);"), 2);
  assert.equal(n('import { setTimeout as sleep } from "node:timers/promises";\nawait sleep(500);'), 1);
  assert.equal(n("const tick = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms));\nawait tick();\nawait tick(20);"), 1);
  assert.equal(n("const sleep = (ms) => new Promise((r) => setTimeout(r, ms));\nconst diag = await Promise.race([work(), sleep(5000).then(() => 1)]);"), 0);
});

test("Rust: 시험의 thread::sleep과 시한 있는 반복을 가른다", () => {
  const n = (code, file = "crates/aaa/tests/x.rs") => fixedSleeps(file, code).length;
  assert.equal(n("fn t() {\n    std::thread::sleep(Duration::from_secs(3));\n    assert_eq!(a, 1);\n}"), 1);
  assert.equal(n("fn t() {\n    thread::sleep(Duration::from_millis(0));\n}"), 0);
  assert.equal(n("async fn t() {\n    tokio::time::sleep(Duration::from_millis(100)).await;\n}"), 1);
  // 시한 있는 반복(lab::wait_for_diag 꼴)
  assert.equal(
    n("fn w() {\n    let deadline = Instant::now() + t;\n    loop {\n        if f() { return; }\n        if Instant::now() >= deadline { panic!(\"늦다\"); }\n        std::thread::sleep(Duration::from_millis(100));\n    }\n}"),
    0,
  );
  assert.equal(n("fn w() {\n    for _ in 0..40 {\n        std::thread::sleep(Duration::from_millis(50));\n    }\n}"), 0);
  // 끝이 없는 반복의 쉼은 센다
  assert.equal(n("fn w() {\n    loop {\n        std::thread::sleep(Duration::from_millis(50));\n    }\n}"), 1);
  assert.equal(n("fn t() {\n    // fixed wait: 그동안 다시 보내지 않아야 한다\n    std::thread::sleep(Duration::from_secs(3));\n}"), 0);
  // 문자열·raw 문자열(가짜 agent 셸 원문) 안은 코드가 아니다
  assert.equal(n('const S: &str = r#"thread::sleep(Duration::from_secs(3))"#;\nfn t() { let s = "std::thread::sleep(x)"; }'), 0);
  // 제품 코드는 #[cfg(test)] 뒤만 본다
  assert.equal(n("fn run() {\n    std::thread::sleep(Duration::from_secs(1));\n}\n", "crates/aaa-host/src/x.rs"), 0);
  assert.equal(n("fn run() {}\n#[cfg(test)]\nmod tests {\n    fn t() { std::thread::sleep(Duration::from_secs(1)); }\n}\n", "crates/aaa-host/src/x.rs"), 1);
});
