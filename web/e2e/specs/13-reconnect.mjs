// 웹 E2E 다시 붙기: agent 끊김·복귀·host 재시작(host를 내리므로 따로 된 실험실).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 13-reconnect(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import { start, stop, until, screenText, shot, base, step, env } from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("agent 끊김 → 읽기 전용 표시", async () => {
    await stop("agent");
    await until("끊김 배너", async () =>
      (await page.getByTestId("readonly-banner").innerText()).includes("agent와 연결이 끊겼다"),
    );
    // 끊긴 머신은 이름 아래 「끊김」, 그 줄들은 흐리고 누를 수 없다(herdr-web-ui MachineSidebar machine-state·machine-offline·inert)
    const labA = page.getByTestId("machine-labA");
    await labA.locator(".machine-state-word", { hasText: "끊김" }).waitFor();
    if (!(await labA.locator(".machine-offline[inert]").count())) throw new Error("끊긴 머신의 줄이 흐리게·누를 수 없게 되지 않았다");
    if ((await labA.locator('.sidebar-status[data-status]:not([data-status="unknown"])').count()) !== 0) throw new Error("끊긴 머신 줄이 상태를 말한다");
    // 입력줄은 그대로 쓸 수 있고 보내기만 막힌다(herdr-web-ui TerminalInput: 끊겨도 쓰던 글은 두고 보내기만 거부)
    if (await page.getByLabel("보낼 글").isDisabled()) throw new Error("끊겼는데 입력줄이 꺼졌다");
    if (!(await page.getByRole("button", { name: "보내기" }).isDisabled())) throw new Error("끊겼는데 보내기가 켜져 있다");
    // 끊긴 동안 터미널에 친 글은 붙잡는다(herdr-web-ui lib/draft.ts): Enter는 붙잡지 않고 그 자리를 빈칸으로 둔다(참고 규칙)
    await page.getByTestId("terminal").click();
    await page.keyboard.type("echo held-$((2+3))");
    await page.keyboard.press("Enter");
    await until("끊김 띠에 붙잡은 입력", async () =>
      (await page.getByTestId("readonly-banner").innerText()).includes("붙잡은 입력: “echo held-$((2+3)) ”"),
    );
    await shot(page, "desktop-disconnected");
  });

  await step("agent 복귀 → 다시 입력", async () => {
    start("agent2", "agent", "--host", "local", "--name", "labA");
    await until("배너 사라짐", async () => (await page.getByTestId("readonly-banner").count()) === 0);
    // 다시 붙어도 붙잡은 입력은 저절로 가지 않는다: 띠에서 사람이 고른다. Enter를 뺐다고 알린다.
    const draftBanner = page.getByTestId("draft-banner");
    await draftBanner.waitFor();
    const draftText = await draftBanner.innerText();
    if (!draftText.includes("echo held-$((2+3))") || !draftText.includes("Enter·줄바꿈은 뺐다"))
      throw new Error(`붙잡은 입력 띠: ${draftText}`);
    if ((await screenText(page)).includes("held-5")) throw new Error("붙잡은 입력이 저절로 보내졌다");
    // 붙잡은 입력이 있는 동안 입력줄 보내기는 거절한다(그 앞부분 없이 줄이 돌면 안 된다)
    await page.getByLabel("보낼 글").fill("echo again-$((9+1))");
    await page.getByRole("button", { name: "터미널에 보내기" }).click();
    await until("먼저 보내거나 버리라는 말", async () =>
      (await page.locator(".terminal-input-note").innerText()).includes("붙잡은 입력을 먼저 보내거나 버린다"),
    );
    if ((await page.getByLabel("보낼 글").inputValue()) !== "echo again-$((9+1))") throw new Error("거절된 입력줄 글이 사라졌다");
    await page.getByTestId("draft-send").click();
    await until("붙잡은 입력 띠 사라짐", async () => (await draftBanner.count()) === 0);
    // 보내기는 Enter를 누르지 않는다: 글만 줄에 들어가 있다. Enter는 사람이 누른다.
    await page.getByTestId("terminal").click();
    await page.keyboard.press("Enter");
    await until("붙잡은 입력 실행", async () => (await screenText(page)).includes("held-5"));
    const left = await page.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith("herdr-web-ui:terminal-draft:")));
    if (left.length !== 0) throw new Error(`보낸 뒤 저장이 남았다: ${left}`);
    await page.getByRole("button", { name: "터미널에 보내기" }).click();
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

  await step("host가 새 웹을 준다(배포 뒤, herdr-web-ui UpdateNotice reload 「updated」, plan §8.162): 다시 붙은 페이지가 「앱 다시 열기」 줄을 보이고, 눌러 다시 열어도 입력줄 초안이 남으며, 같은 웹을 주는 host면 줄이 없다", async () => {
    const line = page.getByTestId("web-update");
    const banner = page.getByTestId("readonly-banner");
    // 다시 붙을 때마다 host가 웹 빌드를 알린다(state보다 먼저): 띠가 사라졌으면 이미 받았다
    if ((await line.count()) !== 0) throw new Error("같은 웹을 주는 host인데 다시 열기 줄이 있다");
    await page.getByLabel("보낼 글").fill("echo keep-draft");
    // 새 웹을 넣은 host로 다시 띄운다(시험은 포함한 웹을 바꾸는 대신 알리는 빌드 이름만 바꾼다)
    await stop("host2");
    await until("host 끊김 표시", async () => (await banner.innerText()).includes("host와 연결이 끊겼다"));
    env.AAA_TEST_WEB_BUILD = "index-newer000";
    try {
      start("host3", "host");
    } finally {
      delete env.AAA_TEST_WEB_BUILD;
    }
    await line.waitFor({ timeout: 30_000 });
    if ((await line.getAttribute("role")) !== "status" || !(await line.innerText()).includes("앱이 업데이트됐다"))
      throw new Error(`다시 열기 줄: ${await line.innerText()}`);
    await shot(page, "desktop-web-update");
    await Promise.all([page.waitForEvent("load"), line.getByRole("button", { name: "앱 다시 열기" }).click()]);
    // 다시 연 페이지는 지금 웹(dist 그대로)이라 이 host가 알리는 가짜 빌드와 여전히 다르다: 줄이 다시 뜬다
    await line.waitFor();
    if ((await page.getByLabel("보낼 글").inputValue()) !== "echo keep-draft")
      throw new Error(`다시 연 뒤 입력줄 초안: ${JSON.stringify(await page.getByLabel("보낼 글").inputValue())}`);
    // 이 페이지와 같은 웹을 주는 host로 돌아오면 줄이 사라진다
    await stop("host3");
    await until("host 끊김 표시", async () => (await banner.innerText()).includes("host와 연결이 끊겼다"));
    start("host4", "host");
    await until("다시 붙음", async () => (await banner.count()) === 0, 30_000);
    await until("다시 열기 줄 사라짐", async () => (await line.count()) === 0);
    await page.getByLabel("보낼 글").fill("");
  });
}
