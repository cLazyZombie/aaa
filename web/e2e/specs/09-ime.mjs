// 웹 E2E 한글 IME: 데스크톱·Safari·iPad 기록 재생·iPhone 기록·입력 기록.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 09-ime(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import path from "node:path";
import {
  webkit, root, sleep, aaa, waitOutput, until, screenText, termDiag, imeType, safariEdit, safariSyllable,
  iphoneKey, ipadKey, ipadFresh, fakeViewport, softKeyboard, ipadReplace, pairPage, base, step, browser,
  openSettings,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("터미널 한글 IME(herdr-web-ui xterm 패치): 데스크톱 조합 사건·CDP 실제 IME, macOS Safari·iPad 기록 재생 → pane에는 완성된 글만", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "imeapp", "--", process.execPath, `${root}/paste.js`);
    aaa("tab", "create", "--machine", "labA", "--label", "imespin", "--", process.execPath, `${root}/spin.js`);
    const wk = await webkit.launch();
    const bad = [];
    const textarea = (q) => q.locator("textarea.xterm-helper-textarea");
    const press = async (q, ...keys) => {
      for (const k of keys) await textarea(q).press(k);
    };
    // 데스크톱(실제 브라우저 판)
    const desktop = [
      ["같은 task", (q) => imeType(q), "상태"],
      // 한글 입력기에서 숫자·문장부호: keydown(229) 뒤 조합 없이 글만 늘어난다
      [
        "조합 없는 229(숫자)",
        (q) =>
          textarea(q).evaluate(async (el) => {
            for (const d of ["7", "."]) {
              el.dispatchEvent(new KeyboardEvent("keydown", { key: "Process", keyCode: 229, which: 229, bubbles: true, cancelable: true }));
              el.value += d;
              el.dispatchEvent(new InputEvent("input", { data: d, inputType: "insertText", bubbles: true, composed: true }));
              el.dispatchEvent(new KeyboardEvent("keyup", { key: d, keyCode: 229, bubbles: true }));
              await new Promise((r) => setTimeout(r, 40));
            }
          }),
        "7.",
      ],
    ];
    const cdpIme = [
      "CDP 실제 IME",
      async (q) => {
        const cdp = await q.context().newCDPSession(q);
        for (const syl of [["ㅅ", "사", "상"], ["ㅌ", "태"]]) {
          for (const t of syl) await cdp.send("Input.imeSetComposition", { text: t, selectionStart: t.length, selectionEnd: t.length });
          await cdp.send("Input.insertText", { text: syl.at(-1) });
        }
        await cdp.detach();
      },
      "상태",
    ];
    // macOS Safari 26.6.2 기록(herdr-web-ui terminal-safari-ime-regression.ts)
    const safari = [
      [
        "영문 뒤 한글",
        async (q) => {
          await press(q, "a", "b", "c");
          await safariSyllable(q, ["ㅎ", "하", "한"]);
          await safariEdit(q, "insertReplacementText", "한", 0, 1);
          await safariSyllable(q, ["ㄱ", "그", "글"], 1);
          await safariEdit(q, "insertReplacementText", "글", 1, 2);
          await press(q, "Space");
        },
        "abc한글 ",
      ],
      [
        "Enter 앞 확정",
        async (q) => {
          await safariSyllable(q, ["ㅎ", "하", "한"]);
          await press(q, "Enter");
        },
        "한\r",
      ],
      [
        "받침 넘김",
        async (q) => {
          await safariSyllable(q, ["ㄱ", "가", "갑", "값"]);
          await safariEdit(q, "insertReplacementText", "갑", 0, 1);
          await safariSyllable(q, ["사"], 1);
          await press(q, "Space");
        },
        "갑사 ",
      ],
      [
        "확정 전 지우기",
        async (q) => {
          await safariSyllable(q, ["ㅎ", "하", "한"]);
          await safariEdit(q, "insertReplacementText", "하", 0, 1);
          await safariEdit(q, "insertReplacementText", "ㅎ", 0, 1);
          await safariEdit(q, "deleteContentBackward", "", 0, 1);
          await press(q, "a");
        },
        "a",
      ],
      [
        "Safari 실제 조합 사건",
        (q) =>
          textarea(q).evaluate(async (box) => {
            box.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
            box.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, inputType: "insertCompositionText", data: "한", isComposing: true }));
            box.value = "한";
            box.dispatchEvent(new CompositionEvent("compositionupdate", { bubbles: true, data: "한" }));
            box.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertCompositionText", data: "한", isComposing: true }));
            await new Promise((r) => requestAnimationFrame(() => r()));
            box.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "한" }));
          }),
        "한",
      ],
    ];
    // iPadOS Safari + 하드웨어 키보드 기록(herdr-web-ui terminal-ipad-ime-regression.ts)
    const ipad = [
      [
        "영문 뒤 한글",
        async (q) => {
          await press(q, "a", "b", "c");
          await ipadFresh(q, "ㅎ");
          await ipadReplace(q, "ㅏ", "하");
          await ipadReplace(q, "ㄴ", "한");
          await ipadFresh(q, "ㄱ");
          await ipadReplace(q, "ㅡ", "그");
          await ipadReplace(q, "ㄹ", "글");
          await press(q, "Space");
        },
        "abc한글 ",
      ],
      [
        "두 음절 바꿔 넣기",
        async (q) => {
          await ipadFresh(q, "ㅁ");
          await ipadReplace(q, "ㅏ", "마");
          await ipadReplace(q, "ㅇ", "망");
          await ipadReplace(q, "ㅏ", "마아");
          await press(q, "Space");
        },
        "마아 ",
      ],
      [
        "Enter 앞 확정",
        async (q) => {
          await ipadFresh(q, "ㅎ");
          await ipadReplace(q, "ㅏ", "하");
          await ipadReplace(q, "ㄴ", "한");
          await press(q, "Enter");
        },
        "한\r",
      ],
    ];
    const SAFARI_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/26.6.2 Safari/605.1.15";
    const IPAD_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.2 Safari/605.1.15";
    const wide = { width: 1280, height: 800 };
    const IPAD_PORTRAIT = { width: 820, height: 1180 };
    const runs = [
      ["Chromium", async () => page, [...desktop, cdpIme]],
      ["Safari 26.6.2 기록", async () => (await pairPage(browser, aaa("web", "pair").code, wide, { userAgent: SAFARI_UA, platform: "MacIntel" })).page, safari],
      // 화면 키보드가 올라온 iPad(사용자 보고 13:0x: 키보드 고침 뒤 자모가 다시 분리됐다). 키보드가 grid를 붙잡고
      // 터미널을 커서 줄로 옮긴 상태에서, 출력이 계속 오는 동안 같은 기록을 재생한다. 조용한 iPad 재생은 「iPad IME 회귀」가 본다
      [
        "WebKit iPad 화면 키보드 iPad 기록",
        async () => (await pairPage(wk, aaa("web", "pair").code, IPAD_PORTRAIT, { userAgent: IPAD_UA, platform: "iPad", touch: true, init: fakeViewport })).page,
        ipad,
        async (p) => {
          await softKeyboard(p, IPAD_PORTRAIT.height - 400);
          await sleep(600);
          const up = await p.evaluate(() => document.documentElement.hasAttribute("data-keyboard"));
          const panned = await p.locator(".term-scroll").evaluate((el) => el.scrollTop);
          if (!up || panned <= 0) throw new Error(`화면 키보드 흉내가 안 됐다(data-keyboard ${up}, 옮긴 거리 ${panned})`);
          // iOS는 caret이 움직이면 보이는 영역을 옮기고 visualViewport scroll을 보낸다(viewport.ts가 그때마다 맞춘다)
          await p.evaluate(() => setInterval(() => window.viewportFixture.viewport.dispatchEvent(new Event("scroll")), 30));
        },
        "imespin",
      ],
    ];
    try {
      for (const [engine, open, ways, keyboard, pane = "imeapp"] of runs) {
        const p = await open();
        await p.getByTestId(`pane-${pane}`).click();
        await until("ime 화면", async () => (await screenText(p)).includes("paste>"));
        // 다른 화면이 쓰던 pane이어도 터미널을 누르고 바로 친다
        await until("입력 가능", async () => (await p.getByTestId("readonly-banner").count()) === 0);
        await p.locator(".xterm-screen").click();
        if (keyboard) await keyboard(p);
        for (const [how, type, want] of ways) {
          aaa("pane", "send-text", pane, "MARK");
          await waitOutput(pane, 'GOT:"MARK"');
          // 기록 재생은 빈 편집기에서 시작한다(herdr-web-ui 재생과 같다)
          await textarea(p).evaluate((el) => {
            el.value = "";
          });
          await type(p);
          await sleep(700);
          const all = aaa("pane", "read", pane, "--source", "visible").text;
          const got = all
            .slice(all.lastIndexOf('GOT:"MARK"') + 10)
            .split("\n")
            .filter((l) => l.startsWith("GOT:"))
            .map((l) => JSON.parse(l.slice(4)))
            .join("");
          console.log(`     IME ${engine} ${how}: ${JSON.stringify(got)}`);
          if (got !== want) bad.push(`${engine} ${how}: pane이 받은 글 ${JSON.stringify(got)}(기대 ${JSON.stringify(want)})`);
        }
        if (p !== page) await p.context().close();
      }
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(bad.join("; "));
    aaa("pane", "kill", "imeapp");
    aaa("pane", "kill", "imespin");
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("iPhone 화면 키보드 사용자 기록 재생(10-17): 「한글」을 자모마다 keydown으로 친 뒤 blur → pane에는 완성된 음절만", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "iphoneA", "--", process.execPath, `${root}/spin.js`);
    const IPHONE_UA = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0.1 Mobile/15E148 Safari/604.1";
    const IPHONE = { width: 430, height: 932 };
    const wk = await webkit.launch();
    const bad = [];
    try {
      for (const [label, b, keyboard] of [["WebKit iPhone 화면 키보드", wk, true], ["Chromium iPhone", browser, false]]) {
        const { page: p } = await pairPage(b, aaa("web", "pair").code, IPHONE, { userAgent: IPHONE_UA, platform: "iPhone", touch: true, init: fakeViewport });
        const sent = [];
        const frames = [];
        let bins = 0;
        let ready = false;
        await p.routeWebSocket(/\/api\/v1\/panes\/[^/]+\/ws/, (socket) => {
          const server = socket.connectToServer();
          server.onMessage((raw) => {
            if (typeof raw === "string") {
              const m = JSON.parse(raw);
              // snapshot은 replay 길이도(빈 화면이 빈 replay였는지 가린다)
              frames.push(m.t === "snapshot" ? `snapshot:${m.replay?.length ?? 0}` : m.t);
              if (m.t === "snapshot" || m.t === "readonly") ready = m.readonly === false;
            } else bins++;
            socket.send(raw);
          });
          socket.onMessage((raw) => {
            const m = typeof raw === "string" ? JSON.parse(raw) : null;
            if (m?.t === "input") sent.push(m.data);
            else server.send(raw);
          });
        });
        // 가로채기는 새로 연 문서부터 붙는다(참고 시험도 reset마다 다시 연다)
        await p.goto(base);
        await p.getByTestId("pane-iphoneA").evaluate((node) => node.click());
        await until("iPhone 준비", () => ready).catch(async (e) => {
          throw new Error(`${e.message}: ${label} 프레임 ${JSON.stringify(frames)} 화면 ${JSON.stringify((await p.locator("body").innerText()).slice(0, 300))}`);
        });
        await until("iPhone 화면", async () => (await screenText(p)).includes("paste>")).catch(async (e) => {
          // CI에서만 실패한 적이 있다(10-17 dbacbfe·372e85d·e784eac): 그때 화면이 어땠는지 남긴다
          const seen = await termDiag(p);
          throw new Error(`${e.message}: ${label} 프레임 ${JSON.stringify(frames.slice(-12))} 출력 ${bins}조각 ${JSON.stringify(seen)} pane ${JSON.stringify(aaa("pane", "read", "iphoneA", "--source", "visible").text.split("\n").filter(Boolean).slice(-2))}`);
        });
        const input = p.locator(".xterm-helper-textarea");
        await input.focus();
        // 기록처럼 화면 키보드가 올라온다(보이는 높이 873 → 485)
        if (keyboard) {
          await softKeyboard(p, 485);
          await until("화면 키보드", () => p.evaluate(() => document.documentElement.hasAttribute("data-keyboard")));
          await sleep(300);
        }
        await input.evaluate((box) => {
          box.value = "";
        });
        sent.length = 0;
        const fresh = (jamo) => iphoneKey(p, jamo, [["insert", jamo]]);
        const replace = (jamo, result) => iphoneKey(p, jamo, [["delete"], ["insert", result]]);
        await fresh("ㅎ");
        await replace("ㅏ", "하");
        await replace("ㄴ", "한");
        if (sent.join("") !== "") bad.push(`${label}: 조합 중 자모를 보냈다 ${JSON.stringify(sent)}`);
        await fresh("ㄱ");
        await replace("ㅡ", "그");
        await replace("ㄹ", "글");
        // 기록 끝: 키보드를 내리며 blur
        await input.evaluate((box) => box.blur());
        await until(`${label} blur 확정`, () => sent.join("").length >= 2, 3000).catch(() => {});
        await sleep(300);
        console.log(`     iPhone 기록 ${label}: ${JSON.stringify(sent.join(""))}`);
        if (sent.join("") !== "한글") bad.push(`${label}: pane이 받은 글 ${JSON.stringify(sent.join(""))}(기대 "한글")`);
        await p.goto("about:blank").catch(() => {});
        await p.context().close();
      }
    } finally {
      await wk.close();
    }
    aaa("pane", "kill", "iphoneA");
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("iPad IME 회귀(herdr-web-ui scripts/terminal-ipad-ime-regression.ts 그대로): Chromium MacIntel·iPad, 화면 키보드가 올라온 WebKit iPad", async () => {
    // 참고 시험처럼 pane WebSocket을 가로채 보낸 input 프레임을 그대로 본다(입력은 pane에 가지 않는다).
    // 두 pane은 참고처럼 조용한 입력창이다(출력이 오는 동안의 조합은 「터미널 한글 IME」 단계의 화면 키보드 판이 본다).
    for (const n of ["ipadA", "ipadB"]) aaa("tab", "create", "--machine", "labA", "--label", n, "--", process.execPath, `${root}/paste.js`);
    const ids = Object.fromEntries(aaa("pane", "list").panes.filter((x) => x.name.startsWith("ipad")).map((x) => [x.id, x.name]));
    const wk = await webkit.launch();
    const IPAD_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.2 Safari/605.1.15";
    const IPAD_PORTRAIT = { width: 820, height: 1180 };
    const runs = [
      ["Chromium MacIntel", () => pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { userAgent: IPAD_UA, platform: "MacIntel" }), false],
      ["Chromium iPad", () => pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { userAgent: IPAD_UA, platform: "iPad" }), false],
      [
        "WebKit iPad 화면 키보드",
        () => pairPage(wk, aaa("web", "pair").code, IPAD_PORTRAIT, { userAgent: IPAD_UA, platform: "iPad", touch: true, init: fakeViewport }),
        true,
      ],
    ];
    const bad = [];
    try {
      for (const [label, open, keyboard] of runs) {
        const { page: p } = await open();
        const sent = [];
        let ready = false;
        // pane이 쓴 것처럼 화면에 보낼 출력(참고 toPage)
        let toPage = () => {};
        await p.routeWebSocket(/\/api\/v1\/panes\/[^/]+\/ws/, (socket) => {
          const pane = ids[decodeURIComponent(socket.url().split("/panes/")[1].split("/")[0])];
          const server = socket.connectToServer();
          toPage = (data) => socket.send(Buffer.from(data));
          server.onMessage((raw) => {
            if (typeof raw === "string") {
              const m = JSON.parse(raw);
              if (m.t === "snapshot" || m.t === "readonly") ready = m.readonly === false;
            }
            socket.send(raw);
          });
          socket.onMessage((raw) => {
            const m = typeof raw === "string" ? JSON.parse(raw) : null;
            if (m?.t === "input") sent.push({ pane, data: m.data });
            else server.send(raw);
          });
        });
        const input = p.locator(".xterm-helper-textarea");
        const select = async (name) => {
          ready = false;
          // 참고처럼 focus를 옮기지 않는 click(조합 중 글자가 blur로 확정되지 않게)
          await p.getByTestId(`pane-${name}`).evaluate((node) => node.click());
          await until(`${name} 준비`, () => ready);
          await until(`${name} 화면`, async () => (await screenText(p)).includes("paste>"));
          await input.focus();
          if (keyboard) {
            await softKeyboard(p, IPAD_PORTRAIT.height - 400);
            await until("화면 키보드", () => p.evaluate(() => document.documentElement.hasAttribute("data-keyboard")));
            await sleep(300);
          }
        };
        const reset = async () => {
          await p.goto(base);
          await select("ipadA");
          await input.evaluate((box) => {
            box.value = "";
          });
          sent.length = 0;
        };
        const fresh = (jamo) => ipadKey(p, jamo, [["insert", jamo]]);
        const replace = (jamo, result) => ipadKey(p, jamo, [["delete"], ["insert", result]]);
        const text = () => sent.map((f) => f.data).join("");
        const check = async (expected, what) => {
          await input.press("Control+g");
          await until(`${what}: ^G`, () => sent.at(-1)?.data === "\x07");
          if (text() !== expected + "\x07") bad.push(`${label} ${what}: ${JSON.stringify(text())}(기대 ${JSON.stringify(expected + "\x07")})`);
          if (!sent.every((f) => f.pane === "ipadA")) bad.push(`${label} ${what}: 다른 pane으로 갔다 ${JSON.stringify(sent)}`);
        };
        await reset();
        await input.press("a");
        await input.press("b");
        await input.press("c");
        if ((await fresh("ㅎ")) !== true) bad.push(`${label}: 한글 keydown을 편집기에 맡기지 않았다`);
        await replace("ㅏ", "하");
        await replace("ㄴ", "한");
        if (text() !== "abc") bad.push(`${label}: 조합 중 자모를 보냈다 ${JSON.stringify(text())}`);
        await fresh("ㄱ");
        await replace("ㅡ", "그");
        await replace("ㄹ", "글");
        await input.press("Space");
        await check("abc한글 ", "영문 뒤 한글은 음절마다 한 번씩 순서대로");
        // 조합 중 글자는 그 줄의 확정 글자 크기로 그린다
        toPage("\r\n한 ");
        await p.waitForFunction(() => [...document.querySelectorAll(".xterm-rows span")].some((span) => span.textContent?.includes("한")));
        await fresh("ㄱ");
        await replace("ㅡ", "그");
        const sizes = await p.evaluate(() => {
          const row = [...document.querySelectorAll(".xterm-rows span")].find((span) => span.textContent?.includes("한"));
          const view = document.querySelector(".composition-view");
          return { row: getComputedStyle(row).fontSize, view: getComputedStyle(view).fontSize, pending: view.classList.contains("active") ? view.textContent : null };
        });
        if (sizes.pending !== "그") bad.push(`${label}: 조합 중 글자 ${JSON.stringify(sizes.pending)}`);
        else if (Math.abs(parseFloat(sizes.view) - parseFloat(sizes.row)) >= 0.01) bad.push(`${label}: 조합 글자 크기 ${sizes.view} ≠ 줄 ${sizes.row}`);

        await reset();
        // 망 다음 ㅏ: 받침 ㅇ이 넘어가며 편집기가 두 음절을 한 번에 바꿔 넣는다
        await fresh("ㅁ");
        await replace("ㅏ", "마");
        await replace("ㅇ", "망");
        await replace("ㅏ", "마아");
        await input.press("Space");
        await check("마아 ", "두 음절 바꿔 넣기");

        for (const [key, expected] of [["Enter", "\r"], ["Control+c", "\x03"], ["Tab", "\t"]]) {
          await reset();
          await fresh("ㅎ");
          await replace("ㅏ", "하");
          await replace("ㄴ", "한");
          await input.press(key);
          await check("한" + expected, `${key} 앞 확정`);
        }

        await reset();
        await fresh("ㅎ");
        await replace("ㅏ", "하");
        await replace("ㄴ", "한");
        await input.evaluate((box) => box.blur());
        await until("blur 확정", () => sent.length > 0);
        await input.focus();
        await check("한", "blur가 조합 중 글자를 한 번 확정");

        await reset();
        await fresh("ㅎ");
        await replace("ㅏ", "하");
        await select("ipadB");
        await input.press("Control+g");
        await until("다른 pane ^G", () => sent.at(-1)?.data === "\x07");
        if (text() !== "\x07" || sent.some((f) => f.pane !== "ipadB"))
          bad.push(`${label}: 조합 중 글자가 다른 pane으로 갔다 ${JSON.stringify(sent)}`);
        console.log(`     iPad IME 회귀 ${label}: ${bad.length ? "실패 있음" : "통과"}`);
        await p.goto("about:blank").catch(() => {});
        await p.context().close();
      }
    } finally {
      await wk.close();
    }
    for (const n of ["ipadA", "ipadB"]) aaa("pane", "kill", n);
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("입력 기록(실기 키보드 진단): 켜면 터미널 사건·textarea 값·화면 키보드 상태·보낸 글을 모으고, 보내면 host diag/에 남는다", async () => {
    // pairing 없이는 남기지 않는다
    const anon = await fetch(`${base}api/v1/diag/input`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: base.slice(0, -1) },
      body: "{}",
    });
    if (anon.status !== 401) throw new Error(`인증 없는 기록 보내기가 ${anon.status}`);
    aaa("tab", "create", "--machine", "labA", "--label", "traceapp", "--", process.execPath, `${root}/paste.js`);
    const tp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    // 목록에는 진단 단추가 없고(참고 목록에 그런 자리가 없다) 설정 → 터미널 쪽 맨 끝 「진단」에 있다(사용자 화면 10-18, plan §8.82·§8.97)
    if ((await tp.getByText("입력 기록", { exact: false }).count()) !== 0) throw new Error("목록에 입력 기록 단추가 있다");
    const dialog = tp.getByRole("dialog", { name: "설정" });
    const traceSwitch = dialog.getByRole("switch", { name: "입력 기록" });
    await openSettings(tp, "터미널");
    await traceSwitch.click();
    if ((await traceSwitch.getAttribute("aria-checked")) !== "true") throw new Error("입력 기록이 켜지지 않았다");
    await tp.keyboard.press("Escape");
    await tp.getByTestId("pane-traceapp").click();
    await until("trace 화면", async () => (await screenText(tp)).includes("paste>"));
    await tp.locator(".xterm-screen").click();
    await imeType(tp);
    await waitOutput("traceapp", 'GOT:"상"');
    await sleep(300);
    await tp.getByRole("button", { name: "목록으로" }).click();
    await openSettings(tp, "터미널");
    await dialog.getByTestId("trace-send").click();
    await until("보냄 표시", async () => (await dialog.getByTestId("trace-msg").innerText()).startsWith("보냈다: input-"));
    const dir = `${root}/s/host/diag`;
    const files = fs.readdirSync(dir).filter((n) => /^input-\d+\.json$/.test(n));
    if (files.length !== 1) throw new Error(`diag 파일 ${files}`);
    const rec = JSON.parse(fs.readFileSync(path.join(dir, files[0]), "utf8"));
    const types = new Set(rec.events.map((e) => e.type));
    for (const t of ["keydown", "compositionstart", "compositionupdate", "compositionend", "input", "focus", "send"])
      if (!types.has(t)) throw new Error(`기록에 ${t}가 없다: ${[...types]}`);
    const sent = rec.events.filter((e) => e.type === "send").map((e) => e.data).join("");
    if (sent !== "상태") throw new Error(`기록의 보낸 글 ${JSON.stringify(sent)}`);
    const upd = rec.events.find((e) => e.type === "compositionupdate" && e.data === "사");
    if (!upd || typeof upd.value !== "string" || !Array.isArray(upd.sel) || typeof upd.kb !== "boolean") throw new Error(`상태가 빠졌다 ${JSON.stringify(upd)}`);
    // 친 글 원문이 들어 있다: 사용자 전용 파일이고, 다른 사이트 폼이 보낼 수 있는 text/plain과 보기 권한 기기는 받지 않는다(보안 감사 t-0340 A6·B8)
    const mode = fs.statSync(path.join(dir, files[0])).mode & 0o777;
    if (mode !== 0o600) throw new Error(`diag 파일 권한 ${mode.toString(8)}`);
    const post = (p, type) =>
      p.evaluate(async (type) => (await fetch("/api/v1/diag/input", { method: "POST", headers: { "content-type": type }, body: "{}" })).status, type);
    if ((await post(tp, "text/plain")) !== 400) throw new Error("text/plain 기록을 받았다");
    const rd = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 })).page;
    if ((await post(rd, "application/json")) !== 403) throw new Error("보기 권한 기기의 기록을 받았다");
    await rd.context().close();
    if (fs.readdirSync(dir).filter((n) => /^input-\d+\.json$/.test(n)).length !== 1) throw new Error("거절한 기록이 남았다");
    // 보내면 비우고, 끄면 모으지 않는다
    await until("보낸 뒤 비움", async () => (await dialog.locator(".settings-row:has([role=switch][aria-label='입력 기록'])").innerText()).includes("0개 모았습니다"));
    await traceSwitch.click();
    if ((await traceSwitch.getAttribute("aria-checked")) !== "false") throw new Error("입력 기록이 꺼지지 않았다");
    await tp.context().close();
    aaa("pane", "kill", "traceapp");
  });
}
