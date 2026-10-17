// 화면 견본(디자인 비교·검수용): 실제 aaa host와 agent 둘(mac-a 로컬, mac-b 원격 등록)을 띄워
// 셸·가짜 Claude(작업 중·입력 대기·질문)·thread(검토 대기·작업 중·사람 차례)·사후 평가자가 있는 상태를 만들고,
// 같은 화면을 데스크톱 1920×1080·휴대폰 390×844, 어두움·밝음으로 찍는다.
//
// 실행(저장소 루트): (cd web && npm run build) && cargo build -p aaa
//                    NODE_PATH=$(npm root -g) node web/e2e/showcase.mjs <스크린샷 폴더> [접두사]
// 끝나면 띄운 프로세스를 PID로 끝내고 실험실 폴더를 지운다.

import { spawn, execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), "noop.js"));
const { chromium } = require("playwright");

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const AA = process.env.AAA_BIN ?? path.join(repo, "target/debug/aaa");
const out = process.argv[2];
const prefix = process.argv[3] ?? "aaa";
if (!out) throw new Error("스크린샷 폴더를 준다");
fs.mkdirSync(out, { recursive: true });
const root = `/tmp/aas-${process.pid}`;
const envA = { ...process.env, AAA_CONFIG_DIR: `${root}/c`, AAA_STATE_DIR: `${root}/s`, RUST_LOG: "aaa=warn" };
const envB = { ...process.env, AAA_CONFIG_DIR: `${root}/cb`, AAA_STATE_DIR: `${root}/sb`, RUST_LOG: "aaa=warn" };
const procs = new Map();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function aaa(...args) {
  // --json은 맨 앞에: `pane run <pane> <글…>`처럼 뒤 인자를 모두 글로 받는 명령에 섞이지 않게
  const a = ["--json", ...args];
  return JSON.parse(execFileSync(AA, a, { env: envA, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
}

function start(tag, env, ...args) {
  const log = fs.openSync(`${root}/${tag}.log`, "w");
  const p = spawn(AA, args, { env, stdio: ["ignore", log, log] });
  procs.set(tag, p);
  return p;
}

async function until(what, fn, ms = 20_000) {
  const deadline = Date.now() + ms;
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      // 아직
    }
    if (Date.now() > deadline) throw new Error(`${ms}ms 안에 ${what}이 되지 않았다`);
    await sleep(100);
  }
}

function freePort() {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

/** setsid로 떨어진 session host 중 이 실험실 것만 PID로 끝낸다 */
function killSessionHosts(state) {
  const dir = `${state}/agent/sessions`;
  if (!fs.existsSync(dir)) return;
  for (const id of fs.readdirSync(dir)) {
    try {
      const meta = JSON.parse(fs.readFileSync(`${dir}/${id}/meta.json`, "utf8"));
      const cmd = execFileSync("ps", ["-o", "command=", "-p", String(meta.host_pid)], { encoding: "utf8" });
      if (cmd.includes(`${dir}/${id}`)) process.kill(meta.host_pid, "SIGKILL");
    } catch {
      // 이미 끝났다
    }
  }
}

fs.rmSync(root, { recursive: true, force: true });
for (const d of ["c", "s", "cb", "sb", "work", "workb"]) fs.mkdirSync(`${root}/${d}`, { recursive: true });
const webPort = await freePort();
const agentPort = await freePort();
fs.writeFileSync(
  `${root}/c/host.toml`,
  `schema = 1\n[host]\nagent_listen = "127.0.0.1:${agentPort}"\n[web]\nlisten = "127.0.0.1:${webPort}"\n[local_agent]\nmode = "none"\n` +
    `[projects]\nroot = "${root}/projects"\nallow_profiles = ["*"]\ntick_secs = 1\n`,
);

// 보고를 쓰는 가짜 thread agent: 할 일에 따라 보고를 쓰거나(결정이면 먼저 `Waiting for you` 진행 보고), 「오래」면 보고 없이 일한다
fs.writeFileSync(
  `${root}/fake.sh`,
  `#!/bin/sh
printf '\\033[1;33m✻\\033[0m 준비됨\\n\\033[2m❯\\033[0m '
while IFS= read -r line; do
  case "$line" in
    *brief.md*)
      dir=$(dirname "$(printf '%s' "$line" | sed 's/를 읽고.*//')")
      task=$(awk '/^# 할 일/{f=1;next} /^# 경로/{f=0} f' "$dir/brief.md" | sed '/^$/d')
      printf '\\033[36m● brief를 읽었다\\033[0m\\n'
      case "$task" in
        *오래*) printf '\\033[2m  · 시험을 돌리는 중…\\033[0m\\n' ;;
        *결정*)
          "$AA" report --percent 50 --activity 'Waiting for you'
          printf '## Report\\n로그인 저장 방식을 정해야 한다.\\n\\n- 쿠키 세션: 서버 상태가 필요하다\\n- JWT: 폐기가 어렵다\\n\\n## Next\\n- 쿠키 세션으로 정하기(추천)\\n' > "$dir/report.md" ;;
        *)
          mkdir -p "$dir/library"
          printf '<!doctype html><meta charset=utf-8><body style="font-family:sans-serif;padding:24px"><h1>성능 보고</h1><p>p95 <b>182ms → 64ms</b></p></body>' > "$dir/library/index.html"
          printf '## Report\\n결제 API의 **p95 지연**을 182ms에서 64ms로 줄였다.\\n\\n- 원인: 주문마다 \\140SELECT\\140을 다시 했다\\n- 고침: 요청 안에서 한 번만 읽는다\\n- 확인: 부하 시험 3회\\n\\n## Next\\n- 결과를 확인하고 병합하기\\n- 캐시 만료 시간 정하기\\n' > "$dir/report.md" ;;
      esac
      ;;
  esac
  printf '\\033[2m❯\\033[0m '
done
`,
);
// Claude처럼 그리는 세션(`<root>/bin/claude <모드>`): 실행 파일 이름이 claude라 agent가 화면을 Claude 규칙으로 보고
// 상태를 정한다(plan §8.85). 화면 아래쪽은 실제 Claude Code 모양이고 판정은 herdr 원본 엔진으로 확인했다(작업 중
// `live_turn_working`, 질문 `bash_permission_prompt`, 입력 대기 `live_prompt_box`). hook은 세션 ID만 알린다.
fs.mkdirSync(`${root}/bin`, { recursive: true });
fs.writeFileSync(
  `${root}/bin/claude`,
  `#!/bin/sh
MODE="$1"
"${AA}" hook --agent claude '{"hook_event_name":"SessionStart","session_id":"demo-'"$MODE"'","source":"startup"}'
printf '\\033[38;5;216m╭───────────────────────────────────────────╮\\033[0m\\n'
printf '\\033[38;5;216m│\\033[0m \\033[1m✻ Claude Code\\033[0m                              \\033[38;5;216m│\\033[0m\\n'
printf '\\033[38;5;216m│\\033[0m   \\033[2mcwd: ~/work/shop-api\\033[0m                    \\033[38;5;216m│\\033[0m\\n'
printf '\\033[38;5;216m╰───────────────────────────────────────────╯\\033[0m\\n\\n'
printf '\\033[2m> \\033[0m결제 API 지연을 줄여 줘\\n\\n'
printf '\\033[38;5;111m⏺\\033[0m Read(\\033[1msrc/pay/order.rs\\033[0m)\\n  ⎿  Read 214 lines\\n\\n'
printf '\\033[38;5;111m⏺\\033[0m Update(\\033[1msrc/pay/order.rs\\033[0m)\\n'
printf '  ⎿  \\033[32m+ let order = cache.get_or_load(id).await?;\\033[0m\\n'
printf '     \\033[31m- let order = db.load(id).await?;\\033[0m\\n\\n'
L='────────────────────────────────────────────'
box() { printf '\\033[2m%s\\033[0m\\n\\033[2m❯\\033[0m \\n\\033[2m%s\\033[0m\\n  \\033[2m? for shortcuts\\033[0m' "$L" "$L"; }
case "$MODE" in
  working)
    printf '\\033[38;5;216m✻\\033[0m Running load test… \\033[2m(1m 12s · esc to interrupt)\\033[0m\\n\\n'
    box ;;
  question)
    printf '\\033[2m%s\\033[0m\\n \\033[1mBash command\\033[0m\\n\\n   cargo test -p shop-api\\n   Run the shop-api tests\\n\\n' "$L"
    printf ' Do you want to proceed?\\n \\033[38;5;216m❯\\033[0m 1. Yes\\n   2. Yes, and don\\047t ask again for cargo test commands in ~/work/shop-api\\n'
    printf '   3. No, and tell Claude what to do differently (esc)\\n\\n \\033[2mEsc to cancel · Tab to amend\\033[0m\\n' ;;
  *)
    printf '\\033[38;5;111m⏺\\033[0m p95가 182ms에서 64ms로 줄었다. 시험 3회 통과.\\n\\n'
    box ;;
esac
exec cat > /dev/null
`,
  { mode: 0o755 },
);
const agentToml = (work) =>
  `schema = 1\n[agent]\nwork_root = "${work}"\n[profiles.claude-opus]\nharness = "custom"\ncommand = ["/bin/sh", "${root}/fake.sh"]\ndescription = "견본"\n[profiles.claude-opus.env]\nAA = "${AA}"\n`;
fs.writeFileSync(`${root}/c/agent.toml`, agentToml(`${root}/work`));
fs.writeFileSync(`${root}/cb/agent.toml`, agentToml(`${root}/workb`));

const base = `http://127.0.0.1:${webPort}/`;
const browser = await chromium.launch();
let ok = false;
try {
  start("host", envA, "host");
  await until("host 준비", () => aaa("status").web_listen);
  start("agentA", envA, "agent", "--host", "local", "--name", "mac-a");
  await until("mac-a", () => aaa("machine", "list").machines.some((m) => m.name === "mac-a" && m.connection === "connected"));
  // mac-b: 원격 등록
  aaa("machine", "enroll", "--open", "5m");
  const enroll = spawn(AA, ["agent", "enroll", `127.0.0.1:${agentPort}`, "--name", "mac-b"], { env: envB });
  const code = await new Promise((resolve, reject) => {
    let buf = "";
    const on = (d) => {
      buf += d;
      const m = /--code\s+(\S+)/.exec(buf);
      if (m) resolve(m[1]);
    };
    enroll.stdout.on("data", on);
    enroll.stderr.on("data", on);
    enroll.on("exit", () => reject(new Error(`enroll 끝남: ${buf}`)));
  });
  aaa("machine", "approve", "mac-b", "--code", code);
  await new Promise((r) => (enroll.exitCode === null ? enroll.once("exit", r) : r()));
  start("agentB", envB, "agent", "--host", `127.0.0.1:${agentPort}`, "--name", "mac-b");
  await until("mac-b", () => aaa("machine", "list").machines.some((m) => m.name === "mac-b" && m.connection === "connected"));

  // pane: 셸 둘, 가짜 Claude 셋(작업 중·입력 대기·질문)
  // 새 탭에 띄우고 그 pane ID를 돌려준다
  const run = (machine, name, argv) => aaa("tab", "create", "--machine", machine, "--label", name, "--", ...argv).tabs.at(-1).root.pane;
  const sh = run("mac-a", "build", ["/bin/sh"]);
  for (const [mode, name, m] of [
    ["working", "claude-api", "mac-a"],
    ["idle", "claude-web", "mac-b"],
    ["question", "claude-infra", "mac-b"],
  ]) {
    run(m, name, [`${root}/bin/claude`, mode]);
  }
  run("mac-b", "logs", ["/bin/sh"]);
  aaa("pane", "run", sh, "printf '\\033[1;32m✓\\033[0m cargo build --release\\n  Compiling shop-api v0.4.2\\n  \\033[1;32mFinished\\033[0m release in 41.2s\\n'");

  // 프로젝트·thread
  aaa("project", "create", "shop");
  const task = (n, t) => (fs.writeFileSync(`${root}/${n}`, t), `${root}/${n}`);
  const t1 = aaa("thread", "start", "shop", "--title", "결제 API 지연 줄이기", "--machine", "mac-a", "--profile", "claude-opus",
    "--kind", "dir", "--task-file", task("t1.md", "p95를 100ms 아래로"));
  aaa("thread", "start", "shop", "--title", "주문 목록 페이지 나누기", "--machine", "mac-b", "--profile", "claude-opus",
    "--kind", "dir", "--task-file", task("t2.md", "오래 걸리는 시험"));
  aaa("thread", "start", "shop", "--title", "로그인 세션 저장 방식", "--machine", "mac-b", "--profile", "claude-opus",
    "--kind", "dir", "--task-file", task("t3.md", "결정이 필요한 일"));
  aaa("evaluator", "assign", "shop", "--profile", "claude-opus", "--machine", "mac-a");
  await until("보고 회수", () => aaa("thread", "show", "shop", t1.id).report_hash, 30_000);
  await until("질문", () => aaa("question", "list").questions.length > 0, 30_000);
  await sleep(1500);

  const shots = [];
  for (const scheme of ["dark", "light"]) {
    for (const [w, h, kind] of [
      [1920, 1080, "desktop"],
      [820, 1180, "pad"],
      [390, 844, "phone"],
    ]) {
      const phone = kind === "phone";
      const touch = kind !== "desktop";
      const ctx = await browser.newContext({
        viewport: { width: w, height: h },
        deviceScaleFactor: touch ? 2 : 1,
        isMobile: phone,
        hasTouch: touch,
        colorScheme: scheme,
      });
      // 테마는 설정이 정한다(기본 어두움, herdr-web-ui settings.ts): 밝음 견본은 설정 값을 넣고 연다
      await ctx.addInitScript((theme) => localStorage.setItem("aaa:appearance", JSON.stringify({ theme })), scheme);
      const page = await ctx.newPage();
      // thread 줄은 그 agent pane을 연다(차이 목록 L4): 보고서는 터미널 머리의 「보고」로
      const openReport = async (id) => {
        await page.getByTestId(`thread-shop-${id}`).click();
        const report = page.getByTestId("open-report");
        await Promise.race([page.getByTestId("report").waitFor(), report.waitFor()]);
        if (await report.isVisible()) await report.click();
        await until("보고", async () => (await page.getByTestId("report").innerText()).includes("p95"));
      };
      const shot = async (name) => {
        await sleep(400);
        const f = path.join(out, `${prefix}-${kind}-${name}-${scheme}.png`);
        await page.screenshot({ path: f });
        shots.push(f);
      };
      await page.goto(base);
      await shot("pair");
      await page.getByLabel("페어링 코드").fill(aaa("web", "pair").code);
      await page.getByRole("button", { name: "이 기기 페어링" }).click();
      await page.getByTestId("machine-mac-a").waitFor();
      if (!phone) {
        await page.getByTestId("pane-claude-api").click();
        await until("터미널", async () => (await page.locator(".xterm-rows").innerText()).includes("Claude"));
        await shot("terminal");
        await page.getByTestId("pane-claude-infra").click();
        await until("질문 화면", async () => (await page.locator(".xterm-rows").innerText()).includes("proceed"));
        await shot("question");
        await openReport(t1.id);
        await shot("thread");
      } else {
        await shot("list");
        await page.getByTestId("pane-claude-api").click();
        await until("터미널", async () => (await page.locator(".xterm-rows").innerText()).includes("Claude"));
        await shot("terminal");
        await page.goBack().catch(() => {});
        await page.goto(base);
        await openReport(t1.id);
        await shot("thread");
      }
      await ctx.close();
    }
  }
  // TUI(aaa attach)를 웹 터미널 안에서 찍는다: workspace demo = [claude-api │ 질문 중인 Claude] 탭 + build 탭
  const paneId = (n) => aaa("pane", "list").panes.find((p) => p.name === n).id;
  aaa("pane", "move", paneId("claude-api"), "--new-tab", "-w", "demo");
  aaa("pane", "split", paneId("claude-api"), "-w", "demo", "--direction", "right", "--machine", "mac-b", "--", `${root}/bin/claude`, "question");
  aaa("pane", "move", paneId("build"), "--new-tab", "-w", "demo");
  aaa("tab", "create", "--machine", "mac-a", "--label", "tui", "--cols", "150", "--rows", "40", "--",
    "/usr/bin/env", `AAA_CONFIG_DIR=${root}/c`, `AAA_STATE_DIR=${root}/s`, "TERM=xterm-256color", AA, "attach", "-w", "demo");
  for (const scheme of ["dark", "light"]) {
    const ctx = await browser.newContext({ viewport: { width: 1920, height: 1080 }, colorScheme: scheme });
    await ctx.addInitScript((theme) => localStorage.setItem("aaa:appearance", JSON.stringify({ theme })), scheme);
    const page = await ctx.newPage();
    await page.goto(base);
    await page.getByLabel("페어링 코드").fill(aaa("web", "pair").code);
    await page.getByRole("button", { name: "이 기기 페어링" }).click();
    await page.getByTestId("pane-tui").click();
    await until("TUI", async () => {
      const t = await page.locator(".xterm-rows").innerText();
      return t.includes("1:") && t.includes("2:") && t.includes("proceed");
    });
    await sleep(600);
    const f = path.join(out, `${prefix}-desktop-tui-${scheme}.png`);
    await page.screenshot({ path: f });
    shots.push(f);
    await ctx.close();
  }
  console.log(shots.join("\n"));
  ok = true;
} finally {
  await browser.close();
  for (const [, p] of procs) if (p.exitCode === null) p.kill("SIGKILL");
  await sleep(300);
  killSessionHosts(`${root}/s`);
  killSessionHosts(`${root}/sb`);
  if (ok) fs.rmSync(root, { recursive: true, force: true });
  else console.error(`실험실을 남겼다: ${root}`);
}
