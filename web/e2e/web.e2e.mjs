// 웹 front E2E: 실제 aa host·agent를 띄우고 headless Chromium으로 pairing·보기·입력·권한·끊김을 확인한다.
//
// 실행(저장소 루트): cargo build -p aa && (cd web && npm run build) && cargo build -p aa
//                    NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs [스크린샷 폴더]
// AA_BIN으로 실행 파일을 바꿀 수 있다. 실패하면 실험실 폴더를 남기고 종료 코드 1로 끝난다.

import { spawn, execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), "noop.js"));
const { chromium } = require("playwright");

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const AA = process.env.AA_BIN ?? path.join(repo, "target/debug/aa");
const shots = process.argv[2] ?? null;
const root = `/tmp/aaw-${process.pid}`;
const env = { ...process.env, AA_CONFIG_DIR: `${root}/c`, AA_STATE_DIR: `${root}/s`, RUST_LOG: "aa=info" };
const procs = new Map();

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function aa(...args) {
  const sep = args.indexOf("--");
  const a = sep < 0 ? [...args, "--json"] : [...args.slice(0, sep), "--json", ...args.slice(sep)];
  return JSON.parse(execFileSync(AA, a, { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
}

function start(tag, ...args) {
  const log = fs.openSync(`${root}/${tag}.log`, "w");
  const p = spawn(AA, args, { env, stdio: ["ignore", log, log] });
  procs.set(tag, p);
  return p;
}

async function stop(tag) {
  const p = procs.get(tag);
  procs.delete(tag);
  if (!p || p.exitCode !== null) return;
  const done = new Promise((r) => p.once("exit", r));
  p.kill("SIGKILL");
  await done;
}

async function until(what, fn, ms = 20_000) {
  const deadline = Date.now() + ms;
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      // 아직 준비되지 않았다
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

/** session host는 setsid로 분리돼 있다. meta.json의 PID 중 명령줄이 이 실험실을 가리키는 것만 끝낸다. */
function killSessionHosts() {
  const dir = `${root}/s/agent/sessions`;
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

const screenText = (page) => page.locator(".xterm-rows").innerText();
const shot = async (page, name) => shots && (await page.screenshot({ path: path.join(shots, `${name}.png`) }));

async function pairPage(browser, code, viewport) {
  const ctx = await browser.newContext({ viewport, isMobile: viewport.width < 700, hasTouch: viewport.width < 700 });
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(base);
  await page.getByLabel("pairing 코드").fill(code);
  await page.getByRole("button", { name: "연결" }).click();
  await page.getByTestId("machine-labA").waitFor();
  return { page, errors };
}

let base = "";
let failed = false;
const steps = [];
async function step(name, fn) {
  const t0 = Date.now();
  try {
    await fn();
    steps.push(`ok   ${name} (${Date.now() - t0}ms)`);
  } catch (e) {
    steps.push(`FAIL ${name}: ${e.message}`);
    failed = true;
    throw e;
  }
}

fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(`${root}/c`, { recursive: true });
if (shots) fs.mkdirSync(shots, { recursive: true });
const webPort = await freePort();
fs.writeFileSync(
  `${root}/c/host.toml`,
  `schema = 1\n[host]\nagent_listen = ""\n[web]\nlisten = "127.0.0.1:${webPort}"\n[local_agent]\nmode = "none"\n` +
    `[projects]\nroot = "${root}/projects"\nallow_profiles = ["fake"]\nlibrary_copy_mb = 1\n`,
);
// 가짜 thread agent: brief 안내를 받으면 Markdown 보고와 결과물(HTML·그림·복사 상한을 넘는 파일)을 쓴다.
// 결과물 HTML은 자기가 격리됐는지(부모 문서·cookie·API에 닿는지) body data에 적는다.
fs.writeFileSync(
  `${root}/pic.png`,
  Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==", "base64"),
);
fs.writeFileSync(
  `${root}/index.html`,
  `<!doctype html><meta charset=utf-8><body><h1 id=t>결과 보고</h1><img id=pic src="pic.png"><script>
var r = document.body.dataset;
try { parent.document.title; r.parent = "open"; } catch (e) { r.parent = "blocked"; }
try { r.cookie = document.cookie === "" ? "none" : "visible"; } catch (e) { r.cookie = "blocked"; }
fetch("/api/v1/me").then(function (x) { r.api = String(x.status); }).catch(function () { r.api = "blocked"; });
</script>`,
);
fs.writeFileSync(
  `${root}/fake.sh`,
  `#!/bin/sh
printf 'fake> '
while IFS= read -r line; do
  case "$line" in
    *brief.md*)
      dir=$(dirname "$(printf '%s' "$line" | sed 's/를 읽고.*//')")
      cp "${root}/index.html" "${root}/pic.png" "$dir/library/"
      head -c 2097152 /dev/urandom > "$dir/library/big.bin"
      printf '[완료]\\n\\n## Report\\n- 웹 보기 **확인**: \\140index.html\\140\\n\\n## Next\\n- 결과 확인\\n' > "$dir/report.md"
      echo done ;;
  esac
  printf 'fake> '
done
`,
);
fs.writeFileSync(
  `${root}/c/agent.toml`,
  `schema = 1\n[agent]\nwork_root = "${root}/work"\n[profiles.fake]\nharness = "custom"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`,
);
base = `http://127.0.0.1:${webPort}/`;
const browser = await chromium.launch();

try {
  await step("host·agent 시작", async () => {
    start("host", "host");
    await until("host 준비", () => aa("status").web_listen);
    start("agent", "agent", "--host", "local", "--name", "labA");
    await until("labA 연결", () => aa("machine", "list").machines.some((m) => m.name === "labA" && m.connection === "connected"));
    aa("pane", "run", "--machine", "labA", "--name", "t1", "--", "/bin/sh");
  });

  await step("인증 없는 요청·다른 Origin 거부", async () => {
    const r1 = await fetch(`${base}api/v1/call`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: base.slice(0, -1) },
      body: JSON.stringify({ op: "panes" }),
    });
    if (r1.status !== 401) throw new Error(`인증 없는 call이 ${r1.status}`);
    const r2 = await fetch(`${base}api/v1/pair`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: "https://evil.example" },
      body: JSON.stringify({ code: "000000" }),
    });
    if (r2.status !== 403) throw new Error(`다른 Origin pair가 ${r2.status}`);
    // fetch는 Host 헤더를 바꾸지 못한다(DNS rebinding 흉내)
    const r3 = await new Promise((resolve, reject) =>
      http.get({ host: "127.0.0.1", port: webPort, path: "/", headers: { host: "evil.example" } }, resolve).on("error", reject),
    );
    r3.resume();
    if (r3.statusCode !== 421) throw new Error(`다른 Host가 ${r3.statusCode}`);
  });

  const ctl = await pairPage(browser, aa("web", "pair").code, { width: 1280, height: 800 });
  const page = ctl.page;

  await step("controller: 키보드 입력", async () => {
    await page.getByTestId("pane-t1").click();
    await page.getByTestId("terminal").click();
    await until("터미널 준비", async () => (await screenText(page)).includes("$"));
    await page.keyboard.type("echo key-$((5*5))\n");
    await until("키보드 입력 결과", async () => (await screenText(page)).includes("key-25"));
  });

  await step("controller: 입력 막대로 한글", async () => {
    await page.getByLabel("보낼 글").fill("echo 한글-$((2*4))");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("한글 입력 결과", async () => (await screenText(page)).includes("한글-8"));
    await shot(page, "desktop-controller");
  });

  await step("두 controller: 조작 중 표시 → 가져오기 → 연결이 끝나면 풀린다", async () => {
    const other = await pairPage(browser, aa("web", "pair").code, { width: 1280, height: 800 });
    const op = other.page;
    await op.getByTestId("pane-t1").click();
    await until("다른 웹이 조작 중 표시", async () =>
      /웹\(.{4}\)이 조작 중/.test(await op.getByTestId("lease-banner").innerText()),
    );
    if (!(await op.getByLabel("보낼 글").isDisabled())) throw new Error("조작 중인데 입력 막대가 켜져 있다");
    await op.getByTestId("take").click();
    await until("가져온 쪽 배너 사라짐", async () => (await op.getByTestId("lease-banner").count()) === 0);
    await until("빼앗긴 쪽 표시", async () => (await page.getByTestId("lease-banner").count()) === 1);
    if (!(await page.getByLabel("보낼 글").isDisabled())) throw new Error("빼앗긴 쪽 입력 막대가 켜져 있다");
    await op.getByLabel("보낼 글").fill("echo taken-$((3*7))");
    await op.getByRole("button", { name: "보내기" }).click();
    await until("가져온 쪽 입력", async () => (await screenText(page)).includes("taken-21"));
    await shot(page, "desktop-lease-held");
    if (other.errors.length) throw new Error(other.errors.join("; "));
    await other.page.context().close();
    await until("연결이 끝나 lease 해제", async () => (await page.getByTestId("lease-banner").count()) === 0);
    if (await page.getByLabel("보낼 글").isDisabled()) throw new Error("풀렸는데 입력 막대가 꺼져 있다");
  });

  await step("reader(휴대폰 크기): 보기만 된다", async () => {
    const rd = await pairPage(browser, aa("web", "pair", "--reader").code, { width: 390, height: 844 });
    const rp = rd.page;
    if ((await rp.getByRole("button", { name: "새 셸" }).count()) !== 0) throw new Error("reader에게 새 셸 버튼");
    await shot(rp, "mobile-list");
    await rp.getByTestId("pane-t1").click();
    await until("reader 화면", async () => (await screenText(rp)).includes("한글-8"));
    const banner = await rp.getByTestId("readonly-banner").innerText();
    if (!banner.includes("보기 권한")) throw new Error(`reader 배너: ${banner}`);
    if ((await rp.getByLabel("보낼 글").count()) !== 0) throw new Error("reader에게 입력 막대");
    await rp.getByTestId("terminal").click();
    await rp.keyboard.type("echo reader-$((1+1))\n");
    await sleep(800);
    const lines = aa("pane", "read", "t1").lines.join("\n");
    if (lines.includes("reader-")) throw new Error("reader 입력이 PTY에 들어갔다");
    await shot(rp, "mobile-reader");
    if (rd.errors.length) throw new Error(rd.errors.join("; "));
    await rd.page.context().close();
  });

  await step("controller: 새 셸", async () => {
    const before = aa("pane", "list").panes.length;
    await page.getByTestId("machine-labA").getByRole("button", { name: "새 셸" }).click();
    await until("새 pane", () => aa("pane", "list").panes.length === before + 1);
    await page.getByTestId("pane-t1").click();
  });

  await step("확인창 질문: 카드·상태 표시 → reader는 못 답함 → 예", async () => {
    // 세션 안에서 Claude처럼 선택 창을 그리고 hook 사건(PermissionRequest)을 보낸다
    const script =
      'printf "Allow Bash(make test)?\\n\\342\\235\\257 1. Yes\\n  2. No\\nEsc to cancel\\n"; ' +
      '"$0" hook --agent claude \'{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"make test"}}\'; ' +
      "stty -echo; exec cat > /dev/null";
    aa("pane", "run", "--machine", "labA", "--name", "q1", "--", "/bin/sh", "-c", script, AA);
    const card = page.getByTestId("questions").locator(".question");
    await until("질문 카드", async () => (await card.count()) === 1);
    if (!(await card.innerText()).includes("make test")) throw new Error(`카드 본문: ${await card.innerText()}`);
    await until("pane 질문 표시", async () => (await page.getByTestId("agent-q1").innerText()) === "질문");
    const rd = await pairPage(browser, aa("web", "pair", "--reader").code, { width: 390, height: 844 });
    await rd.page.getByTestId("questions").waitFor();
    if ((await rd.page.getByRole("button", { name: "예" }).count()) !== 0) throw new Error("reader에게 답 버튼");
    await shot(page, "desktop-question");
    await card.getByRole("button", { name: "예" }).click();
    await until("카드 사라짐", async () => (await page.getByTestId("questions").count()) === 0);
    await until("reader 카드도 사라짐", async () => (await rd.page.getByTestId("questions").count()) === 0);
    const all = aa("question", "list", "--all").questions;
    if (all.length !== 1 || all[0].state !== "answered" || all[0].answer !== "yes") throw new Error(JSON.stringify(all));
    await until("pane 작업 중", async () => (await page.getByTestId("agent-q1").innerText()) === "작업 중");
    if (rd.errors.length) throw new Error(rd.errors.join("; "));
    await rd.page.context().close();
  });

  await step("thread 보고서·결과물: Markdown 보고, sandbox 결과물, agent에 남은 큰 파일, ack", async () => {
    aa("project", "create", "demo");
    fs.writeFileSync(`${root}/task.md`, "웹 보기");
    const t = aa("thread", "start", "demo", "--title", "웹 결과물", "--machine", "labA", "--profile", "fake", "--kind", "dir",
      "--task-file", `${root}/task.md`);
    await until("보고 회수", () => aa("thread", "show", "demo", t.id).report_hash);
    const item = page.getByTestId(`thread-demo-${t.id}`);
    await until("thread 목록에 검토 대기", async () => (await item.innerText()).includes("검토 대기"));
    await item.click();
    const report = page.getByTestId("report");
    await until("보고 Markdown", async () => (await report.locator("strong").innerText()) === "확인");
    if ((await report.locator("code").innerText()) !== "index.html") throw new Error(await report.innerHTML());
    const frameEl = await page.getByTestId("library-frame").elementHandle();
    const frame = await until("결과물 frame", async () => frameEl && (await frameEl.contentFrame()));
    const probe = await until("결과물 HTML 실행", async () => {
      const d = await frame.evaluate(() => ({
        title: document.getElementById("t")?.textContent,
        pic: document.getElementById("pic")?.naturalWidth,
        ...document.body.dataset,
      }));
      return d.api ? d : null;
    });
    if (probe.title !== "결과 보고" || probe.pic !== 1) throw new Error(`결과물 표시: ${JSON.stringify(probe)}`);
    // opaque origin: 부모 문서·cookie·제어 API에 닿지 못한다
    if (probe.parent !== "blocked" || probe.cookie === "visible" || probe.api === "200") {
      throw new Error(`결과물이 격리되지 않았다: ${JSON.stringify(probe)}`);
    }
    const big = page.getByTestId("files").locator("li", { hasText: "big.bin" });
    if (!(await big.innerText()).includes("agent에서 읽음")) throw new Error(await big.innerText());
    const href = await big.locator("a").getAttribute("href");
    const n = await page.evaluate(async (u) => (await (await fetch(u)).arrayBuffer()).byteLength, href);
    if (n !== 2097152) throw new Error(`agent 파일 길이 ${n}`);
    await shot(page, "desktop-thread-library");
    await page.getByRole("button", { name: "확인(ack)" }).click();
    await until("ack 뒤 검토 대기 아님", async () => !(await item.innerText()).includes("검토 대기"));
    if (aa("thread", "show", "demo", t.id).acked_hash !== aa("thread", "show", "demo", t.id).report_hash) {
      throw new Error("ack가 기록되지 않았다");
    }
    // 다음 단계는 t1 터미널을 본다
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("agent 끊김 → 읽기 전용 표시", async () => {
    await stop("agent");
    await until("끊김 배너", async () =>
      (await page.getByTestId("readonly-banner").innerText()).includes("agent와 연결이 끊겼다"),
    );
    await page.getByText("실행·끊김(읽기 전용)").first().waitFor();
    if (!(await page.getByLabel("보낼 글").isDisabled())) throw new Error("끊겼는데 입력 막대가 켜져 있다");
    await shot(page, "desktop-disconnected");
  });

  await step("agent 복귀 → 다시 입력", async () => {
    start("agent2", "agent", "--host", "local", "--name", "labA");
    await until("배너 사라짐", async () => (await page.getByTestId("readonly-banner").count()) === 0);
    await page.getByLabel("보낼 글").fill("echo again-$((9+1))");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("복귀 뒤 입력", async () => (await screenText(page)).includes("again-10"));
  });

  await step("host 재시작 → 웹이 스스로 다시 붙는다", async () => {
    await stop("host");
    await until("host 끊김 표시", async () =>
      (await page.getByTestId("readonly-banner").innerText()).includes("host와 연결이 끊겼다"),
    );
    start("host2", "host");
    await until("다시 붙음", async () => (await page.getByTestId("readonly-banner").count()) === 0, 30_000);
    // agent 재연결(입력 가능)과 화면 재구독(snapshot)은 따로 도착한다. 화면이 돌아오는 것을 본다.
    await until("재시작 뒤 화면 복원", async () => (await screenText(page)).includes("again-10"));
    await page.getByLabel("보낼 글").fill("echo host-back");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("재시작 뒤 입력", async () => (await screenText(page)).split("host-back").length >= 3);
    await shot(page, "desktop-after-host-restart");
  });

  if (ctl.errors.length) throw new Error(`페이지 오류: ${ctl.errors.join("; ")}`);
} catch (e) {
  failed = true;
  if (!steps.some((s) => s.startsWith("FAIL"))) steps.push(`FAIL ${e.message}`);
} finally {
  await browser.close();
  for (const tag of [...procs.keys()]) await stop(tag);
  killSessionHosts();
  console.log(steps.join("\n"));
  if (failed) {
    console.log(`실험실 기록을 남겼다: ${root}`);
    process.exitCode = 1;
  } else {
    fs.rmSync(root, { recursive: true, force: true });
  }
}
