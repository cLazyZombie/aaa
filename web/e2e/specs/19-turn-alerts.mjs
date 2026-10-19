// 웹 E2E 다른 agent의 턴 끝남 앱 안 알림(plan §8.185): host `notice` 사건을 「작업 완료」 Droplet으로.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 19-turn-alerts(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import { aaa, until, shot, step, claudeSh, claudeWorking, claudeIdle, root } from "../lib/harness.mjs";

// 화면 감지 시작 유예 0(가짜 harness 화면은 뜨자마자 그대로다, 기본값 길은 15-ios-rules). 긴 턴 기준(herdr-web-ui
// LONG_TURN_MS 60초)을 3초로 줄인다: 기본값 길은 m13 turn_end_notices_follow_reports가 본다(짧은 턴은 long false)
const LONG_MS = 3000;
export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0", AAA_TEST_LONG_TURN_MS: String(LONG_MS), AAA_SESSION_ID: "" };

// 공용 실험실의 넉넉한 post 한도를 이 파일에서는 제품 기본값으로 둔다. host 시작 전에 자기 fixture만 고친다.
const hostConfig = `${root}/c/host.toml`;
const originalConfig = fs.readFileSync(hostConfig, "utf8");
const postFixture = "[post]\nburst = 100\nhourly = 100\ndaily = 100\n";
if (!originalConfig.includes(postFixture)) throw new Error("웹 post 한도 fixture가 달라졌다");
fs.writeFileSync(hostConfig, originalConfig.replace(postFixture, "[post]\nburst = 3\nhourly = 6\ndaily = 30\n"));

export default async function ({ page }) {
  await step("다른 agent의 턴 끝남(herdr-web-ui Droplet `done`·dropletAllows, 차이 목록 N3): 보는 화면 없이 끝난 긴 턴만 「작업 완료」로 내려오고 누르면 그 pane, 짧은 턴·보고 있는 pane은 띄우지 않는다(휴대폰 push가 없음은 m13 turn_end_notices_follow_reports)", async () => {
    // 줄 하나를 받을 때마다 턴을 열고(일하는 화면) 다음 줄에 닫는다(입력 상자): 시험이 턴의 끝을 정한다
    const script = `stty -echo; ${claudeIdle}; while read a; do ${claudeWorking}; read b; ${claudeIdle}; done`;
    for (const name of ["a1", "a2"]) aaa("tab", "create", "--machine", "labA", "--label", name, "--", claudeSh, "-c", script);
    // 내려온 알림을 차례대로 남긴다(제목·종류). 한 번에 하나라 앞 알림이 접히기 전 다음 것은 그 뒤에 온다
    await page.evaluate(() => {
      const drops = [];
      Object.defineProperty(window, "__drops", { value: drops });
      new MutationObserver((ms) => {
        for (const m of ms)
          for (const n of m.addedNodes)
            if (n.classList?.contains("droplet")) drops.push(`${n.dataset.kind} ${n.querySelector(".droplet-title")?.textContent}`);
      }).observe(document, { childList: true, subtree: true });
    });
    const drops = () => page.evaluate(() => window.__drops.join(","));
    const is = (name, s) => page.getByTestId(`agent-${name}`).and(page.locator(`[data-status="${s}"]`)).waitFor();
    // 「쓰는 화면」(보이고 focus, host가 그 pane을 봤다고 치는 조건)을 켜고 끈다. 끄면 보이지만 focus가 없는 화면이다
    const inUse = (on) =>
      page.evaluate((on) => {
        if (on) delete document.hasFocus;
        else document.hasFocus = () => false;
        window.dispatchEvent(new Event(on ? "focus" : "blur"));
      }, on);
    const turn = async (name, { long, end }) => {
      aaa("pane", "run", name, "go");
      await is(name, "working");
      // fixed wait: 긴 턴 기준(LONG_MS)을 넘긴다(그 길이가 이 시험이 보는 규칙이다)
      if (long) await new Promise((r) => setTimeout(r, LONG_MS + 300));
      aaa("pane", "run", name, "end");
      await is(name, end);
    };
    const droplet = page.locator(".droplet");
    const dropped = (title) =>
      until(`앱 안 알림 ${title}`, async () => (await droplet.count()) === 1 && (await droplet.locator(".droplet-title").innerText()) === title);
    await is("a1", "idle");
    await is("a2", "idle");

    // 1) 짧은 턴(긴 턴 기준 밑, a2)은 띄우지 않는다. 울타리: 뒤이은 a1 긴 턴의 알림이 같은 연결로 뒤에 와서 내려온다
    // (a2가 내려왔다면 그 앞에 남는다)
    await turn("a2", { long: false, end: "done" });
    await turn("a1", { long: true, end: "done" });
    await dropped("a1");
    if ((await drops()) !== "done a1") throw new Error(`짧은 턴도 내려왔다: ${await drops()}`);
    if ((await droplet.getAttribute("data-kind")) !== "done" || (await droplet.locator(".droplet-detail").innerText()) !== "작업 완료") {
      throw new Error(`앱 안 알림 ${await droplet.getAttribute("data-kind")} ${await droplet.locator(".droplet-detail").innerText()}`);
    }
    await shot(page, "desktop-turn-droplet");
    // 2) 누르면 그 pane이다
    await droplet.locator(".droplet-card").click();
    await until("누르면 그 pane", async () => (await page.evaluate(() => location.hash)).startsWith("#pane="));
    await until("접힘", async () => (await droplet.count()) === 0);

    // 3) 보고 있는 pane(a1)의 끝남은 띄우지 않는다. 쓰는 화면이 아니면(focus 없음) host는 그 pane을 봤다고 치지 않아 알리지만,
    // 보이는 이 화면이 그 pane을 열고 있다(참고 dropIn: 고른 pane은 건너뛴다). 울타리: 뒤이은 a2의 알림이 내려온다
    await inUse(false);
    await turn("a1", { long: true, end: "done" });
    await turn("a2", { long: true, end: "done" });
    await dropped("a2");
    if ((await drops()) !== "done a1,done a2") throw new Error(`보고 있는 pane의 끝남이 내려왔다: ${await drops()}`);
    await inUse(true);
    for (const name of ["a1", "a2"]) aaa("pane", "kill", name);
  });
  await step("post 음소거: 실제 host가 거부를 누적하면 데스크톱·휴대폰 앱 안 알림과 context 경고, 누르면 프로젝트", async () => {
    aaa("project", "create", "mute-demo");
    const drop = page.locator(".droplet");
    const send = (n) => {
      try {
        aaa("post", "mute-demo", "user", "--request", "--needs-user", "--topic", "other-action", "--title", `자료 ${n}`);
        if (n >= 4) throw new Error("post rate가 거부하지 않았다");
      } catch (e) { if (n < 4 || !e.message.includes("rate:")) throw e; }
    };
    for (const [round, width, height] of [[0, 1280, 800], [1, 390, 844]]) {
      await page.setViewportSize({ width, height });
      if (round) aaa("post", "unmute", "mute-demo", "local:cli");
      // 두 번째 mute는 앞의 수락 원장 때문에 바로 rate로 거부된다. 한도는 바꾸지 않는다.
      for (let n = round ? 7 : 1; n <= (round ? 9 : 6); n++) send(n);
      await until("post 음소거 알림", async () => (await drop.locator(".droplet-title").textContent()) === "mute-demo · post 음소거: local:cli");
      const context = aaa("context", "mute-demo", "--peek");
      if (!context.post_warnings.some((w) => w.includes("aaa post unmute mute-demo local:cli"))) throw new Error(JSON.stringify(context));
      const rect = await drop.locator(".droplet-card").boundingBox();
      if (!rect || rect.x < 0 || rect.x + rect.width > width || rect.y < 0 || rect.y + rect.height > height) throw new Error(`알림이 화면 밖: ${JSON.stringify(rect)}`);
      await shot(page, round ? "phone-post-muted" : "desktop-post-muted");
      await drop.locator(".droplet-card").click();
      await until("프로젝트로 이동", async () => (await page.evaluate(() => location.hash)) === "#project=mute-demo");
      await until("알림 닫힘", async () => (await drop.count()) === 0);
    }
  });
}
