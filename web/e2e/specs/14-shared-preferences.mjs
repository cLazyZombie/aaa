// 웹 E2E 공유 설정: 기기 사이 테마·색상·사이드바 행, 보기 권한 거부(빈 공유 설정 문서에서 시작).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 14-shared-preferences(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import { aaa, shot, pairPage, step, browser, openSettings, domIs } from "../lib/harness.mjs";

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
    await b.getByTestId("machine-labA").waitFor();
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
    await domIs(b, "닫힌 기기에서 바꾼 테마가 다른 기기에", () => document.documentElement.getAttribute("data-theme") === "dark");
    if ((await held(a)).settings.theme !== "dark") throw new Error("닫힌 기기에서 바꾼 테마가 host 문서에 없다");
    await q.page.context().close();
    for (const p of [a, b, r]) await p.context().close();
  });
}
