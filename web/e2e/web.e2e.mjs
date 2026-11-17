// 웹 front E2E: 실제 aaa host·agent를 띄우고 headless Chromium·WebKit으로 pairing·보기·입력·권한·알림·끊김을 확인한다.
// 단계는 기능별 파일(web/e2e/specs/번호-기능.mjs, 번호는 두 자리 이상)에 있고, 파일마다 새 실험실(host·agent·Chromium, /tmp/aaw-<pid>)에서 돈다.
// 파일은 worker 여러 개가 함께 돈다(실험실끼리 나누는 것이 없다). 긴 파일부터 넣는다(durations.json, 기록한 초).
//
// 실행(저장소 루트): cargo build --profile e2e -p aaa && (cd web && npm run build) && cargo build --profile e2e -p aaa
//   NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs [스크린샷 폴더]   모든 파일을
//     --workers 1                    함께 도는 작업 수(기본: core 둘마다 하나·최대6개).
//                                    AAA_E2E_WORKERS와 같다.
//                                    1이면 차례로 돌며 출력을 바로 보이고, 2 이상이면 파일이 끝날 때 그 파일 출력을 한 덩어리로 찍는다
//     --spec 07-scroll-history,09    고른 파일만(이름이나 앞 번호)
//     --grep '앞 기록'               이름이 맞는 단계만(AAA_E2E_ONLY와 같다). 그 단계가 든 파일만 띄운다
//     --repeat 25                    고른 파일을 새 실험실로 25번(간헐 실패 재기)
//     --list                         파일과 단계 이름만 찍는다
//     --write-durations              이번에 잰 파일 시간(여럿이면 중간)을 durations.json에 적는다(모두 통과했을 때만)
//     --shard 1/3                    spec 상태를 보존해 세 러너에 나눈 첫 묶음(AAA_E2E_SHARD).
//     --retries 1                    실패 작업만 새 프로세스·실험실에서 한 번 재확인(AAA_E2E_RETRIES, 기본 0).
// AAA_BIN으로 실행 파일을 바꿀 수 있다. 실패한 파일은 실험실 폴더를 남기고(「실험실 기록을 남겼다」), 다른 파일은 계속 돈다.
// 재확인 뒤에도 실패한 작업이 있으면 종료 코드 1. 최초 FAIL 줄도 남긴다. 재확인 통과는 warning·요약·산출물에 따로 적는다.

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { planJobs, shardSpecs, specDurations } from "./lib/jobs.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const specDir = path.join(here, "specs");
const compareSpecs = (a, b) => parseInt(a, 10) - parseInt(b, 10) || a.localeCompare(b);
const STEP = /^ {2}await step\("((?:[^"\\]|\\.)*)"/;
const durationsFile = path.join(here, "durations.json");
// 단독 실행은 core 둘마다 하나·최대6개다. ci-local은 Rust와 나눈 예산을 환경으로 전달한다.
// 전후 시간은 docs/testing.md 「웹 E2E 병렬 작업」처럼 같은 실행 파일과 모든 기존 조합으로 비교한다.
const MAX_WORKERS = 6;
const cores = os.availableParallelism();
const defaultWorkers = Math.max(1, Math.min(MAX_WORKERS, Math.floor(cores / 2)));

const args = process.argv.slice(2);
const opt = { spec: null, grep: null, repeat: 1, list: false, shots: null, workers: null, writeDurations: false,
  shard: process.env.AAA_E2E_SHARD || null, retries: Number(process.env.AAA_E2E_RETRIES || 0) };
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
  else if (a === "--shard") opt.shard = value();
  else if (a === "--retries") opt.retries = Number(value());
  else if (a.startsWith("--")) throw new Error(`모르는 선택: ${a}(머리 주석을 본다)`);
  else opt.shots = path.resolve(a);
}
if (!Number.isInteger(opt.repeat) || opt.repeat < 1) throw new Error("--repeat은 1 이상의 정수다");
if (![0, 1].includes(opt.retries)) throw new Error("--retries(AAA_E2E_RETRIES)는 0 또는 1이다");
const workers =
  opt.workers ??
  (process.env.AAA_E2E_WORKERS ? Number(process.env.AAA_E2E_WORKERS) : defaultWorkers);
if (!Number.isInteger(workers) || workers < 1) throw new Error("--workers(AAA_E2E_WORKERS)는 1 이상의 정수다");
const grep = opt.grep ?? process.env.AAA_E2E_ONLY ?? null;
if (grep && opt.writeDurations) throw new Error("--write-durations는 파일 전체 시간을 적는다: --grep과 함께 쓰지 않는다");
if (opt.writeDurations && (opt.shard || opt.retries)) throw new Error("시간 원장은 분할·재시도 없이 전체 실행에서 적는다");
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
const durations = JSON.parse(fs.readFileSync(durationsFile, "utf8"));
const picked = shardSpecs(specs, opt.shard, durations)
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
record({ event: "suite-start", cores, workers, shard: opt.shard, retries: opt.retries, load: os.loadavg(), platform: process.platform, arch: process.arch,
  specs: picked.length, selection: picked.map(s => s.name), jobs: jobs.length });
const runOne = ({ s, r, filter, part }, worker, attempt) =>
  new Promise((resolve) => {
    const head = `## specs/${s.file}${part ? ` [독립 단계 ${part}]` : ""}${opt.repeat > 1 ? ` (${r}/${opt.repeat})` : ""} [시도 ${attempt}]`;
    if (workers === 1) console.log(head);
    const started = Date.now();
    record({ event: "job-start", spec: s.name, repeat: r, part, worker, attempt, load: os.loadavg() });
    const out = [];
    const p = spawn(process.execPath, [path.join(here, "lib/child.mjs"), path.join(specDir, s.file)], {
      env: filter ? { ...env, AAA_E2E_ONLY: filter } : env,
      stdio: workers === 1 ? "inherit" : ["ignore", "pipe", "pipe"],
    });
    p.stdout?.on("data", (c) => out.push(c));
    p.stderr?.on("data", (c) => out.push(c));
    p.on("error", error => console.error(`실험실 프로세스 시작 실패: ${error.message}`));
    p.on("close", (c, sig) => {
      if (workers > 1) process.stdout.write(`${head}\n${Buffer.concat(out).toString("utf8")}`);
      const run = { name: s.name, r, part, attempt, ok: !sig && c === 0, ms: Date.now() - started, code: c, signal: sig };
      record({ event: "job-end", spec: s.name, repeat: r, part, worker, attempt, ms: run.ms, ok: run.ok, code: c, signal: sig, load: os.loadavg() });
      resolve(run);
    });
  });
let next = 0;
await Promise.all(
  Array.from({ length: Math.min(workers, jobs.length) }, async (_, worker) => {
    while (next < jobs.length) {
      const job = jobs[next++];
      const first = await runOne(job, worker + 1, 1);
      runs.push({ ...first, job, attempts: [first] });
    }
  }),
);

// 최초 전수가 끝나 자원이 반환된 뒤 실패 작업만 하나씩 재확인한다.
// child가 완전히 종료된 close 뒤 새 child를 띄우므로 host·agent·브라우저·fixture를 공유하지 않는다.
if (opt.retries) for (const run of runs.filter(run => !run.ok)) {
  const again = await runOne(run.job, 1, 2);
  run.attempts.push(again);
  run.ms += again.ms;
  run.ok = again.ok;
  run.attempt = again.attempt;
  run.code = again.code;
  run.signal = again.signal;
  if (again.ok) console.log(`::warning title=E2E 간헐 실패::${run.name}${run.part ? ` [${run.part}]` : ""}: 최초 실패, 새 실험실에서 재확인 통과. 최초 로그·산출물을 확인한다.`);
}
const recovered = runs.filter(run => run.ok && run.attempts.length > 1);
const unresolved = runs.filter(run => !run.ok);
record({ event: "suite-end", jobs: runs.length, failed: unresolved.length, recovered: recovered.length, attempts: runs.flatMap(run => run.attempts).length, load: os.loadavg() });
const summary = { shard: opt.shard, workers, retries: opt.retries, specs: picked.map(s => s.name),
  jobs: runs.map(({ job, ...run }) => run), failed: unresolved.length, recovered: recovered.length };
if (timingDir) fs.writeFileSync(path.join(timingDir, "run-summary.json"), `${JSON.stringify(summary, null, 2)}\n`);
if (process.env.GITHUB_ACTIONS === "true" && process.env.GITHUB_STEP_SUMMARY) {
  const rows = runs.map(run => `| ${run.name}${run.part ? ` (${run.part})` : ""} | ${run.attempts.map(a => a.ok ? "통과" : "실패").join(" → ")} |`);
  fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n### 웹 E2E ${opt.shard ?? "전체"}\n\n최종 실패 ${unresolved.length}, 재확인 통과 ${recovered.length}. 최초 실패도 산출물에 보존한다.\n\n| 작업 | 시도 결과 |\n| --- | --- |\n${rows.join("\n")}\n`);
}
const sec = (ms) => `${(ms / 1000).toFixed(1)}초`;
console.log(`## 요약: 파일 ${picked.length}개 × ${opt.repeat}번, worker ${workers}개, 모두 ${sec(Date.now() - t0)}`);
console.log(`## 작업: ${runs.length}개(독립 단계 분할 포함), 재확인 통과 ${recovered.length}개, 최종 실패 ${unresolved.length}개`);
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
