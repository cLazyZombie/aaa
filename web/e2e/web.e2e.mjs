// 웹 front E2E: 실제 aaa host·agent를 띄우고 headless Chromium·WebKit으로 pairing·보기·입력·권한·알림·끊김을 확인한다.
// 단계는 기능별 파일(web/e2e/specs/NN-*.mjs)에 있고, 파일마다 새 실험실(host·agent·Chromium, /tmp/aaw-<pid>)에서 돈다.
// 파일은 worker 여러 개가 함께 돈다(실험실끼리 나누는 것이 없다). 긴 파일부터 넣는다(durations.json, 기록한 초).
//
// 실행(저장소 루트): cargo build -p aaa && (cd web && npm run build) && cargo build -p aaa
//   NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs [스크린샷 폴더]   모든 파일을
//     --workers 1                    함께 도는 파일 수(기본: core 4개마다 하나, 많아야 MAX_WORKERS. AAA_E2E_WORKERS와 같다).
//                                    1이면 차례로 돌며 출력을 바로 보이고, 2 이상이면 파일이 끝날 때 그 파일 출력을 한 덩어리로 찍는다
//     --spec 07-scroll-history,09    고른 파일만(이름이나 앞 번호)
//     --grep '앞 기록'               이름이 맞는 단계만(AAA_E2E_ONLY와 같다). 그 단계가 든 파일만 띄운다
//     --repeat 25                    고른 파일을 새 실험실로 25번(간헐 실패 재기)
//     --list                         파일과 단계 이름만 찍는다
//     --write-durations              이번에 잰 파일 시간(여럿이면 중간)을 durations.json에 적는다(모두 통과했을 때만)
// AAA_BIN으로 실행 파일을 바꿀 수 있다. 실패한 파일은 실험실 폴더를 남기고(「실험실 기록을 남겼다」), 다른 파일은 계속 돈다.
// 하나라도 실패하면 종료 코드 1. 단계 결과 줄(`ok   이름`·`FAIL 이름: …`)은 ci.yml·ci-local.sh가 읽는다.

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const specDir = path.join(here, "specs");
const STEP = /^ {2}await step\("((?:[^"\\]|\\.)*)"/;
const durationsFile = path.join(here, "durations.json");
// 기본 worker 상한: 16 core에서 전체 묶음을 여러 번 재 이보다 많으면 간헐 실패가 늘었다(docs/testing.md 「웹 E2E」)
const MAX_WORKERS = 4;

const args = process.argv.slice(2);
const opt = { spec: null, grep: null, repeat: 1, list: false, shots: null, workers: null, writeDurations: false };
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  const value = () => {
    if (i + 1 >= args.length) throw new Error(`${a} 뒤에 값이 없다`);
    return args[++i];
  };
  if (a === "--spec") opt.spec = value().split(",").filter(Boolean);
  else if (a === "--grep") opt.grep = value();
  else if (a === "--repeat") opt.repeat = Number(value());
  else if (a === "--list") opt.list = true;
  else if (a === "--workers") opt.workers = Number(value());
  else if (a === "--write-durations") opt.writeDurations = true;
  else if (a.startsWith("--")) throw new Error(`모르는 선택: ${a}(머리 주석을 본다)`);
  else opt.shots = path.resolve(a);
}
if (!Number.isInteger(opt.repeat) || opt.repeat < 1) throw new Error("--repeat은 1 이상의 정수다");
const workers =
  opt.workers ??
  (process.env.AAA_E2E_WORKERS ? Number(process.env.AAA_E2E_WORKERS) : Math.min(MAX_WORKERS, Math.max(1, Math.floor(os.availableParallelism() / 4))));
if (!Number.isInteger(workers) || workers < 1) throw new Error("--workers(AAA_E2E_WORKERS)는 1 이상의 정수다");
const grep = opt.grep ?? process.env.AAA_E2E_ONLY ?? null;
if (grep && opt.writeDurations) throw new Error("--write-durations는 파일 전체 시간을 적는다: --grep과 함께 쓰지 않는다");
const only = grep ? new RegExp(grep) : null;

const specs = fs
  .readdirSync(specDir)
  .filter((f) => /^\d\d-.+\.mjs$/.test(f))
  .sort()
  .map((f) => {
    const steps = fs
      .readFileSync(path.join(specDir, f), "utf8")
      .split("\n")
      .map((l) => STEP.exec(l)?.[1])
      .filter(Boolean)
      .map((s) => JSON.parse(`"${s}"`));
    return { file: f, name: f.replace(/\.mjs$/, ""), steps };
  });
const picked = specs
  .filter((s) => !opt.spec || opt.spec.some((w) => s.name.startsWith(w)))
  .filter((s) => !only || s.steps.some((n) => only.test(n)));
if (!picked.length) throw new Error(`고른 파일이 없다(--spec ${opt.spec ?? "-"}, --grep ${grep ?? "-"}): --list로 이름을 본다`);

if (opt.list) {
  for (const s of picked) {
    console.log(`specs/${s.file}`);
    for (const n of s.steps.filter((n) => !only || only.test(n))) console.log(`  ${n}`);
  }
  process.exit(0);
}

const env = { ...process.env };
if (opt.shots) env.AAA_E2E_SHOTS = opt.shots;
if (grep) env.AAA_E2E_ONLY = grep;

// 차례로 돌 때는 이름 차례, 함께 돌 때는 긴 파일부터(기록이 없는 새 파일은 가장 긴 것으로 보고 맨 앞)
const durations = JSON.parse(fs.readFileSync(durationsFile, "utf8"));
const weight = (s) => durations[s.name] ?? Infinity;
const jobs = [];
for (let r = 1; r <= opt.repeat; r++) for (const s of picked) jobs.push({ s, r });
if (workers > 1) jobs.sort((a, b) => weight(b.s) - weight(a.s) || a.r - b.r);

const t0 = Date.now();
const runs = [];
const runOne = ({ s, r }) =>
  new Promise((resolve) => {
    const head = `## specs/${s.file}${opt.repeat > 1 ? ` (${r}/${opt.repeat})` : ""}`;
    if (workers === 1) console.log(head);
    const started = Date.now();
    const out = [];
    const p = spawn(process.execPath, [path.join(here, "lib/child.mjs"), path.join(specDir, s.file)], {
      env,
      stdio: workers === 1 ? "inherit" : ["ignore", "pipe", "pipe"],
    });
    p.stdout?.on("data", (c) => out.push(c));
    p.stderr?.on("data", (c) => out.push(c));
    p.on("close", (c, sig) => {
      if (workers > 1) process.stdout.write(`${head}\n${Buffer.concat(out).toString("utf8")}`);
      runs.push({ name: s.name, ok: !sig && c === 0, ms: Date.now() - started });
      resolve();
    });
  });
let next = 0;
await Promise.all(
  Array.from({ length: Math.min(workers, jobs.length) }, async () => {
    while (next < jobs.length) await runOne(jobs[next++]);
  }),
);

const sec = (ms) => `${(ms / 1000).toFixed(1)}초`;
console.log(`## 요약: 파일 ${picked.length}개 × ${opt.repeat}번, worker ${workers}개, 모두 ${sec(Date.now() - t0)}`);
for (const s of picked) {
  const mine = runs.filter((x) => x.name === s.name);
  const ok = mine.filter((x) => x.ok).length;
  const ms = mine.map((x) => x.ms).sort((a, b) => a - b);
  const time = ms.length > 1 ? `중간 ${sec(ms[ms.length >> 1])}, 가장 긴 ${sec(ms.at(-1))}` : sec(ms[0]);
  console.log(`##   ${s.name.padEnd(22)} ${ok === mine.length ? "통과" : "실패"} ${ok}/${mine.length}  ${time}`);
}
if (runs.some((x) => !x.ok)) process.exitCode = 1;
else if (opt.writeDurations) {
  for (const s of picked) {
    const ms = runs.filter((x) => x.name === s.name).map((x) => x.ms).sort((a, b) => a - b);
    durations[s.name] = Math.round(ms[ms.length >> 1] / 1000);
  }
  const sorted = Object.fromEntries(Object.entries(durations).sort(([a], [b]) => a.localeCompare(b)));
  fs.writeFileSync(durationsFile, `${JSON.stringify(sorted, null, 2)}\n`);
  console.log(`## ${path.relative(process.cwd(), durationsFile)}에 파일 ${picked.length}개 시간을 적었다`);
}
