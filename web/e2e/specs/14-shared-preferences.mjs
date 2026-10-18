// 웹 E2E 공유 설정: 기기 사이 테마·색상·사이드바 행, 보기 권한 거부(빈 공유 설정 문서에서 시작).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 14-shared-preferences(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import { aaa, shot, pairPage, step, browser, openSettings, domIs, until } from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("공유 설정(herdr-web-ui preferencesSync·shared/preferences.ts, 차이 목록 T2): 한 기기에서 바꾼 테마·색상·사이드바 행은 다른 기기에 곧 오고, 기기 설정(상태 표시)은 남으며, 여는 것만으로는 쓰지 않고, 보기 권한 기기의 쓰기는 거부되며 다음 공유 변경을 모두 받는다, 바꾸고 곧 닫아도 간다(QA Q11)", async () => {
    // 기다림은 시간이 아니라 사건으로 끝낸다: <html> 속성은 MutationObserver, host 쓰기는 그 요청의 답
    const htmlIs = (p, name, value) =>
      p.evaluate(
        ([name, value]) =>
          new Promise((done) => {
            const ok = () => document.documentElement.getAttribute(name) === value;
            if (ok()) return done();
            const mo = new MutationObserver(() => ok() && (mo.disconnect(), done()));
            mo.observe(document.documentElement, { attributes: true, attributeFilter: [name] });
          }),
        [name, value],
      );
    const wrote = (p) =>
      p
        .waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "preferences_set")
        .then(async (r) => ({ sent: r.request().postDataJSON(), ...(await r.json()) }));
    const keys = (o) => JSON.stringify(Object.keys(o).sort());
    const held = (p) =>
      p.evaluate(() =>
        fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "preferences" }) })
          .then((r) => r.json())
          .then((o) => o.ok),
      );
    const settings = (p) => openSettings(p, "모양");
    const close = (p) => p.keyboard.press("Escape");
    const a = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    const b = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    await a.getByTestId("machine-labA").waitFor();
    await b.getByTestId("machine-labA").waitFor({ state: "attached" });
    // 여는 것만으로는 아무것도 쓰지 않는다
    const before = await held(a);
    if (before.revision !== 0) throw new Error(`열기 전에 공유 설정이 있다: ${JSON.stringify(before)}`);

    // 데스크톱에서 밝게 → 휴대폰도 밝게(새로 고치지 않음)
    let d = await settings(a);
    let sent = wrote(a);
    await d.getByRole("group", { name: "테마" }).getByRole("button", { name: "밝게" }).click();
    const first = await sent;
    // 무엇을 보내는지(바꾼 것만·처음 고정)는 web/test/preferences.test.mjs가 본다
    if (first.ok?.revision !== 1 || first.ok.settings.theme !== "light") throw new Error(`첫 쓰기 ${JSON.stringify(first)}`);
    await htmlIs(b, "data-theme", "light");
    // 휴대폰에서 색상 Catppuccin과 상태 표시 「점」(기기 설정) → 데스크톱은 색상만 받는다
    d = await settings(b);
    await d.getByRole("group", { name: "상태 표시" }).getByRole("button", { name: "점" }).click();
    sent = wrote(b);
    await d.locator("#settings-palette").selectOption("catppuccin");
    const second = await sent;
    if (keys(second.ok.settings) !== keys({ palette: 1, theme: 1 })) throw new Error(`host 문서에 기기 설정이 섞였다: ${JSON.stringify(second)}`);
    await htmlIs(a, "data-palette", "catppuccin");
    await close(b);
    await shot(a, "desktop-shared-light-catppuccin");

    // 보기 권한 기기: 열면 host 문서를 받고, 제 변경은 거부되어 이 기기에만 남다가 다음 공유 변경을 모두 받는다
    const r = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 })).page;
    await htmlIs(r, "data-palette", "catppuccin");
    await htmlIs(r, "data-theme", "light");
    d = await settings(r);
    sent = wrote(r);
    await d.getByRole("group", { name: "테마" }).getByRole("button", { name: "어둡게" }).click();
    const refused = await sent;
    if (refused.err?.code !== "forbidden") throw new Error(`보기 권한 쓰기가 거부되지 않았다: ${JSON.stringify(refused)}`);
    await htmlIs(r, "data-theme", "dark");
    if ((await held(a)).revision !== 2) throw new Error("거부된 쓰기가 host 문서를 바꿨다");
    await close(r);
    // 데스크톱에서 사이드바 한 줄 → 휴대폰은 한 줄, 보기 권한 기기는 밝음까지 모두 받는다
    sent = wrote(a);
    d = a.getByRole("dialog", { name: "설정" });
    await d.getByRole("group", { name: "사이드바 행" }).getByRole("button", { name: "한 줄" }).click();
    if ((await sent).ok?.revision !== 3) throw new Error("셋째 쓰기");
    await htmlIs(r, "data-theme", "light");
    await b.locator(".machine-workspaces:not(.is-two-line)").first().waitFor();
    await shot(r, "phone-reader-shared");

    // 바꾸고 곧바로 닫아도(보내기를 미루는 600ms 안) 값이 간다(QA t-0337 Q11): 떠날 때(pagehide) keepalive로 지금 보낸다
    const q = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    await htmlIs(q.page, "data-theme", "light");
    const qd = await settings(q.page);
    await qd.getByRole("group", { name: "테마" }).getByRole("button", { name: "어둡게" }).click();
    await q.page.close({ runBeforeUnload: true });
    // host 문서를 먼저 본다: 실패가 보내는 쪽(host에 안 닿음)인지 받는 쪽(host에는 왔는데 다른 기기가 못 받음)인지 가른다(spec 14 판정)
    const arrives = async (theme, what) => {
      let doc = null;
      try {
        await until(`${what} host 문서에`, async () => (doc = await held(a))?.settings.theme === theme);
      } catch (e) {
        const other = await b.evaluate(() => document.documentElement.getAttribute("data-theme"));
        throw new Error(`${e.message}: 보내는 쪽 — host 문서 revision ${doc?.revision} theme ${doc?.settings.theme}, 다른 기기 ${other}`);
      }
      try {
        await domIs(b, `${what} 다른 기기에`, (t) => document.documentElement.getAttribute("data-theme") === t, theme);
      } catch (e) {
        throw new Error(`${e.message}: 받는 쪽 — host 문서에는 왔다(revision ${doc.revision})`);
      }
    };
    await arrives("dark", "닫힌 기기에서 바꾼 테마가");
    await q.page.context().close();
    // 미룬 보내기(600ms)가 이미 나간 뒤 닫아도 간다: 느린 CI에서는 누름과 닫음 사이가 600ms를 넘을 수 있다. 그때 나간 보통 요청은 문서와
    // 함께 끊기고, 떠날 때 보내기는 「가는 중」이라 다시 보내지 않았다(코드로 찾은 빈틈, plan §8.173). 그래서 보내기는 늘 keepalive다.
    // 로컬 host는 몇 ms에 답해 고치기 전에도 이 경우는 통과한다(결정적 재현 아님): 그 요청이 나간 것을 본 뒤 닫는 꼴만 지킨다
    const w = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    await htmlIs(w.page, "data-theme", "dark");
    const wd = await settings(w.page);
    const issued = w.page.waitForRequest((x) => x.url().endsWith("/api/v1/call") && x.postDataJSON()?.op === "preferences_set");
    await wd.getByRole("group", { name: "테마" }).getByRole("button", { name: "밝게" }).click();
    await issued;
    await w.page.close({ runBeforeUnload: true });
    await arrives("light", "보내는 중에 닫힌 기기에서 바꾼 테마가");
    await w.page.context().close();
    for (const p of [a, b, r]) await p.context().close();
  });
}
