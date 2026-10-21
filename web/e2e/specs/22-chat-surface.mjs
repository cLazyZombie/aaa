// 웹 E2E 대화 보기의 바탕·끌기·폭(사용자 10-18): 기록과 입력창 둘레 띠는 한 색이고(참고 herdr-web-ui ChatView.css 「chat mode is ONE
// surface」), iPad에서 입력창을 끌어도 문서가 따라가지 않으며(style.css html·body overscroll, pageHold.ts) 대화와 입력칸 글은 그대로 굴러간다.
// 긴 경로·inline code·URL은 줄을 바꿔 대화 칸이 가로로 굴러가지 않고, code block만 제 상자 안에서 가로로 굴러간다.
// iOS 문서 고무줄 자체는 headless가 그리지 않는다: 그 조건(계산된 overscroll-behavior)과 막는 손(touchmove preventDefault)을 본다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 22-chat-surface(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import { aaa, until, step, browser, webkit, pairPage, inList, openSettings, domIs, shot, shots, fakeViewport, softKeyboard, root, AA, waitOutput, paneHash } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

/** 긴 대화 기록을 가진 Claude pane(hook으로 기록 자리를 알린다, 11-agent-panes 「대화 기록」과 같은 길) */
function chatPane(label, session) {
  const dir = `${root}/claude/projects/-lab-${label}`;
  fs.mkdirSync(dir, { recursive: true });
  const tr = `${dir}/s1.jsonl`;
  const L = (o) => JSON.stringify(o) + "\n";
  let body = "";
  for (let i = 0; i < 30; i++)
    body += L({ type: "user", message: { role: "user", content: `질문 ${i}` } }) + L({ type: "assistant", message: { content: [{ type: "text", text: `답 ${i}\n\n${"긴 줄 ".repeat(40)}` }] } });
  fs.writeFileSync(tr, body);
  aaa("tab", "create", "--machine", "labA", "--label", label, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: session, source: "startup", transcript_path: tr }));
}

/** 그 pane의 대화 보기를 열고 기록과 입력창이 그려지기를 기다린다(휴대폰은 처음부터 대화다) */
async function openChat(p, label) {
  await inList(p, `pane-${label}`);
  if ((await p.getByTestId("chat").count()) === 0) {
    await until("대화 단추", async () => (await p.getByTestId("chat-toggle").count()) === 1);
    await p.getByTestId("chat-toggle").click();
  }
  await p.getByTestId("chat-composer").waitFor();
  await until("대화 기록", async () => (await p.locator('[data-testid="chat"] .chat-msg').count()) >= 60);
}

export default async function () {
  await step("대화 보기 한 바탕(사용자 10-18 「위에 내용과 밑에 치는 곳 옆부분 색이 달라」): 휴대폰·iPad·데스크톱, 어둡게·밝게 × 색상 셋에서 기록 배경과 입력창 둘레 띠의 칠해진 색이 같고(참고 ChatView.css `.terminal-stack.is-chat .composer` transparent), 입력 상자는 따로 카드다", async () => {
    chatPane("surf1", "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b9b");
    const wk = await webkit.launch();
    const bad = [];
    try {
      const devices = [
        ["desktop", (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page],
        ["ipad", (await pairDevice(wk, aaa("web", "pair").code, "ipad")).page],
        ["phone", (await pairDevice(wk, aaa("web", "pair").code, "iphone")).page],
      ];
      for (const [, p] of devices) await openChat(p, "surf1");
      // 칠해진 색: 그 칸에서 위로 올라가며 처음 만나는 불투명한 배경(투명한 띠는 그 뒤 칸의 색이 보인다)
      const surface = (p) =>
        p.evaluate(() => {
          const paint = (el) => {
            for (; el; el = el.parentElement) {
              const c = getComputedStyle(el).backgroundColor;
              if (c !== "rgba(0, 0, 0, 0)" && c !== "transparent") return c;
            }
            return null;
          };
          const composer = document.querySelector('[data-testid="chat-composer"]');
          return { chat: paint(document.querySelector('[data-testid="chat"]')), band: paint(composer), box: paint(composer.querySelector(".composer-surface")) };
        });
      const set = async (theme, palette) => {
        const d = await openSettings(devices[0][1], "모양");
        await d.getByRole("group", { name: "테마" }).getByRole("button", { name: theme === "light" ? "밝게" : "어둡게" }).click();
        await d.locator("#settings-palette").selectOption(palette);
        await devices[0][1].keyboard.press("Escape");
        await d.waitFor({ state: "detached" });
        // 공유 설정이라 다른 기기에도 온다
        for (const [, p] of devices)
          await domIs(p, `테마 ${theme} ${palette}`, ([t, c]) => document.documentElement.dataset.theme === t && document.documentElement.dataset.palette === c, [theme, palette]);
      };
      for (const theme of ["dark", "light"])
        for (const palette of ["amber", "charcoal", "catppuccin"]) {
          await set(theme, palette);
          for (const [name, p] of devices) {
            const s = await surface(p);
            if (s.chat !== s.band) bad.push(`${name} ${theme} ${palette}: 기록 ${s.chat} ≠ 입력 띠 ${s.band}`);
            if (s.box === s.band) bad.push(`${name} ${theme} ${palette}: 입력 상자가 띠와 같은 색 ${s.box}`);
            if (palette === "amber") await shot(p, `chat-surface-${name}-${theme}`);
          }
        }
      await set("dark", "amber");
      for (const [, p] of devices) await p.context().close();
    } finally {
      await wk.close();
    }
    aaa("pane", "kill", "surf1");
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("iPad 입력창 끌기(사용자 10-18 「입력창 왼쪽을 잡고 올리면 창이 통째로 올라간다」): 문서는 고무줄·당겨 새로 읽기가 없고(html·body overscroll-behavior-y none, 대화는 contain), 입력창 둘레·짧은 글 칸을 끄는 손은 Safari 탭·홈 화면 앱·화면 키보드에서 문서로 가지 않으며, 넘치는 글 칸과 대화는 그대로 굴러간다", async () => {
    chatPane("surf2", "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b9c");
    const bad = [];
    // 끄는 자리: 입력창 왼쪽 띠, 입력 상자(글 칸) 가운데
    const spots = (p) =>
      p.evaluate(() => {
        const c = document.querySelector('[data-testid="chat-composer"]').getBoundingClientRect();
        const t = document.querySelector(".composer-text").getBoundingClientRect();
        const chat = document.querySelector('[data-testid="chat"]').getBoundingClientRect();
        return { band: { x: c.left + 4, y: t.top + t.height / 2 }, box: { x: t.left + t.width / 2, y: t.top + t.height / 2 }, chat: { x: chat.left + chat.width / 2, y: chat.top + chat.height / 2 } };
      });
    // WebKit: 07처럼 손가락 목록을 붙인 같은 이름의 사건(Playwright WebKit에는 Touch 생성자가 없다). 막았는지는 dispatchEvent가 알린다
    const wkDrag = (p, at, travel) =>
      p.evaluate(
        ({ x, y0, travel }) => {
          const target = document.elementFromPoint(x, y0);
          const ev = (type, y) => {
            const t = { identifier: 9, target, clientX: x, clientY: y, pageX: x, pageY: y };
            const on = type === "touchend" ? [] : [t];
            const e = new Event(type, { bubbles: true, cancelable: true, composed: true });
            for (const [key, value] of [["touches", on], ["targetTouches", on], ["changedTouches", [t]]]) Object.defineProperty(e, key, { value });
            return e;
          };
          target.dispatchEvent(ev("touchstart", y0));
          const prevented = [];
          for (let i = 1; i <= 6; i++) prevented.push(!target.dispatchEvent(ev("touchmove", y0 + (travel * i) / 6)));
          target.dispatchEvent(ev("touchend", y0 + travel));
          return { on: target.className, prevented };
        },
        { x: at.x, y0: at.y, travel },
      );
    const allHeld = (r) => r.prevented.every(Boolean);
    const noneHeld = (r) => r.prevented.every((v) => !v);
    const longText = Array.from({ length: 40 }, (_, i) => `줄 ${i}`).join("\n");

    const wk = await webkit.launch();
    try {
      for (const standalone of [false, true]) {
        const where = standalone ? "홈 화면 앱" : "Safari 탭";
        const p = (await pairDevice(wk, aaa("web", "pair").code, "ipad", { standalone, init: fakeViewport })).page;
        await openChat(p, "surf2");
        const look = await p.evaluate(() => ({
          html: getComputedStyle(document.documentElement).overscrollBehaviorY,
          body: getComputedStyle(document.body).overscrollBehaviorY,
          chat: getComputedStyle(document.querySelector('[data-testid="chat"]')).overscrollBehaviorY,
        }));
        if (look.html !== "none" || look.body !== "none" || look.chat !== "contain") bad.push(`${where} overscroll ${JSON.stringify(look)}`);
        const at = await spots(p);
        for (const [what, spot] of [["띠", at.band], ["짧은 글 칸", at.box]])
          for (const travel of [-160, 160]) {
            const r = await wkDrag(p, spot, travel);
            if (!allHeld(r)) bad.push(`${where} ${what}(${r.on}) ${travel}px 끌기가 문서로 간다 ${JSON.stringify(r.prevented)}`);
          }
        // 대화 위 끌기는 대화 것이다(막지 않는다)
        const onChat = await wkDrag(p, at.chat, 160);
        if (!noneHeld(onChat)) bad.push(`${where} 대화 끌기를 막았다 ${JSON.stringify(onChat.prevented)}`);
        // 넘치는 글 칸: 맨 위에서 손가락을 올리면 글이 굴러간다(막지 않는다). 맨 위에서 내리면 굴러갈 것이 없다(막는다)
        const box = p.getByRole("textbox", { name: "메시지" });
        await box.fill(longText);
        await until("글 칸이 넘친다", () => box.evaluate((el) => el.scrollHeight > el.clientHeight + 20));
        await box.evaluate((el) => (el.scrollTop = 0));
        const tall = await spots(p);
        const up = await wkDrag(p, tall.box, -120);
        if (!noneHeld(up)) bad.push(`${where} 넘치는 글 칸 올리기를 막았다 ${JSON.stringify(up.prevented)}`);
        const down = await wkDrag(p, tall.box, 120);
        if (!allHeld(down)) bad.push(`${where} 맨 위 글 칸 내리기가 문서로 간다 ${JSON.stringify(down.prevented)}`);
        await box.fill("");
        // 화면 키보드(보이는 높이만 준다, iOS Safari): 글 칸에 focus, 올라온 뒤에도 띠 끌기는 막는다
        await box.focus();
        await softKeyboard(p, 700);
        await domIs(p, "화면 키보드", () => document.documentElement.hasAttribute("data-keyboard"));
        const kb = await spots(p);
        const r = await wkDrag(p, kb.band, -160);
        if (!allHeld(r)) bad.push(`${where} 키보드 위 띠 끌기가 문서로 간다 ${JSON.stringify(r.prevented)}`);
        if (!standalone) await shot(p, "ipad-chat-keyboard");
        await p.context().close();
      }
    } finally {
      await wk.close();
    }

    // Chromium iPad 크기의 진짜 터치(CDP): 대화와 넘치는 글 칸은 굴러가고, 띠를 끈 뒤 문서는 0에 있다
    const p = (await pairPage(browser, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true })).page;
    await openChat(p, "surf2");
    const cdp = await p.context().newCDPSession(p);
    await p.evaluate(() => {
      window.__held = [];
      window.addEventListener("touchmove", (e) => window.__held.push(e.defaultPrevented), { passive: true });
    });
    const drag = async (at, travel) => {
      await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: at.x, y: at.y, id: 1 }] });
      for (let i = 1; i <= 12; i++) await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: at.x, y: at.y + (travel * i) / 12, id: 1 }] });
      await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    };
    const page0 = () => p.evaluate(() => ({ doc: document.scrollingElement.scrollTop, y: window.scrollY, vv: window.visualViewport?.offsetTop ?? 0 }));
    const chatTop = () => p.getByTestId("chat").evaluate((el) => el.scrollTop);
    const at = await spots(p);
    const before = await chatTop();
    await drag(at.chat, 300);
    await until("대화가 굴러간다", async () => (await chatTop()) < before - 50);
    await p.evaluate(() => (window.__held.length = 0));
    await drag(at.band, -300);
    const held = await p.evaluate(() => window.__held.splice(0));
    if (!held.length || !held.every(Boolean)) bad.push(`Chromium 띠 끌기 ${JSON.stringify(held)}`);
    const pos = await page0();
    if (pos.doc !== 0 || pos.y !== 0 || pos.vv !== 0) bad.push(`띠를 끈 뒤 문서 자리 ${JSON.stringify(pos)}`);
    const box = p.getByRole("textbox", { name: "메시지" });
    await box.fill(longText);
    await until("글 칸이 넘친다", () => box.evaluate((el) => el.scrollHeight > el.clientHeight + 20));
    await box.evaluate((el) => (el.scrollTop = 0));
    await drag((await spots(p)).box, -150);
    await until("넘치는 글 칸이 굴러간다", () => box.evaluate((el) => el.scrollTop > 20));
    await box.fill("");
    await p.context().close();
    aaa("pane", "kill", "surf2");
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("대화 칸 가로 넘침 0(사용자 iPhone 10-18 19:48 「대화 보기가 좌우로 스크롤된다」): 휴대폰·iPad·데스크톱에서 도구 줄의 긴 경로, 답의 긴 경로 inline code·긴 inline code·긴 URL은 줄을 바꿔 문서와 대화 칸이 가로로 굴러가지 않고 기둥이 칸 안에 서며, code block은 제 상자 안에서만 가로로 굴러간다(참고 HighlightedCode.css `.hl-code` overflow-x auto)", async () => {
    const dir = `${root}/claude/projects/-lab-wide`;
    fs.mkdirSync(dir, { recursive: true });
    const tr = `${dir}/s1.jsonl`;
    const L = (o) => JSON.stringify(o) + "\n";
    const deep = `/srv/aaa-work/shadow/t-0002/.aaa/shadow-t-0002/library/${"deeply-nested-folder/".repeat(4)}check.md`;
    const answer = [
      "brief의 할 일을 끝냈습니다.",
      "",
      `- \`.aaa/shadow-t-0002/library/${"long-segment-".repeat(4)}check.md\` 에 한 문장을 썼습니다.`,
      `- \`${"aaa_report_percent_".repeat(6)}done\` 같은 긴 inline code도 있다.`,
      `- 링크 https://example.com/${"very-long-path-segment/".repeat(6)}index.html 를 본다.`,
      "",
      "```rust",
      `fn main() { println!("${"wide ".repeat(40)}"); }`,
      "```",
      "",
      "```",
      deep,
      "```",
    ].join("\n");
    fs.writeFileSync(
      tr,
      L({ type: "user", message: { role: "user", content: `이 경로를 봐 ${deep}` } }) +
        L({ type: "assistant", message: { content: [{ type: "tool_use", name: "Write", input: { file_path: deep, content: "x" } }] } }) +
        L({ type: "assistant", message: { content: [{ type: "text", text: answer }] } }),
    );
    aaa("tab", "create", "--machine", "labA", "--label", "wide1", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
      JSON.stringify({ hook_event_name: "SessionStart", session_id: "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b9d", source: "startup", transcript_path: tr }));
    const wk = await webkit.launch();
    const bad = [];
    try {
      const devices = [
        ["desktop", (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page],
        ["ipad", (await pairDevice(wk, aaa("web", "pair").code, "ipad")).page],
        ["phone", (await pairDevice(wk, aaa("web", "pair").code, "iphone")).page],
      ];
      for (const [name, p] of devices) {
        await inList(p, "pane-wide1");
        if ((await p.getByTestId("chat").count()) === 0) {
          await until("대화 단추", async () => (await p.getByTestId("chat-toggle").count()) === 1);
          await p.getByTestId("chat-toggle").click();
        }
        await until("대화 기록", async () => (await p.locator('[data-testid="chat"] .chat-msg').count()) === 3);
        // 글 속 경로는 누르면 여는 단추다(조작 기기): 넘침을 보는 자리가 실제로 그려졌다
        await p.locator('[data-testid="chat"] .markdown-file').first().waitFor();
        await p.locator('[data-testid="chat"] .markdown-code').first().waitFor();
        const got = await p.evaluate(() => {
          const chat = document.querySelector('[data-testid="chat"]');
          const box = chat.getBoundingClientRect();
          const se = document.scrollingElement;
          const out = [];
          if (se.scrollWidth > se.clientWidth) out.push(`문서 ${se.scrollWidth}>${se.clientWidth}`);
          if (chat.scrollWidth > chat.clientWidth) out.push(`대화 칸 ${chat.scrollWidth}>${chat.clientWidth}`);
          const col = chat.querySelector(".chat-transcript").getBoundingClientRect();
          if (col.left < box.left - 0.5 || col.right > box.right + 0.5) out.push(`기둥 ${Math.round(col.left)}~${Math.round(col.right)} 칸 ${Math.round(box.left)}~${Math.round(box.right)}`);
          // 기둥 밖으로 나간 글 조각 가운데 가장 안쪽 것들(코드 상자 안은 그 상자가 굴린다). 자리는 요소 경로와 white-space로 말한다
          const over = [...chat.querySelectorAll(".chat-msg, .chat-msg *")].filter((el) => {
            const r = el.getBoundingClientRect();
            return !el.closest(".hl-code") && r.width > 0 && r.right > col.right + 0.5;
          });
          const innermost = over.filter((el) => !over.some((o) => o !== el && el.contains(o)));
          const seen = new Set();
          for (const el of innermost) {
            const path = [];
            for (let at = el; at && at !== chat; at = at.parentElement) path.push(`${at.tagName.toLowerCase()}${at.className ? `.${String(at.className).split(" ")[0]}` : ""}(${getComputedStyle(at).whiteSpace})`);
            const key = path.slice(0, 3).join(" < ");
            if (!seen.has(key)) seen.add(key) && out.push(`넘친 ${key} 오른쪽 ${Math.round(el.getBoundingClientRect().right)} > ${Math.round(col.right)}`);
          }
          const codes = [...chat.querySelectorAll(".markdown-code .hl-code")];
          const wide = codes.filter((c) => c.scrollWidth > c.clientWidth && /(auto|scroll)/.test(getComputedStyle(c).overflowX));
          if (wide.length === 0) out.push(`code block이 제 상자에서 굴러가지 않는다 ${codes.map((c) => `${c.scrollWidth}/${c.clientWidth} ${getComputedStyle(c).overflowX}`)}`);
          return out;
        });
        for (const g of got) bad.push(`${name}: ${g}`);
        await shot(p, `chat-wide-${name}`);
      }
      for (const [, p] of devices) await p.context().close();
    } finally {
      await wk.close();
    }
    aaa("pane", "kill", "wide1");
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("대화 입력창 높이: 휴대폰 가로·세로·데스크톱에서 짧으면 한 줄, 줄바꿈·자동 줄넘김이면 높이만 늘고 단추는 옆에 유지(t-0563 사용자 1A), 10줄에서 멈춰 굴러가며 지우면 처음 높이로 돌아온다. focus·아래끝은 그대로다", async () => {
    chatPane("line1", "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b9e");
    const bad = [];
    // 그 순간의 입력창: 판정 표지(data-lines), 상자·글 칸·단추 자리, 한 줄 높이, focus, 글 칸 안에서 잘린 것
    const look = (p) =>
      p.evaluate(() => {
        const surface = document.querySelector('[data-testid="chat-composer"] .composer-surface');
        const text = surface.querySelector(".composer-text");
        const send = surface.querySelector(".composer-send");
        const r = (el) => { const b = el.getBoundingClientRect(); return { top: b.top, bottom: b.bottom, left: b.left, right: b.right, height: b.height, mid: b.top + b.height / 2 }; };
        return {
          lines: surface.dataset.lines,
          surface: r(surface), text: r(text), send: r(send),
          lh: parseFloat(getComputedStyle(text).lineHeight),
          room: parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--app-height")) || innerHeight,
          focused: document.activeElement === text,
          scroll: { h: text.scrollHeight, ch: text.clientHeight, w: text.scrollWidth, cw: text.clientWidth },
          pageWide: document.scrollingElement.scrollWidth > innerWidth,
        };
      });
    const wk = await webkit.launch();
    try {
      const phone = (await pairDevice(wk, aaa("web", "pair").code, "iphone")).page;
      const desk = (await pairPage(browser, aaa("web", "pair").code, { width: 1920, height: 1080 })).page;
      for (const [name, p, size] of [["phone-landscape", phone, { width: 874, height: 402 }], ["phone-portrait", phone, { width: 402, height: 874 }], ["desktop", desk, null]]) {
        if (size) await p.setViewportSize(size);
        await openChat(p, "line1");
        const box = p.locator('[data-testid="chat-composer"] .composer-text');
        await box.click();
        await box.fill("");
        // 표지가 바뀐 횟수: 한 번 입력에 한 번만 바뀐다(한 줄 ↔ 여러 줄이 오가며 튀지 않는다)
        await p.evaluate(() => {
          const surface = document.querySelector('[data-testid="chat-composer"] .composer-surface');
          window.__lineFlips = [];
          window.__lineWatch?.disconnect();
          window.__lineWatch = new MutationObserver(() => window.__lineFlips.push(surface.dataset.lines));
          window.__lineWatch.observe(surface, { attributes: true, attributeFilter: ["data-lines"] });
        });
        const capture = async (state) => shots && (await p.getByTestId("chat-composer").screenshot({ path: `${shots}/composer-${name}-${state}.png`, animations: "disabled" }));
        const one = async (state) => {
          await domIs(p, `${name} ${state} 한 줄`, () => document.querySelector('[data-testid="chat-composer"] .composer-surface')?.dataset.lines === "one").catch((e) => bad.push(e.message));
          await capture(state);
          const s = await look(p);
          if (s.lines !== "one") bad.push(`${name} ${state}: 한 줄 표지가 아니다(${s.lines})`);
          if (Math.abs(s.send.mid - s.text.mid) > 1) bad.push(`${name} ${state}: 단추 가운데 ${Math.round(s.send.mid)} ≠ 글 칸 가운데 ${Math.round(s.text.mid)}`);
          if (s.send.left < s.text.right - 0.5) bad.push(`${name} ${state}: 단추(${Math.round(s.send.left)})가 글 칸(~${Math.round(s.text.right)})과 겹친다`);
          if (s.text.height > s.lh * 2) bad.push(`${name} ${state}: 글 칸 ${Math.round(s.text.height)}px이 한 줄(${s.lh}px) 높이가 아니다`);
          if (s.scroll.h > s.scroll.ch + 1 || s.scroll.w > s.scroll.cw + 1) bad.push(`${name} ${state}: 글 칸 안에서 잘린다 ${JSON.stringify(s.scroll)}`);
          return s;
        };
        const more = async (state, lines) => {
          await domIs(p, `${name} ${state} 여러 줄`, () => document.querySelector('[data-testid="chat-composer"] .composer-surface')?.dataset.lines === "more").catch((e) => bad.push(e.message));
          await capture(state);
          const s = await look(p);
          if (s.lines !== "more") bad.push(`${name} ${state}: 여러 줄 표지가 아니다(${s.lines})`);
          if (s.send.left < s.text.right - 0.5 || s.send.top >= s.text.bottom - 0.5) bad.push(`${name} ${state}: 단추가 글 칸 옆에 있지 않다`);
          // 한도: 10줄, 보이는 높이의 절반(휴대폰 가로)
          const cap = Math.min(s.lh * 10, s.room / 2);
          if (lines > 10 ? Math.abs(s.text.height - cap) > 1 : s.text.height < s.lh * lines || s.text.height > s.lh * (lines + 2))
            bad.push(`${name} ${state}: 글 칸 ${Math.round(s.text.height)}px이 ${lines > 10 ? `한도 ${Math.round(cap)}px` : `${lines}줄(${s.lh}px씩)`}이 아니다`);
          if (lines > 10 ? s.scroll.h <= s.scroll.ch : s.scroll.h > s.scroll.ch + 1) bad.push(`${name} ${state}: 굴림이 맞지 않다 ${JSON.stringify(s.scroll)}`);
          return s;
        };
        const empty = await one("empty");
        await p.keyboard.type("짧은 글");
        const short = await one("short");
        await p.keyboard.press("Shift+Enter");
        await p.keyboard.type("둘째 줄");
        const two = await more("two-lines", 2);
        for (let i = 3; i <= 14; i++) {
          await p.keyboard.press("Shift+Enter");
          await p.keyboard.type(`${i}째 줄`);
        }
        const many = await more("many-lines", 14);
        await box.fill("");
        // 상자 폭의 한 배 반쯤 되는 한 문단: 두 줄로 넘어간다
        const paragraph = await p.evaluate(() => {
          const text = document.querySelector('[data-testid="chat-composer"] .composer-surface').clientWidth;
          const ctx = document.createElement("canvas").getContext("2d");
          ctx.font = getComputedStyle(document.querySelector(".composer-text")).font;
          let out = "";
          while (ctx.measureText(out).width < text * 1.5) out += "줄바꿈 없이 길게 쓰는 글 ";
          return out;
        });
        await p.keyboard.type(paragraph);
        const wrapped = await more("wrapped", 2);
        await p.keyboard.press("ControlOrMeta+a");
        await p.keyboard.press("Backspace");
        const cleared = await one("cleared");
        for (const [state, s] of Object.entries({ empty, short, two, many, wrapped, cleared })) {
          if (Math.abs((s.text.right - s.text.left) - (empty.text.right - empty.text.left)) > 0.5) bad.push(`${name} ${state}: 줄 수에 따라 글 칸 폭이 바뀌었다`);
          if (!s.focused) bad.push(`${name} ${state}: 글 칸이 focus를 잃었다`);
          if (s.pageWide) bad.push(`${name} ${state}: 화면이 가로로 넘친다`);
          if (Math.abs(s.surface.bottom - empty.surface.bottom) > 0.5) bad.push(`${name} ${state}: 상자 아래끝 ${Math.round(s.surface.bottom)} ≠ ${Math.round(empty.surface.bottom)}`);
        }
        if (Math.abs(short.surface.height - empty.surface.height) > 0.5 || Math.abs(cleared.surface.height - empty.surface.height) > 0.5)
          bad.push(`${name}: 한 줄 상자 높이가 다르다 ${[empty, short, cleared].map((s) => Math.round(s.surface.height))}`);
        const flips = await p.evaluate(() => window.__lineFlips);
        if (flips.join() !== "more,one,more,one") bad.push(`${name}: 표지가 튄다 ${flips}`);
        // t-0546의 두 폭 사이 글에도 단추만 있는 밑 줄이 없어야 한다. t-0563 사용자 1A에 따라
        // 이제 모든 길이에서 단추 옆 폭을 유지하고, 표지는 그 실제 폭의 줄 수를 나타낸다.
        const between = await p.evaluate(() => {
          const surface = document.querySelector('[data-testid="chat-composer"] .composer-surface');
          const text = surface.querySelector(".composer-text");
          const cs = getComputedStyle(text);
          const pad = parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight);
          const beside = text.clientWidth - pad;
          const full = surface.clientWidth - (text.offsetWidth - text.clientWidth) - pad;
          const ctx = document.createElement("canvas").getContext("2d");
          ctx.font = cs.font;
          const fill = (width) => { let out = ""; while (ctx.measureText(out + "가").width < width) out += "가나다라마 바사아자차 ".at(out.length % 12); return out; };
          return { beside, full, mid: fill((beside + full) / 2), over: fill(full * 1.2) };
        });
        const settled = (value) => p.evaluate((v) => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() =>
          done(document.querySelector('[data-testid="chat-composer"] .composer-text').value === v)))), value);
        for (const [state, value, want] of [["between", between.mid, "more"], ["over", between.over, "more"], ["between-again", between.mid, "more"]]) {
          await box.fill(value);
          if (!(await settled(value))) bad.push(`${name} ${state}: 글이 들어가지 않았다`);
          await capture(state);
          const s = await look(p);
          if (s.lines !== want) bad.push(`${name} ${state}: 표지 ${s.lines} ≠ ${want}(단추 옆 ${Math.round(between.beside)}px·펼친 ${Math.round(between.full)}px)`);
          if (s.send.top >= s.text.bottom - 0.5 || s.send.left < s.text.right - 0.5) bad.push(`${name} ${state}: 단추가 글 칸 옆에 있지 않다`);
          if (Math.abs((s.text.right - s.text.left) - (empty.text.right - empty.text.left)) > 0.5) bad.push(`${name} ${state}: 글 칸 폭이 바뀌었다`);
          if (s.scroll.h > s.scroll.ch + 1 || s.scroll.w > s.scroll.cw + 1) bad.push(`${name} ${state}: 글 칸 안에서 잘린다 ${JSON.stringify(s.scroll)}`);
          if (!s.focused) bad.push(`${name} ${state}: 글 칸이 focus를 잃었다`);
          if (Math.abs(s.surface.bottom - empty.surface.bottom) > 0.5) bad.push(`${name} ${state}: 상자 아래끝 ${Math.round(s.surface.bottom)} ≠ ${Math.round(empty.surface.bottom)}`);
        }
      }
      await phone.context().close();
      await desk.context().close();
    } finally {
      await wk.close();
    }
    aaa("pane", "kill", "line1");
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("대화 맨 아래 붙음(사용자 10-19 14:0x 「스크롤이 되지 않는 경우가 있어」): 맨 아래를 보던 대화는 입력창이 여러 줄로 커져도(굴림 사건 없이 보기가 줄어도) 맨 아래이고 새 답도 따라가며, 위로 올려 읽는 중이면 입력창이 커지거나 새 답이 와도 끌어내리지 않는다(참고 herdr-web-ui ChatView.tsx ResizeObserver·onScroll)", async () => {
    chatPane("stick1", "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b9f");
    const tr = `${root}/claude/projects/-lab-stick1/s1.jsonl`;
    const L = (o) => JSON.stringify(o) + "\n";
    const p = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await openChat(p, "stick1");
    const chat = p.getByTestId("chat");
    // 두 번 그린 뒤의 아래까지 거리와 위치(ResizeObserver는 그리기 전에 돈다)
    const at = () => chat.evaluate((el) => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() =>
      done({ gap: Math.round(el.scrollHeight - el.scrollTop - el.clientHeight), top: Math.round(el.scrollTop) })))));
    const box = p.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" });
    let answers = 0;
    const answer = async () => {
      const before = await p.locator('[data-testid="chat"] .chat-msg').count();
      fs.appendFileSync(tr, L({ type: "assistant", message: { content: [{ type: "text", text: `새 답 ${++answers}\n\n${"- 항목\n".repeat(8)}` }] } }));
      await until("새 답", async () => (await p.locator('[data-testid="chat"] .chat-msg').count()) > before);
    };
    const bad = [];
    if ((await at()).gap > 1) bad.push(`처음부터 맨 아래가 아니다 ${JSON.stringify(await at())}`);
    await box.fill("가\n나\n다\n라\n마\n바");
    const grew = await at();
    if (grew.gap > 1) bad.push(`입력창이 커지자 마지막 ${grew.gap}px가 가려졌다`);
    await box.fill("");
    await answer();
    if ((await at()).gap > 1) bad.push("새 답을 따라가지 않았다");
    // 위로 올려 읽는 중: 끌어내리지 않는다
    await chat.hover();
    const bottom = (await at()).top;
    await p.mouse.wheel(0, -400);
    await until("위로 굴렀다", async () => (await at()).top < bottom - 100);
    const reading = await at();
    await box.fill("가\n나\n다\n라\n마\n바");
    if ((await at()).top !== reading.top) bad.push(`읽는 중에 입력창이 커지자 자리가 바뀌었다 ${reading.top} → ${(await at()).top}`);
    await box.fill("");
    await answer();
    if ((await at()).top !== reading.top) bad.push(`읽는 중에 새 답이 와서 끌어내렸다 ${reading.top} → ${(await at()).top}`);
    await p.context().close();
    aaa("pane", "kill", "stick1");
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("coordinator 입력창(사용자 10-19 「코디네이터 입력 창은 보라색으로 테두리가 나오면」, t-0437 3안): coordinator pane의 대화 입력창만 보라 테두리이고 빛이 transform으로 돌며, 포커스에도 보라를 지킨다. thread·일반 pane 입력창은 그대로다. 탭이 숨으면 빛이 멈췄다 돌아오면 다시 돌고, 움직임 줄이기면 빛 없이 테두리만 남는다. 데스크톱(Chromium)·iPhone(WebKit)", async () => {
    // coordinator: fake profile pane 안에서 Claude hook을 불러 대화 보기가 되게 한다(post-probe는 fake.sh가 그 pane에서 돌리는 script)
    aaa("project", "create", "glow");
    const coord = aaa("project", "open", "glow", "--profile", "fake").pane.id;
    await waitOutput(coord, "fake>");
    const dir = `${root}/claude/projects/-lab-glow`;
    fs.mkdirSync(dir, { recursive: true });
    const L = (o) => JSON.stringify(o) + "\n";
    fs.writeFileSync(`${dir}/s1.jsonl`, L({ type: "user", message: { role: "user", content: "다음 일 맡겨 줘" } }) + L({ type: "assistant", message: { content: [{ type: "text", text: "thread를 시작했습니다." }] } }));
    const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "6e5d4c3b-2a19-4f08-8e7d-6c5b4a392817", source: "startup", transcript_path: `${dir}/s1.jsonl` });
    fs.writeFileSync(`${dir}/hook.sh`, `'${AA}' hook --agent claude '${hook}'\nprintf 'glow-hooked\\n'\n`);
    aaa("pane", "run", coord, `post-probe ${dir}/hook.sh`);
    await waitOutput(coord, "glow-hooked");
    chatPane("glow1", "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0ba0");
    const look = (p) =>
      p.evaluate(() => {
        const s = document.querySelector('[data-testid="chat-composer"] .composer-surface');
        const glow = s.querySelector(":scope > .composer-glow");
        const spin = document.getAnimations().filter((a) => a.animationName === "coordinator-glow");
        const probe = document.createElement("i");
        probe.style.color = "var(--coordinator)";
        document.body.append(probe);
        const purple = getComputedStyle(probe).color;
        probe.remove();
        return {
          coordinator: s.classList.contains("is-coordinator"),
          border: getComputedStyle(s).borderTopColor,
          purple,
          glow: glow !== null,
          paused: glow?.hasAttribute("data-paused") ?? null,
          spin: spin.map((a) => a.playState),
          props: [...new Set(spin.flatMap((a) => a.effect.getKeyframes().flatMap((k) => Object.keys(k).filter((x) => !["offset", "computedOffset", "easing", "composite"].includes(x)))))],
        };
      });
    const open = async (p) => {
      if ((await p.getByTestId("chat-composer").count()) === 0) await p.getByTestId("chat-toggle").click();
      await p.getByTestId("chat-composer").waitFor();
    };
    const bad = [];
    const wk = await webkit.launch();
    try {
      const plain = paneHash("glow1");
      const pages = (hash) => [
        ["desktop", async () => (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash })).page],
        ["phone", async () => (await pairDevice(wk, aaa("web", "pair").code, "iphone", { hash })).page],
      ];
      for (const [name, make] of pages(`#pane=${encodeURIComponent(coord)}`)) {
        const p = await make();
        await open(p);
        await domIs(p, "coordinator 입력창", () => document.querySelector('[data-testid="chat-composer"] .composer-surface.is-coordinator') !== null);
        let got = await look(p);
        if (got.border !== got.purple || !got.glow || got.paused || got.spin.join() !== "running" || got.props.join() !== "transform")
          bad.push(`${name} coordinator ${JSON.stringify(got)}`);
        const box = p.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" });
        await box.focus();
        got = await look(p);
        if (got.border !== got.purple) bad.push(`${name} 포커스에 보라가 아니다 ${JSON.stringify(got)}`);
        await box.blur();
        // 탭이 숨음: 브라우저의 visibilitychange를 흉내 낸다(headless 탭은 늘 보인다)
        const hide = (hidden) => p.evaluate((hidden) => {
          Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (hidden ? "hidden" : "visible") });
          document.dispatchEvent(new Event("visibilitychange"));
        }, hidden);
        await hide(true);
        await domIs(p, "숨은 탭 표지", () => document.querySelector(".composer-glow")?.hasAttribute("data-paused"));
        got = await look(p);
        if (got.spin.join() !== "paused") bad.push(`${name} 숨은 탭에서 빛이 돈다 ${JSON.stringify(got)}`);
        await hide(false);
        await domIs(p, "보인 탭 표지", () => !document.querySelector(".composer-glow")?.hasAttribute("data-paused"));
        got = await look(p);
        if (got.spin.join() !== "running") bad.push(`${name} 다시 보여도 빛이 서 있다 ${JSON.stringify(got)}`);
        await p.emulateMedia({ reducedMotion: "reduce" });
        got = await look(p);
        if (got.spin.length !== 0 || got.border !== got.purple) bad.push(`${name} 움직임 줄이기 ${JSON.stringify(got)}`);
        await p.emulateMedia({ reducedMotion: "no-preference" });
        // 촬영은 맨 끝에: shot(animations disabled)은 무한 애니메이션을 script로 다시 틀어, 그 뒤로는 CSS animation-play-state가 먹지 않는다
        await shot(p, `coordinator-composer-${name}`);
        await p.context().close();
      }
      // 일반 pane(thread와 같은 입력창): 보라도 빛도 없다
      for (const [name, make] of pages(plain)) {
        const p = await make();
        await open(p);
        const got = await look(p);
        if (got.coordinator || got.glow || got.spin.length || got.border === got.purple) bad.push(`${name} 일반 pane ${JSON.stringify(got)}`);
        await p.context().close();
      }
    } finally {
      await wk.close();
    }
    aaa("pane", "kill", "glow1");
    if (bad.length) throw new Error(bad.join("; "));
  });
}
