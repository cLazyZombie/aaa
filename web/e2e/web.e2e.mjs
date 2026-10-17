// 웹 front E2E: 실제 aaa host·agent를 띄우고 headless Chromium·WebKit으로 pairing·보기·입력·권한·알림·끊김을 확인한다.
// 단계는 기능별 파일(web/e2e/specs/NN-*.mjs)에 있고, 파일마다 새 실험실(host·agent·Chromium, /tmp/aaw-<pid>)에서 돈다.
//
// 실행(저장소 루트): cargo build -p aaa && (cd web && npm run build) && cargo build -p aaa
//   NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs [스크린샷 폴더]   모든 파일을 차례로
//     --spec 07-scroll-history,09    고른 파일만(이름이나 앞 번호)
//     --grep '앞 기록'               이름이 맞는 단계만(AAA_E2E_ONLY와 같다). 그 단계가 든 파일만 띄운다
//     --repeat 25                    고른 파일을 새 실험실로 25번(간헐 실패 재기)
//     --list                         파일과 단계 이름만 찍는다
// AAA_BIN으로 실행 파일을 바꿀 수 있다. 실패한 파일은 실험실 폴더를 남기고(「실험실 기록을 남겼다」), 다른 파일은 계속 돈다.
// 하나라도 실패하면 종료 코드 1. 단계 결과 줄(`ok   이름`·`FAIL 이름: …`)은 ci.yml·ci-local.sh가 읽는다.

import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const specDir = path.join(here, "specs");
const STEP = /^ {2}await step\("((?:[^"\\]|\\.)*)"/;

const args = process.argv.slice(2);
const opt = { spec: null, grep: null, repeat: 1, list: false, shots: null };
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
  else if (a.startsWith("--")) throw new Error(`모르는 선택: ${a}(머리 주석을 본다)`);
  else opt.shots = path.resolve(a);
}
if (!Number.isInteger(opt.repeat) || opt.repeat < 1) throw new Error("--repeat은 1 이상의 정수다");
const grep = opt.grep ?? process.env.AAA_E2E_ONLY ?? null;
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

const t0 = Date.now();
const runs = [];
for (let r = 1; r <= opt.repeat; r++) {
  for (const s of picked) {
    const round = opt.repeat > 1 ? ` (${r}/${opt.repeat})` : "";
    console.log(`## specs/${s.file}${round}`);
    const started = Date.now();
    const code = await new Promise((resolve) => {
      const p = spawn(process.execPath, [path.join(here, "lib/child.mjs"), path.join(specDir, s.file)], { env, stdio: "inherit" });
      p.on("exit", (c, sig) => resolve(sig ? 1 : c));
    });
    runs.push({ name: s.name, ok: code === 0, ms: Date.now() - started });
  }
}

const sec = (ms) => `${(ms / 1000).toFixed(1)}초`;
console.log(`## 요약: 파일 ${picked.length}개 × ${opt.repeat}번, 모두 ${sec(Date.now() - t0)}`);
for (const s of picked) {
  const mine = runs.filter((x) => x.name === s.name);
  const ok = mine.filter((x) => x.ok).length;
  const ms = mine.map((x) => x.ms).sort((a, b) => a - b);
  const time = ms.length > 1 ? `중간 ${sec(ms[ms.length >> 1])}, 가장 긴 ${sec(ms.at(-1))}` : sec(ms[0]);
  console.log(`##   ${s.name.padEnd(22)} ${ok === mine.length ? "통과" : "실패"} ${ok}/${mine.length}  ${time}`);
}
if (runs.some((x) => !x.ok)) process.exitCode = 1;
