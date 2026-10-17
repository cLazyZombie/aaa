// 웹 E2E 입력줄: send-text·여러 줄·보내는 동안 친 글.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 08-input-line(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import { root, sleep, aaa, waitOutput, until, screenText, pairPage, step, browser, sentFrames, sentCount } from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("입력 막대·CLI send-text 뒤 send-keys enter: 글 뒤 Enter를 따로 보낸다(Codex식 붙여넣기 판정)", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "burst", "--", process.execPath, `${root}/burst.js`);
    await page.getByTestId("pane-burst").click();
    await until("burst 화면", async () => (await screenText(page)).includes("burst>"));
    await page.getByLabel("보낼 글").fill("hello web");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("웹 제출", async () => (await screenText(page)).includes("SUBMIT:hello web"));
    // CLI는 글과 Enter를 따로 쓴다(herdr send-text·send-keys). `pane run`은 글+Enter를 한 번에 쓰니(herdr) 여기서는 쓰지 않는다
    aaa("pane", "send-text", "burst", "hello cli");
    aaa("pane", "send-keys", "burst", "enter");
    await waitOutput("burst", "SUBMIT:hello cli");
    if ((await screenText(page)).includes("[newline]")) throw new Error("Enter가 글과 한 덩어리로 갔다");
    // 가짜 입력창이 한 덩어리를 실제로 가려내는지(이 단언이 없으면 위 단언은 아무것도 보지 않는다)
    aaa("pane", "send-text", "burst", "one\r");
    await until("한 덩어리 판정", async () => (await screenText(page)).includes("[newline]"));
    aaa("pane", "kill", "burst");
  });

  await step("입력줄: 여러 줄은 bracketed paste, Enter 보내기·Shift+Enter 줄바꿈, 조합 중 Enter 무시, 초안 보존, 휴대폰도 Enter는 보내기", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "pasteapp", "--", process.execPath, `${root}/paste.js`);
    const got = () => aaa("pane", "read", "pasteapp", "--source", "visible").text;
    const gots = () => got().split("GOT:").length - 1;
    await page.getByTestId("pane-pasteapp").click();
    await until("paste 화면", async () => (await screenText(page)).includes("paste>"));
    const box = page.getByLabel("보낼 글");
    // 여러 줄: 프로그램이 bracketed paste를 켰으니 감싸서 한 덩어리로, 줄바꿈은 CR로. Enter는 따로.
    await box.fill("줄1\n줄2");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("여러 줄 붙여넣기", () => got().includes('GOT:"\\u001b[200~줄1\\r줄2\\u001b[201~"'));
    await until("뒤 Enter", () => got().split('GOT:"\\r"').length - 1 === 1);
    // 데스크톱 Enter는 보낸다
    await box.fill("한 줄");
    await box.press("Enter");
    await until("Enter로 보냄", () => got().includes('GOT:"한 줄"') && got().split('GOT:"\\r"').length - 1 === 2);
    // Shift+Enter는 줄바꿈이고 보내지 않는다
    let n = gots();
    await box.press("a");
    await box.press("Shift+Enter");
    await box.press("b");
    if ((await box.inputValue()) !== "a\nb") throw new Error(JSON.stringify(await box.inputValue()));
    // 조합 중 Enter는 보내지 않는다. 같은 사건이 조합 중이 아니면 보낸다(이 단언이 실제로 가려내는지).
    // 보내기(submit)는 그 keydown 처리 안에서 일어난다: 그 사건 뒤 이 화면이 보낸 것이 없으면 보내지 않았다
    const k = await sentCount(page);
    await box.evaluate((el) => el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", isComposing: true, bubbles: true })));
    const composed = (await sentFrames(page, k)).filter((f) => f.t === "submit" || f.t === "input");
    if (composed.length || gots() !== n || (await box.inputValue()) !== "a\nb") throw new Error(`조합 중 Enter로 보냈다: ${JSON.stringify(composed)} ${got()}`);
    // 초안: 다시 열어도 남는다
    await page.reload();
    await page.getByTestId("machine-labA").waitFor();
    await until("초안 보존", async () => (await page.getByLabel("보낼 글").inputValue()) === "a\nb");
    // 다시 연 화면은 host의 readonly 알림이 올 때까지 입력줄이 꺼져 있고, 꺼진 동안의 Enter는 초안을 두고 보내지 않는다
    await until("입력줄 켜짐", () => page.getByLabel("보낼 글").isEnabled());
    await page.getByLabel("보낼 글").evaluate((el) => el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    // 여러 줄 붙여넣기 다음 Enter는 300ms 뒤 따로 온다(둘 다 와야 끝난다)
    await until("조합이 아닌 Enter는 보냄", () => gots() >= n + 2 && got().trimEnd().endsWith('GOT:"\\r"\npaste>')).catch(async (e) => {
      // 보내기가 어디서 멈췄는지: 입력줄에 남은 글·안내 말(끊김·붙잡은 입력 먼저 등)·붙잡은 입력 띠
      const line = page.getByLabel("보낼 글");
      const note = await page.locator(".terminal-input-note").allInnerTexts();
      const held = await page.locator(".terminal-banners").allInnerTexts();
      throw new Error(`${e.message}: 입력줄 ${JSON.stringify(await line.inputValue())} 켜짐 ${await line.isEnabled()}, 말 ${JSON.stringify(note)}, 띠 ${JSON.stringify(held)}`);
    });
    if ((await page.getByLabel("보낼 글").inputValue()) !== "") throw new Error("보낸 뒤 글이 남았다");
    // 조합 중에 누른 보내기(herdr-web-ui scripts/terminal-input-regression.ts와 같은 네 경우)
    {
      const line = page.getByLabel("보낼 글");
      const send = page.getByRole("button", { name: "보내기" });
      const banner = () => page.locator(".terminal-input-note").evaluateAll((els) => els[0]?.textContent ?? null);
      const since = () => got().split('GOT:"MARK"').at(-1);
      const mark = async () => {
        aaa("pane", "send-text", "pasteapp", "MARK");
        await until("표지", () => got().includes('GOT:"MARK"'));
      };
      // 1) 실제 조합(Chrome IME 길): 누르는 사이 확정되어 바로 보낸다. 입력칸은 focus를 지킨다(키보드가 그대로)
      await mark();
      await line.fill("real 한");
      await line.focus();
      const cdp = await page.context().newCDPSession(page);
      await cdp.send("Input.imeSetComposition", { text: "글", selectionStart: 1, selectionEnd: 1 });
      await until("조합 중 값", async () => (await line.inputValue()) === "real 한글");
      await send.click();
      await until("조합 중 보내기", () => since().includes('GOT:"real 한글"') && since().includes('GOT:"\\r"'));
      await cdp.detach();
      if (!(await line.evaluate((el) => el === document.activeElement))) throw new Error("보낸 뒤 입력칸이 focus를 잃었다");
      // 2) 기다리는 사이 확정: 확정된 글을 한 번 보낸다(compositionend와 값 바꾸기가 같은 task여도)
      await mark();
      await line.fill("commit 한");
      await line.dispatchEvent("compositionstart", { data: "" });
      await send.click();
      await until("기다림 표시", async () => (await banner()) === "글자를 마치는 중…");
      if (since().includes("commit")) throw new Error("확정 전에 보냈다");
      await line.evaluate((el) => {
        const setValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set;
        el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "글" }));
        setValue.call(el, "commit 한글");
        el.dispatchEvent(new InputEvent("input", { bubbles: true, data: "글", inputType: "insertFromComposition" }));
      });
      await until("확정 뒤 보냄", () => since().includes('GOT:"commit 한글"') && since().includes('GOT:"\\r"'));
      // 3) 확정 전에 지워짐: 글 보내기였으니 Enter도 보내지 않는다
      await mark();
      await line.fill("한");
      await line.dispatchEvent("compositionstart", { data: "" });
      await send.click();
      await until("기다림 표시", async () => (await banner()) === "글자를 마치는 중…");
      await line.evaluate((el) => {
        const setValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set;
        el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "" }));
        setValue.call(el, "");
        el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "deleteCompositionText" }));
      });
      await sleep(400);
      if (since().includes("GOT:")) throw new Error(`빈 확정에 보냈다: ${since()}`);
      // 4) 조합이 이어져도 누른 때부터의 기한을 넘기지 않는다
      await line.fill("chain 한");
      await line.dispatchEvent("compositionstart", { data: "" });
      await send.click();
      await until("기다림 표시", async () => (await banner()) === "글자를 마치는 중…");
      const t0 = Date.now();
      while (Date.now() - t0 < 450) {
        await line.evaluate((el) => {
          el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "한" }));
          el.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
        });
        await sleep(40);
      }
      if ((await banner()) !== "글자를 마친 뒤 보낸다") throw new Error(`기한 뒤 표시: ${await banner()}`);
      await line.dispatchEvent("compositionend", { data: "한" });
      await sleep(100);
      if (since().includes("GOT:")) throw new Error(`기한 넘은 조합을 보냈다: ${since()}`);
      await line.fill("");
    }
    // 휴대폰(터치)도 Enter는 보내기다(herdr-web-ui TerminalInput: enterKeyHint="send", Shift+Enter만 줄바꿈)
    const mob = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { line: true });
    await mob.page.getByTestId("pane-pasteapp").click();
    const mbox = mob.page.getByLabel("보낼 글");
    // 연결(snapshot)되기 전에는 읽기 전용이다: 화면이 온 뒤에 본다
    await until("휴대폰 paste 화면", async () => (await screenText(mob.page)).includes("paste>"));
    // 데스크톱 화면이 쓰는 중이어도 가져오기 없이 입력된다
    if (await mbox.isDisabled()) throw new Error("휴대폰 입력 막대가 꺼져 있다");
    if ((await mbox.getAttribute("enterkeyhint")) !== "send") throw new Error(`휴대폰 enterkeyhint ${await mbox.getAttribute("enterkeyhint")}`);
    n = gots();
    await mbox.fill("휴대폰");
    await mbox.press("Enter");
    await until("휴대폰 Enter로 보냄", () => got().includes('GOT:"휴대폰"') && gots() >= n + 2);
    if ((await mbox.inputValue()) !== "") throw new Error(`휴대폰 보낸 뒤 글이 남았다: ${JSON.stringify(await mbox.inputValue())}`);
    if (mob.errors.length) throw new Error(mob.errors.join("; "));
    await mob.page.context().close();
    aaa("pane", "kill", "pasteapp");
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("보내는 동안 친 글 지키기(herdr-web-ui submit·submit-result, terminalDraft): 입력줄은 host 답을 받고서야 보낸 앞부분만 지운다. 그 사이 친 글은 남고, 거절·끊김이면 글을 두고 이유", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "submitapp", "--", process.execPath, `${root}/paste.js`);
    const got = () => aaa("pane", "read", "submitapp", "--source", "visible").text;
    // host의 답(submitted)을 붙잡아 두는 화면: 답이 오기 전 상태를 본다
    const held = [];
    let pipe = null;
    const holdReplies = (ws) => {
      const server = ws.connectToServer();
      pipe = { ws, server };
      ws.onMessage((m) => server.send(m));
      server.onMessage((m) => {
        if (typeof m === "string" && m.startsWith('{"t":"submitted"')) held.push(m);
        else ws.send(m);
      });
      server.onClose((code, reason) => ws.close({ code, reason }));
      ws.onClose((code, reason) => server.close({ code, reason }));
    };
    const sp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { line: true, wsRoute: [/\/api\/v1\/panes\//, holdReplies] })).page;
    await sp.getByTestId("pane-submitapp").click();
    await until("submitapp 화면", async () => (await screenText(sp)).includes("paste>"));
    const box = sp.getByLabel("보낼 글");
    const sendBtn = sp.getByRole("button", { name: "터미널에 보내기" });
    const note = () => sp.locator(".terminal-input-note").evaluateAll((els) => els[0]?.textContent ?? null);
    const reply = async () => {
      await until("host 답", () => held.length > 0);
      return JSON.parse(held.shift());
    };
    // 1) host가 글과 Enter를 쓴 뒤 답한다. 답 전에는 글이 남고 보내기는 잠긴다. 그 사이 덧붙인 글은 답 뒤에도 남는다
    await box.fill("첫 줄");
    await box.press("Enter");
    const first = await reply();
    if (!first.ok || !got().includes('GOT:"첫 줄"') || !got().includes('GOT:"\\r"')) throw new Error(`답 전에 글·Enter가 pane에 없다: ${JSON.stringify(first)} ${got()}`);
    if ((await box.inputValue()) !== "첫 줄" || !(await sendBtn.isDisabled())) throw new Error(`답 전 입력줄 ${JSON.stringify(await box.inputValue())} 잠김 ${await sendBtn.isDisabled()}`);
    await box.press("End");
    await box.pressSequentially(" 더");
    pipe.ws.send(JSON.stringify(first));
    await until("보낸 앞부분만 지움", async () => (await box.inputValue()) === " 더");
    if ((await note()) !== null) throw new Error(`알림 ${await note()}`);
    // 2) 그 사이 글을 바꿨으면(보낸 글로 시작하지 않으면) 지우지 않는다
    await box.fill("둘");
    await box.press("Enter");
    const second = await reply();
    await box.fill("다른 글");
    pipe.ws.send(JSON.stringify(second));
    await sleep(300);
    if ((await box.inputValue()) !== "다른 글") throw new Error(`바꾼 글을 지웠다: ${JSON.stringify(await box.inputValue())}`);
    // 3) host가 거절하면 글을 두고 이유를 보인다(답을 바꿔 보낸다. 실제 거절은 아래 끝난 pane)
    await box.fill("셋");
    await box.press("Enter");
    const third = await reply();
    pipe.ws.send(JSON.stringify({ ...third, ok: false, message: "시험 거절" }));
    await until("거절 이유", async () => (await note()) === "보내지 못했다: 시험 거절");
    if ((await box.inputValue()) !== "셋" || (await sendBtn.isDisabled())) throw new Error(`거절 뒤 입력줄 ${JSON.stringify(await box.inputValue())}`);
    // 4) 답 전에 끊기면 들어갔는지 모른다: 글을 두고 터미널을 보게 한다
    await box.fill("넷");
    await box.press("Enter");
    await reply();
    pipe.ws.close({ code: 1011, reason: "test" });
    await until("확인 못 함", async () => (await note()) === "확인하지 못했다. 다시 보내기 전에 터미널을 본다");
    if ((await box.inputValue()) !== "넷") throw new Error(`끊긴 뒤 입력줄 ${JSON.stringify(await box.inputValue())}`);
    // 5) 실제 host 거절: 끝난 pane에 보낸 submit은 ok:false와 이유로 답한다(답 없이 두지 않는다)
    aaa("tab", "create", "--machine", "labA", "--label", "submitend", "--", process.execPath, "-e", "process.exit(0)");
    await until("submitend 끝남", () => aaa("pane", "list").panes.find((x) => x.name === "submitend")?.state !== "running");
    // 붙잡는 route가 없는 화면에서 연다
    const refused = await page.evaluate(
      (id) =>
        new Promise((resolve) => {
          const w = new WebSocket(`ws://${location.host}/api/v1/panes/${encodeURIComponent(id)}/ws?flow=ack`);
          w.onmessage = (m) => {
            if (typeof m.data !== "string") return;
            const j = JSON.parse(m.data);
            if (j.t === "snapshot") w.send(JSON.stringify({ t: "submit", id: 7, data: "late" }));
            if (j.t === "submitted") resolve(j);
          };
          w.onclose = (e) => resolve({ closed: e.code });
          setTimeout(() => resolve({ timeout: true }), 5000);
        }),
      aaa("pane", "list").panes.find((x) => x.name === "submitend").id,
    );
    if (refused.id !== 7 || refused.ok !== false || !refused.message) throw new Error(`끝난 pane submit 답 ${JSON.stringify(refused)}`);
    await sp.context().close();
    aaa("pane", "kill", "submitapp");
  });
}
