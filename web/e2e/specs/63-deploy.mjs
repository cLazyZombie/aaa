// 웹 배포·재시작 W6a(plan §8.361): 설정 「배포」에서 빌드 머신에 이미 있는 후보(옆에 provenance.json)를 점검(provenance·SHA 대조와
// dry-run self-check, 대상 순서)한 뒤 확인 창 하나로 rollout을 시작하고, 화면을 다시 열어 같은 rollout ID를 기록에서 본다. 서비스 재시작은
// 확인 창으로 여는 재시작만 하는 rollout이다(W6c, plan §8.402): 독립 감시가 새 agent를 확인해야 끝난다. 보기 전용 기기, 휴대폰 390.
// 실험실 집(HOME)과 가짜 launchctl을 쓴다: 재시작이 이 Mac의 실제 aaa 서비스에 닿지 않는다. 후보는 허용 표지가 있을 때만 self-check를
// 통과해, 점검은 통과하고 rollout은 대상 전송의 dry-run에서 멈춘다(실험실 실행 파일을 바꾸는 교체 단계에 닿지 않는다).
import fs from "node:fs";
import crypto from "node:crypto";
import { root, aaa, AA, step, start, stop, until, browser, pairPage, shot, openSettings, LOAD_TIMEOUT_MS } from "../lib/harness.mjs";

const home = `${root}/svchome`;
const bin = `${root}/svcbin`;
const calls = `${root}/launchctl.log`;
fs.mkdirSync(`${home}/Library/LaunchAgents`, { recursive: true });
fs.mkdirSync(bin, { recursive: true });
for (const role of ["host", "agent"]) fs.writeFileSync(`${home}/Library/LaunchAgents/dev.aaa.${role}.plist`, "<plist/>");
fs.writeFileSync(`${bin}/launchctl`, `#!/bin/sh\ncase "$1" in\nprint) printf 'state = running\\npid = 100\\n' ;;\nkickstart) echo "$@" >> '${calls}' ;;\n*) exit 1 ;;\nesac\n`, { mode: 0o755 });
export const labEnv = { HOME: home, PATH: `${bin}:${process.env.PATH}` };

const commit = "c".repeat(40);
const allow = `${root}/cand-allow`;
const cand = `${home}/cand`;
fs.mkdirSync(cand, { recursive: true });
fs.writeFileSync(`${cand}/aaa`, `#!/bin/sh\n[ -e '${allow}' ] || exit 1\ncase "$1" in\n--version) echo "aaa 0.1.2+${commit.slice(0, 12)}" ;;\nesac\nexit 0\n`, { mode: 0o755 });
const sha = crypto
  .createHash("sha256")
  .update(fs.readFileSync(`${cand}/aaa`))
  .digest("hex");
const provenance = (s) => fs.writeFileSync(`${cand}/provenance.json`, JSON.stringify({ commit, sha256: s, built_at: "2026-10-21T00:00:00Z" }));
const exeSha = () => crypto.createHash("sha256").update(fs.readFileSync(AA)).digest("hex");

export default async function ({ page }) {
  let dialog;
  const before = exeSha();
  await step("점검: provenance와 다른 후보는 거부, 맞는 후보는 SHA·판·대상(host 먼저)을 보이고 시작을 연다", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(allow, "");
    provenance("0".repeat(64));
    dialog = await openSettings(page, "배포");
    const nr = dialog.getByTestId("new-rollout");
    await nr.getByTestId("deploy-path").fill("~/cand/aaa");
    await nr.getByTestId("deploy-check").click();
    await nr.getByRole("alert").getByText("SHA-256").waitFor();
    if (await nr.getByTestId("deploy-start").isEnabled()) throw new Error("점검을 통과하지 않은 후보로 시작이 열렸다");
    provenance(sha);
    await nr.getByTestId("deploy-check").click();
    await nr.getByTestId("deploy-sha").filter({ hasText: sha }).waitFor({ timeout: LOAD_TIMEOUT_MS });
    await nr
      .getByTestId("deploy-plan")
      .getByText(`aaa 0.1.2+${commit.slice(0, 12)}`)
      .waitFor();
    await nr.getByTestId("deploy-plan-targets").getByText("labA (host 먼저) · 준비됨").waitFor();
    if (!(await nr.getByTestId("deploy-start").isEnabled())) throw new Error("점검을 통과했는데 시작이 닫혀 있다");
    // 점검 뒤 경로를 바꾸면 다시 점검해야 한다
    await nr.getByTestId("deploy-path").fill("~/cand/other/aaa");
    if (await nr.getByTestId("deploy-start").isEnabled()) throw new Error("점검하지 않은 경로로 시작이 열렸다");
    await nr.getByTestId("deploy-path").fill("~/cand/aaa");
    await shot(page, "63-plan-desktop");
  });

  let id;
  await step("시작: 확인 창 하나 → rollout ID → 대상 dry-run에서 멈추고 실행 파일·서비스는 그대로", async () => {
    const nr = dialog.getByTestId("new-rollout");
    // 이제부터 후보 self-check는 실패한다: 대상에 보낸 뒤 사전 검사에서 멈춘다
    fs.rmSync(allow);
    await nr.getByTestId("deploy-start").click();
    const ask = page.getByRole("alertdialog", { name: `aaa 0.1.2+${commit.slice(0, 12)}을 배포할까요?` });
    await ask.getByText(sha).waitFor();
    await shot(page, "63-confirm-desktop");
    await ask.getByRole("button", { name: "배포 시작" }).click();
    await ask.waitFor({ state: "detached" });
    const started = await nr.getByTestId("deploy-started").textContent();
    id = /rollout ([0-9A-Z]{26})/.exec(started ?? "")?.[1];
    if (!id) throw new Error(`rollout ID가 보이지 않는다: ${started}`);
    await dialog.getByTestId("rollout-history").locator(`[data-rollout="${id}"][data-state="stopped"]`).waitFor();
    if (exeSha() !== before) throw new Error("실험실 실행 파일이 바뀌었다");
    if (fs.existsSync(calls)) throw new Error(`배포가 서비스를 다시 띄웠다: ${fs.readFileSync(calls, "utf8")}`);
  });

  await step("다시 연 화면: 같은 rollout ID의 상태를 기록에서 본다(요약만)", async () => {
    await page.reload();
    dialog = await openSettings(page, "배포");
    const item = dialog.getByTestId("rollout-history").locator(`[data-rollout="${id}"]`);
    await item.locator("> summary").click();
    await item.getByTestId("rollout-card").filter({ hasText: "배포 중단" }).waitFor();
    await item.getByText("상세 진단은 coordinator의 배포 기록에서").waitFor();
    // 전송 단계에서 멈췄다: 사전 검사·교체 단계는 하나도 끝나지 않았다
    const done = await item.locator('li[data-completed="true"]').allTextContents();
    if (done.some((t) => !t.includes("전송·대조"))) throw new Error(`교체 쪽 단계가 끝났다: ${done}`);
    await item.locator('li[aria-current="step"]').getByText("labA · 전송·대조").waitFor();
    if ((await item.textContent()).includes(cand)) throw new Error("기록 요약에 후보 경로가 보인다");
    await shot(page, "63-history-desktop");
  });

  const restarts = () => dialog.getByTestId("rollout-history").locator("details").filter({ hasText: "agent 재시작 ·" });
  await step("서비스 재시작: 확인 창 → 재시작 rollout(가짜 launchctl) → 감시가 새 agent를 확인하고 끝", async () => {
    await dialog.getByTestId("deploy-restarts").locator('[data-machine="labA"]').getByRole("button", { name: "agent 재시작…" }).click();
    const ask = page.getByRole("alertdialog", { name: "labA의 agent를 다시 띄울까요?" });
    await ask.getByText("감시가 다시 띄운다").waitFor();
    await ask.getByRole("button", { name: "다시 띄우기" }).click();
    await ask.waitFor({ state: "detached" });
    await restarts().first().waitFor();
    const log = await until("감독자 재시작 기록", () => fs.existsSync(calls) && /kickstart -k gui\/\d+\/dev\.aaa\.agent/.test(fs.readFileSync(calls, "utf8")));
    if (!log) throw new Error("감독자 재시작 기록이 없다");
    // 실행 신원이 그대로인 옛 agent는 새 프로세스가 아니다: 아직 끝나지 않는다
    if ((await restarts().first().getAttribute("data-state")) === "done") throw new Error("새 agent 없이 재시작이 끝났다");
    // 가짜 launchctl은 기록만 한다. 감독자 대신 같은 실행 파일로 agent를 다시 띄운다
    await stop("agent");
    start("agent-restarted", "agent", "--host", "local", "--name", "labA");
    await dialog.getByTestId("rollout-history").locator('details[data-state="done"]').filter({ hasText: "agent 재시작 ·" }).waitFor();
    await shot(page, "63-restart-desktop");
  });

  await step("보기 전용 기기: 기록은 보고 시작·재시작 단추는 없다", async () => {
    const { page: rd } = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 });
    const d = await openSettings(rd, "배포");
    await d.getByTestId("rollout-history").locator(`[data-rollout="${id}"]`).waitFor();
    await d.getByTestId("rollout-history").locator('details[data-state="done"]').filter({ hasText: "agent 재시작 ·" }).waitFor();
    if (await d.getByTestId("new-rollout").count()) throw new Error("보기 전용 기기에 새 배포가 보인다");
    if (await d.getByTestId("deploy-restarts").count()) throw new Error("보기 전용 기기에 재시작 단추가 보인다");
    await rd.close();
  });

  await step("휴대폰 390: 배포 쪽이 폭 안에", async () => {
    const { page: ph } = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    const d = await openSettings(ph, "배포");
    await d.getByTestId("rollout-history").locator(`[data-rollout="${id}"]`).waitFor();
    const wide = await ph.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    if (wide) throw new Error("배포 쪽이 가로로 넘친다");
    await shot(ph, "63-deploy-phone");
    await ph.close();
  });
}
