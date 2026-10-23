import fs from "node:fs";
import { root, inTime } from "./harness.mjs";

/** location.hash가 일으키는 popstate와 Back의 popstate, 전체 칸 수 불변을 실제 엔진에서 대조한다. */
export async function fragmentHistory(browser) {
  const page = await browser.newPage();
  try {
    await page.route("http://127.0.0.1/fragment-history", (route) => route.fulfill({ contentType: "text/html", body: "<!doctype html><title>기록 탐침</title><p>기록 탐침</p>" }));
    await page.goto("http://127.0.0.1/fragment-history#a");
    const result = await inTime(page.evaluate(async () => {
      const events = [];
      for (const type of ["popstate", "hashchange"]) addEventListener(type, () => events.push({ type, hash: location.hash, length: history.length }));
      const move = (fn) => new Promise((done) => { addEventListener("hashchange", done, { once: true }); fn(); });
      const initial = history.length;
      await move(() => { location.hash = "b"; });
      const first = history.length;
      await move(() => history.back());
      const back = history.length;
      await move(() => { location.hash = "b"; });
      const repeated = history.length;
      const beforePush = events.length;
      history.pushState(null, "", "#c");
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
      return { initial, first, back, repeated, beforePush, events };
    }), 20_000, "fragment 기록 탐침");
    if (result.first !== result.initial + 1 || result.back !== result.first || result.repeated !== result.first
      || result.events.length !== result.beforePush
      || result.events.filter((event) => event.type === "popstate").map((event) => event.hash).join() !== "#b,#a,#b")
      throw new Error(`fragment/Back/pushState 사건 불일치 ${JSON.stringify(result)}`);
    const dir = `${root}/diagnostics`;
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(`${dir}/fragment-${browser.browserType().name()}.json`, JSON.stringify(result, null, 2));
    console.log(`fragment 탐침 ${browser.browserType().name()}: ${JSON.stringify(result)}`);
  } finally { await page.close(); }
}
