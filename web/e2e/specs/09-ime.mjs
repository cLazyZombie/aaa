// 웹 E2E 한글 IME: 데스크톱·Safari·iPad 기록 재생·iPhone 기록·입력 기록.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 09-ime(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import path from "node:path";
import {
  webkit, root, sleep, aaa, waitOutput, until, screenText, termDiag, imeType, safariEdit, safariSyllable,
  iphoneKey, ipadKey, ipadFresh, fakeViewport, softKeyboard, ipadReplace, pairPage, base, step, browser,
  openSettings, inList, sentFrames, sentCount, sentText, domIs,
} from "../lib/harness.mjs";
import { compositionOutput } from "../lib/composition-output.mjs";

export default async function ({ ctl, page }) {
  await step("한글 조합 위치 유지: Chromium·WebKit 실제 IME와 분리된 상태 줄 출력 frame", compositionOutput);

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
        await inList(p, `pane-${pane}`);
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
          const from = await sentCount(p);
          await type(p);
          // 700ms 뒤면 PTY에 도착한다는 어림 대신 송신 전체와 실제 수신 전체를 각각 확인한다.
          // 늦은 입력이 다음 MARK 뒤에 섞이지 않도록 이 사례의 수신을 마친 뒤 넘어간다.
          let sent = "";
          await until(`${engine} ${how} 입력 송신`, async () => {
            sent = sentText(await sentFrames(p, from));
            return sent.length >= want.length;
          });
          if (sent !== want) bad.push(`${engine} ${how}: 화면이 보낸 글 ${JSON.stringify(sent)}(기대 ${JSON.stringify(want)})`);
          let got = "";
          await until(`${engine} ${how} PTY 수신`, () => {
            const all = aaa("pane", "read", pane, "--source", "visible").text;
            got = all
              .slice(all.lastIndexOf('GOT:"MARK"') + 10)
              .split("\n")
              .filter((l) => l.startsWith("GOT:"))
              .map((l) => JSON.parse(l.slice(4)))
              .join("");
            return got.length >= sent.length;
          });
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
    await inList(page, "pane-t1");
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("휴대폰 영어 조합은 다음 키 없이 PTY 도착·자동 수정·한글 보류", async () => {
    // DEL은 JSON.stringify에도 제어 byte로 남아 터미널 화면이 버린다. 실제 stdin bytes를 파일로 보존한다.
    const received = `${root}/mobile-input.bin`;
    fs.writeFileSync(`${root}/mobile-input.js`, `
      const fs = require("node:fs");
      fs.writeFileSync(${JSON.stringify(received)}, "");
      process.stdin.setRawMode(true);
      process.stdout.write("\\x1b[?2004hpaste> ");
      process.stdin.on("data", data => fs.appendFileSync(${JSON.stringify(received)}, data));
    `);
    aaa("tab", "create", "--machine", "labA", "--label", "mobileLatin", "--", process.execPath, `${root}/mobile-input.js`);
    const wk = await webkit.launch();
    const userAgent = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 Version/27.0 Mobile/15E148 Safari/604.1";
    try {
      for (const [name, engine] of [["WebKit", wk], ["Chromium", browser]]) {
        const { page: p } = await pairPage(engine, aaa("web", "pair").code, { width: 390, height: 844 }, { userAgent, platform: "iPhone", touch: true });
        try {
          await inList(p, "pane-mobileLatin");
          await until("영어 입력 준비", async () => (await screenText(p)).includes("paste>") && await p.getByTestId("readonly-banner").count() === 0);
          const input = p.locator(".xterm-helper-textarea");
          await input.focus();
          await input.evaluate(el => { el.value = ""; });
          const offset = fs.statSync(received).size;
          const got = () => fs.readFileSync(received).subarray(offset).toString("utf8");
          const from = await sentCount(p);
          const expect = async (want) => {
            await until(`${name} 입력 송신 ${JSON.stringify(want)}`, async () => sentText(await sentFrames(p, from)).length >= want.length)
              .catch(async error => { throw new Error(`${error.message}; 실제 송신=${JSON.stringify(sentText(await sentFrames(p, from)))}`); });
            const sent = sentText(await sentFrames(p, from));
            if (sent !== want) throw new Error(`${name} 송신 ${JSON.stringify(sent)} != ${JSON.stringify(want)}`);
            await until(`${name} 실제 PTY 수신 ${JSON.stringify(want)}`, () => got().length >= want.length)
              .catch(error => { throw new Error(`${error.message}; 수신 bytes=${JSON.stringify(got())}`); });
            if (got() !== want) throw new Error(`${name} PTY bytes ${JSON.stringify(got())} != ${JSON.stringify(want)}`);
          };
          // 마지막 s의 compositionend/다음 키를 주지 않는다. 구판은 statu에 머문다.
          for (const ch of "status") {
            await input.evaluate((el, ch) => {
              if (el.value) el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: el.value.at(-1) }));
              el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "Unidentified", keyCode: 229, isComposing: true }));
              el.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
              el.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, inputType: "insertCompositionText", data: ch, isComposing: true }));
              el.dispatchEvent(new CompositionEvent("compositionupdate", { bubbles: true, data: ch }));
              el.value += ch;
              el.setSelectionRange(el.value.length, el.value.length);
              el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertCompositionText", data: ch, isComposing: true }));
              el.dispatchEvent(new KeyboardEvent("keyup", { bubbles: true, keyCode: 229 }));
            }, ch);
          }
          await expect("status");
          await input.dispatchEvent("compositionend", { data: "s" });
          // 예측 교정은 이미 보낸 status의 꼬리 us를 지우고 e로 바꾼다.
          await input.evaluate(el => {
            el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "Unidentified", keyCode: 229 }));
            el.setSelectionRange(0, el.value.length);
            el.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, inputType: "insertReplacementText", data: "state" }));
            el.value = "state";
            el.setSelectionRange(5, 5);
            el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertReplacementText", data: "state" }));
            el.dispatchEvent(new KeyboardEvent("keyup", { bubbles: true, keyCode: 229 }));
          });
          let want = "status\x7f\x7fe";
          await expect(want);
          await input.press("Space"); want += " ";
          await input.press("Backspace"); want += "\x7f";
          await input.press("Enter"); want += "\r";
          await expect(want);
          // 실제 composition 사건과 iPhone의 사건 없는 한글 replacement 둘 다 보류한다.
          await input.evaluate(el => {
            el.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
            for (const text of ["ㅎ", "하", "한"]) {
              el.dispatchEvent(new CompositionEvent("compositionupdate", { bubbles: true, data: text }));
              el.value = text; el.setSelectionRange(text.length, text.length);
              el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertCompositionText", data: text, isComposing: true }));
            }
          });
          // 입력 handler가 모두 끝난 상태의 정확한 bytes: 미확정 자모가 없어야 한다.
          if (sentText(await sentFrames(p, from)) !== want) throw new Error(`${name} 미확정 한글을 보냈다`);
          await input.dispatchEvent("compositionend", { data: "한" });
          await expect(want += "한");
          await input.press("Enter"); await expect(want += "\r");
          await iphoneKey(p, "ㅎ", [["insert", "ㅎ"]]);
          await iphoneKey(p, "ㅏ", [["delete"], ["insert", "하"]]);
          await iphoneKey(p, "ㄴ", [["delete"], ["insert", "한"]]);
          if (sentText(await sentFrames(p, from)) !== want) throw new Error(`${name} 한글 replacement를 확정 전에 보냈다`);
          // 붙여넣기는 보류 음절부터 확정하고 이어서 원문을 전달한다.
          await input.evaluate(el => {
            const clipboardData = new DataTransfer();
            clipboardData.setData("text/plain", "paste");
            el.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData }));
          });
          await expect(want += "한\x1b[200~paste\x1b[201~");
          await input.press("Enter"); await expect(want += "\r");
          // composition 없는 iPhone 영어 키도 정확히 한 번 전달한다.
          for (const ch of "status") await iphoneKey(p, ch, [["insert", ch]]);
          await expect(want += "status");
          console.log(`     ${name} 390px: status 마지막 글자까지 다음 키 전에 PTY 수신`);
        } finally { await p.context().close(); }
      }
    } finally { await wk.close(); }
    aaa("pane", "kill", "mobileLatin");
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

  await step("iPad IME 회귀(herdr-web-ui scripts/terminal-ipad-ime-regression.ts 그대로): Chromium MacIntel, 화면 키보드가 올라온 WebKit iPad", async () => {
    // 참고 시험처럼 pane WebSocket을 가로채 보낸 input 프레임을 그대로 본다(입력은 pane에 가지 않는다).
    // 두 pane은 참고처럼 조용한 입력창이다(출력이 오는 동안의 조합은 「터미널 한글 IME」 단계의 화면 키보드 판이 본다).
    for (const n of ["ipadA", "ipadB"]) aaa("tab", "create", "--machine", "labA", "--label", n, "--", process.execPath, `${root}/paste.js`);
    const ids = Object.fromEntries(aaa("pane", "list").panes.filter((x) => x.name.startsWith("ipad")).map((x) => [x.id, x.name]));
    const wk = await webkit.launch();
    const IPAD_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.2 Safari/605.1.15";
    const IPAD_PORTRAIT = { width: 820, height: 1180 };
    const runs = [
      ["Chromium MacIntel", () => pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { userAgent: IPAD_UA, platform: "MacIntel" }), false],
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

  let tracePage;

}
