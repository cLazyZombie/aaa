import { reflowContextButton } from "./diff-context-tap.mjs";

/** 실제 DOM의 제거가 관측 준비 전·후에 오는 두 순서를 고정한다. */
export async function checkContextReflow(page) {
  const origin = "https://diff-context.test";
  let requests = 0;
  await page.route(`${origin}/**`, async (route) => {
    if (route.request().method() === "POST") {
      const body = route.request().postDataJSON();
      if (body?.op !== "thread_diff" || !body.query?.patch || body.query?.full_context) throw new Error("맥락 재조회가 아니다");
      requests++;
      await route.fulfill({ json: {} });
    } else await route.fulfill({ contentType: "text/html", body: "<!doctype html><body></body>" });
  });
  try {
    for (const before of [true, false]) {
      await page.goto(`${origin}/fixture`);
      await page.evaluate((removeBefore) => {
        const host = document.createElement("div");
        document.body.append(host);
        const shadow = host.attachShadow({ mode: "open" });
        const button = document.createElement("button");
        button.textContent = "위로 20줄 펼치기";
        shadow.append(button);
        window.addEventListener("focus", async () => {
          await fetch("/diff", { method: "POST", body: JSON.stringify({ op: "thread_diff", query: { patch: true } }) });
          if (!removeBefore) button.remove();
        });
      }, before);
      const old = await page.getByRole("button", { name: "위로 20줄 펼치기" }).elementHandle();
      try {
        // 타이머 없이, 실제 ElementHandle을 얻은 뒤 제거하고 관측을 시작한다.
        if (before) await old.evaluate((button) => button.remove());
        const result = await reflowContextButton(page, old);
        if (result.before.connected !== !before || !result.detached || result.after?.connected !== false) {
          throw new Error(`맥락 제거 순서 before=${before}: ${JSON.stringify(result)}`);
        }
        const expected = before ? "old-resolved,old-detached,focus-refresh" : "old-resolved,focus-refresh,old-detached";
        if (result.events.map((e) => e.event).join(",") !== expected) throw new Error(`맥락 제거 사건: ${JSON.stringify(result)}`);
      } finally { await old.dispose(); }
    }
    if (requests !== 2) throw new Error(`맥락 focus 재조회 ${requests}회`);
  } finally {
    await page.unroute(`${origin}/**`);
  }
}
