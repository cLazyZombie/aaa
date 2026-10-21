// 「CLI 0회」 인수(관리 GUI W8, plan §8.417): 설치 뒤의 운영 한 바퀴를 웹 화면만으로 끝낸다. 새 머신 추가 → safety 승인 → 자동화 두
// 방식(사건마다 새 thread `do.start`, 열린 thread에 글 `do.message`) → 배포·agent 재시작 → agent·host 장애와 복구 → 깨진 설정의 안전 모드와
// 되돌리기 → 정리(thread 끝내기·자동화 지우기·머신 폐기). 이 파일이 지키는 것은 각 화면의 세부(55·60·61·63·68이 맡는다)가 아니라 이
// 사슬이 설치 뒤 aaa 명령 없이 이어진다는 것이다.
// 경계: 프로젝트 만들기·첫 기기 짝짓기·실험실 셸 탭 t1은 설치다(plan §6 Q7: 프로젝트 만들기는 설치·인스턴스 흐름). 새 머신에서 치는
// `aaa agent enroll`은 그 머신의 설치(웹 「직접 설치」 안내, M1)다. host·agent 프로세스를 죽이고 다시 띄우는 것은 감독자(launchd)와 장애를
// 흉내 내는 세상이지 사람의 터미널 입력이 아니다. 설치가 끝나면 `cli()`는 보기 명령만 통과시키고 나머지는 「CLI 0회 위반」으로 멈춘다.
// 실험실 집(HOME)·가짜 launchctl·가짜 tailscale을 쓴다: 이 Mac의 실제 aaa 서비스와 tailscale에 닿지 않는다.
import fs from "node:fs";
import crypto from "node:crypto";
import { spawn } from "node:child_process";
import readline from "node:readline";
import { root, AA, env, procs, aaa, step, start, stop, until, inList, inTime, openSettings, screenText, base, freePort, LOAD_TIMEOUT_MS } from "../lib/harness.mjs";

const home = `${root}/svchome`;
const bin = `${root}/svcbin`;
const calls = `${root}/launchctl.log`;
fs.mkdirSync(`${home}/Library/LaunchAgents`, { recursive: true });
fs.mkdirSync(bin, { recursive: true });
for (const role of ["host", "agent"]) fs.writeFileSync(`${home}/Library/LaunchAgents/dev.aaa.${role}.plist`, "<plist/>");
fs.writeFileSync(`${bin}/launchctl`, `#!/bin/sh\ncase "$1" in\nprint) printf 'state = running\\npid = 100\\n' ;;\nkickstart) echo "$@" >> '${calls}' ;;\n*) exit 1 ;;\nesac\n`, { mode: 0o755 });
// 머신 추가 안내는 tailnet 이름을 읽어 본다: 꺼진 tailscale 흉내
fs.writeFileSync(`${bin}/tailscale`, "#!/bin/sh\nexit 1\n", { mode: 0o755 });
export const labEnv = { HOME: home, PATH: `${bin}:${process.env.PATH}` };

const toml = `${root}/c/host.toml`;
const kept = `${root}/c/last-good/host.toml`;
// 새 머신(labB)이 붙을 agent 연결: 다시 띄운 host가 같은 주소로 열어야 labB가 다시 붙는다(`:0`이면 harness의 web port 고정이 이 줄을
// 먼저 바꾼다). labB의 설정·상태(신원·host 고정)는 따로 둔다
const agentAddr = `127.0.0.1:${await freePort()}`;
fs.writeFileSync(toml, fs.readFileSync(toml, "utf8").replace('agent_listen = ""', `agent_listen = "${agentAddr}"`));
const nodeB = { AAA_CONFIG_DIR: `${root}/b/c`, AAA_STATE_DIR: `${root}/b/s` };
fs.mkdirSync(`${root}/b/c`, { recursive: true });
fs.mkdirSync(`${root}/b/work`, { recursive: true });
fs.writeFileSync(`${root}/b/c/agent.toml`, `schema = 1\n[agent]\nwork_root = "${root}/b/work"\n`);
// 배포 후보: 허용 표지가 있을 때만 self-check를 통과한다(63과 같다: rollout은 대상 dry-run에서 멈추고 실험실 실행 파일은 그대로)
const commit = "c".repeat(40);
const allow = `${root}/cand-allow`;
const cand = `${home}/cand`;
fs.mkdirSync(cand, { recursive: true });
fs.writeFileSync(`${cand}/aaa`, `#!/bin/sh\n[ -e '${allow}' ] || exit 1\ncase "$1" in\n--version) echo "aaa 0.1.2+${commit.slice(0, 12)}" ;;\nesac\nexit 0\n`, { mode: 0o755 });
const candSha = crypto.createHash("sha256").update(fs.readFileSync(`${cand}/aaa`)).digest("hex");
fs.writeFileSync(`${cand}/provenance.json`, JSON.stringify({ commit, sha256: candSha, built_at: "2026-10-21T00:00:00Z" }));

/** 설치가 끝난 뒤 통과하는 명령: 상태를 바꾸지 않는 보기뿐이다 */
const OBSERVE = ["status", "machine list", "thread list", "thread show", "routine list", "routine runs", "pane list"];
let installed = false;
const seen = [];
function cli(...args) {
  const head = args.slice(0, 2).join(" ");
  if (installed && !OBSERVE.includes(head) && !OBSERVE.includes(args[0])) throw new Error(`CLI 0회 위반: aaa ${args.join(" ")}`);
  if (installed) seen.push(head);
  return aaa(...args);
}
const connected = (...names) => () => names.every((n) => cli("machine", "list").machines.some((m) => m.name === n && m.connection === "connected"));

async function closeSettings(page) {
  const dialog = page.getByRole("dialog", { name: "설정" });
  await dialog.getByRole("button", { name: "설정 닫기" }).click();
  await dialog.waitFor({ state: "detached" });
}
/** 자동화 폼: 이름·제목·thread 사건 조건(저절로 돌지 않음)을 채우고 동작 칸은 `fill`이 채운다. 사용량 문턱은 비운다(실험실 구독은 주간 5% 남음) */
async function newAutomation(page, name, title, fill) {
  await page.getByTestId("activity-automations").click();
  await page.getByTestId("automation-new-demo").click();
  const sheet = page.getByTestId("automation-editor");
  await sheet.waitFor();
  await sheet.getByTestId("automation-form-name").fill(name);
  await sheet.getByTestId("automation-form-title").fill(title);
  await sheet.getByTestId("automation-form-condition").selectOption("thread");
  await sheet.getByTestId("automation-form-events").fill("thread_failed");
  await fill(sheet);
  await sheet.getByTestId("automation-form-usage-session").fill("");
  await sheet.getByTestId("automation-form-usage-week").fill("");
  await sheet.getByTestId("automation-save").click();
  await sheet.waitFor({ state: "detached" });
  const detail = page.getByTestId("automation-detail");
  await detail.waitFor();
  return detail;
}
async function runNow(page, detail) {
  await detail.getByRole("button", { name: "지금 실행", exact: true }).click();
  const confirm = page.getByRole("alertdialog");
  await confirm.getByRole("button", { name: "지금 실행", exact: true }).click();
  await confirm.waitFor({ state: "detached" });
}
async function openThread(page, id) {
  await page.evaluate((h) => (location.hash = h), `#thread=demo%2F${id}`);
  await page.getByTestId("thread-view").waitFor();
}

export default async function ({ page }) {
  let agentTag = "agent";
  let hostTag = "host";
  await step("설치: 프로젝트 만들기(이 뒤로는 보기 명령만)", async () => {
    cli("project", "create", "demo");
    installed = true;
    let refused = null;
    try {
      cli("thread", "list");
      cli("project", "create", "other");
    } catch (e) {
      refused = e.message;
    }
    if (!refused?.startsWith("CLI 0회 위반: aaa project create")) throw new Error(`설치 뒤 쓰기 명령이 막히지 않았다: ${refused}`);
  });

  await step("새 머신: 웹 등록 창 → 새 머신의 설치 명령 → 확인 코드 승인 → 연결", async () => {
    const addr = cli("status").agent_listen;
    if (addr !== agentAddr) throw new Error(`agent 연결 주소 ${addr}`);
    const dialog = await openSettings(page, "머신");
    const mp = dialog.getByTestId("settings-machines");
    await mp.getByTestId("enroll-open-button").click();
    await mp.getByTestId("enroll-open").waitFor();
    const guide = mp.getByTestId("enroll-guide");
    await guide.locator("summary").click();
    await guide.getByTestId("enroll-guide-address").locator(`option[value="${addr}"]`).waitFor({ state: "attached" });
    await guide.getByTestId("enroll-guide-name").fill("labB");
    const enrollCmd = (await guide.getByTestId("enroll-guide-steps").locator("li").nth(1).locator("code").innerText()).trim();
    if (enrollCmd !== `~/.local/bin/aaa agent enroll ${addr} --name labB`) throw new Error(`안내 명령 ${enrollCmd}`);
    // 새 머신의 설치: 안내 명령의 실행 파일 자리만 실험실 binary로 바꾼다
    const p = spawn(AA, enrollCmd.split(" ").slice(1), { env: { ...env, ...nodeB }, stdio: ["ignore", "pipe", "pipe"] });
    procs.set("enroll", p);
    const exited = new Promise((done) => p.once("exit", done));
    const code = await inTime(
      new Promise((done, fail) => {
        readline.createInterface({ input: p.stdout }).on("line", (l) => {
          const m = l.match(/--code (\d{6})/);
          if (m) done(m[1]);
        });
        p.once("exit", (c) => fail(new Error(`enroll이 코드 전에 끝났다: ${c}`)));
      }),
      20_000,
      "새 머신 확인 코드",
    );
    await mp.getByTestId("enroll-pending").locator('[data-name="labB"]').getByRole("button", { name: "승인…" }).click();
    const ap = page.getByRole("dialog", { name: "labB 승인" });
    await ap.getByTestId("enroll-code").fill(code);
    await ap.getByTestId("enroll-approve").click();
    await ap.waitFor({ state: "detached" });
    if ((await inTime(exited, 20_000, "enroll 끝")) !== 0) throw new Error("enroll이 실패했다");
    // 설치 안내의 마지막 단계(서비스로 agent 띄우기)는 그 머신의 감독자가 한다
    start("agentB", nodeB, "agent", "--host", addr, "--name", "labB");
    await mp.getByTestId("machine-list").locator('[data-name="labB"]').filter({ hasText: "연결됨" }).waitFor();
    await mp.getByRole("button", { name: "창 닫기" }).click();
  });

  await step("승인: 설정 「안전」에서 자동화가 thread를 바로 띄우게(start_threads = auto) 바꾼다", async () => {
    const dialog = page.getByRole("dialog", { name: "설정" });
    await dialog.getByRole("tab", { name: "안전" }).click();
    const row = dialog.getByTestId("settings-safety").getByTestId("safety-rows").locator('[data-key="start_threads"]');
    await row.locator("select").selectOption("auto");
    await row.getByTestId("safety-value").filter({ hasText: "auto" }).waitFor();
    await closeSettings(page);
  });

  let auto;
  await step("자동화 ①(사건마다 새 thread): 폼 → 시작 범위 승인 → 지금 실행 → 목록에 자동화 thread", async () => {
    const detail = await newAutomation(page, "triage", "실패 분류", async (sheet) => {
      await sheet.getByTestId("automation-form-action").selectOption("start");
      await sheet.getByTestId("automation-form-start-title").fill("분류");
      await sheet.getByTestId("automation-form-body").fill("실패를 분류한다");
      await sheet.getByTestId("automation-form-profiles").fill("fake");
      await sheet.getByTestId("automation-form-machines").fill("labA");
    });
    await detail.getByTestId("automation-start-approval").getByRole("button", { name: "승인 검토", exact: true }).click();
    const ask = page.getByRole("alertdialog");
    await ask.getByRole("button", { name: "승인", exact: true }).click();
    await ask.waitFor({ state: "detached" });
    await detail.getByTestId("automation-start-approval").waitFor({ state: "detached" });
    await runNow(page, detail);
    auto = await until("자동화가 띄운 thread의 보고", () => cli("thread", "list").threads.find((t) => t.automation === "triage" && t.report_hash), LOAD_TIMEOUT_MS);
    await detail.getByTestId("automation-close").click();
    await page.getByTestId("activity-work").click();
    await inList(page, `thread-demo-${auto.id}`);
  });

  await step("자동화 ②(열린 thread에 글): 폼에 ①의 thread 번호 → 지금 실행 → 그 thread 화면에 글", async () => {
    const detail = await newAutomation(page, "nudge", "확인 글", async (sheet) => {
      await sheet.getByTestId("automation-form-action").selectOption("message");
      await sheet.getByTestId("automation-form-thread").fill(auto.id);
      await sheet.getByTestId("automation-form-body").fill("w8-nudge-42");
    });
    await runNow(page, detail);
    await until("글 전달 기록", () => cli("routine", "runs", "demo", "nudge").runs.some((r) => r.status === "done"));
    await detail.getByTestId("automation-close").click();
    await page.getByTestId("activity-work").click();
    await openThread(page, auto.id);
    await page.getByTestId("open-terminal").click();
    await until("thread 터미널의 자동화 글", async () => (await screenText(page)).includes("w8-nudge-42"));
  });

  await step("배포: 후보 점검 → 확인 창 → rollout 기록, agent 재시작 → 감독자 재시작 → 끝", async () => {
    fs.writeFileSync(allow, "");
    const dialog = await openSettings(page, "배포");
    const nr = dialog.getByTestId("new-rollout");
    await nr.getByTestId("deploy-path").fill("~/cand/aaa");
    await nr.getByTestId("deploy-check").click();
    await nr.getByTestId("deploy-sha").filter({ hasText: candSha }).waitFor({ timeout: LOAD_TIMEOUT_MS });
    // 이제부터 후보 self-check는 실패한다: 대상에 보낸 뒤 사전 검사에서 멈춘다(실험실 실행 파일을 바꾸지 않는다)
    fs.rmSync(allow);
    await nr.getByTestId("deploy-start").click();
    const ask = page.getByRole("alertdialog", { name: `aaa 0.1.2+${commit.slice(0, 12)}을 배포할까요?` });
    await ask.getByRole("button", { name: "배포 시작" }).click();
    await ask.waitFor({ state: "detached" });
    const id = /rollout ([0-9A-Z]{26})/.exec((await nr.getByTestId("deploy-started").textContent()) ?? "")?.[1];
    if (!id) throw new Error("rollout ID가 보이지 않는다");
    await dialog.getByTestId("rollout-history").locator(`[data-rollout="${id}"][data-state="stopped"]`).waitFor();

    await dialog.getByTestId("deploy-restarts").locator('[data-machine="labA"]').getByRole("button", { name: "agent 재시작…" }).click();
    const again = page.getByRole("alertdialog", { name: "labA의 agent를 다시 띄울까요?" });
    await again.getByRole("button", { name: "다시 띄우기" }).click();
    await again.waitFor({ state: "detached" });
    await until("감독자 재시작 기록", () => fs.existsSync(calls) && /kickstart -k gui\/\d+\/dev\.aaa\.agent/.test(fs.readFileSync(calls, "utf8")));
    // 가짜 launchctl은 기록만 한다: 감독자 대신 같은 실행 파일로 agent를 다시 띄운다
    await stop(agentTag);
    agentTag = "agent-restarted";
    start(agentTag, "agent", "--host", "local", "--name", "labA");
    await dialog.getByTestId("rollout-history").locator('details[data-state="done"]').filter({ hasText: "agent 재시작 ·" }).waitFor();
    await closeSettings(page);
  });

  await step("agent 장애: 죽으면 화면이 끊김을 말하고, 감독자가 다시 띄우면 저절로 돌아온다", async () => {
    await page.getByTestId("pane-t1").click();
    await stop(agentTag);
    await until("agent 끊김 배너", async () => (await page.getByTestId("readonly-banner").innerText()).includes("agent와 연결이 끊겼다"));
    agentTag = "agent-back";
    start(agentTag, "agent", "--host", "local", "--name", "labA");
    await until("배너 사라짐", async () => (await page.getByTestId("readonly-banner").count()) === 0);
  });

  let good;
  await step("host 장애: 죽으면 화면이 끊김을 말하고, 다시 뜬 host에 화면·두 머신이 저절로 다시 붙으며 그 설정을 마지막 정상 설정으로 둔다", async () => {
    await stop(hostTag);
    await until("host 끊김 배너", async () => (await page.getByTestId("readonly-banner").innerText()).includes("host와 연결이 끊겼다"));
    good = fs.readFileSync(toml, "utf8");
    hostTag = "host-good";
    start(hostTag, "host");
    await until("다시 붙음", async () => (await page.getByTestId("readonly-banner").count()) === 0, 30_000);
    await until("마지막 정상 설정 보관", () => fs.existsSync(kept) && fs.readFileSync(kept, "utf8") === good);
    await until("labA·labB 다시 연결", connected("labA", "labB"), 30_000);
  });

  await step("깨진 설정: 같은 주소에 안전 모드 → 「마지막 정상 설정으로 되돌리기」 → 정상 host와 두 머신", async () => {
    await stop(hostTag);
    fs.writeFileSync(toml, `${good}\n[broken\n`);
    hostTag = "host-safe";
    start(hostTag, "host");
    await page.getByTestId("unreachable").getByText("안전 모드").waitFor({ timeout: 30_000 });
    await page.goto(base);
    await page.getByRole("heading", { name: "aaa 안전 모드" }).waitFor();
    await page.getByRole("button", { name: "마지막 정상 설정으로 되돌리기" }).click();
    await page.getByTestId("pane-t1").waitFor({ timeout: 20_000 });
    if (fs.readFileSync(toml, "utf8") !== good) throw new Error("host.toml이 정상 설정으로 돌아오지 않았다");
    await until("labA·labB 다시 연결", connected("labA", "labB"), 30_000);
  });

  await step("정리: 자동화 thread 끝내기, 자동화 둘 지우기, labB 폐기", async () => {
    await openThread(page, auto.id);
    await page.getByTestId("thread-manage").click();
    await page.locator('[role="menuitem"]').filter({ hasText: "끝내기" }).first().click();
    const sheet = page.getByTestId("resolve-dialog");
    await sheet.getByTestId("resolve-facts").waitFor();
    await sheet.getByTestId("resolve-confirm").click();
    await until("thread 끝남", () => cli("thread", "show", "demo", auto.id).status === "resolved");

    await page.getByTestId("activity-automations").click();
    for (const name of ["triage", "nudge"]) {
      await page.getByTestId(`automation-demo-${name}`).click();
      const detail = page.getByTestId("automation-detail");
      await detail.getByRole("button", { name: "지우기", exact: true }).click();
      await page.getByRole("alertdialog").getByRole("button", { name: "지우기", exact: true }).click();
      await page.getByTestId(`automation-demo-${name}`).waitFor({ state: "detached" });
    }
    if (cli("routine", "list").routines.some((r) => ["triage", "nudge"].includes(r.name))) throw new Error("지운 자동화가 남았다");

    const dialog = await openSettings(page, "머신");
    const mp = dialog.getByTestId("settings-machines");
    await mp.getByRole("button", { name: "labB 폐기" }).click();
    await page.getByRole("alertdialog", { name: "labB의 등록을 폐기할까요?" }).getByRole("button", { name: "폐기" }).click();
    await mp.getByTestId("machine-list").locator('[data-name="labB"]').waitFor({ state: "detached" });
    if (cli("machine", "list").machines.some((m) => m.name === "labB")) throw new Error("host에 labB가 남았다");
    await closeSettings(page);
    console.log(`설치 뒤 aaa 명령: 보기 ${seen.length}번(${[...new Set(seen)].join(", ")}), 쓰기 0번`);
  });
}
