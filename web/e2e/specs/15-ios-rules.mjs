// 웹 E2E iPhone 규칙: 소리는 누른 직후에만 켜지고 홈 화면에서 돌아오면 다시 눌러야 하며 무음 스위치를 따른다, Safari 탭과 홈 화면 앱의 알림.
// 기기는 lib/devices.mjs의 iOS 가짜(WebKit 규칙)다. 헤드리스 Chromium의 진짜 AudioContext·Notification은 이 규칙이 없어 iPhone에서만
// 드러난 결함(가려졌다 돌아온 뒤 소리 없음, 무음 스위치에 묻힌 소리, 탭에 없는 Notification을 부름)을 못 잡는다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 15-ios-rules(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import { aaa, until, step, browser, openSettings, shot, claudeSh, claudeAsk, claudeIdle, root, inList } from "../lib/harness.mjs";
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

  await step("iPhone 대화 보기 머리줄(사용자 실기 10-18 「위쪽 흐려지는건 어쩔수없는건가?」): 홈 화면 앱 맨 위의 머리줄은 배경 있는 sticky라 iOS 26 Liquid Glass 가장자리 흐림 대신 그 색을 쓰고(참고 herdr-web-ui CHANGELOG, 헤드리스는 그 띠를 그리지 못해 조건만 본다), 제목·위치 줄이 길어도 상태 표시와 겹치지 않고 말줄임된다 — 심은 결함(줄 폭을 풀기)은 이 검사가 잡는다", async () => {
    // 사진과 같은 모양: 긴 제목, 긴 위치 줄(머신 › harness · 폴더 · 칸 수), 「입력 대기」
    const cwd = `${root}/hdr/aaa-work/shadow/t-0001-이름-뜻-메모-폴더`;
    fs.mkdirSync(cwd, { recursive: true });
    aaa("tab", "create", "--machine", "labA", "--label", "shadow-이름-뜻-메모-머리줄", "--cwd", cwd, "--", claudeSh, "-c", `stty -echo; ${claudeIdle}; exec cat > /dev/null`);
    const app = await pairDevice(browser, aaa("web", "pair").code, "iphone", { standalone: true });
    const ph = app.page;
    await inList(ph, "pane-shadow-이름-뜻-메모-머리줄");
    await ph.getByTestId("chat").waitFor();
    await ph.locator(".term-head .chip.agent.idle").waitFor();
    // 머리줄의 문제를 모은다(빈 목록이면 통과). 겹침은 줄 오른쪽 끝이 상태 표시 왼쪽을 넘는 것이다
    const problems = () => ph.locator(".term-head").evaluate((head) => {
      const out = [];
      const css = getComputedStyle(head);
      if (css.position !== "sticky" && css.position !== "fixed") out.push(`position ${css.position}`);
      const alpha = /^rgba\(.*,\s*([\d.]+)\)$/.exec(css.backgroundColor);
      if (css.backgroundColor === "transparent" || (alpha && Number(alpha[1]) < 1)) out.push(`배경 ${css.backgroundColor}`);
      if (head.getBoundingClientRect().top !== 0) out.push(`맨 위가 아니다 ${head.getBoundingClientRect().top}`);
      const chip = head.querySelector(".chip.agent").getBoundingClientRect();
      for (const sel of [".term-title-line", ".term-title .muted"]) {
        const el = head.querySelector(sel);
        const r = el.getBoundingClientRect();
        if (r.right > chip.left + 0.5) out.push(`${sel} 오른쪽 ${r.right.toFixed(1)} > 상태 왼쪽 ${chip.left.toFixed(1)}`);
      }
      // 위치 줄이 실제로 넘쳐 말줄임됐다: 짧아서 그냥 통과한 것이 아니다
      const muted = head.querySelector(".term-title .muted");
      if (!(muted.scrollWidth > muted.clientWidth)) out.push(`위치 줄이 넘치지 않았다 ${muted.scrollWidth}/${muted.clientWidth}`);
      return out;
    });
    // 결함 심기: 세로 배치의 줄 폭을 풀면(고치기 전 모양) 위치 줄이 상태 밑으로 칠해진다. 검사가 그것을 잡아야 한다
    const seeded = await ph.addStyleTag({ content: ".term-title > * { max-width: none !important; }" });
    const before = await problems();
    if (!before.some((p) => p.startsWith(".term-title .muted 오른쪽"))) throw new Error(`심은 겹침을 잡지 못했다: ${JSON.stringify(before)}`);
    await shot(ph, "iphone-chat-head-before");
    await seeded.evaluate((el) => el.remove());
    const after = await problems();
    if (after.length) throw new Error(`머리줄: ${after.join("; ")}`);
    await shot(ph, "iphone-chat-head-after");
    if (app.errors.length) throw new Error(`화면 오류: ${app.errors.join("; ")}`);
    await app.page.context().close();
    aaa("pane", "kill", "shadow-이름-뜻-메모-머리줄");
  });

  await step("iPhone 다시 올라온 웹의 머리줄(coordinator 실기 10-18 13:57 「백그라운드에서 올라오며 새 웹이 실린 뒤 머리줄이 통째로 사라지고 내용이 상태 막대 밑까지」): 홈 화면 앱이 상태 막대 밑부터 그리면(위 안전 영역 59px, CDP로 흉내) 머리줄은 참고 .app-header처럼 그만큼 길어져 ≡·제목·상태가 상태 막대 아래에 남고, visibilitychange·pageshow(persisted)·새로 읽기 뒤에도 그렇다. drawer는 머리줄 바로 아래에 서고 그 안 목록 머리는 안전 영역을 더하지 않는다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "resume-head", "--", claudeSh, "-c", `stty -echo; ${claudeIdle}; exec cat > /dev/null`);
    const app = await pairDevice(browser, aaa("web", "pair").code, "iphone", { standalone: true });
    const ph = app.page;
    await inList(ph, "pane-resume-head");
    await ph.getByTestId("chat").waitFor();
    await ph.locator(".term-head .chip.agent.idle").waitFor();
    // 머리줄의 문제를 모은다(빈 목록이면 통과). inset은 상태 막대가 가린 위쪽 높이다
    const problems = (inset) =>
      ph.evaluate((inset) => {
        const out = [];
        const head = document.querySelector(".term-head");
        const box = head.getBoundingClientRect();
        if (box.top !== 0) out.push(`머리줄 맨 위가 아니다 ${box.top}`);
        for (const sel of [".drawer-toggle", ".term-title-line", ".chip.agent"]) {
          const r = head.querySelector(sel).getBoundingClientRect();
          if (r.top < inset - 0.5) out.push(`${sel} 위 ${r.top.toFixed(1)} < 상태 막대 ${inset}`);
          if (r.bottom > box.bottom + 0.5) out.push(`${sel} 아래 ${r.bottom.toFixed(1)} > 머리줄 ${box.bottom.toFixed(1)}`);
        }
        const chat = document.querySelector('[data-testid="chat"]').getBoundingClientRect();
        if (chat.top < box.bottom - 0.5) out.push(`대화 위 ${chat.top.toFixed(1)} < 머리줄 아래 ${box.bottom.toFixed(1)}`);
        return out;
      }, inset);
    const fits = async (inset, what) => {
      const got = await problems(inset);
      if (got.length) throw new Error(`${what}: ${got.join("; ")}`);
    };
    await fits(0, "처음 연 앱");
    // 다시 올라온 웹: 상태 막대 밑부터 그린다(위 안전 영역). override는 이 CDP 연결이 있는 동안 새로 읽기 뒤에도 남는다
    const cdp = await ph.context().newCDPSession(ph);
    await cdp.send("Emulation.setSafeAreaInsetsOverride", { insets: { top: 59, bottom: 34 } });
    await until("안전 영역 흉내", () => ph.evaluate(() => {
      const probe = document.createElement("div");
      probe.style.cssText = "position:fixed;top:0;height:env(safe-area-inset-top,0px)";
      document.body.append(probe);
      const h = probe.getBoundingClientRect().height;
      probe.remove();
      return h === 59;
    }));
    await ph.evaluate(() => {
      document.dispatchEvent(new Event("visibilitychange"));
      window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true }));
    });
    await fits(59, "다시 보인 화면(visibilitychange·pageshow)");
    await shot(ph, "iphone-resume-head");
    await ph.reload();
    await ph.getByTestId("chat").waitFor();
    await ph.locator(".term-head .chip.agent.idle").waitFor();
    await fits(59, "새로 읽은 웹");
    // drawer는 머리줄 바로 아래, 그 안 목록 머리는 상태 막대를 비키지 않는다(머리줄이 비켰다)
    await ph.locator(".term-head .drawer-toggle").click();
    await ph.locator(".list.is-open").waitFor();
    const drawer = await ph.evaluate(() => ({
      top: Math.round(document.querySelector(".list").getBoundingClientRect().top),
      head: Math.round(document.querySelector(".term-head").getBoundingClientRect().bottom),
      listHead: Math.round(document.querySelector(".list-head").getBoundingClientRect().height),
    }));
    same(drawer, { top: drawer.head, head: 52 + 59, listHead: 52 }, "drawer 자리");
    // 머리줄이 없는 모든 thread 화면도 맨 위의 ≡를 상태 막대 아래에 둔다
    await ph.evaluate(() => (location.hash = "#threads"));
    await ph.getByTestId("all-threads").waitFor();
    const threadsToggle = await ph.getByTestId("all-threads").locator(".drawer-toggle").evaluate((e) => Math.round(e.getBoundingClientRect().top));
    if (threadsToggle < 59) throw new Error(`모든 thread 화면 ≡ 위 ${threadsToggle} < 상태 막대 59`);
    await cdp.send("Emulation.setSafeAreaInsetsOverride", { insets: {} });
    if (app.errors.length) throw new Error(`화면 오류: ${app.errors.join("; ")}`);
    await app.page.context().close();
    aaa("pane", "kill", "resume-head");
  });
}
