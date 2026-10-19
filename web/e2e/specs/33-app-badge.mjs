// 설치 앱 OS 표시 대신 실제 Badging API 호출을 기록한다. host·agent·DB·WS·보고서 열기는 실제 경로다.
import fs from "node:fs";
import {
  root, aaa, step, pairPage, browser, webkit, domIs, inList, until,
  pushLab, pushDevice, pushes, nextPush, setInUse, coordinatorPost,
} from "../lib/harness.mjs";

function badgeRecorder() {
  window.__appBadges = [];
  Object.defineProperty(navigator, "setAppBadge", { configurable: true, value: async (n) => {
    window.__appBadges.push(n);
    document.documentElement.dataset.appBadge = String(n);
  } });
  Object.defineProperty(navigator, "clearAppBadge", { configurable: true, value: async () => {
    window.__appBadges.push(0);
    document.documentElement.dataset.appBadge = "0";
  } });
}
const badgeIs = (page, n) => domIs(page, `설치 앱 배지 ${n}`, (n) => document.documentElement.dataset.appBadge === String(n), n);

export default async function ({ page }) {
  await pushLab(page);
  await step("설치 앱 배지: 보고서 도착 1·focus 유지·다른 기기 열람 0·확인 필요 증가와 처리 감소, 허용된 push에도 같은 수", async () => {
    const dev = pushDevice("push/badge");
    await page.evaluate(async (body) => {
      const res = await fetch("/api/v1/push/subscribe", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
      if (!res.ok) throw new Error(`배지 push 구독 ${res.status}`);
    }, dev.json);
    const first = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { init: badgeRecorder });
    const safari = await webkit.launch();
    const second = await pairPage(safari, aaa("web", "pair").code, { width: 390, height: 844 }, { init: badgeRecorder });
    const a = first.page, b = second.page;
    try {
      await Promise.all([badgeIs(a, 0), badgeIs(b, 0)]);
      const input = `${root}/badge-report`;
      fs.mkdirSync(input);
      fs.writeFileSync(`${input}/index.html`, "<!doctype html><h1>배지 보고서</h1>");
      aaa("library", "publish", input, "--project", "demo", "--name", "badge", "--to-user");
      await Promise.all([badgeIs(a, 1), badgeIs(b, 1)]);
      await a.evaluate(() => window.dispatchEvent(new Event("focus")));
      if (await a.evaluate(() => window.__appBadges.at(-1)) !== 1) throw new Error("focus가 안 본 보고서를 지웠다");
      // 다른 브라우저·기기에서 실제로 열어도 양쪽의 공유 수가 줄어든다.
      await inList(b, "activity-reports");
      await b.getByTestId("report-demo-badge").getByRole("button", { name: "열기", exact: true }).click();
      await b.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "배지 보고서" }).waitFor();
      await Promise.all([badgeIs(a, 0), badgeIs(b, 0)]);
      fs.writeFileSync(`${root}/badge-task.md`, "사람을 기다림\n");
      const t = aaa("thread", "start", "demo", "--title", "배지 결정", "--profile", "fake", "--machine", "labA", "--kind", "dir", "--task-file", `${root}/badge-task.md`);
      await until("확인 필요 thread", () => aaa("thread", "show", "demo", t.id).group === "waiting_on_you");
      await Promise.all([badgeIs(a, 1), badgeIs(b, 1)]);
      aaa("thread", "resolve", "demo", t.id);
      await Promise.all([badgeIs(a, 0), badgeIs(b, 0)]);
      // 읽지 않은 새 보고가 있는 동안 기존 coordinator 완료 post로만 외부 push한다.
      aaa("library", "publish", input, "--project", "demo", "--name", "second", "--to-user");
      await Promise.all([badgeIs(a, 1), badgeIs(b, 1)]);
      await first.page.context().close();
      await second.page.context().close();
      await setInUse(page, false);
      const from = pushes.length;
      await coordinatorPost();
      const pushed = await nextPush(dev, from, "배지 수가 든 coordinator push");
      if (pushed.msg.app_badge !== 1) throw new Error(`push 배지 ${JSON.stringify(pushed.msg)}`);
      const status = await page.evaluate(async () => (await fetch("/api/v1/app-badge")).json());
      if (status.ok !== 1) throw new Error(`API 배지 ${JSON.stringify(status)}`);
    } finally {
      await first.page.context().close();
      await second.page.context().close();
      await safari.close();
    }
  });
}
