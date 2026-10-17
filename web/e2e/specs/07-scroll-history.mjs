// 웹 E2E 스크롤과 기록: 터미널 스크롤·늦은 글꼴·줄 수 줄임·앞 기록·출력 밀림.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 07-scroll-history(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import {
  webkit, root, sleep, aaa, waitOutput, until, screenText, termDiag, shot, pairPage, base, steps, step, browser,
  openSettings, start, stop,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("터미널 스크롤(herdr-web-ui PaneTerminal 휠·터치, herdr pane_input 휠 나누기, 사용자 요청 10-17): 열기 전 기록도 굴러가고, 출력이 와도 올려 둔 자리, 키를 치면 맨 아래, alternate screen은 화살표·mouse 보고면 프로그램에, 휴대폰·iPad는 한 손가락으로 손가락만큼", async () => {
    // 터치용 alt·mouse pane은 따로 둔다(휠로 받은 조각과 섞이지 않게)
    const names = ["plain", "alt", "mouse", "touch-alt", "touch-mouse"];
    for (const mode of names) aaa("tab", "create", "--machine", "labA", "--label", `scroll-${mode}`, "--", process.execPath, `${root}/scroll.js`, mode.replace("touch-", ""));
    for (const mode of names) await waitOutput(`scroll-${mode}`, "s-200");
    const got = (name) => aaa("pane", "read", name, "--source", "visible").text;
    /** 보이는 첫 줄의 번호(s-NNN), 없으면 null */
    const firstNo = (p) =>
      p.evaluate(() => {
        const m = /s-(\d{3})/.exec(document.querySelector(".xterm-rows")?.firstElementChild?.textContent ?? "");
        return m ? Number(m[1]) : null;
      });
    const bad = [];
    const copies = () => {
      window.__copied = [];
      // xterm이 copy 사건에 넣은 글(bubble 끝에서 읽는다)
      window.addEventListener("copy", (e) => window.__copied.push(e.clipboardData?.getData("text/plain") ?? ""));
    };
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { init: copies })).page;
    // 열기 전에 쓴 줄: host가 붙을 때 기록을 채워야 위로 굴러간다(전에는 보이는 줄만 받아 굴릴 것이 없었다)
    await dp.getByTestId("pane-scroll-plain").click();
    await until("plain 화면", async () => (await screenText(dp)).includes("s-200")).catch(async (e) => {
      // CI(macos-15)에서만 실패한 적이 있다(10-17 69224fd): 그때 화면과 grid·기록 상태를 남긴다
      const seen = {
        title: await dp.locator(".term-title").textContent(),
        history: await dp.locator(".xterm-viewport").evaluate((vp) => vp.scrollHeight - vp.clientHeight),
        ...(await termDiag(dp)),
      };
      throw new Error(`${e.message}: ${JSON.stringify(seen)} pane ${JSON.stringify(aaa("pane", "read", "scroll-plain", "--source", "visible").text.split("\n").filter(Boolean).slice(-3))}`);
    });
    const box = await dp.getByTestId("terminal").boundingBox();
    await dp.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await until("휠로 맨 위 s-001", async () => {
      await dp.mouse.wheel(0, -2000);
      return (await firstNo(dp)) === 1;
    }).catch(async (e) => {
      throw new Error(`${e.message}: 첫 줄 ${await firstNo(dp)}, xterm 기록 ${await dp.locator(".xterm-viewport").evaluate((v) => v.scrollHeight - v.clientHeight)}px`);
    });
    // 올려 둔 동안 출력이 와도 그 자리(키 입력이 아니다: 다른 곳에서 보낸 입력)
    for (let i = 0; i < 3; i++) aaa("pane", "send-text", "scroll-plain", "p");
    await until("more-3 출력", () => got("scroll-plain").includes("more-3"));
    await sleep(300);
    if ((await firstNo(dp)) !== 1) bad.push(`출력이 오자 올려 둔 자리를 잃었다(첫 줄 ${await firstNo(dp)})`);
    // 키를 치면 맨 아래로 돌아온다
    await dp.keyboard.press("x");
    // 맨 아래에는 올려 둔 동안 온 출력도 있다(그 출력이 이 화면에 왔는데도 자리를 지켰다는 뜻)
    await until("키를 치면 맨 아래", async () => {
      const text = await screenText(dp);
      return text.includes('GOT:"x"') && text.includes("more-3");
    });
    if ((await firstNo(dp)) === 1) bad.push("키를 쳤는데 맨 아래로 오지 않았다");
    // 「맨 아래로 이동」(herdr-web-ui TerminalViewportTools): 맨 아래에서는 없고, 위로 굴리면 grid 오른쪽 아래에 뜨며,
    // 누르면 맨 아래로 오고 터미널 focus는 그대로다
    const jump = dp.getByTestId("jump-bottom");
    if ((await jump.count()) !== 0) bad.push("맨 아래인데 「맨 아래로 이동」이 있다");
    await dp.mouse.wheel(0, -600);
    await jump.waitFor();
    await shot(dp, "desktop-jump-bottom");
    const jb = await jump.boundingBox();
    const sb = await dp.locator('[data-testid="terminal"] .xterm-screen').boundingBox();
    if (!(jb.x + jb.width <= sb.x + sb.width && jb.y + jb.height <= sb.y + sb.height && jb.x > sb.x + sb.width / 2 && jb.y > sb.y + sb.height / 2))
      bad.push(`「맨 아래로 이동」 자리 ${JSON.stringify(jb)}, grid ${JSON.stringify(sb)}`);
    await jump.click();
    await until("「맨 아래로 이동」을 누르면 맨 아래", async () => (await jump.count()) === 0 && (await screenText(dp)).includes('GOT:"x"'));
    if (!(await dp.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")))) bad.push("「맨 아래로 이동」이 터미널 focus를 가져갔다");
    // 끌어 고르는 중 휠(herdr-web-ui dragWheel): 선택을 지우지 않고 기록을 굴린다. 놓으면 화면 밖까지 고른 글을 복사한다
    // (aaa xterm은 기록을 가져 선택이 화면을 넘는다: 차이 목록 J 「선택을 놓으면 복사(화면 밖 포함)」)
    const top0 = await firstNo(dp);
    const rowH = sb.height / aaa("pane", "list").panes.find((x) => x.name === "scroll-plain").rows;
    const anchorY = sb.y + sb.height - rowH * 1.5;
    await dp.mouse.move(sb.x + 3, anchorY);
    await dp.mouse.down();
    await dp.mouse.move(sb.x + 40, anchorY, { steps: 2 });
    await dp.mouse.wheel(0, -rowH * 12);
    await until("끌기 중 휠이 기록을 굴림", async () => (await firstNo(dp)) === top0 - 12).catch(async (e) => bad.push(`${e.message}: 첫 줄 ${await firstNo(dp)}(전 ${top0})`));
    // 맨 윗줄 첫 칸까지 끈다(첫 칸이어야 그 줄 글 전체가 고른 글에 든다)
    await dp.mouse.move(sb.x + 2, sb.y + rowH * 0.5, { steps: 4 });
    await dp.mouse.up();
    const wantTop = `s-${String(top0 - 12).padStart(3, "0")}`;
    await until("화면 밖까지 고른 글 복사", async () => (await dp.evaluate(() => window.__copied)).some((t) => t.includes(wantTop) && t.includes("more-3"))).catch(async (e) =>
      bad.push(`${e.message}: ${JSON.stringify((await dp.evaluate(() => window.__copied)).map((t) => t.slice(0, 40) + "…" + t.slice(-40)))}`),
    );
    await dp.keyboard.press("y");
    await until("키를 치면 다시 맨 아래", async () => (await screenText(dp)).includes('GOT:"y"'));
    // alternate screen에 mouse 보고가 없으면 휠은 화살표(herdr AlternateScroll), mouse 보고면 그 보고가 프로그램에 간다
    // 휠 한 번이 여러 줄이면 화살표도 여럿이 한 조각으로 온다
    for (const [mode, want] of [["alt", 'GOT:"\\u001b[A'], ["mouse", 'GOT:"\\u001b[<64;']]) {
      await dp.getByTestId(`pane-scroll-${mode}`).click();
      await until(`${mode} 화면`, async () => (await screenText(dp)).includes("s-200"));
      await dp.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await dp.mouse.wheel(0, -100);
      await until(`${mode} 휠 → ${want}`, () => got(`scroll-${mode}`).includes(want)).catch((e) => bad.push(`${e.message}: ${got(`scroll-${mode}`).split("\n").filter((l) => l.startsWith("GOT")).join(" ")}`));
    }
    // 휠 속도(herdr-web-ui terminalWheelSpeed): 3×면 휠 한 번에 보고 셋(xterm은 휠 사건마다 보고 하나)
    const reports = () => got("scroll-mouse").split("<64;").length - 1;
    await sleep(300);
    const before = reports();
    await dp.mouse.wheel(0, -100);
    await until("1× 보고 하나", () => reports() === before + 1);
    const dlg = await openSettings(dp, "터미널");
    for (let i = 0; i < 2; i++) await dlg.getByRole("button", { name: "휠 스크롤 빠르게" }).click();
    if ((await dlg.getByRole("group", { name: "휠 스크롤 속도" }).innerText()).trim() !== "3×") bad.push("설정이 3×가 아니다");
    if ((await dp.evaluate(() => localStorage.getItem("aaa:terminal-wheel-speed"))) !== "3") bad.push("휠 속도 저장이 없다");
    await dp.keyboard.press("Escape");
    await dp.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await sleep(300);
    const mid = reports();
    await dp.mouse.wheel(0, -100);
    await until("3× 보고 셋", () => reports() === mid + 3).catch((e) => bad.push(`${e.message}: ${reports() - mid}개`));
    await sleep(300);
    if (reports() !== mid + 3) bad.push(`3×에서 보고 ${reports() - mid}개`);
    await dp.context().close();

    // 한 손가락 끌기: 손가락이 한 칸 높이 내려갈 때마다 기록 한 줄(xterm 자기 터치 스크롤과 두 번 굴리지 않는다)
    const swipeCheck = async (label, p, swipe) => {
      // 화면을 열면 첫 snapshot은 그 pane의 지금 크기(다른 기기가 건 grid)로 그려지고, 이 화면이 크기를 되찾으면 다시 그려진다.
      // 끌 자리는 되찾은 뒤에 잰다: 글자(s-200)만 보고 재면 넓은 grid의 가운데가 휴대폰 밖이라 끌기가 아무 데도 닿지 않았다(CI 10-18)
      const ownGrid = (name) =>
        until(`${label} ${name} 이 화면 크기`, async () => {
          const pane = aaa("pane", "list").panes.find((x) => x.name === name);
          if (!pane || pane.lease === null) return false;
          return p.evaluate((rows) => {
            const scroll = document.querySelector(".term-scroll");
            const screen = document.querySelector(".xterm-screen");
            if (!scroll || !screen || scroll.hasAttribute("data-adopted-grid")) return false;
            return document.querySelectorAll(".xterm-rows > div").length === rows && screen.getBoundingClientRect().right <= scroll.getBoundingClientRect().right + 0.5;
          }, pane.rows);
        });
      await p.getByTestId("pane-scroll-plain").click();
      await until(`${label} 화면`, async () => (await screenText(p)).includes('GOT:"x"'));
      await ownGrid("scroll-plain");
      const cell = await p.evaluate(() => {
        const screen = document.querySelector(".xterm-screen").getBoundingClientRect();
        return { h: screen.height / document.querySelectorAll(".xterm-rows > div").length, top: screen.top, x: screen.left + screen.width / 2 };
      });
      const before = await firstNo(p);
      const travel = 240;
      await swipe(cell, travel);
      await sleep(200);
      const after = await firstNo(p);
      const want = Math.round(travel / cell.h);
      // 위로 굴렸으니 「맨 아래로 이동」이 grid 오른쪽 아래에 있다
      if (!(await p.getByTestId("jump-bottom").isVisible())) bad.push(`${label}: 위로 굴렸는데 「맨 아래로 이동」이 없다`);
      await shot(p, label.startsWith("iPad") ? "ipad-jump-bottom" : "phone-jump-bottom");
      if (before === null || after === null || Math.abs(before - after - want) > 1)
        bad.push(`${label}: ${travel}px 끌어 첫 줄 ${before} → ${after}(기대 ${want}줄 위, 한 칸 ${cell.h.toFixed(1)}px)`);
      // alternate screen·mouse 보고 프로그램(Claude Code·Codex 같은)에서는 xterm 터치 스크롤이 아무것도 하지 않는다:
      // 끌기가 휠이 되어 화살표·mouse 보고로 프로그램에 가야 한다
      for (const [mode, want] of [["touch-alt", 'GOT:"\\u001b[A'], ["touch-mouse", 'GOT:"\\u001b[<64;']]) {
        if (label.startsWith("iPad") && mode === "touch-mouse") continue; // 한 기기에서 한 번이면 된다(iPad는 alt만)
        // 휴대폰은 pane을 열면 목록이 가려진다
        const back = p.getByRole("button", { name: "목록으로" });
        if (await back.isVisible()) await back.click();
        await p.getByTestId(`pane-scroll-${mode}`).click();
        // 그 프로그램의 화면이 그려졌는지는 글만으로 모른다: 앞 pane(plain)도 s-200을 보인다(10-18 전체 실행에서 끌기가 바뀌는
        // 사이 화면에 가 사라졌다). alternate screen이면 기록이 없고(viewport가 늘지 않는다), mouse면 xterm이 mouse 보고 중이다
        await until(`${label} ${mode} 화면`, async () =>
          (await screenText(p)).includes("s-200") &&
          (await p.evaluate((mode) => {
            const vp = document.querySelector(".xterm-viewport");
            const alt = vp && vp.scrollHeight <= vp.clientHeight + 1;
            return alt && (mode !== "touch-mouse" || !!document.querySelector(".xterm.enable-mouse-events"));
          }, mode)),
        );
        await ownGrid(`scroll-${mode}`);
        const c = await p.evaluate(() => {
          const r = document.querySelector(".xterm-screen").getBoundingClientRect();
          return { top: r.top, x: r.left + r.width / 2 };
        });
        await swipe({ ...cell, ...c }, travel);
        await until(`${label} ${mode} 끌기 → ${want}`, () => got(`scroll-${mode}`).includes(want)).catch(async (e) => {
          // 실패하면 그때 xterm이 mouse 보고 중이었는지(xterm Terminal.ts `enable-mouse-events`)와 viewport를 남긴다(t-0327 넘김 (마))
          const at = await p.evaluate(() => {
            const vp = document.querySelector(".xterm-viewport");
            return {
              mouse: !!document.querySelector(".xterm.enable-mouse-events"),
              viewport: vp ? [vp.scrollTop, vp.scrollHeight, vp.clientHeight] : null,
              log: (window.__aaaTermLog ?? []).slice(-12).map((x) => (x.kind ? `${x.at} bin×${x.bins}` : `${x.at} ${x.e}`)),
            };
          });
          bad.push(`${e.message}: ${got(`scroll-${mode}`).split("\n").filter((l) => l.startsWith("GOT")).join(" ")} ‖ ${JSON.stringify(at)}`);
        });
      }
      return { before, after, want };
    };
    const phone = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    const cdp = await phone.context().newCDPSession(phone);
    const ph = await swipeCheck("휴대폰(Chromium 터치)", phone, async (cell, travel) => {
      const y0 = cell.top + 40;
      await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: cell.x, y: y0, id: 1 }] });
      for (let i = 1; i <= 12; i++) await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: cell.x, y: y0 + (travel * i) / 12, id: 1 }] });
      await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    });
    await phone.context().close();
    const wk = await webkit.launch();
    try {
      const pad = (await pairPage(wk, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true })).page;
      await swipeCheck("iPad(WebKit 터치)", pad, (cell, travel) =>
        pad.evaluate(
          async ({ x, y0, travel }) => {
            const target = document.querySelector(".xterm-screen");
            // Playwright WebKit에는 Touch 생성자가 없다(Illegal constructor): 같은 이름의 사건에 손가락 목록을 붙인다
            const ev = (type, y) => {
              const t = { identifier: 7, target, clientX: x, clientY: y, pageX: x, pageY: y };
              const on = type === "touchend" ? [] : [t];
              const e = new Event(type, { bubbles: true, cancelable: true, composed: true });
              for (const [key, value] of [["touches", on], ["targetTouches", on], ["changedTouches", [t]]]) Object.defineProperty(e, key, { value });
              return e;
            };
            target.dispatchEvent(ev("touchstart", y0));
            for (let i = 1; i <= 12; i++) {
              target.dispatchEvent(ev("touchmove", y0 + (travel * i) / 12));
              await new Promise((r) => requestAnimationFrame(r));
            }
            target.dispatchEvent(ev("touchend", y0 + travel));
          },
          { x: cell.x, y0: cell.top + 40, travel },
        ),
      );
      await pad.context().close();
    } finally {
      await wk.close();
    }
    for (const mode of names) aaa("pane", "kill", `scroll-${mode}`);
    if (bad.length) throw new Error(bad.join("; "));
    console.log(`     휴대폰 240px 끌기: 첫 줄 s-${ph.before} → s-${ph.after}(${ph.want}줄)`);
  });

  await step("글꼴이 늦게 와도 맨 아래(넣은 WOFF2, plan §8.60): 터미널을 연 뒤 글꼴이 받아져 칸 높이가 바뀌어도 맨 아래를 보던 화면은 맨 아래에 남는다", async () => {
    // CI 러너와 처음 여는 기기에는 Meslotendard가 깔려 있지 않다: 대신 글꼴로 열고, 넣은 글꼴은 늦게 받는다.
    // 대신 글꼴은 칸 높이가 확실히 다른 것을 쓴다(같으면 이 시험은 아무것도 보지 않는다)
    aaa("tab", "create", "--machine", "labA", "--label", "fontjump", "--", process.execPath, `${root}/scroll.js`, "plain");
    await waitOutput("fontjump", "s-200");
    const init = () =>
      document.addEventListener("DOMContentLoaded", () => {
        const st = document.createElement("style");
        // 이 머신에 깔린 Meslotendard(local)를 쓰지 않고 WOFF2만 받게 같은 이름의 @font-face를 뒤에 둔다
        st.textContent = [400, 700]
          .map((w) => `@font-face{font-family:"Meslotendard Web";src:url("/fonts/meslotendard-0.3.1/Meslotendard-${w === 400 ? "Regular" : "Bold"}.woff2") format("woff2");font-weight:${w}}`)
          .concat(':root{--font-term:"Meslotendard Web", "Courier New", monospace !important}')
          .join("\n");
        document.head.append(st);
      });
    let release;
    const fontGate = new Promise((r) => (release = r));
    const hold = async (r) => {
      await fontGate;
      await r.continue();
    };
    const p = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { init, route: ["**/fonts/**", hold] })).page;
    const rowHeight = () => p.evaluate(() => document.querySelector(".xterm-rows > div")?.getBoundingClientRect().height ?? 0);
    const atBottom = () =>
      p.evaluate(() => {
        const vp = document.querySelector(".xterm-viewport");
        return vp.scrollTop >= vp.scrollHeight - vp.clientHeight - 1;
      });
    await p.getByTestId("pane-fontjump").click();
    await until("대신 글꼴로 맨 아래 s-200", async () => (await screenText(p)).includes("s-200") && (await atBottom()));
    const before = await rowHeight();
    release();
    await until("넣은 글꼴로 칸 높이가 바뀜", async () => Math.abs((await rowHeight()) - before) >= 1);
    const after = await rowHeight();
    await until("글꼴이 바뀐 뒤에도 맨 아래 s-200", async () => (await screenText(p)).includes("s-200") && (await atBottom())).catch(async (e) => {
      throw new Error(`${e.message}: 칸 ${before.toFixed(1)}px → ${after.toFixed(1)}px, ${JSON.stringify(await termDiag(p))}`);
    });
    console.log(`     칸 높이 ${before.toFixed(1)}px → ${after.toFixed(1)}px`);
    await p.context().close();
    aaa("pane", "kill", "fontjump");
  });

  await step("줄 수가 줄면 위 줄을 기록으로(xterm·herdr처럼, plan §8.59): 작은 화면이 크기를 걸어도 host 화면에 마지막 줄·커서 줄이 남고, 다시 붙은 웹은 그 위를 굴려 본다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "shrink", "--", process.execPath, `${root}/scroll.js`, "plain");
    await waitOutput("shrink", "s-200");
    // pty-size는 새 크기가 host에 닿은 뒤 돌아온다: 바로 읽어도 줄인 화면이다
    aaa("pane", "pty-size", "shrink", "80", "8");
    const r = aaa("pane", "read", "shrink", "--source", "visible").text.split("\n");
    // 커서는 s-200 다음 빈 줄: 그 위 일곱 줄이 s-194~s-200
    const want = Array.from({ length: 7 }, (_, i) => `s-${194 + i}`);
    if (JSON.stringify(r.slice(0, 7)) !== JSON.stringify(want)) throw new Error(`줄인 화면 ${JSON.stringify(r)}`);
    // 늦게 붙은 웹: 기록에 줄인 때 올린 줄까지 있어 위로 굴리면 s-001
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await dp.getByTestId("pane-shrink").click();
    await until("shrink 화면", async () => (await screenText(dp)).includes("s-200"));
    const box = await dp.getByTestId("terminal").boundingBox();
    await dp.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await until("기록 맨 위 s-001", async () => {
      await dp.mouse.wheel(0, -2000);
      return (await screenText(dp)).includes("s-001");
    });
    await dp.context().close();
    aaa("pane", "kill", "shrink");
  });

  await step("앞 기록(사용자 요청 10-17 「뒤로 스크롤하다 데이터가 부족해지면 받아오면 어때?」, plan §8.67): 끊긴 동안 쏟아진 출력을 다시 붙은 뒤 위로 굴리며 차례로 받아 보고, 붙일 때 보던 자리가 튀지 않는다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "histapp", "--", process.execPath, `${root}/history.js`);
    await waitOutput("histapp", "hist>");
    // pane WebSocket 문: 닫혀 있으면 새 연결을 바로 끊는다(끊긴 동안). host의 앞 기록 답은 잠깐 붙잡아, 붙이기 전 화면을 잰다
    // 첫 답을 붙잡는 동안 온 출력도 붙잡았다가 답 바로 뒤에 보낸다: 다시 쓰는 사이에 출력이 오는 경우
    let gate = "open";
    let pipe = null;
    let replies = 0;
    let holdOutput = null;
    let heldOutput = 0;
    const route = (ws) => {
      if (gate === "closed") return void ws.close({ code: 1011, reason: "test offline" });
      const server = ws.connectToServer();
      pipe = { ws, server };
      ws.onMessage((m) => server.send(m));
      server.onMessage((m) => {
        if (typeof m === "string" && m.startsWith('{"t":"history"')) {
          replies += 1;
          if (replies === 1) holdOutput = [];
          return void setTimeout(() => {
            ws.send(m);
            for (const o of holdOutput ?? []) ws.send(o);
            heldOutput += holdOutput?.length ?? 0;
            holdOutput = null;
          }, replies === 1 ? 1500 : 600);
        }
        // 지금 연결의 출력만(닫은 앞 연결의 늦은 출력이 섞이지 않게)
        if (holdOutput && typeof m !== "string" && pipe?.server === server) return void holdOutput.push(m);
        ws.send(m);
      });
      server.onClose((code, reason) => ws.close({ code, reason }));
      ws.onClose((code, reason) => server.close({ code, reason }));
    };
    const hp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { wsRoute: [/\/api\/v1\/panes\//, route] })).page;
    await hp.getByTestId("pane-histapp").click();
    await until("histapp 화면", async () => (await screenText(hp)).includes("hist>"));
    const box = hp.getByTestId("terminal");
    const attr = (k) => box.evaluate((el, k) => el.dataset[k] ?? null, k);
    // 끊고, 끊긴 동안 6000줄
    gate = "closed";
    pipe.ws.close({ code: 1011, reason: "test offline" });
    pipe.server.close({ code: 1000, reason: "test offline" });
    aaa("pane", "send-text", "histapp", "h");
    await waitOutput("histapp", "history-done");
    gate = "open";
    await until("다시 붙음", async () => (await screenText(hp)).includes("history-done"), 30000);
    if ((await attr("historyMore")) !== "1") throw new Error(`다시 붙은 뒤 앞 기록 표시 ${await attr("historyMore")}`);
    const rows = () => hp.locator(".xterm-rows").innerText();
    const firstNum = (t) => Number(/h-(\d{5})/.exec(t)?.[1] ?? NaN);
    // 이 화면을 쓴다(누르면 이 화면 크기로 grid를 되찾는다: 남의 grid를 보는 동안은 휠이 칸을 굴린다)
    await box.click();
    await until("이 화면 grid", async () => (await hp.locator(".term-scroll[data-adopted-grid]").count()) === 0);
    await box.hover();
    let applied = 0;
    const seen = [];
    for (let round = 0; round < 10; round++) {
      // 받은 기록 맨 위 근처까지 굴린다: 요청이 나가면 멈추고 그 화면을 잰다
      await until(`위로 굴려 요청(${round + 1})`, async () => {
        if ((await attr("historyWaiting")) === "1" || (await attr("historyMore")) === "0") return true;
        await hp.mouse.wheel(0, -3000);
        return false;
      });
      if ((await attr("historyMore")) === "0" && (await attr("historyWaiting")) !== "1") break;
      const before = await rows();
      // 첫 덩어리를 기다리는 동안 pane에 새 줄(tick-1): 다시 쓰는 사이에 도착한다
      if (round === 0) {
        aaa("pane", "send-text", "histapp", "p");
        await waitOutput("histapp", "tick-1");
      }
      await until(`붙임(${round + 1})`, async () => Number(await attr("historyApplied")) === applied + 1);
      applied += 1;
      const after = await rows();
      if (after !== before) throw new Error(`붙일 때 화면이 튀었다(${round + 1}): ${before.slice(0, 80)} → ${after.slice(0, 80)}`);
      seen.push(firstNum(before));
    }
    if (applied < 2 || replies !== applied) throw new Error(`앞 기록을 ${applied}번 붙였다(답 ${replies})`);
    if (heldOutput === 0) throw new Error("다시 쓰는 사이에 온 출력이 없다(시험이 그 경우를 만들지 못했다)");
    if (!seen.every((n, i) => i === 0 || n < seen[i - 1])) throw new Error(`붙인 자리가 위로 가지 않았다: ${seen}`);
    // 버퍼 전체를 한 화면씩 훑어: h-00001~h-06000이 빠짐·겹침 없이 차례로(덩어리 경계도)
    const sweep = await hp.evaluate(async () => {
      const box = document.querySelector('[data-testid="terminal"]');
      const vp = box.querySelector(".xterm-viewport");
      const rows = () => [...box.querySelectorAll(".xterm-rows > div")];
      // 그 줄이 맨 위로 그려질 때까지(Term onRender가 data-view-top에 적는다). 걸음마다 맨 위 줄이 바뀌므로 그리기가 꼭 온다.
      // 오지 않으면 끝없이 기다리지 않고 그때 모습으로 실패한다(기다림의 끝은 사건이고, 10초는 고장 판정일 뿐이다)
      const drawnAt = (line) =>
        new Promise((done, fail) => {
          const ok = () => box.dataset.viewTop === String(line);
          if (ok()) return done();
          const stuck = setTimeout(() => {
            mo.disconnect();
            fail(new Error(`줄 ${line}이 그려지지 않았다: ${JSON.stringify({ top: box.dataset.viewTop, base: box.dataset.viewBase, scrollTop: vp.scrollTop, scrollHeight: vp.scrollHeight })}`));
          }, 10_000);
          const mo = new MutationObserver(() => ok() && (clearTimeout(stuck), mo.disconnect(), done()));
          mo.observe(box, { attributes: true, attributeFilter: ["data-view-top"] });
        });
      // xterm(5.5 Viewport)은 scrollTop을 줄 높이(css cell 높이 = 줄 div 높이)로 나눠 반올림한 줄을 맨 위로 그린다. scroll 영역은
      // 줄 높이×줄 수에 (viewport 높이 − canvas 높이)를 더한 높이라, scrollHeight를 줄 수로 나누면 줄이 커져 먼 줄에서 한 줄 넘는다
      const goTo = async (line) => {
        if (box.dataset.viewTop === String(line)) return;
        const rowPx = rows()[0].getBoundingClientRect().height;
        vp.scrollTop = line * rowPx;
        vp.dispatchEvent(new Event("scroll"));
        await drawnAt(line);
      };
      const nums = new Set();
      const bad = [];
      const ticks = new Map();
      let last = -1;
      const half = Math.floor(rows().length / 2);
      for (let line = 0; ; ) {
        await goTo(line);
        const divs = rows();
        const got = divs.map((d) => /^h-(\d{5})/.exec(d.textContent)?.[1]).filter(Boolean).map(Number);
        for (let i = 1; i < got.length; i++) if (got[i] !== got[i - 1] + 1) bad.push([got[i - 1], got[i]]);
        got.forEach((n) => nums.add(n));
        // tick-1·h-06000 줄의 버퍼 줄 번호(겹쳐 본 화면에서 같은 줄은 한 번 센다)
        divs.forEach((d, i) => {
          if (d.textContent.startsWith("tick-1")) ticks.set(line + i, "tick");
          if (d.textContent.startsWith("h-06000")) last = line + i;
        });
        const base = Number(box.dataset.viewBase);
        if (line >= base) break;
        line = Math.min(base, line + half);
      }
      const tickAt = [...ticks.keys()];
      return { count: nums.size, min: Math.min(...nums), max: Math.max(...nums), bad: bad.slice(0, 5), tickAt, last };
    });
    if (sweep.count !== 6000 || sweep.min !== 1 || sweep.max !== 6000 || sweep.bad.length) throw new Error(`버퍼 기록 ${JSON.stringify(sweep)}`);
    // 다시 쓰는 사이에 온 줄은 한 번, 기록 뒤에
    if (sweep.tickAt.length !== 1 || !(sweep.tickAt[0] > sweep.last)) throw new Error(`tick-1 자리 ${JSON.stringify(sweep)}`);
    // 다시 쓴 버퍼도 프로그램이 켠 입력 모드(bracketed paste)를 그대로 갖는다: 붙여 넣은 여러 줄이 감싸여 간다
    await box.click();
    await hp.locator(".xterm-helper-textarea").evaluate((el) => {
      const dt = new DataTransfer();
      dt.setData("text/plain", "pa\nste");
      el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    });
    await waitOutput("histapp", 'GOT:"\\u001b[200~pa\\rste\\u001b[201~"');
    await hp.context().close();
    aaa("pane", "kill", "histapp");
  });

  await step("앞 기록 빠른 휠(QA t-0337 Q1: 데스크톱 Chromium 6/8에서 화면 전체가 빈 채로 굳음): 긴 출력 pane을 열자마자 휠을 연달아 굴려 앞 기록을 받는 동안에도, 붙인 뒤에도 줄이 그려지고, 키를 치면 맨 아래 새 출력이 보인다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "fastwheel", "--", process.execPath, `${root}/long.js`);
    await waitOutput("fastwheel", "long-done");
    // 붙을 때는 최근 기록(aaa_vt REPLAY_HISTORY 2000줄)만 오고 앞이 남는다: 굴리면 받는다
    const looks = [];
    for (let trial = 0; trial < 8; trial++) {
      const page = (await pairPage(browser, aaa("web", "pair").code, { width: 1440, height: 900 })).page;
      await page.getByTestId("pane-fastwheel").click();
      await until("fastwheel 화면", async () => (await screenText(page)).includes("long-done"));
      const box = page.getByTestId("terminal");
      const attr = (k) => box.evaluate((el, k) => el.dataset[k] ?? null, k);
      await until("앞 기록 있음", async () => (await attr("historyMore")) === "1");
      // QA probe-scroll5.mjs처럼 누르지 않고 가운데에 올린다
      await box.hover();
      // QA probe-scroll5.mjs 그대로: -1200px를 40번 연달아(기다리지 않는다)
      for (let i = 0; i < 40; i++) await page.mouse.wheel(0, -1200);
      const look = () =>
        box.evaluate((el) => {
          const rows = [...el.querySelectorAll(".xterm-screen:not(.term-history-cover) .xterm-rows > div")].map((d) => d.textContent);
          return {
            nonblank: rows.filter((t) => t.trim()).length,
            first: rows.find((t) => t.trim())?.slice(0, 20) ?? null,
            covers: el.querySelectorAll(".term-history-cover").length,
            applied: el.dataset.historyApplied,
            waiting: el.dataset.historyWaiting,
            more: el.dataset.historyMore,
            top: el.dataset.viewTop,
            base: el.dataset.viewBase,
          };
        });
      // 붙이기가 끝나고(덮개 없음) 그려진 줄이 있다. 고장이면 20초 기다림이 그때 모습으로 끝난다
      let last = null;
      try {
        await until(`붙인 뒤 화면(${trial + 1})`, async () => {
          last = await look();
          return Number(last.applied) >= 1 && last.waiting !== "1" && last.covers === 0 && last.nonblank > 0;
        });
      } catch (e) {
        throw new Error(`${e.message}: ${JSON.stringify(last)}`);
      }
      looks.push(last);
      // 키를 치면 맨 아래로 가고 새 출력이 그려진다
      await box.click();
      await page.keyboard.press("p");
      await until(`키 뒤 새 출력(${trial + 1})`, async () => (await screenText(page)).includes(`tick-${trial + 1}`));
      await page.context().close();
    }
    if (looks.some((l) => l.first === null)) throw new Error(`빈 화면: ${JSON.stringify(looks)}`);
    aaa("pane", "kill", "fastwheel");
  });
  await step("앞 기록 reset 직후 scroll(QA t-0337 Q1 원인, patches/xterm-5.5.0-viewport.patch, plan §8.105): 앞 기록을 붙이려고 xterm을 reset한 뒤 줄 높이를 다시 재기 전에 scrollTop 0의 scroll 사건이 와도 보던 줄이 숫자로 남고 그려지며, 붙잡은 출력이 풀린다", async () => {
    // 짧은 줄 2600개: host가 처음부터 구독해 다 갖고(붙을 때 2000줄, 앞 기록 600줄), 덩어리와 화면을 한 번에 다 읽는다.
    // 읽는 중에 온 scroll의 NaN은 남은 출력이 되살리므로, 다 읽은 뒤 줄 높이를 재기 전의 scroll이 결함이다(QA probe-stuck)
    aaa("tab", "create", "--machine", "labA", "--label", "resetscroll", "--", process.execPath, `${root}/history.js`);
    await waitOutput("resetscroll", "hist>");
    aaa("pane", "send-text", "resetscroll", "q");
    await waitOutput("resetscroll", "short-done");
    const page = (await pairPage(browser, aaa("web", "pair").code, { width: 1440, height: 900 })).page;
    await page.getByTestId("pane-resetscroll").click();
    await until("resetscroll 화면", async () => (await screenText(page)).includes("short-done"));
    const box = page.getByTestId("terminal");
    const attr = (k) => box.evaluate((el, k) => el.dataset[k] ?? null, k);
    await until("앞 기록 있음", async () => (await attr("historyMore")) === "1");
    // 덮개는 reset 바로 앞에 붙고, 새 버퍼를 다 읽고 처음 그린 frame에 떨어진다(Term.tsx prependHistory). xterm은 reset 뒤
    // rAF 두 번이 지나야 줄 높이를 다시 잰다(Viewport.reset → syncScrollArea → _refresh → _innerRefresh). 그래서 덮개가 떨어진
    // 그때(MutationObserver, 다음 frame 전)가 다 읽은 뒤·재기 전이다: 거기서 맨 위로 굴린 scroll 사건을 보낸다(QA probe-stuck에서
    // 휠 연타가 닿는 자리). 읽는 동안 온 scroll의 NaN은 남은 출력이 되살린다
    await box.evaluate((el) => {
      const vp = el.querySelector(".xterm-viewport");
      window.__resetScroll = [];
      let armed = false;
      const mo = new MutationObserver(() => {
        if (el.querySelector(".term-history-cover")) return void (armed = true);
        if (!armed) return;
        mo.disconnect();
        vp.scrollTop = 0;
        vp.dispatchEvent(new Event("scroll"));
        window.__resetScroll.push("scrolled");
      });
      mo.observe(el, { childList: true, subtree: true });
    });
    await box.hover();
    await until("위로 굴려 요청", async () => {
      if ((await attr("historyWaiting")) === "1" || (await attr("historyApplied")) === "1") return true;
      await page.mouse.wheel(0, -3000);
      return false;
    });
    await until("덮개가 떨어진 때 scroll을 보냄", () => page.evaluate(() => window.__resetScroll.length === 1));
    // 붙이기가 끝나고(덮개 없음) 보던 줄이 숫자이며 줄이 그려진다. 고장이면 view-top이 NaN이고 덮개가 남는다(20초는 고장 판정)
    let last = null;
    await until("붙인 뒤 화면", async () => {
      last = await box.evaluate((el) => ({
        applied: el.dataset.historyApplied,
        top: el.dataset.viewTop,
        covers: el.querySelectorAll(".term-history-cover").length,
        nonblank: [...el.querySelectorAll(".xterm-screen:not(.term-history-cover) .xterm-rows > div")].filter((d) => d.textContent.trim()).length,
      }));
      return last.applied === "1" && last.covers === 0 && Number.isFinite(Number(last.top)) && last.top !== "NaN" && last.nonblank > 0;
    }).catch((e) => {
      throw new Error(`${e.message}: ${JSON.stringify(last)}`);
    });
    // 키를 치면 맨 아래로 가고 새 출력이 그려진다(붙잡은 출력이 풀렸다)
    await box.click();
    await page.keyboard.press("p");
    await until("키 뒤 새 출력", async () => (await screenText(page)).includes("tick-1"));
    await page.context().close();
    aaa("pane", "kill", "resetscroll");
  });

  await step("깨어나면 바로 다시 붙기(QA t-0337 Q5, herdr-web-ui ws.ts onWake·wake, t3code connection supervisor): pane 연결이 끊겨 backoff를 기다리는 동안 망이 돌아오거나(online) 화면이 다시 보이면(pageshow·visibilitychange) 기다리지 않고 그 자리에서 새로 붙는다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "wake", "--", process.execPath, `${root}/history.js`);
    await waitOutput("wake", "hist>");
    // 새 연결을 이만큼 바로 끊는다(망이 없는 동안). 그 뒤는 그대로 잇는다
    let reject = 0;
    let pipe = null;
    const routed = [];
    const route = (ws) => {
      routed.push(reject);
      if (reject > 0) {
        reject -= 1;
        return void ws.close({ code: 1011, reason: "test offline" });
      }
      const server = ws.connectToServer();
      pipe = { ws, server };
      ws.onMessage((m) => server.send(m));
      server.onMessage((m) => ws.send(m));
      server.onClose((code, reason) => ws.close({ code, reason }));
      ws.onClose((code, reason) => server.close({ code, reason }));
    };
    const page = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { wsRoute: [/\/api\/v1\/panes\//, route] })).page;
    await page.getByTestId("pane-wake").click();
    await until("wake 화면", async () => (await screenText(page)).includes("hist>"));
    // 앱이 만드는 pane WebSocket을 센다. 진단 기록(__aaaTermLog)은 Playwright routeWebSocket이 서버에 이을 때 만드는 진짜
    // 연결만 보아, 길에서 거절한 연결은 남지 않는다. 그래서 페이지가 뜬 뒤 앱이 부르는 WebSocket을 감싼다
    await page.evaluate(() => {
      const seen = (window.__appWs = { made: 0, closed: 0, onClose: null });
      const WS = window.WebSocket;
      window.WebSocket = class extends WS {
        constructor(...a) {
          super(...a);
          if (!String(a[0]).includes("/panes/")) return;
          seen.made += 1;
          // 앱의 onclose(뒤에 단다)보다 먼저 돈다
          this.addEventListener("close", () => {
            seen.closed += 1;
            seen.onClose?.();
          });
        }
      };
    });
    const realOpens = () => page.evaluate(() => window.__aaaTermLog.filter((x) => x.e === "ws open").length);
    for (const [round, kind] of ["online", "pageshow", "visibilitychange"].entries()) {
      // 끊고, 다시 붙기 두 번을 거절한다: 두 번째 거절 뒤에는 다음 시도가 backoff(2초)를 기다린다.
      // 듣개를 먼저 단 뒤에 끊는다(처음 연결은 감싸기 전에 만들어 그 닫힘은 세지 않는다). 20초는 고장 판정일 뿐이다
      await page.evaluate((closes) => {
        const seen = window.__appWs;
        const want = seen.closed + closes;
        window.__wakeClosed = new Promise((done, fail) => {
          const stuck = setTimeout(() => fail(new Error(`닫힘 ${seen.closed}/${want}, 만든 연결 ${seen.made}`)), 20_000);
          seen.onClose = () => {
            if (seen.closed < want) return;
            seen.onClose = null;
            clearTimeout(stuck);
            // 앱의 onclose가 다음 시도를 건 뒤
            setTimeout(done, 0);
          };
        });
      }, round === 0 ? 2 : 3);
      reject = 2;
      pipe.ws.close({ code: 1011, reason: "test offline" });
      pipe.server.close({ code: 1000, reason: "test offline" });
      await page.evaluate(() => window.__wakeClosed).catch((e) => {
        throw new Error(`${kind}: ${e.message} 길 ${JSON.stringify(routed)}`);
      });
      if (reject !== 0) throw new Error(`${kind}: 거절이 남았다 ${reject}`);
      const opened = await realOpens();
      // 사건을 보낸 그 자리에서 새 연결이 열린다(타이머를 기다리지 않는다)
      const now = await page.evaluate((kind) => {
        const at = window.__appWs.made;
        if (kind === "online") window.dispatchEvent(new Event("online"));
        else if (kind === "pageshow") window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true }));
        else document.dispatchEvent(new Event("visibilitychange"));
        return window.__appWs.made - at;
      }, kind);
      if (now !== 1) throw new Error(`${kind}: 사건에 바로 붙지 않았다(새 연결 ${now}개)`);
      // 그 연결이 snapshot을 받는다
      await until(`${kind} 뒤 snapshot`, () =>
        page.evaluate((opened) => {
          const log = window.__aaaTermLog;
          const i = log.findLastIndex((x) => x.e === "ws open");
          return log.filter((x) => x.e === "ws open").length > opened && log.slice(i).some((x) => x.e?.startsWith("snapshot"));
        }, opened),
      );
    }
    await page.context().close();
    aaa("pane", "kill", "wake");
  });
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
    const slow = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { wsRoute: [/\/api\/v1\/panes\//, swallow] })).page;
    await slow.getByTestId("pane-flood").click();
    await until("느린 화면 flood>", async () => (await screenText(slow)).includes("flood>"));
    const fast = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await fast.getByTestId("pane-flood").click();
    await until("따라오는 화면 flood>", async () => (await screenText(fast)).includes("flood>"));
    // ACK 전의 옛 웹(주소에 flow=ack가 없고 ACK도 없다, 다시 띄우기 전에 받은 화면): host는 보내기 줄로만 재므로
    // 잘 받는 동안은 닫지 않는다(참고의 ACK 없는 연결)
    const old = await fast.context().newPage();
    await old.goto(base);
    await old.evaluate((id) => {
      const w = new WebSocket(`ws://${location.host}/api/v1/panes/${encodeURIComponent(id)}/ws`);
      w.binaryType = "arraybuffer";
      window.oldClient = { bytes: 0, code: null, acks: 0, joined: false };
      w.onmessage = (m) => {
        if (typeof m.data !== "string") window.oldClient.bytes += m.data.byteLength;
        else if (m.data.startsWith('{"t":"snapshot"')) window.oldClient.joined = true;
      };
      w.onclose = (e) => (window.oldClient.code = e.code);
    }, aaa("pane", "list").panes.find((x) => x.name === "flood").id);
    // snapshot이 출력 구독의 시작이다. 그 전에 쏟으면 앞부분이 snapshot(text)에 들어가 bytes가 모자란다(t-0327, 착지 검증에서 1/8)
    await until("옛 웹 붙음", async () => await old.evaluate(() => window.oldClient.joined));
    // 400KB를 한 번에 쓴다(HIGH 256KiB를 넘는다)
    aaa("pane", "send-text", "flood", "f");
    await until("출력 정지 띠", async () => await slow.getByTestId("output-stalled").isVisible());
    const stalled = await log(slow);
    if (!stalled.includes("ws close 4008")) throw new Error(`4008로 닫히지 않았다: ${JSON.stringify(stalled.filter((e) => e !== "io"))}`);
    if (dropped === 0) throw new Error("삼킨 ACK가 없다: 화면이 ACK를 보내지 않는다");
    // 과부하는 다시 보내지 않는다: 닫히기 전 snapshot을 다시 받지 않았고, 닫힌 뒤 스스로 다시 붙지 않는다
    if (count(stalled, "snapshot") !== 1) throw new Error(`snapshot을 다시 받았다: ${JSON.stringify(stalled.filter((e) => e !== "io"))}`);
    await sleep(1500);
    const later = await log(slow);
    if (count(later, "ws open") !== 1 || !(await slow.getByTestId("output-stalled").isVisible())) throw new Error(`4008 뒤 스스로 다시 붙었다: ${JSON.stringify(later.filter((e) => e !== "io"))}`);
    // 따라오는 화면은 닫히지 않고 끝까지 받는다
    await until("따라오는 화면 flood-done", async () => (await screenText(fast)).includes("flood-done"));
    const kept = await log(fast);
    if (kept.some((e) => e.startsWith("ws close")) || count(kept, "snapshot") !== 1) throw new Error(`따라오는 화면이 끊겼다: ${JSON.stringify(kept.filter((e) => e !== "io"))}`);
    await until("옛 웹도 끝까지 받음", async () => (await old.evaluate(() => window.oldClient.bytes)) >= 400_000);
    const oldClient = await old.evaluate(() => window.oldClient);
    if (oldClient.code !== null) throw new Error(`ACK 없는 옛 웹이 닫혔다: ${JSON.stringify(oldClient)}`);
    await old.close();
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

  await step("host를 다시 띄운 뒤 앞 기록(QA t-0337 Q8, plan §8.112): 다시 뜬 host의 mirror는 session host snapshot(최근 2000줄)부터 채워진다. 그 앞은 session host에 물어 받아, 위로 굴려 첫 줄까지 빠짐 없이 본다", async () => {
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
    await qp.getByTestId("pane-q8").click();
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
