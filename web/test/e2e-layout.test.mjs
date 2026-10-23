// 웹 E2E 파일 규칙(docs/testing.md 「웹 E2E」): 기능별 spec과 공통 harness. 돌리기 전에(3분 실행 없이) 틀을 본다.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { read, repo, stripJs } from "./fixed-sleeps.mjs";

const SPECS = "web/e2e/specs";
const HARNESS = "web/e2e/lib/harness.mjs";
const specs = fs.readdirSync(path.join(repo, SPECS)).sort((a, b) => parseInt(a, 10) - parseInt(b, 10) || a.localeCompare(b));
const STEP = /^ {2}await step\("((?:[^"\\]|\\.)*)"/;
const list = (body) => body.split(",").map((n) => n.trim()).filter(Boolean);

const harness = read(HARNESS);
const exported = list(/\nexport \{([^}]*)\};\n*$/.exec(harness)?.[1] ?? "");

test("spec 파일은 두 자리 이상 번호-이름.mjs이고 단계를 기본 함수로 내보낸다", () => {
  const bad = specs.filter((f) => !/^\d{2,}-[a-z0-9-]+\.mjs$/.test(f));
  assert.deepEqual(bad, [], `${SPECS}에는 두 자리 이상 번호의 01-http-pwa.mjs·100-coordinator-auto-mode.mjs 꼴만 둔다(도우미는 web/e2e/lib)`);
  for (const f of specs) {
    const src = read(`${SPECS}/${f}`);
    assert.match(src, /^export default async function \((\{ [\w, ]+ \})?\) \{$/m, `${f}: export default async function ({ ctl, page }) {(쓰는 것만, 없으면 ())`);
    assert.ok(src.split("\n").some((l) => STEP.test(l)), `${f}: 단계(\`  await step("…", async () => {\`)가 없다`);
  }
});

test("단계 이름은 모든 spec에서 하나씩(--grep·CI 요약이 이름으로 찾는다)", () => {
  const seen = new Map();
  const dup = [];
  for (const f of specs)
    for (const l of read(`${SPECS}/${f}`).split("\n")) {
      const m = STEP.exec(l);
      if (!m) continue;
      if (seen.has(m[1])) dup.push(`${m[1].slice(0, 60)}: ${seen.get(m[1])}, ${f}`);
      else seen.set(m[1], f);
    }
  assert.deepEqual(dup, []);
});

test("harness는 위에 둔 이름을 모두 내보내고, spec은 있는 이름만 불러온다", () => {
  const declared = [];
  for (const l of stripJs(harness, { strings: true }).split("\n")) {
    const m = /^(?:async\s+)?function\*?\s+(\w+)|^(?:const|let|var|class)\s+(\w+)/.exec(l);
    if (m) declared.push(m[1] ?? m[2]);
  }
  assert.deepEqual(declared.filter((n) => !exported.includes(n)), [], `${HARNESS} 끝의 export { … }에 더한다`);
  for (const f of specs) {
    const m = /^import \{([^}]*)\} from "\.\.\/lib\/harness\.mjs";$/m.exec(read(`${SPECS}/${f}`));
    assert.ok(m, `${f}: import { … } from "../lib/harness.mjs"`);
    assert.deepEqual(list(m[1]).filter((n) => !exported.includes(n)), [], `${f}: harness에 없는 이름`);
  }
});

test("durations.json은 있는 spec의 초만 적는다(입구가 긴 파일부터 worker에 넣는다)", () => {
  const durations = JSON.parse(read("web/e2e/durations.json"));
  const names = specs.map((f) => f.replace(/\.mjs$/, ""));
  // 새 spec은 적지 않아도 맨 앞에 돈다. 이름을 바꾸거나 지운 spec의 기록은 남기지 않는다
  assert.deepEqual(Object.keys(durations).filter((n) => !names.includes(n)), [], "없는 spec: node web/e2e/web.e2e.mjs --write-durations로 다시 적는다");
  for (const [n, v] of Object.entries(durations)) assert.ok(Number.isInteger(v) && v > 0, `${n}: 초는 양의 정수`);
});

// 실제 입구를 실행한다. 규칙 검사만 넓히고 discovery가 E2E100을 빼는 회귀를 잡는다.
test("실제 수집 목록은 E2E99·100을 포함한 모든 파일을 숫자 순으로 표시한다", () => {
  const output = execFileSync(process.execPath, ["web/e2e/web.e2e.mjs", "--list"], {
    cwd: repo, encoding: "utf8", env: { ...process.env, AAA_E2E_ONLY: "" },
  });
  const listed = output.split("\n").filter((line) => line.startsWith("specs/")).map((line) => line.slice(6));
  assert.deepEqual(new Set(listed), new Set(specs), "파일을 빠뜨리거나 중복 수집하지 않는다");
  assert.equal(listed.length, specs.length);
  assert.ok(listed.includes("99-machine-workspaces.mjs"));
  assert.ok(listed.includes("100-coordinator-auto-mode.mjs"));
  assert.equal(listed.indexOf("100-coordinator-auto-mode.mjs"), listed.indexOf("99-machine-workspaces.mjs") + 1);
  for (let i = 1; i < listed.length; i++) {
    assert.ok(parseInt(listed[i - 1], 10) <= parseInt(listed[i], 10), `${listed[i - 1]} → ${listed[i]}`);
  }
});

// 대량 출력은 pipe의 용량을 넘긴다. 강제 exit는 끝을 잃고 자연 종료는 write 완료를 기다린다.
test("목록은 pipe에 남은 출력까지 모두 전달하고 끝난다", (t) => {
  const cache = path.join(repo, "web/node_modules/.cache");
  fs.mkdirSync(cache, { recursive: true });
  const root = fs.mkdtempSync(path.join(cache, "e2e-list-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, "specs"));
  fs.mkdirSync(path.join(root, "lib"));
  fs.copyFileSync(path.join(repo, "web/e2e/web.e2e.mjs"), path.join(root, "runner.mjs"));
  fs.copyFileSync(path.join(repo, "web/e2e/lib/jobs.mjs"), path.join(root, "lib/jobs.mjs"));
  const names = Array.from({ length: 400 }, (_, i) => `${String(i).padStart(3, "0")}-fixture.mjs`);
  for (const name of names) fs.writeFileSync(path.join(root, "specs", name), `  await step("${"가".repeat(1000)}", async () => {});\n`);
  const output = execFileSync(process.execPath, [path.join(root, "runner.mjs"), "--list"], {
    encoding: "utf8", maxBuffer: 4 * 1024 * 1024, env: { ...process.env, AAA_E2E_ONLY: "" },
  });
  assert.deepEqual(output.split("\n").filter((line) => line.startsWith("specs/")).map((line) => line.slice(6)), names);
  assert.equal(output.split("\n").filter((line) => line.startsWith("  ")).length, names.length);
  assert.ok(output.endsWith(`  ${"가".repeat(1000)}\n`));
});
