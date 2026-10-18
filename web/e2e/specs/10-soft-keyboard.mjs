// 웹 E2E 화면 키보드: 조합 상자·키보드 위 화면.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 10-soft-keyboard(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import path from "node:path";
import {
  webkit, shots, root, sleep, aaa, until, screenText, fakeViewport, softKeyboard, pairPage, step, sentFrames, sentCount,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("조합 상자(WebKit iPad·iPhone 크기, 어두운·밝은 테마): 크기는 herdr-web-ui fitView, 색은 그 칸 배경·글자색, 테두리 없음, 세로는 커서 줄에 맞춰 커서가 새지 않음", async () => {
    // 배경 있는 입력줄(Claude 입력줄처럼)과 배경 없는 줄, 커서는 줄 끝에 둔다
    const app = `${root}/compose.sh`;
    // 셋째 줄은 Codex 입력줄처럼 화면 맨 아래 배경 줄의 「› 」 뒤에서 「가」를 조합한다(사용자 캡처 12:05)
    fs.writeFileSync(
      app,
      "printf 'plain 상태 '; read a; printf '\\033[48;5;238m› 상태 \\033[K'; read b; printf '\\033[0m'; " +
        "i=0; while [ $i -lt 80 ]; do echo; i=$((i+1)); done; printf '\\033[48;5;238m› \\033[K'; cat >/dev/null\n",
    );
    const wk = await webkit.launch();
    const bad = [];
    try {
      // 밝은 iPad(2배)와 어두운 iPhone(3배): 색은 테마마다 그 칸을 따르고, 커서가 새던 결함(사용자 캡처 12:05)은 3배에서 났다
      const devices = [
        ["light", "ipad", { width: 820, height: 1180 }, 2],
        ["dark", "iphone", { width: 390, height: 844 }, 3],
      ];
      for (const [scheme, dev, viewport, deviceScaleFactor] of devices) {
        const name = `compose-${scheme}-${dev}`;
        aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", app);
        const p = (await pairPage(wk, aaa("web", "pair").code, viewport, { colorScheme: scheme, deviceScaleFactor })).page;
        // 커서는 herdr-web-ui처럼 깜빡인다(cursorBlink). 꺼진 순간에 재면 커서색이 투명이라 샘을 볼 수 없다:
        // 깜빡임만 멈춰 커서가 그려진 상태에서 잰다
        await p.addStyleTag({ content: ".xterm .xterm-cursor-blink, .xterm .xterm-cursor-blink * { animation: none !important; }" });
        await p.getByTestId(`pane-${name}`).click();
        await until("조합 화면", async () => (await screenText(p)).includes("plain 상태"));
        await p.locator(".xterm-screen").click();
        for (const [line, ch] of [["plain", "ㅅ"], ["bg", "ㅅ"], ["bottom", "가"]]) {
          if (line === "bg") {
            aaa("pane", "send-keys", name, "enter");
            await until("배경 줄", async () => (await screenText(p)).includes("› 상태"));
          }
          if (line === "bottom") {
            aaa("pane", "send-keys", name, "enter");
            await until("맨 아래 줄", async () => !(await screenText(p)).includes("plain 상태"));
          }
          await sleep(300);
          const ta = p.locator("textarea.xterm-helper-textarea");
          await ta.evaluate(async (el, ch) => {
            el.focus();
            el.dispatchEvent(new CompositionEvent("compositionstart", { data: "", bubbles: true }));
            el.value += ch;
            el.dispatchEvent(new CompositionEvent("compositionupdate", { data: ch, bubbles: true }));
            el.dispatchEvent(new InputEvent("input", { data: ch, inputType: "insertCompositionText", isComposing: true, bubbles: true }));
            await new Promise((r) => setTimeout(r, 100));
          }, ch);
          const info = await p.evaluate(() => {
            const view = document.querySelector(".xterm .composition-view.active");
            if (!view) return null;
            const r = view.getBoundingClientRect();
            const st = getComputedStyle(view);
            const rows = document.querySelector(".xterm-rows");
            // 비교할 칸: 조합 상자 바로 왼쪽 칸(같은 줄의 「상태 」 뒤 빈칸)
            // 자리로 찾는다: 터치 화면은 줄이 터치를 받지 않아(pointer-events: none) elementsFromPoint가 건너뛴다
            const [x, y] = [r.left - 3, r.top + r.height / 2];
            const inside = (e) => {
              const b = e.getBoundingClientRect();
              return b.left <= x && x < b.right && b.top <= y && y < b.bottom;
            };
            const row = [...rows.children].find(inside);
            const left = row && ([...row.children].find(inside) ?? row);
            let bg = "";
            for (let e = left ?? rows; e && e !== document.documentElement; e = e.parentElement) {
              const c = getComputedStyle(e).backgroundColor;
              if (c && c !== "rgba(0, 0, 0, 0)" && c !== "transparent") {
                bg = c;
                break;
              }
            }
            const screen = document.querySelector(".xterm-screen");
            const cur = document.querySelector(".xterm-rows .xterm-cursor");
            const c = cur?.getBoundingClientRect();
            return {
              rect: { x: r.x, y: r.y, width: r.width, height: r.height },
              cursor: c ? { x: c.x, y: c.y, width: c.width, height: c.height, cls: cur.className, bg: getComputedStyle(cur).backgroundColor, outline: getComputedStyle(cur).outlineStyle, shadow: getComputedStyle(cur).boxShadow } : null,
              rowH: parseFloat(screen.style.height) / document.querySelectorAll(".xterm-rows > div").length,
              bg: st.backgroundColor,
              fg: st.color,
              border: st.borderTopWidth,
              outline: st.outlineStyle,
              cellBg: bg,
              cellFg: left ? getComputedStyle(left).color : "",
              screenW: parseFloat(screen.style.width),
            };
          });
          const cols = aaa("pane", "list").panes.find((x) => x.name === name).cols;
          // 조합 상자 둘레(위·아래·옆 3px)에 커서색 픽셀이 새는지 캡처로 센다(사용자 캡처 12:05: 「가」 위 가로줄)
          if (info?.cursor) {
            const m = 3;
            const png = await p.screenshot({ clip: { x: info.rect.x - m, y: info.rect.y - m, width: info.rect.width + 2 * m, height: info.rect.height + 2 * m } });
            info.leak = await p.evaluate(
              async ({ b64, color, m }) => {
                const img = new Image();
                img.src = `data:image/png;base64,${b64}`;
                await img.decode();
                const c = document.createElement("canvas");
                c.width = img.width;
                c.height = img.height;
                const g = c.getContext("2d");
                g.drawImage(img, 0, 0);
                const d = g.getImageData(0, 0, c.width, c.height).data;
                const [cr, cg, cb] = color.match(/\d+/g).map(Number);
                const k = c.width / (c.width / devicePixelRatio);
                const rows = new Map();
                for (let y = 0; y < c.height; y++)
                  for (let x = 0; x < c.width; x++) {
                    const i = (y * c.width + x) * 4;
                    if (Math.abs(d[i] - cr) + Math.abs(d[i + 1] - cg) + Math.abs(d[i + 2] - cb) < 60) rows.set(y, (rows.get(y) ?? 0) + 1);
                  }
                return { dpr: k, size: [c.width, c.height], inner: [Math.round(m * k), Math.round(c.height - m * k)], rows: [...rows] };
              },
              { b64: png.toString("base64"), color: info.cursor.bg, m },
            );
          }
          if (shots && info) {
            await p.screenshot({
              path: path.join(shots, `${name}-${line}.png`),
              clip: { x: Math.max(0, info.rect.x - 160), y: Math.max(0, info.rect.y - 24), width: 260, height: info.rect.height + 48 },
            });
          }
          console.log(`     조합 상자 ${scheme} ${dev} ${line}: ${JSON.stringify(info)}`);
          if (!info) bad.push(`${scheme} ${line}: 조합 상자가 없다`);
          else {
            const cell = info.screenW / cols;
            if (info.bg !== info.cellBg) bad.push(`${scheme} ${line}: 배경 ${info.bg} ≠ 칸 ${info.cellBg}`);
            if (info.fg !== info.cellFg) bad.push(`${scheme} ${line}: 글자 ${info.fg} ≠ 칸 ${info.cellFg}`);
            if (info.border !== "0px" || info.outline !== "none") bad.push(`${scheme} ${line}: 테두리 ${info.border} ${info.outline}`);
            if (Math.abs(info.rect.width - 2 * cell) > 1.5) bad.push(`${scheme} ${line}: 폭 ${info.rect.width} ≠ 두 칸 ${2 * cell}`);
            const at = `${scheme} ${dev} ${line}`;
            if (!info.cursor) bad.push(`${at}: 커서 칸이 없다`);
            else if (Math.abs(info.rect.y - info.cursor.y) > 0.02 || Math.abs(info.rect.height - info.cursor.height) > 0.02)
              bad.push(`${at}: 상자 세로 ${info.rect.y}+${info.rect.height} ≠ 커서 줄 ${info.cursor.y}+${info.cursor.height}`);
            if (info.leak?.rows.length) bad.push(`${at}: 상자 둘레에 커서색이 샌다(device 줄, 픽셀 수) ${JSON.stringify(info.leak.rows)}`);
          }
          // 조합을 끝내 지운다(보내지 않는다: 값을 비운 뒤 끝낸다)
          await ta.evaluate((el) => {
            el.value = el.value.slice(0, -1);
            el.dispatchEvent(new CompositionEvent("compositionend", { data: "", bubbles: true }));
          });
          await sleep(150);
        }
        await p.context().close();
        aaa("pane", "kill", name);
      }
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("화면 키보드(WebKit iPad·iPhone 크기, visualViewport를 줄여 흉내): 커서 줄·입력 막대·키 막대가 키보드 위에 보이고, PTY 크기는 그대로", async () => {
    // herdr-web-ui scripts/keyboard-viewport-regression.ts처럼 플랫폼 입력(visualViewport·innerHeight·100lvh 자)만 가짜로 둔다.
    // 사용자 캡처(12:26): iPad 화면 키보드가 터미널 아래쪽(프롬프트 줄·커서·입력 막대·키 막대)을 가렸다.
    const init = fakeViewport;
    const app = `${root}/keyboard.sh`;
    fs.writeFileSync(app, "i=0; while [ $i -lt 200 ]; do echo kb-line-$i; i=$((i+1)); done; printf '› '; cat >/dev/null\n");
    const wk = await webkit.launch();
    const bad = [];
    try {
      for (const [dev, viewport, kb] of [["ipad", { width: 820, height: 1180 }, 400], ["iphone", { width: 390, height: 844 }, 336]]) {
        const name = `kb-${dev}`;
        aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", app);
        const p = (await pairPage(wk, aaa("web", "pair").code, viewport, { touch: true, init })).page;
        const direct = p.locator('.key-bar .key[data-key="direct"]');
        const pty = () => aaa("pane", "list").panes.find((x) => x.name === name);
        // 맞추기 판단(Term data-fit)과 크기 바뀜 뒤 기다리는 맞추기(data-fit-pending). 시험이 지운 뒤 다시 달린 판단을 기다린다
        const term = p.getByTestId("terminal");
        const fit = async () => ({ fit: await term.getAttribute("data-fit"), pending: await term.getAttribute("data-fit-pending") });
        const forget = () => term.evaluate((el) => delete el.dataset.fit);
        const decided = (what) =>
          until(what, async () => {
            const f = await fit();
            return f.fit !== null && f.pending === null;
          }).then(fit, async (e) => {
            throw new Error(`${e.message}: ${JSON.stringify(await fit())}`);
          });
        /** 이 화면과 host가 크기에 맞다(맞추기 판단 same, 기다리는 맞추기 없음)·host 크기가 ok */
        const agreed = (what, ok = () => true) =>
          until(what, async () => {
            const f = await fit();
            return f.fit === "same" && f.pending === null && ok(pty());
          }).catch(async (e) => {
            const r = pty();
            throw new Error(`${e.message}: ${JSON.stringify(await fit())} PTY ${r.cols}x${r.rows}`);
          });
        await p.getByTestId(`pane-${name}`).click();
        await until("키보드 화면", async () => (await screenText(p)).includes("kb-line-199"));
        await agreed("연 화면의 크기");
        let before = pty();
        for (const field of ["terminal", "input-bar"]) {
          // 터미널은 바로 입력(기본)에서, 입력줄은 ⌨로 입력줄을 켠 뒤 본다
          if (field === "terminal") await p.locator(".xterm-screen").tap();
          else {
            // 입력줄 칸(한 줄)이 흐름에 들어와 그리드가 한 번 줄어든다(참고와 같다). 키보드가 바꾸는지는 그 뒤부터 본다.
            const opened = before;
            await direct.tap();
            await p.locator(".terminal-input").waitFor();
            await agreed("입력줄 칸만큼 줄어든 크기", (r) => r.rows < opened.rows);
            before = pty();
            await p.getByLabel("보낼 글").focus();
          }
          // 화면 키보드가 올라온다: 보이는 높이만 줄고(iOS Safari) 레이아웃은 그대로다. 앱 높이(--app-height)가 줄어 터미널 칸의
          // 크기 바뀜이 맞추기를 부르고, 그 판단(keyboard: grid는 그대로, 커서 줄로 옮김)이 난 뒤에 본다(sleep으로 어림하지 않는다)
          const k = await sentCount(p);
          await forget();
          await softKeyboard(p, viewport.height - kb);
          const up = await decided("키보드 뒤 맞추기 판단");
          const resized = (await sentFrames(p, k)).filter((f) => f.t === "resize");
          const seen = await p.evaluate(() => {
            const bottom = (sel) => document.querySelector(sel)?.getBoundingClientRect().bottom ?? null;
            const cur = document.querySelector(".xterm-rows .xterm-cursor")?.getBoundingClientRect();
            const area = document.querySelector(".term-scroll")?.getBoundingClientRect();
            return {
              keyboard: document.documentElement.hasAttribute("data-keyboard"),
              area: area ? { top: area.top, bottom: area.bottom } : null,
              visible: window.visualViewport.height,
              app: document.querySelector(".app")?.getBoundingClientRect().height,
              cursor: cur ? { top: cur.top, bottom: cur.bottom } : null,
              input: bottom(".terminal-input-text"),
              keys: bottom(".key-bar"),
              scroll: window.scrollY,
            };
          });
          const rows = aaa("pane", "list").panes.find((x) => x.name === name);
          console.log(`     화면 키보드 ${dev} ${field}: ${JSON.stringify(seen)} PTY ${before.cols}x${before.rows} → ${rows.cols}x${rows.rows}`);
          if (shots) await p.screenshot({ path: path.join(shots, `keyboard-${dev}-${field}.png`) });
          const at = `${dev} ${field}`;
          const vis = seen.visible;
          if (!seen.cursor || seen.cursor.top < 0 || seen.cursor.bottom > vis + 0.5) bad.push(`${at}: 커서 줄이 키보드에 가렸다 ${JSON.stringify(seen.cursor)} > ${vis}`);
          // 터미널 영역(입력 막대 위) 안에 보여야 한다: 영역 밖이면 잘려 보이지 않는다
          else if (!seen.area || seen.cursor.top < seen.area.top - 0.5 || seen.cursor.bottom > seen.area.bottom + 0.5)
            bad.push(`${at}: 커서 줄이 터미널 영역 밖이다 ${JSON.stringify(seen.cursor)} ∉ ${JSON.stringify(seen.area)}`);
          if (field === "input-bar" && (seen.input === null || seen.input > vis + 0.5)) bad.push(`${at}: 입력줄 아래 ${seen.input} > ${vis}`);
          if (field === "terminal" && seen.input !== null) bad.push(`${at}: 바로 입력인데 입력줄이 있다`);
          if (seen.keys === null || seen.keys > vis + 0.5) bad.push(`${at}: 키 막대 아래 ${seen.keys} > ${vis}`);
          if (up.fit !== "keyboard" || resized.length || rows.cols !== before.cols || rows.rows !== before.rows)
            bad.push(`${at}: 키보드에 PTY 크기가 바뀌었다 ${before.cols}x${before.rows} → ${rows.cols}x${rows.rows}, 판단 ${up.fit}, 보낸 크기 ${JSON.stringify(resized)}`);
          // 키보드가 내려간다
          await forget();
          await softKeyboard(p, viewport.height);
          await p.evaluate(() => document.activeElement?.blur());
          await decided("키보드가 내려간 뒤 맞추기 판단");
        }
        await p.context().close();
        aaa("pane", "kill", name);
      }
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(bad.join("; "));
  });
}
