// 웹 front E2E: 실제 aaa host·agent를 띄우고 headless Chromium·WebKit으로 pairing·보기·입력·권한·알림·끊김을 확인한다.
// 단계는 기능별 파일(web/e2e/specs/번호-기능.mjs, 번호는 두 자리 이상)에 있고, 파일마다 새 실험실(host·agent·Chromium, /tmp/aaw-<pid>)에서 돈다.
// 파일은 worker 여러 개가 함께 돈다(실험실끼리 나누는 것이 없다). 긴 파일부터 넣는다(durations.json, 기록한 초).
//
// 실행(저장소 루트): cargo build -p aaa && (cd web && npm run build) && cargo build -p aaa
//   NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs [스크린샷 폴더]   모든 파일을
//     --workers 1                    함께 도는 작업 수(기본: 10 CPU 이상은 5/8 내림·최대10개, 그 밖은 core 둘마다 하나).
//                                    AAA_E2E_WORKERS와 같다.
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
import { planJobs, specDurations } from "./lib/jobs.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const specDir = path.join(here, "specs");
const compareSpecs = (a, b) => parseInt(a, 10) - parseInt(b, 10) || a.localeCompare(b);
const STEP = /^ {2}await step\("((?:[^"\\]|\\.)*)"/;
const durationsFile = path.join(here, "durations.json");
// 10 CPU 이상은 5/8 내림·최대10개, 그 밖은 core 둘마다 하나다. 실험실·브라우저는 작업마다 격리한다.
// 전후 시간은 docs/testing.md 「웹 E2E 병렬 작업」처럼 같은 실행 파일과 모든 기존 조합으로 비교한다.
const MAX_WORKERS = 10;
const cores = os.availableParallelism();
const defaultWorkers = cores >= 10 ? Math.min(MAX_WORKERS, Math.floor(cores * 5 / 8)) : Math.max(1, Math.floor(cores / 2));

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
  (process.env.AAA_E2E_WORKERS ? Number(process.env.AAA_E2E_WORKERS) : defaultWorkers);
if (!Number.isInteger(workers) || workers < 1) throw new Error("--workers(AAA_E2E_WORKERS)는 1 이상의 정수다");
const grep = opt.grep ?? process.env.AAA_E2E_ONLY ?? null;
if (grep && opt.writeDurations) throw new Error("--write-durations는 파일 전체 시간을 적는다: --grep과 함께 쓰지 않는다");
const only = grep ? new RegExp(grep) : null;

const specs = fs
  .readdirSync(specDir)
  .filter((f) => /^\d{2,}-[a-z0-9-]+\.mjs$/.test(f))
  .sort(compareSpecs)
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
} else {
// 목록은 stdout의 비동기 전송까지 자연 종료로 비운다. 실행은 이 분기에서만 시작한다.

const env = { ...process.env };
if (opt.shots) env.AAA_E2E_SHOTS = opt.shots;
if (grep) env.AAA_E2E_ONLY = grep;

// 차례로 돌 때는 이름 차례, 함께 돌 때는 긴 파일부터(기록이 없는 새 파일은 가장 긴 것으로 보고 맨 앞)
const durations = JSON.parse(fs.readFileSync(durationsFile, "utf8"));
const jobs = planJobs(picked, { repeat: opt.repeat, workers, durations, only });

const t0 = Date.now();
const runs = [];
// 시작 때도 남겨야 취소된 run의 마지막 완료 파일과 아직 도는 파일을 구별할 수 있다.
// 이 파일에는 시험 이름·시각·실행 조건만 쓰며 URL·환경 값·세션 내용은 쓰지 않는다.
const timingDir = process.env.AAA_E2E_ARTIFACTS;
if (timingDir) fs.mkdirSync(timingDir, { recursive: true });
const timingFile = timingDir ? path.join(timingDir, "run-timings.jsonl") : null;
if (timingFile) fs.writeFileSync(timingFile, "");
const record = (event) => {
  const row = { at: new Date().toISOString(), elapsedMs: Date.now() - t0, ...event };
  console.log(`## 실행 ${JSON.stringify(row)}`);
  if (timingFile) fs.appendFileSync(timingFile, `${JSON.stringify(row)}\n`);
};
record({ event: "suite-start", cores, workers, platform: process.platform, arch: process.arch, specs: picked.length, jobs: jobs.length });
const runOne = ({ s, r, filter, part }, worker) =>
  new Promise((resolve) => {
    const head = `## specs/${s.file}${part ? ` [독립 단계 ${part}]` : ""}${opt.repeat > 1 ? ` (${r}/${opt.repeat})` : ""}`;
    if (workers === 1) console.log(head);
    const started = Date.now();
    record({ event: "job-start", spec: s.name, repeat: r, part, worker });
    const out = [];
    const p = spawn(process.execPath, [path.join(here, "lib/child.mjs"), path.join(specDir, s.file)], {
      env: filter ? { ...env, AAA_E2E_ONLY: filter } : env,
      stdio: workers === 1 ? "inherit" : ["ignore", "pipe", "pipe"],
    });
    p.stdout?.on("data", (c) => out.push(c));
    p.stderr?.on("data", (c) => out.push(c));
    p.on("close", (c, sig) => {
      if (workers > 1) process.stdout.write(`${head}\n${Buffer.concat(out).toString("utf8")}`);
      const run = { name: s.name, r, ok: !sig && c === 0, ms: Date.now() - started };
      runs.push(run);
      record({ event: "job-end", spec: s.name, repeat: r, part, worker, ms: run.ms, ok: run.ok, code: c, signal: sig });
      resolve();
    });
  });
let next = 0;
await Promise.all(
  Array.from({ length: Math.min(workers, jobs.length) }, async (_, worker) => {
    while (next < jobs.length) await runOne(jobs[next++], worker + 1);
  }),
);

record({ event: "suite-end", jobs: runs.length, failed: runs.filter((run) => !run.ok).length });
const sec = (ms) => `${(ms / 1000).toFixed(1)}초`;
console.log(`## 요약: 파일 ${picked.length}개 × ${opt.repeat}번, worker ${workers}개, 모두 ${sec(Date.now() - t0)}`);
console.log(`## 작업: ${runs.length}개(독립 단계 분할 포함, 실패한 작업은 다시 실행하지 않음)`);
for (const s of picked) {
  const mine = runs.filter((x) => x.name === s.name);
  const ok = mine.filter((x) => x.ok).length;
  const ms = specDurations(runs, s.name, opt.repeat);
  const time = ms.length > 1 ? `중간 ${sec(ms[ms.length >> 1])}, 가장 긴 ${sec(ms.at(-1))}` : sec(ms[0]);
  console.log(`##   ${s.name.padEnd(22)} ${ok === mine.length ? "통과" : "실패"} ${ok}/${mine.length}  ${time}${mine.length > opt.repeat ? " (독립 작업 합계)" : ""}`);
}
if (runs.some((x) => !x.ok)) process.exitCode = 1;
else if (opt.writeDurations) {
  for (const s of picked) {
    const ms = specDurations(runs, s.name, opt.repeat);
    durations[s.name] = Math.round(ms[ms.length >> 1] / 1000);
  }
  const sorted = Object.fromEntries(Object.entries(durations).sort(([a], [b]) => compareSpecs(a, b)));
  fs.writeFileSync(durationsFile, `${JSON.stringify(sorted, null, 2)}\n`);
  console.log(`## ${path.relative(process.cwd(), durationsFile)}에 파일 ${picked.length}개 시간을 적었다`);
}

}
