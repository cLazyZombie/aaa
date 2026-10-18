// 웹 E2E 파일 규칙(docs/testing.md 「웹 E2E」): 기능별 spec과 공통 harness. 돌리기 전에(3분 실행 없이) 틀을 본다.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { read, repo, stripJs } from "./fixed-sleeps.mjs";

const SPECS = "web/e2e/specs";
const HARNESS = "web/e2e/lib/harness.mjs";
const specs = fs.readdirSync(path.join(repo, SPECS)).sort();
const STEP = /^ {2}await step\("((?:[^"\\]|\\.)*)"/;
const list = (body) => body.split(",").map((n) => n.trim()).filter(Boolean);

const harness = read(HARNESS);
const exported = list(/\nexport \{([^}]*)\};\n*$/.exec(harness)?.[1] ?? "");

test("spec 파일은 NN-이름.mjs이고 단계를 기본 함수로 내보낸다", () => {
  const bad = specs.filter((f) => !/^\d\d-[a-z0-9-]+\.mjs$/.test(f));
  assert.deepEqual(bad, [], `${SPECS}에는 01-http-pwa.mjs 꼴의 spec만 둔다(도우미는 web/e2e/lib)`);
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
