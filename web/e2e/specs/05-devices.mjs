// 웹 E2E 기기·pairing.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 05-devices(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import {
  aaa, until, screenText, shot, pairPage, base, openPages, step, browser, openSettings,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("기기·pairing(herdr-web-ui DevicesPanel·AccessGate·PhonePanel, 차이 목록 P1·P2·P3, D6): 설정 → 기기에서 코드·QR, QR 주소는 바로 내고 주소에서 지움, 새 코드는 앞 코드를 버림, 틀린 코드는 말, 이름 바꾸기, 폐기하면 열린 화면이 바로 pairing 화면, 보기 기기는 목록만, 로그아웃", async () => {
    // 휴대폰이 열 주소는 host의 tailscale serve 상태(`web_status`)다. 시험 host의 실제 tailscale에 기대지 않게 답을 바꾼다
    const served = "https://mac-a.example.ts.net:8443";
    const fakePublish = async (route) => {
      const req = route.request();
      if (req.method() === "POST" && JSON.parse(req.postData() ?? "{}").op === "web_status") {
        const body = { ok: { kind: "publish", tailscale: "running", dns_name: "mac-a.example.ts.net", url: served, owned_port: null, web_port: 0, message: null } };
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
      }
      return route.continue();
    };
    const openDevices = async (p) => {
      const dialog = await openSettings(p, "폰 및 기기");
      await dialog.locator(".devices-panel").scrollIntoViewIfNeeded();
      return dialog;
    };
    const desk = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { route: ["**/api/v1/call", fakePublish] })).page;
    const dlg = await openDevices(desk);
    // 폰: 공개 주소와 그 QR
    await dlg.getByRole("img", { name: `QR 코드: ${served}` }).waitFor();
    // 기기: 이 기기 표시와 역할
    const mine = await desk.evaluate(async () => (await (await fetch("/api/v1/me")).json()).ok.device);
    const mineRow = dlg.locator(`.devices-row[data-device="${mine}"]`);
    await mineRow.locator(".devices-current", { hasText: "이 기기" }).waitFor();
    await dlg.getByRole("button", { name: "기기 페어링" }).click();
    const codeText = async () => (await dlg.locator(".devices-code").textContent()).replace(/\s/g, "");
    await dlg.locator(".devices-code").waitFor();
    const first = await codeText();
    if (!/^\d{6}$/.test(first)) throw new Error(`코드 ${first}`);
    const qr = dlg.getByRole("img", { name: `코드 ${first}로 폰을 페어링하는 QR 코드` });
    await qr.waitFor();
    const link = await dlg.locator(".devices-link a").getAttribute("href");
    if (link !== `${served}/?pair=${first}`) throw new Error(`QR 주소 ${link}`);
    if (!/[0-9]:[0-5][0-9] 뒤 만료됩니다/.test(await dlg.locator(".devices-pairing").textContent())) throw new Error("남은 시간이 없다");
    await shot(desk, "settings-devices-desktop");
    // 새 코드는 앞 코드를 버린다(참고 startPairing)
    await dlg.getByRole("button", { name: "새 코드" }).click();
    await until("새 코드", async () => (await codeText()) !== first);
    const second = await codeText();
    const pr = await fetch(`${base}api/v1/pair`, { method: "POST", headers: { "content-type": "application/json", origin: base.slice(0, -1) }, body: JSON.stringify({ code: first, label: "x" }) });
    if (pr.status !== 401) throw new Error(`버린 코드가 ${pr.status}`);
    // 휴대폰이 QR 주소를 연다: 코드를 바로 내고, 주소에서 지운다(기록·referrer에 남지 않게)
    const phoneCtx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0.1 Mobile/15E148 Safari/604.1" });
    const phone = await phoneCtx.newPage();
    openPages.add(phone);
    phone.on("close", () => openPages.delete(phone));
    await phone.goto(`${base}?pair=${second}`);
    await phone.getByTestId("machine-labA").waitFor();
    if (phone.url().includes("pair=")) throw new Error(`주소에 코드가 남았다: ${phone.url()}`);
    // 데스크톱 화면은 그 코드로 들어온 기기를 알아채고 코드를 닫는다
    await dlg.getByRole("status").filter({ hasText: "페어링됨: iPhone · Safari." }).waitFor();
    await dlg.locator(".devices-pairing").waitFor({ state: "detached" });
    const phoneId = await phone.evaluate(async () => (await (await fetch("/api/v1/me")).json()).ok.device);
    const phoneRow = dlg.locator(`.devices-row[data-device="${phoneId}"]`);
    await phoneRow.waitFor();
    // 이름 바꾸기
    await phoneRow.getByRole("button", { name: "iPhone · Safari 이름 바꾸기" }).click();
    await phoneRow.getByLabel("기기 이름").fill("  내   폰  ");
    await phoneRow.getByRole("button", { name: "이름 저장" }).click();
    await phoneRow.locator(".settings-label", { hasText: "내 폰" }).waitFor();
    const listed = aaa("web", "devices").devices.find((d) => d.id === phoneId);
    if (listed?.label !== "내 폰" || listed.role !== "controller") throw new Error(`CLI 목록 ${JSON.stringify(listed)}`);
    // 폰은 터미널을 연다(그 WebSocket도 바로 닫혀야 한다)
    await phone.getByTestId("pane-t1").click();
    await until("폰 t1 화면", async () => (await screenText(phone)).includes("$"));
    // 폐기: 두 번 눌러야 한다
    const revoke = phoneRow.getByRole("button", { name: "폐기" });
    await revoke.click();
    await phoneRow.getByRole("button", { name: "폐기할까요?" }).click();
    await phoneRow.waitFor({ state: "detached" });
    // 폰의 열린 화면(events·터미널 WebSocket)은 4401로 닫혀 바로 pairing 화면이 된다
    await phone.getByLabel("페어링 코드").waitFor();
    await shot(phone, "phone-pair-gate-after-revoke");
    const after = await phone.evaluate(async () => (await fetch("/api/v1/me")).status);
    if (after !== 401) throw new Error(`폐기한 기기가 ${after}`);
    // 틀린 코드는 말한다
    await phone.getByLabel("페어링 코드").fill("000000");
    await phone.getByRole("button", { name: "이 기기 페어링" }).click();
    await phone.getByRole("alert").filter({ hasText: "코드가 틀렸거나 이미 쓰였거나 만료되었습니다" }).waitFor();
    // 휴대폰·iPad의 설정 → 기기(코드가 나가 있는 모습)
    for (const [name, viewport, touch] of [
      ["settings-devices-phone", { width: 390, height: 844 }, true],
      ["settings-devices-ipad", { width: 820, height: 1180 }, true],
    ]) {
      const p = (await pairPage(browser, aaa("web", "pair").code, viewport, { touch, route: ["**/api/v1/call", fakePublish] })).page;
      const d = await openDevices(p);
      await d.getByRole("button", { name: "기기 페어링" }).click();
      await d.locator(".devices-pairing").scrollIntoViewIfNeeded();
      await d.getByRole("img", { name: /로 폰을 페어링하는 QR 코드$/ }).waitFor();
      // 화면 밖으로 넘치지 않는다
      const over = await p.evaluate(() => [...document.querySelectorAll(".devices-panel *")].filter((e) => e.getBoundingClientRect().right > window.innerWidth + 0.5).map((e) => e.className).slice(0, 3));
      if (over.length) throw new Error(`${name} 넘침: ${over}`);
      await shot(p, name);
      await p.context().close();
    }
    // 보기 기기: 목록은 보고, 페어링·이름·폐기는 못 한다(host도 막는다)
    const rd = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 })).page;
    const rdlg = await openDevices(rd);
    await rdlg.getByText("보기 권한 기기에서는 기기를 페어링하거나 바꿀 수 없습니다.").waitFor();
    if (await rdlg.getByRole("button", { name: "기기 페어링" }).count()) throw new Error("보기 기기에 페어링 단추");
    const rdId = await rd.evaluate(async () => (await (await fetch("/api/v1/me")).json()).ok.device);
    const rdRow = rdlg.locator(`.devices-row[data-device="${rdId}"]`);
    await rdRow.getByText(/^보기만 · /).waitFor();
    if (!(await rdRow.getByRole("button", { name: "폐기" }).isDisabled())) throw new Error("보기 기기의 폐기 단추가 켜져 있다");
    const minted = await rd.evaluate(async () =>
      (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "web_pair", role: "controller" }) })).json(),
    );
    if (minted.err?.code !== "forbidden") throw new Error(`보기 기기가 코드를 만들었다: ${JSON.stringify(minted)}`);
    await rd.context().close();
    // 웹 controller도 보기 코드는 못 만든다(D6: 웹은 controller만)
    const readerCode = await desk.evaluate(async () =>
      (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "web_pair", role: "reader" }) })).json(),
    );
    if (readerCode.err?.code !== "forbidden") throw new Error(`웹이 보기 코드를 만들었다: ${JSON.stringify(readerCode)}`);
    // 로그아웃: 확인 창(취소가 focus) → 로그아웃 → pairing 화면. 기기 기록은 남는다(참고와 같다)
    await dlg.getByRole("button", { name: "로그아웃" }).click();
    const confirm = desk.getByRole("alertdialog", { name: "이 브라우저에서 로그아웃할까요?" });
    await confirm.waitFor();
    // focus는 창이 그려진 다음 frame에 취소로 간다(useFocusTrap): frame마다 본다
    await desk.waitForFunction(() => document.activeElement?.textContent === "취소" && document.activeElement.closest('[role="alertdialog"]') !== null, null, { polling: "raf", timeout: 5000 });
    await shot(desk, "settings-sign-out-confirm");
    await confirm.getByRole("button", { name: "로그아웃" }).click();
    await desk.getByLabel("페어링 코드").waitFor();
    if (!aaa("web", "devices").devices.some((d) => d.id === mine)) throw new Error("로그아웃한 기기 기록이 없어졌다");
    aaa("web", "revoke", mine);
    await desk.context().close();
    await phoneCtx.close();
  });
}
