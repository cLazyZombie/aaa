// 고유 프로세스·전송 경계는 남기고 화면 모델과 겹치는 시나리오는 단위 owner에 맡긴다.
import {
  webkit, root, sleep, aaa, waitOutput, until, screenText, termDiag, shot, pairPage, base, steps, step, browser,
  openSettings, start, stop, sentFrames, sentText, inList, paneHash,
} from "../lib/harness.mjs";

export default async function () {
  await step("출력이 밀릴 때(herdr-web-ui OutputWindow·pty-ack·4008 「Reconnect」, D7, plan §8.62): ACK를 못 하는 화면은 host가 4008로 닫고 다시 보내지 않으며 「다시 연결」로 새로 붙는다. 따라오는 화면은 그대로 받는다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "flood", "--", process.execPath, `${root}/flood.js`);
    await waitOutput("flood", "flood>");
    /** 이 화면의 pane WebSocket 기록(진단 기록과 같은 것): snapshot·닫힘·열림 */
    const log = (p) => p.evaluate(() => window.__aaaTermLog.map((x) => x.e ?? x.kind));
    const count = (l, prefix) => l.filter((e) => e.startsWith(prefix)).length;
    // ACK를 삼키는 화면: 브라우저가 쓴 뒤 알리는 것이 host에 닿지 않는다(느린 기기·멈춘 탭과 같다)
    let dropAcks = true;
    let dropped = 0;
    const swallow = (ws) => {
      const server = ws.connectToServer();
      ws.onMessage((m) => {
        if (dropAcks && typeof m === "string" && m.startsWith('{"t":"ack"')) return void dropped++;
        server.send(m);
      });
      server.onMessage((m) => ws.send(m));
      server.onClose((code, reason) => ws.close({ code, reason }));
      ws.onClose((code, reason) => server.close({ code, reason }));
    };
    // 첫 화면이 flood다(주소): 셀 WebSocket 기록이 그 pane 것뿐이다
    const slow = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { wsRoute: [/\/api\/v1\/panes\//, swallow], hash: paneHash("flood") })).page;
    await until("느린 화면 flood>", async () => (await screenText(slow)).includes("flood>"));
    const fast = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { hash: paneHash("flood") })).page;
    await until("따라오는 화면 flood>", async () => (await screenText(fast)).includes("flood>"));
    // 400KB를 한 번에 쓴다(HIGH 256KiB를 넘는다)
    aaa("pane", "send-text", "flood", "f");
    await until("출력 정지 띠", async () => await slow.getByTestId("output-stalled").isVisible());
    const stalled = await log(slow);
    if (!stalled.includes("ws close 4008")) throw new Error(`4008로 닫히지 않았다: ${JSON.stringify(stalled.filter((e) => e !== "io"))}`);
    if (dropped === 0) throw new Error("삼킨 ACK가 없다: 화면이 ACK를 보내지 않는다");
    // 과부하는 다시 보내지 않는다: 닫히기 전 snapshot을 다시 받지 않았고, 닫힌 뒤 스스로 다시 붙지 않는다
    if (count(stalled, "snapshot") !== 1) throw new Error(`snapshot을 다시 받았다: ${JSON.stringify(stalled.filter((e) => e !== "io"))}`);
    // 띠가 보이면 그 close 처리는 끝났다: 거기서 다시 붙기를 걸지 않았다(Term이 걸 때 data-retry를 단다. sleep으로 타이머를 어림하지 않는다)
    const retry = await slow.getByTestId("terminal").getAttribute("data-retry");
    const later = await log(slow);
    if (retry !== null || count(later, "ws open") !== 1 || !(await slow.getByTestId("output-stalled").isVisible())) throw new Error(`4008 뒤 스스로 다시 붙었다(다시 붙기 ${retry}): ${JSON.stringify(later.filter((e) => e !== "io"))}`);
    // 따라오는 화면은 닫히지 않고 끝까지 받는다
    await until("따라오는 화면 flood-done", async () => (await screenText(fast)).includes("flood-done"));
    const kept = await log(fast);
    if (kept.some((e) => e.startsWith("ws close")) || count(kept, "snapshot") !== 1) throw new Error(`따라오는 화면이 끊겼다: ${JSON.stringify(kept.filter((e) => e !== "io"))}`);
    await shot(slow, "desktop-output-stalled");
    // 「다시 연결」: 새로 붙어 snapshot을 받고 입력도 간다
    dropAcks = false;
    await slow.getByTestId("output-reconnect").click();
    // 옛 화면에도 flood-done이 있다: 새 snapshot을 받은 뒤의 화면을 본다
    await until("다시 붙음", async () => count(await log(slow), "snapshot") === 2 && (await slow.getByTestId("output-stalled").count()) === 0);
    await until("다시 붙은 화면", async () => (await screenText(slow)).includes("flood-done"));
    const back = await log(slow);
    if (count(back, "ws open") !== 2) throw new Error(`다시 연결이 새로 붙지 않았다: ${JSON.stringify(back.filter((e) => e !== "io"))}`);
    await slow.locator(".xterm-helper-textarea").focus();
    await slow.keyboard.type("z");
    await waitOutput("flood", 'GOT:"z"');
    await slow.context().close();
    await fast.context().close();
    aaa("pane", "kill", "flood");
  });
  await step("host를 다시 띄운 뒤 앞 기록(QA t-0337 Q8, plan §8.118): 다시 뜬 host의 mirror는 session host snapshot(최근 2000줄)부터 채워진다. 그 앞은 session host에 물어 받아, 위로 굴려 첫 줄까지 빠짐 없이 본다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "q8", "--", process.execPath, `${root}/history.js`);
    await waitOutput("q8", "hist>");
    aaa("pane", "send-text", "q8", "h");
    await waitOutput("q8", "history-done");
    // host를 다시 띄운다(운영 binary 바꾸기처럼). session host는 그대로 6000줄을 가진다
    await stop("host");
    start("host-q8", "host");
    const q8 = () => aaa("pane", "list").panes.find((p) => p.name === "q8");
    await until("agent가 다시 붙음", () => {
      try {
        return q8()?.connection === "connected";
      } catch {
        return false;
      }
    }, 30_000);
    const qp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await inList(qp, "pane-q8");
    const box = qp.getByTestId("terminal");
    const attr = (k) => box.evaluate((el, k) => el.dataset[k] ?? null, k);
    await until("q8 화면", async () => (await screenText(qp)).includes("history-done"));
    if ((await attr("historyMore")) !== "1") throw new Error(`앞 기록 표시 ${await attr("historyMore")}(session host에 더 있다)`);
    await box.click();
    await until("이 화면 grid", async () => (await qp.locator(".term-scroll[data-adopted-grid]").count()) === 0);
    await box.hover();
    // 받은 기록 맨 위에 닿을 때마다 앞 덩어리를 받는다. 더 없을 때까지(답이 빨라 기다림 표지는 거의 보이지 않는다: 끝 상태를 본다)
    await until("앞 기록 끝까지", async () => {
      if ((await attr("historyMore")) === "0" && (await attr("historyWaiting")) !== "1") return true;
      await qp.mouse.wheel(0, -3000);
      return false;
    }, 30_000);
    const applied = Number(await attr("historyApplied"));
    // mirror는 처음부터 2000줄(웹이 snapshot으로 이미 받음): 앞 4000줄은 session host에서 두 덩어리(2000줄 상한)로 온다
    if (applied < 2) throw new Error(`앞 기록을 ${applied}번 붙였다(session host에 묻지 않았다)`);
    // 버퍼 맨 위부터 한 화면씩: h-00001~h-06000이 빠짐·겹침 없이
    const sweep = await qp.evaluate(async () => {
      const box = document.querySelector('[data-testid="terminal"]');
      const vp = box.querySelector(".xterm-viewport");
      const rows = () => [...box.querySelectorAll(".xterm-rows > div")];
      const drawnAt = (line) =>
        new Promise((done, fail) => {
          const ok = () => box.dataset.viewTop === String(line);
          if (ok()) return done();
          const stuck = setTimeout(() => {
            mo.disconnect();
            fail(new Error(`줄 ${line}이 그려지지 않았다: ${JSON.stringify({ top: box.dataset.viewTop, base: box.dataset.viewBase })}`));
          }, 10_000);
          const mo = new MutationObserver(() => ok() && (clearTimeout(stuck), mo.disconnect(), done()));
          mo.observe(box, { attributes: true, attributeFilter: ["data-view-top"] });
        });
      const goTo = async (line) => {
        if (box.dataset.viewTop === String(line)) return;
        vp.scrollTop = line * rows()[0].getBoundingClientRect().height;
        vp.dispatchEvent(new Event("scroll"));
        await drawnAt(line);
      };
      const nums = new Set();
      const bad = [];
      const half = Math.floor(rows().length / 2);
      for (let line = 0; ; ) {
        await goTo(line);
        const got = rows().map((d) => /^h-(\d{5})/.exec(d.textContent)?.[1]).filter(Boolean).map(Number);
        for (let i = 1; i < got.length; i++) if (got[i] !== got[i - 1] + 1) bad.push([got[i - 1], got[i]]);
        got.forEach((n) => nums.add(n));
        const base = Number(box.dataset.viewBase);
        if (line >= base) break;
        line = Math.min(base, line + half);
      }
      return { count: nums.size, min: Math.min(...nums), max: Math.max(...nums), bad: bad.slice(0, 5) };
    });
    if (sweep.count !== 6000 || sweep.min !== 1 || sweep.max !== 6000 || sweep.bad.length) throw new Error(`버퍼 기록 ${JSON.stringify(sweep)}`);
    await qp.context().close();
    aaa("pane", "kill", "q8");
  });
}
