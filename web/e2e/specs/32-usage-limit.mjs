// 사용량 한도에 막힌 thread(t-0403, plan §8.267, t3code Sidebar.logic.ts:939 「limited」): host가 읽어 둔 구독 사용량에서 그 thread
// harness의 창이 다 찼으면 목록 줄이 「한도」(경고 색)와 다시 차오르기까지 남은 때를 보이고, 숫자가 내려가면 걷힌다.
// 다시 차오르는 때가 지나면 coordinator inbox 항목 하나와 앱 안 「한도 풀림」 알림이 간다(저절로 잇는 길은 없다).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 32-usage-limit(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.
import fs from "node:fs";
import { root, aaa, step, until, domIs, inList, shot, screenText, usageRoutes, inHours } from "../lib/harness.mjs";

// 가짜 구독 숫자를 바꾼 뒤 `?refresh=1`로 바로 다시 읽는다(기본 30초에 한 번)
export const labEnv = { AAA_TEST_USAGE_REFRESH_MS: "0" };

const claudeUsage = (fiveHour) => () => [200, {}, {
  five_hour: { utilization: fiveHour.used, resets_at: fiveHour.resets },
  seven_day: { utilization: 50, resets_at: inHours(50) },
}];

let thread = null;

export default async function ({ page }) {
  await step("사용량 한도(t3code limited): Claude thread의 구독 5시간 창이 100%면 목록 줄이 경고 색 「한도 · 남은 때 뒤 리셋」이고 host 보기에 풀리는 때가 있다. 숫자가 내려가면 다음 ticker에 걷힌다", async () => {
    // harness가 claude인 가짜 profile: 한도는 thread profile의 harness 구독을 본다(실제 Claude는 띄우지 않는다)
    // 새 프로젝트가 thread에 허용하는 profile에 그것을 더한다(실험실 기본은 fake만)
    const toml = `${root}/c/host.toml`;
    fs.writeFileSync(toml, fs.readFileSync(toml, "utf8").replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "limitclaude"]'));
    fs.appendFileSync(toml, `\n[profiles.limitclaude]\nagent = "claude"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`);
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/limit-task.md`, "# 한도 보기\n\n기다림\n");
    const id = aaa("thread", "start", "demo", "--title", "한도 보기", "--machine", "labA", "--profile", "limitclaude", "--kind", "dir", "--task-file", `${root}/limit-task.md`).id;
    thread = id;
    await inList(page, `thread-demo-${id}`);
    const state = `[data-testid="thread-state-demo-${id}"]`;
    const reread = () => page.evaluate(async () => (await (await fetch("/api/v1/usage?refresh=1", { cache: "no-store" })).json()).ok);

    const resets = inHours(2);
    usageRoutes["/api/oauth/usage"] = claudeUsage({ used: 100, resets });
    const report = await reread();
    if (!report.providers.some((p) => p.id === "claude" && p.windows.some((w) => w.used_percent === 100))) throw new Error(`구독 숫자: ${JSON.stringify(report)}`);
    aaa("ticker", "tick");
    const shown = aaa("thread", "show", "demo", id);
    if (shown.limited_until_ms !== Date.parse(resets)) throw new Error(`host 보기 limited_until_ms ${shown.limited_until_ms}, 기대 ${Date.parse(resets)}`);
    // 남은 때는 분 올림이고 목록의 시계는 1분마다 간다(useMinute): 2시간에서 1~2분 더 보일 수 있다
    await domIs(page, "한도 색과 남은 때", (q) => {
      const s = document.querySelector(q);
      const mark = s?.querySelector('[data-testid="usage-limited"]')?.textContent ?? "";
      return s?.dataset.tone === "limited" && /^한도 · (1시간 5\d분|2시간 [0-2]분) 뒤 리셋$/.test(mark) && getComputedStyle(s).color !== getComputedStyle(s.parentElement).color;
    }, state).catch(async (e) => {
      const seen = await page.evaluate((q) => { const s = document.querySelector(q); return s && { tone: s.dataset.tone, html: s.outerHTML, color: getComputedStyle(s).color, parent: getComputedStyle(s.parentElement).color }; }, state);
      throw new Error(`${e.message}: ${JSON.stringify(seen)}`);
    });
    await shot(page, "thread-usage-limited");

    usageRoutes["/api/oauth/usage"] = claudeUsage({ used: 20, resets: inHours(2) });
    await reread();
    aaa("ticker", "tick");
    if (aaa("thread", "show", "demo", id).limited_until_ms !== undefined) throw new Error("숫자가 내려간 뒤에도 host 보기에 한도가 남았다");
    await domIs(page, "한도 표지가 걷힌다", (q) => {
      const s = document.querySelector(q);
      return s != null && s.dataset.tone !== "limited" && !s.querySelector('[data-testid="usage-limited"]');
    }, state);
  });

  await step("다시 차오름(사용자 결정: 그때만 알리고 저절로 잇지 않는다): 한도였던 thread의 풀리는 때가 지나면 다음 ticker에 표지가 걷히고, coordinator inbox에 `usage reset` 항목 하나와 이 화면에 「한도 풀림」 알림 하나가 간다(다음 ticker는 다시 알리지 않는다)", async () => {
    const id = thread;
    const state = `[data-testid="thread-state-demo-${id}"]`;
    // 보는 자리는 알리지 않는다: 이 화면은 t1을 연다(새 thread를 띄우면 그 pane을 열어 둔다)
    await page.getByTestId("pane-t1").click();
    await until("t1 화면", async () => (await screenText(page)).includes("t1-42"));
    // 알림 droplet을 센다(31과 같은 길)
    await page.evaluate(() => {
      window.__drops = [];
      new MutationObserver((ms) => {
        for (const m of ms) for (const n of m.addedNodes) if (n.classList?.contains("droplet")) window.__drops.push({ kind: n.dataset.kind, title: n.querySelector(".droplet-title")?.textContent });
      }).observe(document.body, { childList: true, subtree: true });
    });
    // 몇 초 뒤 다시 차오르는 창: 한도를 먼저 보고, 그 때가 지난 뒤의 ticker를 기다린다(시계가 정한 때라 ticker를 거듭 깨운다).
    // 10초는 다시 읽기·ticker·host 보기·목록 색까지가 그 전에 끝날 여유다(ci-local은 test·clippy와 함께 돌아 host 시작도 2초를 넘는다)
    const resets = new Date(Date.now() + 10_000).toISOString();
    usageRoutes["/api/oauth/usage"] = claudeUsage({ used: 100, resets });
    await page.evaluate(async () => (await fetch("/api/v1/usage?refresh=1", { cache: "no-store" })).json());
    aaa("ticker", "tick");
    if (aaa("thread", "show", "demo", id).limited_until_ms !== Date.parse(resets)) throw new Error("다시 차오르기 전 한도가 아니다");
    await domIs(page, "한도", (q) => document.querySelector(q)?.dataset.tone === "limited", state);
    await until("풀리는 때 뒤 ticker가 표지를 걷는다", () => {
      aaa("ticker", "tick");
      return aaa("thread", "show", "demo", id).limited_until_ms === undefined;
    });
    const items = JSON.stringify(aaa("inbox", "list", "demo", "--peek"));
    if ((items.match(/usage reset/g) ?? []).length !== 1) throw new Error(`inbox의 usage reset 항목: ${items}`);
    aaa("ticker", "tick");
    if ((JSON.stringify(aaa("inbox", "list", "demo", "--peek")).match(/usage reset/g) ?? []).length !== 1) throw new Error("다음 ticker가 다시 알렸다");
    await domIs(page, "한도 풀림 알림 하나", () => {
      const resets = window.__drops.filter((d) => d.kind === "reset");
      return resets.length === 1 && resets[0].title === "한도 보기";
    }).catch(async (e) => {
      const seen = await page.evaluate(() => ({ drops: window.__drops, droplet: document.querySelector(".droplet")?.outerHTML ?? null, settings: Object.fromEntries(Object.entries(localStorage).filter(([k]) => /alert|droplet/i.test(k))) }));
      throw new Error(`${e.message}: ${JSON.stringify(seen)}`);
    });
    await domIs(page, "표지가 걷힌다", (q) => document.querySelector(q)?.dataset.tone !== "limited", state);
    await shot(page, "thread-usage-reset");
  });
}
