// 웹 E2E 키: 키 막대·입력 방식·키 바 편집·키 처리·끌어 고르기.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 03-keys(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import {
  root, sleep, aaa, waitOutput, until, screenText, shot, pairPage, steps, step, browser,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("키 막대(herdr-web-ui KeyBar): 터치 기기에서만, 유지 키는 다시 누를 때까지(래치)·하드웨어 키와 같은 bytes, DECCKM, 꾹 누르면 반복, 조합 중 회색, focus를 안 가져감", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "barapp", "--", process.execPath, `${root}/bar.js`);
    // [일련번호, 받은 조각]
    const lines = () =>
      aaa("pane", "read", "barapp", "--source", "visible")
        .text.split("\n").map((l) => /^GOT:(\d+):(.*)$/.exec(l))
        .filter(Boolean)
        .map((m) => [Number(m[1]), JSON.parse(m[2])]);
    const last = () => Math.max(0, ...lines().map(([i]) => i));
    // 데스크톱(마우스)에는 키 막대가 없다
    await page.getByTestId("pane-barapp").click();
    await until("bar 화면(데스크톱)", async () => (await screenText(page)).includes("bar>"));
    if (await page.locator(".key-bar").isVisible()) throw new Error("데스크톱에 키 막대가 보인다");
    // 터치 기기. Alt를 하드웨어 키와 견주려고 macOS가 아닌 platform으로 둔다(macOS xterm은 Option을 글자로 쓴다).
    const tp = (await pairPage(browser, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true, platform: "Linux x86_64" })).page;
    await tp.getByTestId("pane-barapp").click();
    await until("bar 화면(터치)", async () => (await screenText(tp)).includes("bar>"));
    const bar = tp.locator(".key-bar");
    if (!(await bar.isVisible())) throw new Error("터치 기기에 키 막대가 없다");
    const order = await bar.locator(".key").evaluateAll((ks) => ks.map((k) => k.dataset.key));
    // 터치 기기는 맨 앞에 ⌨(입력줄 전환, KeyBar.tsx 200~205줄)
    const want = ["direct", "Escape", "Tab", "ctrl-c", "Control", "Alt", "Shift", "Enter", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"];
    if (JSON.stringify(order) !== JSON.stringify(want)) throw new Error(`키 순서 ${order}`);
    const key = (k) => bar.locator(`.key[data-key="${k}"]`);
    await tp.locator(".xterm-screen").click();
    // 한 동작이 pane에 보낸 조각들
    const sent = async (act, what = "") => {
      const n = last();
      await act();
      await sleep(250);
      await until(`pane 받음 ${what}`, () => last() > n);
      await sleep(150);
      return lines()
        .filter(([i]) => i > n)
        .map(([, d]) => d)
        .join("");
    };
    const bad = [];
    const expect = async (what, act, wantData) => {
      const got = await sent(act, what);
      if (got !== wantData) bad.push(`${what}: ${JSON.stringify(got)}(기대 ${JSON.stringify(wantData)})`);
    };
    // 키를 눌러도 focus는 터미널에 남는다(화면 키보드가 내려가지 않는다)
    await key("Escape").click();
    if (!(await tp.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")))) bad.push("키가 focus를 가져갔다");
    await sleep(300);
    // 래치: 켠 Ctrl은 두 번 써도 남고, 다시 눌러야 풀린다
    await key("Control").click();
    await expect("Ctrl 래치 a", () => tp.keyboard.press("a"), "\x01");
    if ((await key("Control").getAttribute("aria-pressed")) !== "true") bad.push("Ctrl이 한 번 쓰고 풀렸다");
    await expect("Ctrl 래치 b", () => tp.keyboard.press("b"), "\x02");
    await expect("Ctrl + 키 막대 ^C", () => key("ctrl-c").click(), "\x03");
    await key("Control").click();
    if ((await key("Control").getAttribute("aria-pressed")) !== "false") bad.push("Ctrl이 다시 눌러도 안 풀렸다");
    await expect("풀린 뒤 c", () => tp.keyboard.press("c"), "c");
    // 유지 키 + (키 막대 키 또는 하드웨어 키) = 같은 수식의 하드웨어 키
    const combos = [
      ["Control", "ArrowLeft", "bar"],
      ["Alt", "ArrowLeft", "bar"],
      ["Alt", "ArrowUp", "bar"],
      ["Shift", "ArrowUp", "bar"],
      ["Shift", "Tab", "bar"],
      ["Alt", "Enter", "bar"],
      ["Shift", "Enter", "bar"],
      ["Control", "Home", "hw"],
      ["Control", "PageUp", "hw"],
      ["Alt", "b", "hw"],
      ["Control", "Backspace", "hw"],
      ["Shift", "a", "hw"],
    ];
    for (const [mod, k, how] of combos) {
      // 글자는 물리 키 이름으로 누른다: Playwright는 "Shift+a"를 Shift를 쥔 "a"로 내지만 실제 자판은 "A"를 낸다
      const physical = /^[a-z]$/.test(k) ? `Key${k.toUpperCase()}` : k;
      const hardware = await sent(() => tp.keyboard.press(`${mod}+${physical}`), `하드웨어 ${mod}+${k}`);
      await key(mod).click();
      const viaBar = await sent(() => (how === "bar" ? key(k).click() : tp.keyboard.press(k)), `키 막대 ${mod}+${k}`);
      await key(mod).click();
      if (viaBar !== hardware) bad.push(`${mod}+${k}: 키 막대 ${JSON.stringify(viaBar)} ≠ 하드웨어 ${JSON.stringify(hardware)}`);
    }
    // 화살표 모드(DECCKM)를 켜면 키 막대 화살표도 SS3
    await sent(() => tp.keyboard.press("Shift+K"));
    await expect("DECCKM ↑", () => key("ArrowUp").click(), "\x1bOA");
    await sent(() => tp.keyboard.press("Shift+J"));
    await expect("DECCKM 끔 ↑", () => key("ArrowUp").click(), "\x1b[A");
    // 꾹 누르면 400ms 뒤 50ms마다 되풀이, 손을 떼면 한 번 더 가지 않는다
    const left = await key("ArrowLeft").boundingBox();
    const held = await sent(async () => {
      await tp.mouse.move(left.x + left.width / 2, left.y + left.height / 2);
      await tp.mouse.down();
      await sleep(700);
      await tp.mouse.up();
    });
    const repeats = held.split("\x1b[D").length - 1;
    if (repeats < 3 || held.replaceAll("\x1b[D", "") !== "") bad.push(`꾹 누름: ${JSON.stringify(held)}`);
    // 누른 채 밀면 취소(아무것도 안 감)
    const n0 = last();
    await tp.mouse.move(left.x + left.width / 2, left.y + left.height / 2);
    await tp.mouse.down();
    await tp.mouse.move(left.x + left.width / 2 + 15, left.y + left.height / 2, { steps: 3 });
    await sleep(600);
    await tp.mouse.up();
    await sleep(300);
    if (last() !== n0) bad.push(`밀어서 취소한 누름이 갔다: ${JSON.stringify(lines().filter(([i]) => i > n0))}`);
    // 조합 중에는 키 막대가 회색이고, 조합으로 확정된 글에는 유지 키가 붙지 않는다(다음 키를 기다린다)
    await key("Control").click();
    const textarea = tp.locator("textarea.xterm-helper-textarea");
    await textarea.evaluate((el) => el.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" })));
    await sleep(100);
    if (!(await key("Escape").isDisabled())) bad.push("조합 중에 키 막대가 켜져 있다");
    await textarea.evaluate((el) => el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "" })));
    await sleep(100);
    if (await key("Escape").isDisabled()) bad.push("조합이 끝나도 키 막대가 꺼져 있다");
    const cdp = await tp.context().newCDPSession(tp);
    const ime = await sent(async () => {
      await cdp.send("Input.imeSetComposition", { text: "한", selectionStart: 1, selectionEnd: 1 });
      await cdp.send("Input.insertText", { text: "한" });
    });
    await cdp.detach();
    if (ime !== "한") bad.push(`조합 확정 글: ${JSON.stringify(ime)}`);
    if ((await key("Control").getAttribute("aria-pressed")) !== "true") bad.push("조합 확정이 Ctrl을 풀었다");
    await expect("조합 뒤 Ctrl b", () => tp.keyboard.press("b"), "\x02");
    await key("Control").click();
    if (bad.length) throw new Error(bad.join("; "));
    await tp.context().close();
    aaa("pane", "kill", "barapp");
    await page.getByTestId("pane-t1").click();
  });

  await step("입력 방식(herdr-web-ui terminalInputMode): 기본은 바로 입력, 터치 기기 ⌨로 입력줄, 입력줄은 키보드를 안 올리고 PTY 크기를 안 바꾼다, Enter 보내기·빈 Enter, 조합 중엔 ⌨만 회색", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "lineapp", "--", process.execPath, `${root}/bar.js`);
    // [일련번호, 받은 조각]
    const lines = () =>
      aaa("pane", "read", "lineapp", "--source", "visible")
        .text.split("\n").map((l) => /^GOT:(\d+):(.*)$/.exec(l))
        .filter(Boolean)
        .map((m) => [Number(m[1]), JSON.parse(m[2])]);
    const last = () => Math.max(0, ...lines().map(([i]) => i));
    const since = (n) =>
      lines()
        .filter(([i]) => i > n)
        .map(([, d]) => d);
    const size = () => {
      const x = aaa("pane", "list").panes.find((q) => q.name === "lineapp");
      return `${x.cols}x${x.rows}`;
    };
    const bad = [];
    // 데스크톱 기본: 입력줄이 없다(바로 입력)
    const desk = await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 });
    await desk.page.getByTestId("pane-lineapp").click();
    await until("bar 화면(데스크톱)", async () => (await screenText(desk.page)).includes("bar>"));
    if ((await desk.page.locator(".terminal-input").count()) !== 0) bad.push("데스크톱 기본에 입력줄이 있다");
    await desk.page.context().close();
    // 터치 기기 기본: 바로 입력. 키 막대 맨 앞 ⌨는 꺼져 있고 xterm 칸이 화면 키보드를 올린다
    const tp = (await pairPage(browser, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true, platform: "Linux x86_64" })).page;
    await tp.getByTestId("pane-lineapp").click();
    await until("bar 화면(터치)", async () => (await screenText(tp)).includes("bar>"));
    const toggle = tp.locator('.key-bar .key[data-key="direct"]');
    const first = await tp.locator(".key-bar .key").first().getAttribute("data-key");
    if (first !== "direct") bad.push(`키 막대 맨 앞이 ⌨가 아니다: ${first}`);
    if ((await toggle.getAttribute("aria-pressed")) !== "false") bad.push("기본인데 ⌨가 켜져 있다");
    if ((await tp.locator(".terminal-input").count()) !== 0) bad.push("터치 기본에 입력줄이 있다");
    if ((await tp.locator(".term-scroll[data-direct-typing]").count()) !== 1) bad.push("바로 입력인데 data-direct-typing이 없다");
    const helper = tp.locator("textarea.xterm-helper-textarea");
    // ⌨ → 입력줄. xterm 칸은 화면 키보드를 올리지 않는다(inputmode=none)
    await toggle.click();
    await tp.locator(".terminal-input").waitFor();
    if ((await toggle.getAttribute("aria-pressed")) !== "true") bad.push("입력줄인데 ⌨가 꺼져 있다");
    if ((await helper.getAttribute("inputmode")) !== "none") bad.push(`입력줄인데 xterm inputmode ${await helper.getAttribute("inputmode")}`);
    if ((await tp.locator(".term-scroll[data-direct-typing]").count()) !== 0) bad.push("입력줄인데 data-direct-typing이 있다");
    if ((await tp.evaluate(() => localStorage.getItem("aaa:terminal-input-mode"))) !== "line") bad.push("입력 방식이 이 기기에 저장되지 않았다");
    const line = tp.getByLabel("보낼 글");
    const sendBtn = tp.locator(".terminal-input-send");
    // 글 + Enter: 글 다음 Enter가 따로 간다
    let n = last();
    await line.fill("echo hi");
    await line.press("Enter");
    await until("입력줄 Enter", () => since(n).join("") === "echo hi\r");
    if ((await line.inputValue()) !== "") bad.push("보낸 뒤 입력줄에 글이 남았다");
    // 빈 입력줄의 보내기는 Enter만. 단추 이름도 Enter다. 유지 Alt는 붙는다(PaneTerminal pressEnter)
    if ((await sendBtn.getAttribute("aria-label")) !== "터미널에 Enter 보내기") bad.push(`빈 줄 단추 ${await sendBtn.getAttribute("aria-label")}`);
    n = last();
    await sendBtn.click();
    await until("빈 Enter", () => since(n).join("") === "\r");
    await tp.locator('.key-bar .key[data-key="Alt"]').click();
    n = last();
    await sendBtn.click();
    await until("Alt + 빈 Enter", () => since(n).join("") === "\x1b\r");
    await tp.locator('.key-bar .key[data-key="Alt"]').click();
    // 키 막대 키는 입력줄의 키보드를 터미널로 옮기지 않는다
    await line.focus();
    n = last();
    await tp.locator('.key-bar .key[data-key="Escape"]').click();
    await until("Esc", () => since(n).join("") === "\x1b");
    if (!(await line.evaluate((el) => el === document.activeElement))) bad.push("키 막대 키가 입력줄의 focus를 가져갔다");
    // 입력줄이 4줄까지 자라도 PTY 크기는 그대로다(그리드 위에 뜬다)
    const before = size();
    const box0 = await tp.locator(".term-scroll").boundingBox();
    await line.fill("1\n2\n3\n4\n5\n6");
    await sleep(800);
    if ((await line.getAttribute("rows")) !== "4") bad.push(`여섯 줄 입력줄 rows ${await line.getAttribute("rows")}`);
    const box1 = await tp.locator(".term-scroll").boundingBox();
    if (size() !== before || Math.abs(box1.height - box0.height) > 0.5) bad.push(`입력줄이 자라며 터미널이 줄었다: ${before} → ${size()}, 높이 ${box0.height} → ${box1.height}`);
    await line.fill("");
    // 입력줄 조합 중에는 ⌨만 회색이고 키 막대는 그대로다
    await line.dispatchEvent("compositionstart", { data: "" });
    await sleep(100);
    if (!(await toggle.isDisabled())) bad.push("입력줄 조합 중에 ⌨가 켜져 있다");
    if (await tp.locator('.key-bar .key[data-key="Escape"]').isDisabled()) bad.push("입력줄 조합 중에 키 막대가 꺼졌다");
    await line.dispatchEvent("compositionend", { data: "" });
    await sleep(100);
    // ⌨를 다시 누르면 바로 입력: 입력줄이 사라지고 터미널이 키보드를 받는다
    await toggle.click();
    await until("입력줄 사라짐", async () => (await tp.locator(".terminal-input").count()) === 0);
    if ((await helper.getAttribute("inputmode")) !== null) bad.push("바로 입력인데 xterm inputmode가 남았다");
    if (!(await helper.evaluate((el) => el === document.activeElement))) bad.push("바로 입력을 켰는데 터미널이 focus를 받지 않았다");
    if (bad.length) throw new Error(bad.join("; "));
    await tp.context().close();
    aaa("pane", "kill", "lineapp");
    await page.getByTestId("pane-t1").click();
  });

  await step("키 바 편집(herdr-web-ui KeyBarSettings, 차이 목록 「키 막대 항목 설정」): 설정에서 키를 빼고·옮기고·더하고 조합을 만들면 터미널 키 막대가 바로 그 순서, 이 기기에 저장, 기본값으로 되돌리기", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "kbset", "--", process.execPath, `${root}/scroll.js`, "plain");
    await waitOutput("kbset", "s-200");
    const tp = (await pairPage(browser, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true, platform: "Linux x86_64" })).page;
    await tp.getByTestId("pane-kbset").click();
    await until("kbset 화면", async () => (await screenText(tp)).includes("s-200"));
    const order = () => tp.locator(".key-bar .key").evaluateAll((ks) => ks.map((k) => k.dataset.key));
    const defaults = ["direct", "Escape", "Tab", "ctrl-c", "Control", "Alt", "Shift", "Enter", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"];
    if (JSON.stringify(await order()) !== JSON.stringify(defaults)) throw new Error(`처음 키 막대 ${await order()}`);
    const openKeyBar = async () => {
      await tp.getByTestId("settings-open").click();
      await tp.getByRole("dialog", { name: "설정" }).getByRole("button", { name: "키 바 편집" }).click();
      return tp.getByRole("dialog", { name: "키 바" });
    };
    const kb = await openKeyBar();
    await kb.getByRole("button", { name: "Esc 제거" }).click();
    await kb.getByRole("button", { name: "Tab 아래로 이동" }).click();
    await kb.locator(".key-bar-settings-add select").selectOption("PageUp");
    await kb.locator(".key-bar-settings-add").getByRole("button", { name: "키 추가" }).click();
    // 사용자 조합: 기본 키 ←(처음 값)에 Ctrl
    await kb.getByRole("group", { name: "조합에 사용할 보조 키" }).getByRole("button", { name: "Ctrl" }).click();
    if ((await kb.locator(".key-bar-settings-preview").innerText()) !== "Ctrl+←") throw new Error(`조합 미리 보기 ${await kb.locator(".key-bar-settings-preview").innerText()}`);
    await kb.getByRole("button", { name: "조합 추가" }).click();
    await shot(tp, "tablet-key-bar-settings");
    const combo = 'key:"ArrowLeft":100';
    const want = ["direct", "ctrl-c", "Tab", "Control", "Alt", "Shift", "Enter", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", combo];
    // ← 는 설정으로, Esc는 닫기
    await kb.getByRole("button", { name: "설정으로 돌아가기" }).click();
    await tp.getByRole("dialog", { name: "설정" }).waitFor();
    await tp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await tp.locator(".settings-dialog").count()) === 0);
    // 키 막대는 다시 열지 않아도 바로 바뀐다
    await until("키 막대가 편집한 순서", async () => JSON.stringify(await order()) === JSON.stringify(want));
    const stored = await tp.evaluate(() => JSON.parse(localStorage.getItem("aaa:keybar-items") ?? "null"));
    if (!Array.isArray(stored) || stored.length !== want.length - 1) throw new Error(`저장 ${JSON.stringify(stored)}`);
    // 저장한 조합은 그 보조 키로 간다(Ctrl+← = CSI 1;5D)
    await tp.locator(".xterm-screen").click();
    await tp.locator(`.key-bar .key[data-key='${combo}']`).click();
    await waitOutput("kbset", 'GOT:"\\u001b[1;5D"');
    // 다시 열어도 이 기기의 순서
    await tp.reload();
    await tp.getByTestId("pane-kbset").click();
    await until("다시 연 키 막대", async () => JSON.stringify(await order()) === JSON.stringify(want));
    // 기본값으로 되돌리기: 저장값을 지우고 참고 기본 배열
    await (await openKeyBar()).getByRole("button", { name: "기본값으로 되돌리기" }).click();
    await tp.keyboard.press("Escape");
    await tp.keyboard.press("Escape");
    await until("기본 키 막대", async () => JSON.stringify(await order()) === JSON.stringify(defaults));
    if ((await tp.evaluate(() => localStorage.getItem("aaa:keybar-items"))) !== null) throw new Error("기본값인데 저장값이 남았다");
    await shot(tp, "tablet-key-bar-defaults");
    await tp.context().close();
    aaa("pane", "kill", "kbset");
  });

  await step("키 처리(herdr-web-ui PaneTerminal, macOS): Shift+Enter ESC CR, Ctrl+Enter는 modifyOtherKeys대로, Cmd+⌫ ^U, Cmd+←/→ ^A/^E, Ctrl+V는 브라우저 몫, 선택 중 Ctrl+C는 복사, OSC 52, 늦게 붙은 화면도 모드를 안다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "keysapp", "--", process.execPath, `${root}/keys.js`);
    const got = () => aaa("pane", "read", "keysapp", "--source", "visible").text;
    const gots = () => got().split("GOT:").length - 1;
    // 클립보드는 기록만 한다(headless 권한·focus와 상관없이 무엇을 썼는지 본다)
    const clip = () => {
      window.__clip = [];
      window.__copies = 0;
      Object.defineProperty(navigator, "clipboard", {
        configurable: true,
        value: { writeText: (t) => (window.__clip.push(t), Promise.resolve()), readText: () => Promise.resolve("") },
      });
      document.addEventListener("copy", () => window.__copies++, true);
    };
    const mac = await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { platform: "MacIntel", init: clip });
    const mp = mac.page;
    await mp.getByTestId("pane-keysapp").click();
    await until("keys 화면", async () => (await screenText(mp)).includes("keys>"));
    await mp.getByTestId("terminal").click();
    const press = async (key, want) => {
      const n = gots();
      await mp.keyboard.press(key);
      await until(`${key} → ${want}`, () => gots() > n);
      const last = got().split("GOT:").at(-1).split("\n")[0];
      if (last !== want) throw new Error(`${key}가 ${last}로 갔다(기대 ${want})`);
      // 프로그램이 같은 조각에 쓴 모드 변경을 웹 터미널도 읽은 뒤에 다음 키를 누른다
      await until(`화면 ${want}`, async () => (await screenText(mp)).split("GOT:").at(-1).split("\n")[0] === want);
    };
    await press("Shift+Enter", '"\\u001b\\r"');
    // 화면을 열 때 이미 켜진 모드는 snapshot replay가 다시 말한다
    await press("Control+Enter", '"\\u001b[27;5;13~"');
    await press("n", '"n"');
    await press("Control+Enter", '"\\r"');
    await press("m", '"m"');
    await press("Control+Enter", '"\\u001b[27;5;13~"');
    await press("Meta+Backspace", '"\\u0015"');
    await press("Meta+ArrowLeft", '"\\u0001"');
    await press("Meta+ArrowRight", '"\\u0005"');
    await press("Enter", '"\\r"');
    // Ctrl+V는 xterm이 ^V로 보내지 않는다(브라우저 붙여넣기에 맡긴다): 뒤이은 x만 간다
    await mp.keyboard.press("Control+v");
    await press("x", '"x"');
    // 선택이 있으면 Ctrl+C는 복사하고 ^C를 보내지 않는다
    const rows = await mp.locator(".xterm-rows > div").first().boundingBox();
    await mp.mouse.move(rows.x + 2, rows.y + rows.height / 2);
    await mp.mouse.down();
    await mp.mouse.move(rows.x + 60, rows.y + rows.height / 2, { steps: 4 });
    await mp.mouse.up();
    await mp.keyboard.press("Control+c");
    await press("y", '"y"');
    if ((await mp.evaluate(() => window.__copies)) < 1) throw new Error("선택 중 Ctrl+C가 복사하지 않았다");
    // OSC 52: 프로그램이 쓴 글이 클립보드로
    await press("o", '"o"');
    await until("OSC 52 클립보드", async () => (await mp.evaluate(() => window.__clip)).includes("클립 52"));
    // 늦게 붙은 화면: 프로그램은 모드를 다시 말하지 않지만 replay가 말한다
    const late = await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { platform: "MacIntel" });
    await late.page.getByTestId("pane-keysapp").click();
    await until("keys 화면(늦게 붙음)", async () => (await screenText(late.page)).includes("keys>"));
    await late.page.getByTestId("terminal").click();
    const n = gots();
    await late.page.keyboard.press("Control+Enter");
    await until("늦게 붙은 화면 Ctrl+Enter", () => gots() > n);
    const last = got().split("GOT:").at(-1).split("\n")[0];
    if (last !== '"\\u001b[27;5;13~"') throw new Error(`늦게 붙은 화면의 Ctrl+Enter가 ${last}`);
    if (mac.errors.length || late.errors.length) throw new Error([...mac.errors, ...late.errors].join("; "));
    await late.page.context().close();
    await mp.context().close();
    await page.getByTestId("pane-t1").click();
  });

  await step("그냥 끌어 고르기·제자리 클릭·크기 되찾기(herdr-web-ui PaneTerminal): 마우스 보고 중에도 왼쪽 끌기는 고르고 놓으면 복사, 제자리 클릭은 프로그램에, 키는 보내기 전에 이 화면 크기를 건다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "mouseapp", "--", process.execPath, `${root}/mouse.js`);
    // [일련번호, 종류, 값]
    const events = () =>
      aaa("pane", "read", "mouseapp", "--source", "visible")
        .text.split("\n").map((l) => /^(GOT|SIZE):(\d+):(.*)$/.exec(l))
        .filter(Boolean)
        .map((m) => [Number(m[2]), m[1], m[1] === "GOT" ? JSON.parse(m[3]) : Number(m[3])]);
    const after = (n) => events().filter(([i]) => i > n);
    const lastSeq = () => Math.max(0, ...events().map(([i]) => i));
    const clip = () => {
      window.__copied = [];
      // xterm이 copy 사건에 넣은 글(bubble 끝에서 읽는다)
      window.addEventListener("copy", (e) => window.__copied.push(e.clipboardData?.getData("text/plain") ?? ""));
    };
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { platform: "MacIntel", init: clip })).page;
    // 이 화면이 WebSocket에 쓴 글 frame(보낸 순서를 본다)
    const frames = [];
    dp.on("websocket", (ws) =>
      ws.on("framesent", (f) => {
        if (typeof f.payload !== "string") return;
        try {
          frames.push(JSON.parse(f.payload));
        } catch {}
      }),
    );
    await dp.getByTestId("pane-mouseapp").click();
    await until("mouse 화면", async () => (await screenText(dp)).includes("pick-this-word"));
    const cell = async (col, row) => {
      const r = await dp.locator('[data-testid="terminal"] .xterm-screen').boundingBox();
      const p = aaa("pane", "list").panes.find((x) => x.name === "mouseapp");
      return { x: r.x + (col + 0.5) * (r.width / p.cols), y: r.y + (row + 0.5) * (r.height / p.rows) };
    };
    // 제자리 클릭은 프로그램에 간다(SGR 누름·뗌, 1부터 센 열·줄)
    let n = lastSeq();
    const c2 = await cell(2, 0);
    await dp.mouse.click(c2.x, c2.y);
    await until("클릭 보고", () => after(n).filter(([, k]) => k === "GOT").map(([, , v]) => v).join("").includes("\u001b[<0;3;1m"));
    const clicked = after(n).filter(([, k]) => k === "GOT").map(([, , v]) => v).join("");
    if (!clicked.includes("\u001b[<0;3;1M")) throw new Error(`클릭 누름 보고가 없다: ${JSON.stringify(clicked)}`);
    // 끌기는 고른다: 프로그램에 마우스 보고가 가지 않고, 놓으면 복사하고 알린다
    await sleep(300);
    n = lastSeq();
    const from = await cell(0, 0);
    // xterm은 끝을 가까운 칸 경계로 잡는다: 마지막 글자 다음 칸 가운데까지 끈다
    const to = await cell(14, 0);
    await dp.mouse.move(from.x, from.y);
    await dp.mouse.down();
    await dp.mouse.move(to.x, to.y, { steps: 6 });
    await dp.mouse.up();
    await until("복사 알림", async () => (await dp.getByTestId("clipboard-note").innerText()).includes("클립보드에 복사했다"));
    const copied = await dp.evaluate(() => window.__copied);
    if (!copied.some((t) => t.includes("pick-this-word"))) throw new Error(`복사한 글 ${JSON.stringify(copied)}`);
    await sleep(300);
    const leaked = after(n).filter(([, k, v]) => k === "GOT" && v.includes("\u001b[<"));
    if (leaked.length) throw new Error(`끌기가 프로그램에 갔다: ${JSON.stringify(leaked)}`);
    // 다른 화면이 크기를 가져간 뒤 이 화면에서 키를 치면, 키보다 먼저 이 화면 크기를 건다(첫 글의 답이 이 grid로 그려진다)
    const sp = (await pairPage(browser, aaa("web", "pair").code, { width: 760, height: 560 })).page;
    await sp.getByTestId("pane-mouseapp").click();
    await until("mouse 화면(둘째)", async () => (await screenText(sp)).includes("pick-this-word"));
    const pcols = () => aaa("pane", "list").panes.find((x) => x.name === "mouseapp").cols;
    const dcols = pcols();
    // 둘째 화면이 크기를 가져간다(키 막대 없는 데스크톱: 터미널 누름은 클릭 보고도 보낸다)
    await sp.getByTestId("terminal").click();
    await until("둘째 화면 크기", () => pcols() < dcols);
    await sleep(500);
    // 보낸 순서는 이 화면이 WebSocket에 쓴 순서로 본다(프로그램 쪽은 SIGWINCH와 입력 읽기의 순서가 정해지지 않는다)
    const mark = frames.length;
    n = lastSeq();
    await dp.keyboard.press("z");
    await until("z 받음", () => after(n).some(([, k, v]) => k === "GOT" && v === "z"));
    await until("크기 되찾음", () => pcols() === dcols);
    const order = frames.slice(mark).filter((f) => f.t === "resize" || (f.t === "input" && f.data === "z")).map((f) => f.t);
    if (order[0] !== "resize" || !order.includes("input")) throw new Error(`보낸 순서 ${JSON.stringify(frames.slice(mark))}`);
    await sp.context().close();
    await dp.context().close();
    aaa("pane", "kill", "mouseapp");
  });
}
