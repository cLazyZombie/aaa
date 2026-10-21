// 조작 기기 passkey(관리 GUI W7, 답 1A·2A): 공개 주소로 연 조작 기기는 짝지은 뒤 10분 안에 passkey를 등록하고, 승인 필요 동작
// (여기서는 「기기 페어링」 코드 만들기)마다 그 passkey로 사용자 확인(UV)을 한다. passkey가 없는 기기는 거부되고 등록을 안내받는다.
// 브라우저 passkey는 IP 주소에 묶이지 못해 debug host의 `AAA_TEST_WEB_LOCALHOST`로 `http://localhost:<port>`를 공개 주소처럼
// 쓰고(RP ID `localhost`), Chromium 가상 인증기(CDP WebAuthn)가 사람의 Face ID 대신 확인한다. 실험실 첫 화면(127.0.0.1)은 그대로다.
import { aaa, step, browser, pairPage, base, openSettings, until, shot } from "../lib/harness.mjs";

export const labEnv = { AAA_TEST_WEB_LOCALHOST: "1" };

async function authenticator(page) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("WebAuthn.enable");
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  return { credentials: async () => (await cdp.send("WebAuthn.getCredentials", { authenticatorId })).credentials };
}

const meOf = (page) => page.evaluate(async () => (await (await fetch("/api/v1/me")).json()).ok);

export default async function ({ page }) {
  const at = base.replace("127.0.0.1", "localhost");
  await step("짝지은 뒤 10분 안 passkey 등록: 본문 위 안내 줄 → 가상 인증기 → 설정 → 폰 및 기기에 등록됨·passkey 표지, loopback 화면은 passkey 상태가 없다", async () => {
    const loop = await meOf(page);
    if (loop.passkey !== undefined) throw new Error(`loopback 화면에 passkey 상태 ${JSON.stringify(loop.passkey)}`);
    const phone = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { at })).page;
    const auth = await authenticator(phone);
    const before = await meOf(phone);
    if (before.passkey?.rp_id !== "localhost" || before.passkey.registered || !(before.passkey.register_until_ms > Date.now()))
      throw new Error(`짝지은 직후 상태 ${JSON.stringify(before.passkey)}`);
    const notice = phone.getByTestId("passkey-notice");
    await notice.waitFor();
    await shot(phone, "passkey-notice-phone");
    await notice.getByTestId("passkey-register").click();
    await notice.waitFor({ state: "detached" });
    if ((await auth.credentials()).length !== 1) throw new Error("가상 인증기에 passkey가 없다");
    if (!(await meOf(phone)).passkey.registered) throw new Error("host가 등록을 모른다");
    const dialog = await openSettings(phone, "폰 및 기기");
    await dialog.getByTestId("passkey-status").filter({ hasText: "등록됨(localhost)" }).waitFor();
    await dialog.locator(`.devices-row[data-device="${before.device}"]`).getByTestId("device-passkey").waitFor();

    // UV 표지가 없는 서명·다른 주소·다시 쓴 서명의 거부는 host 유닛(passkey::tests)이 본다: 확인하지 못한 인증기는 브라우저가
    // 제 창에서 기다리므로 여기서는 보지 않는다
    await step("승인 필요 동작은 passkey 확인 뒤에만: 「기기 페어링」 코드는 UV 서명 한 번으로 나온다", async () => {
      const signs = async () => (await auth.credentials())[0].signCount;
      const first = await signs();
      await dialog.getByRole("button", { name: "기기 페어링" }).click();
      await dialog.locator(".devices-code").waitFor();
      await until("passkey 서명 한 번", async () => (await signs()) === first + 1);
      await shot(phone, "passkey-approved-pair-code");
      // 누름이 지난 뒤(WebKit은 host와 오간 뒤 누름이 사라질 수 있다): 「passkey로 승인」 창을 한 번 더 누른 처리기에서 확인한다.
      // 취소하면 아무것도 하지 않는다
      await phone.evaluate(() => Object.defineProperty(navigator, "userActivation", { get: () => ({ isActive: false, hasBeenActive: true }) }));
      const before = (await dialog.locator(".devices-code").textContent()).replace(/\s/g, "");
      await dialog.getByRole("button", { name: "새 코드" }).click();
      const again = phone.getByRole("alertdialog", { name: "passkey로 승인할까요?" });
      await again.getByRole("button", { name: "취소" }).click();
      await dialog.locator(".devices-panel").getByRole("alert").filter({ hasText: "passkey 확인을 취소했다" }).waitFor();
      if ((await signs()) !== first + 1) throw new Error("취소했는데 서명했다");
      await dialog.getByRole("button", { name: "새 코드" }).click();
      await shot(phone, "passkey-gesture-dialog");
      await again.getByRole("button", { name: "passkey로 승인" }).click();
      await until("다시 누른 창의 passkey 서명", async () => (await signs()) === first + 2);
      await until("새 코드", async () => (await dialog.locator(".devices-code").textContent()).replace(/\s/g, "") !== before);
    });

    await step("다른 확인 창 위의 「passkey로 승인」(휴대폰 390): 취소해도 아래 확인 창과 그 안내가 남고, 다시 누르면 승인된다", async () => {
      const signs = async () => (await auth.credentials())[0].signCount;
      const first = await signs();
      // 「이 aaa」 쪽은 프로젝트가 있어야 그린다(60-admin과 같다)
      aaa("project", "create", "demo");
      await dialog.getByRole("button", { name: "설정 닫기" }).click();
      await dialog.waitFor({ state: "detached" });
      const settings = await openSettings(phone, "이 aaa");
      await settings.getByTestId("key-rotate-open").click();
      const ask = phone.getByRole("alertdialog", { name: "공유 링크 키를 바꿀까요?" });
      const again = phone.getByRole("alertdialog", { name: "passkey로 승인할까요?" });
      await ask.getByRole("button", { name: "키 교체" }).click();
      await again.getByRole("button", { name: "취소" }).click();
      await again.waitFor({ state: "detached" });
      await ask.getByRole("alert").filter({ hasText: "passkey 확인을 취소했다" }).waitFor();
      // 「뒤로」 기록이 한 번 더 돌아도(아래 sheet가 닫히던 길) 아래 창은 남는다
      await phone.evaluate(() => new Promise((ok) => setTimeout(ok, 0)));
      await phone.evaluate(() => new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(ok))));
      if (!(await ask.isVisible())) throw new Error("passkey 창을 취소하자 아래 확인 창이 닫혔다");
      await ask.getByRole("button", { name: "키 교체" }).click();
      await again.getByRole("button", { name: "passkey로 승인" }).click();
      await ask.waitFor({ state: "detached" });
      await settings.getByTestId("key-rotate").getByRole("status").waitFor();
      if ((await signs()) !== first + 1) throw new Error("키 교체에 passkey 서명이 한 번이 아니다");
    });
  });

  await step("passkey가 없는 공개 주소 조작 기기는 거부되고 등록을 안내받는다(답 1A), 같은 동작을 실험실 첫 화면(loopback)은 passkey 없이 한다", async () => {
    const bare = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { at })).page;
    const dialog = await openSettings(bare, "폰 및 기기");
    await dialog.getByRole("button", { name: "기기 페어링" }).click();
    await dialog.locator(".devices-panel").getByRole("alert").filter({ hasText: "passkey가 없어 승인할 수 없다" }).waitFor();
    if (await dialog.locator(".devices-code").count()) throw new Error("passkey 없는 기기로 코드가 나왔다");
    await dialog.getByTestId("passkey-status").filter({ hasText: "분 안에 등록한다" }).waitFor();
    await shot(bare, "passkey-missing-refused");
    const lab = await openSettings(page, "폰 및 기기");
    await lab.getByRole("button", { name: "기기 페어링" }).click();
    await lab.locator(".devices-code").waitFor();
    await lab.getByTestId("passkey-status").filter({ hasText: "passkey 없이" }).waitFor();
  });
}
