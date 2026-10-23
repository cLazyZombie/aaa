import fs from "node:fs";
import { root, inTime } from "./harness.mjs";
import { observeHistory, assertHistoryUnchanged } from "./history-observer.mjs";

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
    await historyObserverProbe(page, dir);
  } finally { await page.close(); }
}

// 낡은 renderer 관측값은 합성하고, 실제 Back/Forward·기록 추가·교체는 엔진에서 실행한다.
// 원본 간헐 실패의 브라우저 내부 IPC 순서를 재현했다고 해석하지 않는다.
async function historyObserverProbe(page, dir) {
  const engine = page.context().browser().browserType().name();
  const observer = await observeHistory(page);
  const cases = [];
  const snapshot = async (stage) => {
    const value = await observer.snapshot();
    cases.push({ stage, value });
    return value;
  };
  const changed = (before, after, stage) => {
    let rejected = false;
    try { assertHistoryUnchanged(before, after); } catch { rejected = true; }
    if (!rejected) throw new Error(`${engine}: ${stage}를 기록 불변으로 판정했다`);
  };
  try {
    const before = await snapshot("시작");
    if (engine === "chromium") {
      await page.evaluate((length) => Object.defineProperty(history, "length", { configurable: true, value: length - 1 }), before.length);
      const cached = await page.evaluate(() => history.length);
      await inTime(page.evaluate(() => new Promise((done) => {
        addEventListener("popstate", done, { once: true }); history.back();
      })), 20_000, "기록 관측 대조 Back");
      await inTime(page.evaluate(() => new Promise((done) => {
        addEventListener("popstate", done, { once: true }); history.forward();
      })), 20_000, "기록 관측 대조 Forward");
      await page.evaluate(() => { delete history.length; });
      const actual = await page.evaluate(() => history.length);
      if (cached + 1 !== actual) throw new Error(`낡은 길이 대조가 만들어지지 않았다: ${cached}→${actual}`);
      assertHistoryUnchanged(before, await snapshot("낡은 길이와 실제 왕복"));
      cases.push({ stage: "옛 길이 비교는 실패", cached, actual });
    }
    await page.evaluate(() => history.pushState(null, "", "#추가"));
    const added = await snapshot("실제 추가");
    changed(before, added, "추가");
    changed(added, before, "감소");
    await inTime(page.evaluate(() => new Promise((done) => {
      addEventListener("popstate", done, { once: true }); history.back();
    })), 20_000, "기록 분기 대조 Back");
    await page.evaluate(() => history.pushState(null, "", "#분기"));
    const forked = await snapshot("같은 길이로 앞 기록 교체");
    if (engine === "chromium") {
      if (forked.length !== added.length) throw new Error("같은 길이의 분기가 아니다");
      changed(added, forked, "같은 길이의 다른 항목");
    }
    await page.evaluate(() => history.replaceState(null, "", "#교체"));
    const replaced = await snapshot("주소 교체");
    if (engine === "chromium") changed(forked, replaced, "주소 교체");
    fs.writeFileSync(`${dir}/history-observer-${engine}.json`, JSON.stringify(cases, null, 2));
    console.log(`기록 관측 대조 ${engine}: 실제 증감 거부${engine === "chromium" ? "·낡은 길이 분리·같은 길이 교체 거부" : ""}`);
  } finally { await observer.close(); }
}
