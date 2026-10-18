// 웹 E2E iPhone 규칙: 소리는 누른 직후에만 켜지고 홈 화면에서 돌아오면 다시 눌러야 하며 무음 스위치를 따른다, Safari 탭과 홈 화면 앱의 알림.
// 기기는 lib/devices.mjs의 iOS 가짜(WebKit 규칙)다. 헤드리스 Chromium의 진짜 AudioContext·Notification은 이 규칙이 없어 iPhone에서만
// 드러난 결함(가려졌다 돌아온 뒤 소리 없음, 무음 스위치에 묻힌 소리, 탭에 없는 Notification을 부름)을 못 잡는다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 15-ios-rules(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import { aaa, until, step, browser, openSettings, shot, claudeSh, claudeAsk, inList } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

export default async function () {
  // 확인창(Claude Code 권한 확인창 화면, 화면 감지 plan §8.110)을 띄우는 pane 하나가 이 기기 알림 하나다(11-agent-panes 「확인창 질문」과 같은 가짜)
  let asked = 0;
  const ask = () => {
    const label = `iosq${++asked}`;
    aaa("tab", "create", "--machine", "labA", "--label", label, "--", claudeSh, "-c", `stty -echo; ${claudeAsk(`echo ${label}`)}; exec cat > /dev/null`);
    return label;
  };
  const sounds = (device, from) => device.since(from).filter((e) => /^(tone|muted|silent) /.test(e));
  // 앱 안 알림이 떴다: 이 화면이 그 질문을 처리했다(같은 처리에서 소리를 정한다). 제목은 남기기만 한다
  const titles = [];
  const alerted = async (ph) => {
    await ph.locator(".droplet-title").waitFor();
    titles.push(await ph.locator(".droplet-title").innerText());
  };
  // 닫는다(다음 알림이 앞 알림 뒤에 줄 서지 않게). 닫는 누름도 사람의 누름이다
  const dismiss = async (ph) => {
    await ph.locator(".droplet-close").click();
    await ph.locator(".droplet").waitFor({ state: "detached" });
  };
  // 캡처는 그 요소의 애니메이션(물방울 펼침·스위치 이동)이 끝난 뒤에 찍는다(06-settings와 같다)
  const still = (loc) => loc.evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished.catch(() => null))));
  const same = (got, want, what) => {
    if (JSON.stringify(got) !== JSON.stringify(want)) throw new Error(`${what}: ${JSON.stringify(got)}(기대 ${JSON.stringify(want)})`);
  };

  await step("iPhone 소리(iOS 규칙: 누른 직후에만 AudioContext가 돈다, 홈 화면으로 나가면 interrupted, 무음 스위치는 playback 범주만 지난다): 홈 화면 앱에서 소리를 켜면 무음 스위치에 묻히고, 「무음 모드에서도」를 켜면 들리며, 다른 pane의 확인창마다 울리고, 나갔다 돌아오면 누를 때까지 울리지 않다가 누르면 다시 울린다", async () => {
    const { page: ph, device, errors } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { standalone: true, silentSwitch: true });
    // 소리 켜기: 그 누름 안에서 소리가 풀리고 미리 듣기가 난다. 무음 스위치가 켜진 iPhone에서 기본 범주(auto)는 묻힌다
    const dialog = await openSettings(ph, "알림");
    let m = device.mark();
    await dialog.getByRole("switch", { name: "소리" }).click();
    await device.next("미리 듣기", (e) => e === "muted 660", m);
    if (!device.since(m).includes("audio running")) throw new Error(`누름 안에서 소리가 풀리지 않았다: ${device.since(m)}`);
    same(sounds(device, m), ["muted 880", "muted 660"], "무음 스위치에서 미리 듣기");
    // 「무음 모드에서도」: 소리를 내는 동안만 playback(plan §8.83)
    await device.skipAudio();
    m = device.mark();
    await dialog.getByRole("switch", { name: "무음 모드에서도" }).click();
    await device.next("무음에서도 미리 듣기", (e) => e === "tone 660", m);
    same(device.since(m).filter((e) => /^(session|tone|muted) /.test(e)), ["session playback", "tone 880", "tone 660"], "무음에서도 미리 듣기");
    await still(dialog);
    await shot(ph, "iphone-home-sound-settings");
    await ph.keyboard.press("Escape");
    await inList(ph, "pane-t1");
    // 1) 다른 pane의 확인창: 앱 안 알림과 「입력 필요」 소리(660 → 880)
    await device.skipAudio();
    m = device.mark();
    const q1 = ask();
    await device.next("확인창 소리", (e) => e === "tone 880", m);
    await alerted(ph);
    await still(ph.locator(".droplet"));
    await shot(ph, "iphone-home-app-alert");
    await dismiss(ph);
    // 2) 홈 화면으로 나갔다 돌아옴: 소리가 interrupted라 누르기 전에는 낼 수 없다. 앱 안 알림이 뜬 때까지 소리 기록이 없어야 한다
    await device.skipAudio();
    m = device.mark();
    await device.visible(false);
    await device.next("나가면 interrupted", (e) => e === "audio interrupted", m);
    await device.visible(true);
    const q2 = ask();
    await alerted(ph);
    same(sounds(device, m), [], "누르기 전 소리");
    // 3) 사람이 누르면(그 알림을 닫는 누름) 제품이 그 누름 안에서 다시 풀고, 다음 확인창은 울린다
    m = device.mark();
    await dismiss(ph);
    await device.next("누른 뒤 다시 풀림", (e) => e === "audio running", m);
    const q3 = ask();
    await device.next("누른 뒤 확인창 소리", (e) => e === "tone 880", m);
    await alerted(ph);
    console.log(`     앱 안 알림 제목: ${titles.join(", ")}`);
    if (errors.length) throw new Error(`화면 오류: ${errors.join("; ")}`);
    await ph.context().close();
    for (const q of [q1, q2, q3]) aaa("pane", "kill", q);
  });

  await step("iPhone 알림 범위(iOS 규칙: Safari 탭에는 Notification·PushManager가 없고 Web Push는 홈 화면 앱만, 권한은 누른 직후에만 묻는다): 탭에서는 설정 「테스트 알림」이 홈 화면 앱을 안내하고 테스트 보내기를 막으며 화면 오류가 없다, 홈 화면 앱은 안내 없이 「알림」 항목을 누를 때 권한을 묻는다", async () => {
    const IOS_HINT = "푸시 알림은 HTTPS 또는 localhost가 필요하고, iPhone에서는 홈 화면 앱이어야 합니다.";
    const tab = await pairDevice(browser, aaa("web", "pair").code, "iphone");
    let dialog = await openSettings(tab.page, "알림");
    await until("탭의 홈 화면 앱 안내", async () => (await dialog.getByTestId("push-test-result").innerText()) === IOS_HINT).catch((e) => {
      throw new Error(`${e.message}: 화면 오류 [${tab.errors.join("; ")}]`);
    });
    if (!(await dialog.getByRole("button", { name: "테스트 보내기" }).isDisabled())) throw new Error("탭에서 테스트 보내기가 눌린다");
    await still(dialog);
    await shot(tab.page, "iphone-tab-alerts-hint");
    if (tab.errors.length) throw new Error(`탭 화면 오류: ${tab.errors.join("; ")}`);
    if (tab.device.log.some((e) => e.startsWith("permission"))) throw new Error(`탭에서 권한을 물었다: ${tab.device.log}`);
    await tab.page.context().close();

    const app = await pairDevice(browser, aaa("web", "pair").code, "iphone", { standalone: true });
    dialog = await openSettings(app.page, "알림");
    await dialog.getByTestId("push-test-result").waitFor({ state: "attached" });
    if ((await dialog.getByTestId("push-test-result").innerText()) === IOS_HINT) throw new Error("홈 화면 앱인데 홈 화면 앱 안내");
    await app.page.keyboard.press("Escape");
    // 연 것만으로는 묻지 않는다(iOS는 누름 밖의 요청을 무시하고 그 뒤로도 default로 둔다)
    if (app.device.log.some((e) => e.startsWith("permission"))) throw new Error(`누르기 전에 권한을 물었다: ${app.device.log}`);
    const m = app.device.mark();
    await app.page.getByTestId("more").click();
    await app.page.getByTestId("alerts-item").click();
    await app.device.next("누른 뒤 권한", (e) => e.startsWith("permission"), m);
    same(app.device.since(m).filter((e) => e.startsWith("permission")), ["permission granted"], "권한 물음");
    if (app.errors.length) throw new Error(`홈 화면 앱 화면 오류: ${app.errors.join("; ")}`);
    await app.page.context().close();
  });
}
