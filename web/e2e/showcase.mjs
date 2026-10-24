// 화면 견본(디자인 비교·검수용): 실제 aaa host와 agent 둘(mac-a 로컬, mac-b 원격 등록)을 띄워
// 셸·가짜 Claude(작업 중·입력 대기·질문)·thread(검토 대기·작업 중·확인 필요)가 있는 상태를 만들고,
// 같은 화면을 데스크톱 1920×1080·패드 820×1180·휴대폰 390×844, 어두움·밝음으로 찍는다.
// `--readme`면 README 그림(docs/media)을 만든다: 터미널·질문·보고·auto mode·자동화·workspace·휴대폰과 흐름 영상
// (coordinator에 글 보내기 → thread 진행 → 보고) GIF 하나. 영상은 ffmpeg가 있어야 하고, 영상 폴더를 주면 mp4도 둔다.
//
// 실행(저장소 루트): (cd web && npm run build) && cargo build -p aaa
//                    NODE_PATH=$(npm root -g) node web/e2e/showcase.mjs <스크린샷 폴더> [접두사]
//                    NODE_PATH=$(npm root -g) node web/e2e/showcase.mjs --readme docs/media [영상 폴더]
// 끝나면 띄운 프로세스를 PID로 끝내고 실험실 폴더를 지운다.

import { spawn, execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { launchChromium } from "./lib/chromium-launch.mjs";

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), "noop.js"));
const { chromium } = require("playwright");

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const AA = process.env.AAA_BIN ?? path.join(repo, "target/debug/aaa");
const readme = process.argv[2] === "--readme";
const [out, extra] = process.argv.slice(readme ? 3 : 2);
const prefix = readme ? "" : (extra ?? "aaa");
if (!out) throw new Error("스크린샷 폴더를 준다");
if (readme) execFileSync("ffmpeg", ["-version"], { stdio: "ignore" });
fs.mkdirSync(out, { recursive: true });
const root = `/tmp/aas-${process.pid}`;
// 구독 사용량은 빈 실험실 집에서 읽는다(host usage.rs 시험 고리): 찍는 Mac의 실제 사용량이 화면에 나오지 않는다.
// brief를 치기 전 화면 멈춤(3초)은 시험처럼 줄인다: 영상에서 thread가 바로 일을 시작한다
// 운영 세션·인증·설정은 상속하지 않는다. 모든 앱 데이터와 셸 홈은 가짜 실험실이다.
const cleanEnv = Object.fromEntries(Object.entries(process.env).filter(([k]) => !/^(AAA_|CODEX_|CLAUDE_|XDG_)/.test(k)));
const labEnv = { ...cleanEnv, HOME: `${root}/home`, USER: "demo", LOGNAME: "demo", SHELL: "/bin/sh",
  PS1: "$ ", PATH: `${root}/bin:/usr/bin:/bin:/usr/sbin:/sbin`, TMPDIR: `${root}/tmp`,
  AAA_USAGE_HOME: `${root}/usagehome`, AAA_TEST_BRIEF_SETTLE_MS: "300", RUST_LOG: "aaa=warn" };
const envA = { ...labEnv, AAA_CONFIG_DIR: `${root}/c`, AAA_STATE_DIR: `${root}/s` };
const envB = { ...labEnv, AAA_CONFIG_DIR: `${root}/cb`, AAA_STATE_DIR: `${root}/sb` };
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
  let lastError;
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (error) {
      lastError = error;
    }
    if (Date.now() > deadline) throw new Error(`${ms}ms 안에 ${what}이 되지 않았다`, { cause: lastError });
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
for (const d of ["c", "s", "cb", "sb", "work", "workb", "usagehome", "home", "tmp"]) fs.mkdirSync(`${root}/${d}`, { recursive: true });
execFileSync("mkfifo", [`${root}/finish-thread`]);
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
        *천천히*)
          "$AA" report --percent 20 --activity '코드 읽기'
          printf '\\033[2m  · src/orders/export.rs를 읽는 중…\\033[0m\\n'
          read -r finish < "${root}/finish-thread"
          "$AA" report --percent 70 --activity '변경 시험'
          printf '\\033[2m  · cargo test -p shop-api: 12 passed\\033[0m\\n'
          "$AA" report --percent 100 --activity '보고'
          printf '[검수 대기] 결과를 확인할 차례\\n\\n## Report\\n주문 내역을 **CSV로 내보내기**를 더했다.\\n\\n- 주소: \\140GET /orders/export.csv\\140\\n- 기간·상태로 거르기\\n- 확인: 시험 12개 통과\\n\\n## Next\\n- 결과를 확인하고 병합하기\\n' > "$dir/report.md" ;;
        *결정*)
          "$AA" report --percent 50 --activity 'Waiting for you'
          printf '[검수 대기] 결과를 확인할 차례\\n\\n## Report\\n로그인 저장 방식을 정해야 한다.\\n\\n- 쿠키 세션: 서버 상태가 필요하다\\n- JWT: 폐기가 어렵다\\n\\n## Next\\n- 쿠키 세션으로 정하기(추천)\\n' > "$dir/report.md" ;;
        *)
          mkdir -p "$dir/library"
          printf '<!doctype html><meta charset=utf-8><body style="font-family:sans-serif;padding:24px;background:#1f1d1a;color:#eee"><h1>성능 보고</h1><p>p95 지연(ms)</p><p>전 <span style="display:inline-block;width:70%%;height:14px;background:#d9775a"></span> 182</p><p>후 <span style="display:inline-block;width:25%%;height:14px;background:#7cb87a"></span> 64</p></body>' > "$dir/library/index.html"
          printf '[검수 대기] 결과를 확인할 차례\\n\\n## Report\\n결제 API의 **p95 지연**을 182ms에서 64ms로 줄였다.\\n\\n- 원인: 주문마다 \\140SELECT\\140을 다시 했다\\n- 고침: 요청 안에서 한 번만 읽는다\\n- 확인: 부하 시험 3회\\n\\n## Next\\n- 결과를 확인하고 병합하기\\n- 캐시 만료 시간 정하기\\n' > "$dir/report.md" ;;
      esac
      ;;
  esac
  printf '\\033[2m❯\\033[0m '
done
`,
);
// Claude처럼 그리는 세션(`<root>/bin/claude <모드>`): 실행 파일 이름이 claude라 agent가 화면을 Claude 규칙으로 보고
// 상태를 정한다(plan §8.110). 화면 아래쪽은 실제 Claude Code 모양이고 판정은 herdr 원본 엔진으로 확인했다(작업 중
// `live_turn_working`, 질문 `bash_permission_prompt`, 입력 대기 `live_prompt_box`). hook은 세션 ID만 알린다.
fs.mkdirSync(`${root}/bin`, { recursive: true });
fs.writeFileSync(
  `${root}/bin/claude`,
  `#!/bin/sh
if [ "$1" = "--version" ]; then printf 'Claude Code demo\\n'; exit 0; fi
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
  `schema = 1\n[agent]\nwork_root = "${work}"\n`;
fs.writeFileSync(`${root}/c/agent.toml`, agentToml(`${root}/work`));
fs.writeFileSync(`${root}/cb/agent.toml`, agentToml(`${root}/workb`));

fs.appendFileSync(`${root}/c/host.toml`, `\n[profiles.claude-opus]\nagent = "custom"\ncommand = ["/bin/sh", "${root}/fake.sh"]\ndescription = "견본"\n[profiles.claude-opus.env]\nAA = "${AA}"\n`);

// 대화 화면도 운영 transcript 대신 견본만 읽는다.
const transcript = path.join(labEnv.HOME, ".claude", "projects", "demo", "coordinator.jsonl");
fs.mkdirSync(path.dirname(transcript), { recursive: true });
fs.writeFileSync(transcript, [
  { type: "user", message: { role: "user", content: "결제 API를 빠르게 하고 주문 목록과 로그인도 정리해 줘." } },
  { type: "assistant", message: { content: [{ type: "text", text: "세 작업으로 나눴습니다.\n\n- mac-a: 결제 API 지연 개선 — 보고 도착\n- mac-b: 주문 목록 페이지 나누기 — 진행 중\n- mac-b: 로그인 세션 저장 방식 — 결정 대기\n\n일반적인 구현 선택은 근거를 남겨 진행하고, 사용자가 정해야 할 일은 질문으로 모으겠습니다." }] } },
].map(row => JSON.stringify(row) + "\n").join(""));
const coordinatorHook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "6a1f0c2e-3b4d-4e5f-8a9b-0c1d2e3f4a77", source: "startup", transcript_path: transcript });

// 가짜 coordinator(README 영상): 사람이 보낸 글을 받으면 그 일을 mac-b의 thread로 맡긴다(천천히 일하고 보고하는 가짜 thread)
fs.writeFileSync(`${root}/t4.md`, "천천히 일하고 보고");
fs.writeFileSync(
  `${root}/coord.sh`,
  `#!/bin/sh
"$AA" hook --agent claude '${coordinatorHook}' > /dev/null
printf '\\033[38;5;216m✻\\033[0m \\033[1mcoordinator\\033[0m · shop\\n\\n\\033[2m❯\\033[0m '
while IFS= read -r line; do
  case "$line" in
    *CSV*)
      printf '\\n\\033[38;5;111m⏺\\033[0m mac-b에 thread를 맡긴다: \\033[1m주문 내역 CSV 내보내기\\033[0m\\n'
      "$AA" thread start shop --title "주문 내역 CSV 내보내기" --machine mac-b --profile claude-opus --kind dir --task-file "${root}/t4.md" > /dev/null
      printf '  ⎿  시작했다. 보고가 오면 알린다.\\n\\n' ;;
  esac
  printf '\\033[2m❯\\033[0m '
done
`,
);
fs.appendFileSync(`${root}/c/host.toml`, `\n[profiles.coordinator]\nagent = "claude"\ncommand = ["/bin/sh", "${root}/coord.sh"]\ndescription = "견본 coordinator"\n[profiles.coordinator.env]\nAA = "${AA}"\n`);

const base = `http://127.0.0.1:${webPort}/`;
const browser = await launchChromium(chromium);

/** 새 화면. 테마는 설정이 정한다(기본 어두움, herdr-web-ui settings.ts): 밝음 견본은 설정 값을 넣고 연다. line이면 입력줄(composer)을 쓴다 */
async function openPage({ width, height, kind, scheme = "dark", line = false }) {
  const touch = kind !== "desktop";
  const ctx = await browser.newContext({
    viewport: { width, height },
    deviceScaleFactor: touch ? 2 : 1,
    isMobile: kind === "phone",
    hasTouch: touch,
    colorScheme: scheme,
  });
  await ctx.addInitScript(
    ([theme, line]) => {
      localStorage.setItem("aaa:appearance", JSON.stringify({ theme }));
      if (line) localStorage.setItem("aaa:terminal-input-mode", "line");
    },
    [scheme, line],
  );
  const page = await ctx.newPage();
  await page.goto(base);
  // 넣은 터미널 글꼴을 먼저 받아 둔다(늦게 받으면 터미널이 다른 글꼴로 찍힌다, harness pairPage와 같다)
  await page.evaluate(() => Promise.all([400, 700].map((w) => document.fonts.load(`${w} 13px "Meslotendard Web"`))));
  return { ctx, page };
}

async function pair(page) {
  await page.getByLabel("페어링 코드").fill(aaa("web", "pair").code);
  await page.getByRole("button", { name: "이 기기 페어링" }).click();
  // 붙어서 목록을 받았다(좁은 창은 목록이 닫힌 drawer 안이라 보이지 않는다)
  await page.getByTestId("machine-mac-a").waitFor({ state: "attached" });
}

/** 목록 drawer의 움직임이 끝났다. nextFrame이면 방금 누른 것이 시작한 움직임까지 본다(harness drawerSettled) */
function drawerSettled(page, nextFrame = false) {
  return page.evaluate(async (nextFrame) => {
    if (nextFrame) await new Promise((r) => requestAnimationFrame(() => r()));
    await Promise.all((document.getElementById("workspace-drawer")?.getAnimations() ?? []).map((a) => a.finished.catch(() => null)));
  }, nextFrame);
}

/** 좁은 화면(768px 이하)은 목록이 pane 위의 drawer다(web/src/drawer.tsx): 닫혀 있으면 화면 머리의 「목록 열기」로 연다 */
async function openDrawer(page) {
  await drawerSettled(page);
  const toggle = page.locator(".view [data-testid='drawer-toggle']");
  if ((await toggle.isVisible()) && (await toggle.getAttribute("aria-expanded")) !== "true") {
    await toggle.click();
    await page.locator("#workspace-drawer.is-open").waitFor();
    await drawerSettled(page, true);
  }
}

/** 목록 안의 줄을 누른다(harness inList의 drawer 부분). 줄을 고르면 drawer가 닫히며 미끄러지므로 끝날 때까지 기다린다 */
async function inList(page, item) {
  if (typeof item === "string") item = page.getByTestId(item);
  await item.waitFor({ state: "attached" });
  await openDrawer(page);
  await item.click();
  await drawerSettled(page, true);
}

const termHas = (page, text) => until(`터미널에 ${text}`, async () => (await page.locator(".xterm-rows").innerText()).includes(text));

/** thread 줄은 그 agent pane을 연다(차이 목록 L4): 보고서는 터미널 머리의 「보고」로 */
async function openReport(page, id, text) {
  await inList(page, `thread-shop-${id}`);
  const report = page.getByTestId("open-report");
  await Promise.race([page.getByTestId("report").waitFor(), report.waitFor()]);
  if (await report.isVisible()) await report.click();
  await until("보고", async () => (await page.getByTestId("report").innerText()).includes(text));
}

/** 디자인 검수용 견본: 데스크톱·패드·휴대폰 × 어두움·밝음 */
async function designShots(t1) {
  const shots = [];
  for (const scheme of ["dark", "light"]) {
    for (const [width, height, kind] of [
      [1920, 1080, "desktop"],
      [820, 1180, "pad"],
      [390, 844, "phone"],
    ]) {
      const { ctx, page } = await openPage({ width, height, kind, scheme });
      const shot = async (name) => {
        const f = path.join(out, `${prefix}-${kind}-${name}-${scheme}.png`);
        await page.screenshot({ path: f, animations: "disabled" });
        shots.push(f);
      };
      await shot("pair");
      await pair(page);
      if (kind === "phone") {
        await openDrawer(page);
        await shot("list");
      }
      await inList(page, "pane-claude-api");
      await termHas(page, "Claude");
      await shot("terminal");
      if (kind !== "phone") {
        await inList(page, "pane-claude-infra");
        await termHas(page, "proceed");
        await shot("question");
      }
      await openReport(page, t1, "p95");
      await shot("thread");
      await ctx.close();
    }
  }
  return shots;
}

/** README 그림(docs/media): 데스크톱 질문·보고, 휴대폰 목록, 흐름 영상 */
async function readmeMedia(t1) {
  const files = [];
  const save = async (page, name) => {
    const f = path.join(out, name);
    await page.screenshot({ path: f, animations: "disabled" });
    files.push(f);
  };
  {
    // 여러 머신의 agent와 thread를 한 목록에, Claude의 확인창은 웹 카드로 답한다
    const { ctx, page } = await openPage({ width: 1440, height: 900, kind: "desktop" });
    await pair(page);
    await inList(page, "pane-claude-api");
    await termHas(page, "Claude");
    await save(page, "desktop.png");
    await inList(page, "pane-claude-infra");
    await termHas(page, "proceed");
    await page.locator(".prompt-card").waitFor();
    await save(page, "question.png");
    await openReport(page, t1, "p95");
    await save(page, "report.png");
    await inList(page, page.locator("#sidebar-column .pane-item.is-coordinator").first());
    await page.getByTestId("chat-toggle").waitFor();
    if (!(await page.getByTestId("chat").count())) await page.getByTestId("chat-toggle").click();
    await page.getByText("세 작업으로 나눴습니다.", { exact: false }).waitFor();
    await page.locator('.term-head [data-testid="more"]').click();
    await page.getByTestId("auto-mode-menu").click();
    await page.getByTestId("auto-mode-switch").waitFor();
    await save(page, "auto-mode.png");
    aaa("project", "auto-mode", "off");
    await page.getByRole("button", { name: "auto mode 요약 닫기" }).click();
    await page.getByTestId("activity-automations").click();
    await page.getByTestId("automation-shop-daily-review").click();
    await page.getByTestId("automation-detail").waitFor();
    await page.getByTestId("automation-source").locator("pre").first().waitFor();
    await save(page, "automation.png");
    await page.getByTestId("activity-machines").click();
    await page.getByRole("button", { name: "mac-a 기기 정보", exact: true }).click();
    await page.getByRole("checkbox", { name: "작업 폴더 자동 배정 허용" }).waitFor();
    await save(page, "workspaces.png");
    await ctx.close();
  }
  {
    // 휴대폰: 같은 목록(프로젝트·thread 상태). 뒤에 열린 pane은 Claude 터미널로 둔다(처음 화면은 coordinator)
    const { ctx, page } = await openPage({ width: 390, height: 844, kind: "phone" });
    await pair(page);
    await inList(page, "pane-claude-api");
    if (await page.getByTestId("chat").isVisible()) await page.getByTestId("chat-toggle").click();
    await termHas(page, "Claude");
    await openDrawer(page);
    await save(page, "phone.png");
    await ctx.close();
  }
  files.push(...(await flowVideo()));
  return files;
}

/** 흐름 영상: coordinator에 글 보내기 → mac-b thread가 일하며 진행을 알림 → 보고 도착 → 보고 열기 */
async function flowVideo() {
  const [width, height] = [1024, 640];
  const { ctx, page } = await openPage({ width, height, kind: "desktop", line: true });
  await pair(page);
  await inList(page, page.locator("#sidebar-column .pane-item.is-coordinator").first());
  if (await page.getByTestId("chat").count()) await page.getByTestId("chat-toggle").click();
  await termHas(page, "coordinator");
  const input = page.getByLabel("보낼 글");
  await input.waitFor();
  // 실제 상태가 확인된 네 화면을 정해진 길이로 보여 준다. 기다림은 DOM/보고 사건으로만 끝낸다.
  const dir = `${root}/frames`;
  fs.mkdirSync(dir, { recursive: true });
  const list = ["ffconcat version 1.0"];
  const frame = async (name, duration) => {
    const file = `${dir}/${name}.png`;
    await page.screenshot({ path: file, animations: "disabled" });
    list.push(`file '${file}'`, `duration ${duration}`);
  };
  await frame("coordinator", 1.2);
  await input.fill("주문 내역을 CSV로 내보내는 기능을 만들어 줘");
  await frame("request", 1.5);
  await input.press("Enter");
  const t4 = await until("새 thread", () => aaa("thread", "list", "shop").threads.find((t) => t.title === "주문 내역 CSV 내보내기"));
  const worker = await until("새 thread pane", () => aaa("thread", "show", "shop", t4.id).pane);
  aaa("pane", "wait-output", worker, "--match=src/orders/export.rs", "--timeout", "20000");
  await page.evaluate(pane => { location.hash = `pane=${pane}`; }, worker);
  await termHas(page, "src/orders/export.rs");
  await frame("working", 1.5);
  fs.writeFileSync(`${root}/finish-thread`, "finish\n");
  await until("보고 회수", () => aaa("thread", "show", "shop", t4.id).report_hash, 30_000);
  await openReport(page, t4.id, "CSV");
  await frame("report", 3);
  list.push(`file '${dir}/report.png'`);
  fs.writeFileSync(`${dir}/list.ffconcat`, list.join("\n") + "\n");
  await ctx.close();
  const input_ = ["-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", `${dir}/list.ffconcat`];
  // GIF: 저장소 파일은 1MiB 아래여야 한다(scripts/check-public.sh). 화면이 바뀐 때만 frame을 두고(vfr) 바뀐 곳만 다시 그린다
  const gif = path.join(out, "flow.gif");
  execFileSync("ffmpeg", [
    ...input_,
    "-vf",
    "split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=none:diff_mode=rectangle",
    "-fps_mode",
    "vfr",
    gif,
  ]);
  const made = [gif];
  if (extra) {
    fs.mkdirSync(extra, { recursive: true });
    const mp4 = path.join(extra, "flow.mp4");
    execFileSync("ffmpeg", [...input_, "-vf", "fps=30", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-movflags", "+faststart", mp4]);
    made.push(mp4);
  }
  return made;
}

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
  aaa("machine", "workspaces", "mac-a", "--allow", "작업 폴더");
  fs.mkdirSync(`${root}/projects/shop/automations`, { recursive: true });
  fs.writeFileSync(`${root}/projects/shop/automations/daily-review.toml`, `title = "매일 작업 현황 정리"
enabled = false
[on.schedule]
daily = "09:00"
[do.inbox]
text = "진행 중인 일과 막힌 일을 확인하고 다음 할 일을 정리한다."
`);
  if (readme) aaa("project", "open", "shop", "--profile", "coordinator");
  await until("보고 회수", () => aaa("thread", "show", "shop", t1.id).report_hash, 30_000);
  await until("질문", () => aaa("question", "list").questions.length > 0, 30_000);

  const shots = readme ? await readmeMedia(t1.id) : await designShots(t1.id);
  console.log(shots.join("\n"));
  ok = true;
} catch (error) {
  for (const [i, ctx] of browser.contexts().entries()) {
    for (const [j, page] of ctx.pages().entries()) {
      await page.screenshot({ path: `${root}/failure-${i}-${j}.png` }).catch(() => {});
    }
  }
  throw error;
} finally {
  await browser.close();
  await Promise.all([...procs.values()].map(p => p.exitCode !== null ? Promise.resolve() : new Promise(resolve => { p.once("exit", resolve); p.kill("SIGKILL"); })));
  killSessionHosts(`${root}/s`);
  killSessionHosts(`${root}/sb`);
  if (ok) fs.rmSync(root, { recursive: true, force: true });
  else console.error(`실험실을 남겼다: ${root}`);
}
