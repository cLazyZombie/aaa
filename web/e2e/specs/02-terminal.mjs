// 웹 E2E 터미널 기본: 키보드·크기·글꼴·글자 폭·입력 막대·두 controller·reader·새 pane.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 02-terminal(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import path from "node:path";
import {
  repo, root, aaa, until, waitOutput, screenText, shot, fontOk, pairPage, base, step, browser, openSettings, sentFrames, sentCount,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("controller: 키보드 입력", async () => {
    await page.getByTestId("pane-t1").click();
    await page.getByTestId("terminal").click();
    await until("터미널 준비", async () => (await screenText(page)).includes("$"));
    await page.keyboard.type("echo key-$((5*5))\n");
    await until("키보드 입력 결과", async () => (await screenText(page)).includes("key-25"));
  });

  await step("넓은 창: PTY가 터미널 칸을 채우고 창 크기를 따라간다(조작 중일 때)", async () => {
    // 터미널 글자 영역이 칸(안쪽 여백 뺀 크기)을 한 cell 안쪽까지 채우는지, header 크기·host PTY 크기가 같은지
    const filled = async () => {
      const m = await page.evaluate(() => {
        const box = document.querySelector('[data-testid="terminal"]');
        const scr = box.querySelector(".xterm-screen").getBoundingClientRect();
        const cs = getComputedStyle(box);
        const w = box.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
        const h = box.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
        const crumb = document.querySelector(".term-title .muted").textContent;
        return { w, h, sw: scr.width, sh: scr.height, crumb };
      });
      const p = aaa("pane", "list").panes.find((x) => x.name === "t1");
      const cw = m.sw / p.cols;
      const ch = m.sh / p.rows;
      // 오른쪽에 xterm 세로 scrollbar 자리를 남기지 않는다(참고처럼, plan §8.124): 전에는 15px(두 칸쯤)이 늘 비었다
      const ok = m.sw <= m.w + 1 && m.w - m.sw < cw + 1 && m.sh <= m.h + 1 && m.h - m.sh < ch + 1;
      return { ok: ok && m.crumb.includes(`${p.cols}×${p.rows}`), m, cols: p.cols, rows: p.rows };
    };
    let last = null;
    const fills = async () => (last = await filled()).ok;
    await until("1280×800에서 칸을 채움", fills).catch((e) => {
      throw new Error(`${e.message} 마지막 측정: ${JSON.stringify(last)}`);
    });
    const small = await filled();
    await page.setViewportSize({ width: 1900, height: 1150 });
    await until("넓힌 창을 따라감", async () => {
      const f = await filled();
      return f.ok && f.cols > small.cols && f.rows > small.rows;
    });
    await shot(page, "desktop-wide-fit");
    await page.setViewportSize({ width: 1280, height: 800 });
    await until("되돌린 창을 따라감", async () => {
      const f = await filled();
      return f.ok && f.cols === small.cols && f.rows === small.rows;
    });
  });

  await step("끝 칸까지 보인다(coordinator 10-18 t-0333 발견 「gh pr checks」가 「gh p」로, plan §8.124): 휴대폰·iPad·데스크톱에서 글자가 한 cell씩 나아가 끝 칸이 줄 안에, scrollbar 자리 없이 칸을 끝까지", async () => {
    // 칸 수만큼의 자(끝 두 글자 @#)와, 끝 칸을 넘어 접히는 「gh pr checks」. 크기가 바뀌면 다시 그린다
    const prog = `const draw = () => {
  const c = process.stdout.columns;
  let s = "";
  for (let i = 1; i <= c; i++) s += String(i % 10);
  process.stdout.write("\\x1b[H\\x1b[2J" + s.slice(0, -2) + "@#\\r\\n" + "x".repeat(c - 5) + "gh pr checks\\r\\nendcol " + c + "> ");
};
process.stdout.on("resize", draw);
draw();
process.stdin.resume();`;
    aaa("tab", "create", "--machine", "labA", "--label", "endcol", "--", process.execPath, "-e", prog);
    const cols = () => aaa("pane", "list").panes.find((p) => p.name === "endcol").cols;
    await waitOutput("endcol", "endcol ");
    // 앞 화면이 쥔 크기(처음은 만들 때 크기). 기기마다 폭이 달라, 이 값이 바뀌면 그 화면이 쥔 것이다
    let held = cols();
    const devices = [
      ["phone", { width: 390, height: 844 }, { touch: true, deviceScaleFactor: 3 }],
      ["ipad", { width: 820, height: 1180 }, { touch: true, deviceScaleFactor: 2 }],
      ["desktop", { width: 1280, height: 800 }, { deviceScaleFactor: 2 }],
    ];
    const bad = [];
    for (const [name, viewport, opt] of devices) {
      const p = (await pairPage(browser, aaa("web", "pair").code, viewport, opt)).page;
      await p.getByTestId("pane-endcol").click();
      // 이 화면이 쥔 크기로 다시 그린 자(붙은 직후에는 앞 화면 크기의 자가 보인다)
      await until(`${name}: 자기 폭의 자`, async () => {
        const c = cols();
        return c !== held && (await screenText(p)).includes(`endcol ${c}>`);
      });
      const c = (held = cols());
      const m = await p.evaluate(() => {
        const row = [...document.querySelectorAll(".xterm-rows > div")].find((r) => r.textContent.trimEnd().endsWith("@#"));
        if (!row) return null;
        const at = (i) => {
          const w = document.createTreeWalker(row, NodeFilter.SHOW_TEXT);
          for (let n = w.nextNode(), seen = 0; n; seen += n.data.length, n = w.nextNode())
            if (i < seen + n.data.length) {
              const r = document.createRange();
              r.setStart(n, i - seen);
              r.setEnd(n, i - seen + 1);
              return r.getBoundingClientRect();
            }
          return null;
        };
        const text = row.textContent.trimEnd();
        const first = at(0);
        const last = at(text.length - 1);
        const box = document.querySelector('[data-testid="terminal"]').getBoundingClientRect();
        return { len: text.length, first: first.left, last: last.left, rowRight: row.getBoundingClientRect().right, screen: document.querySelector(".xterm-screen").getBoundingClientRect().width, box: box.width };
      });
      if (!m || m.len !== c) {
        bad.push(`${name}: ${c}칸 자를 찾지 못했다 ${JSON.stringify(m)}`);
      } else {
        const cell = m.screen / c;
        const advance = (m.last - m.first) / (c - 1);
        // 글자 하나가 나아가는 폭이 cell이다. xterm 5.5 DOM renderer는 글자 폭을 32자 offsetWidth(정수)로 재고(1/64px까지 어긋남),
        // 줄 자간을 그 차이로 둔다(배치 단위 1/64px 반올림까지). 그래서 정상은 글자마다 1/32px 안쪽이다. body 자간이 들던 때는 0.065px
        if (Math.abs(advance - cell) > 0.04) bad.push(`${name}: 글자마다 ${advance.toFixed(4)}px 나아간다(cell ${cell.toFixed(4)}px)`);
        // 끝 칸 글자가 반 넘게 줄(overflow hidden) 안에 있다
        if (m.last + cell / 2 > m.rowRight) bad.push(`${name}: 끝 칸 글자가 ${m.last.toFixed(2)}에서 시작해 줄 끝 ${m.rowRight.toFixed(2)}에 잘린다`);
        // 칸이 터미널 자리를 끝까지 쓴다(scrollbar 자리 15px을 남기지 않는다)
        if (m.box - m.screen >= cell + 1) bad.push(`${name}: 오른쪽 ${(m.box - m.screen).toFixed(1)}px이 빈다(cell ${cell.toFixed(2)}px)`);
      }
      await shot(p, `${name}-endcol`);
      await p.context().close();
    }
    aaa("pane", "kill", "endcol");
    if (bad.length) throw new Error(bad.join(" / "));
  });

  await step("터미널 글꼴: Meslotendard로 cell 너비는 영문, 한글은 정확히 두 칸(1280×800)", async () => {
    // 넣은 WOFF2와 고지 파일을 host가 보낸다(버전 폴더라 오래 캐시한다)
    const dir = "fonts/meslotendard-0.3.1/";
    const want = fs.readFileSync(path.join(repo, "web", "public", dir, "Meslotendard-Regular.woff2"));
    const r = await fetch(`${base}${dir}Meslotendard-Regular.woff2`);
    const got = Buffer.from(await r.arrayBuffer());
    if (r.status !== 200 || r.headers.get("content-type") !== "font/woff2" || !got.equals(want))
      throw new Error(`woff2 ${r.status} ${r.headers.get("content-type")} ${got.length}`);
    if (!r.headers.get("cache-control").includes("immutable")) throw new Error(`cache ${r.headers.get("cache-control")}`);
    for (const f of ["LICENSE", "licenses/THIRD_PARTY_NOTICES.md", "licenses/MESLO_APACHE-2.0.txt", "licenses/NERD_FONTS_LICENSE.txt"]) {
      const n = await fetch(`${base}${dir}${f}`);
      if (n.status !== 200 || n.headers.get("content-type") !== "text/plain; charset=utf-8")
        throw new Error(`고지 파일 ${f}: ${n.status} ${n.headers.get("content-type")}`);
    }
    await page.getByLabel("보낼 글").fill("echo 한글 간격 확인 ABCD abcd 0123");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("한글 줄", async () => (await screenText(page)).includes("한글 간격 확인 ABCD"));
    const cols = aaa("pane", "list").panes.find((x) => x.name === "t1").cols;
    console.log("     글꼴 측정(1280×800)", JSON.stringify(await fontOk(page, cols)));
    await shot(page, "desktop-hangul");
  });

  await step("글자 폭(herdr-web-ui terminalWidths): 이모지·새 이모지·ZWJ 묶음·⚠️는 두 칸", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "widths", "--", process.execPath, `${root}/widths.js`);
    const wp = (await pairPage(browser, aaa("web", "pair").code, { width: 900, height: 600 })).page;
    await wp.getByTestId("pane-widths").click();
    await until("폭 화면", async () => (await screenText(wp)).includes("W"));
    // 글자마다 화면 위치로 몇째 칸인지 잰다(DOM renderer의 span 묶음에 기대지 않는다)
    const cols = await wp.evaluate((cols) => {
      const screen = document.querySelector('[data-testid="terminal"] .xterm-screen').getBoundingClientRect();
      const cell = screen.width / cols;
      const at = (ch) => {
        const walk = document.createTreeWalker(document.querySelector(".xterm-rows"), NodeFilter.SHOW_TEXT);
        for (let n = walk.nextNode(); n; n = walk.nextNode()) {
          const i = n.textContent.indexOf(ch);
          if (i < 0) continue;
          const r = document.createRange();
          r.setStart(n, i);
          r.setEnd(n, i + 1);
          return Math.round((r.getBoundingClientRect().left - screen.left) / cell);
        }
        return null;
      };
      return ["X", "Y", "Z", "W"].map(at);
    }, aaa("pane", "list").panes.find((x) => x.name === "widths").cols);
    if (JSON.stringify(cols) !== "[2,2,2,2]") throw new Error(`글자 칸(기대 [2,2,2,2]) ${JSON.stringify(cols)}`);
    await wp.context().close();
    aaa("pane", "kill", "widths");
  });

  await step("controller: 입력 막대로 한글", async () => {
    await page.getByLabel("보낼 글").fill("echo 한글-$((2*4))");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("한글 입력 결과", async () => (await screenText(page)).includes("한글-8"));
    await shot(page, "desktop-controller");
  });

  await step("두 controller(herdr 모델): 가져오기 없이 바로 입력, 입력·focus를 얻은 화면이 크기를 다시 건다, 가려지면 바로 놓는다", async () => {
    const other = await pairPage(browser, aaa("web", "pair").code, { width: 1000, height: 640 }, { line: true });
    const op = other.page;
    // 이 화면이 WebSocket에 쓴 글 frame
    const opFrames = [];
    op.on("websocket", (ws) =>
      ws.on("framesent", (f) => {
        if (typeof f.payload !== "string") return;
        try {
          opFrames.push({ ...JSON.parse(f.payload), at: Date.now() });
        } catch {}
      }),
    );
    const t1size = () => {
      const p = aaa("pane", "list").panes.find((x) => x.name === "t1");
      return `${p.cols}x${p.rows}`;
    };
    const holder = () => aaa("pane", "list").panes.find((x) => x.name === "t1").lease?.label ?? null;
    // 원래 화면이 t1을 쓰는 중에서 시작한다(앞 단계가 t1에 입력했는지에 기대지 않는다)
    await page.locator(".xterm-screen").click();
    await page.keyboard.type("echo own-$((1+2))\n");
    await until("원래 화면 입력", async () => (await screenText(page)).includes("own-3"));
    await until("원래 화면이 쓰는 중", () => holder() !== null);
    const held = t1size();
    const first = holder();
    const opSent = await sentCount(op);
    await op.getByTestId("pane-t1").click();
    await until("둘째 화면 터미널", async () => (await screenText(op)).includes("own-3"));
    // 「다른 화면이 크기를 정하는 중」 띠와 「화면에 맞추기」 단추는 참고에 없어 없앴다
    if ((await op.getByTestId("lease-banner").count()) !== 0) throw new Error("크기를 정하는 중 띠가 있다");
    if ((await op.getByRole("button", { name: "화면에 맞추기" }).count()) !== 0) throw new Error("화면에 맞추기 단추가 있다");
    // 입력 막대가 켜져 있다(보기 권한이 아니면 언제나 입력된다). 열기만 해서는 크기를 바꾸지 않는다
    if (await op.getByLabel("보낼 글").isDisabled()) throw new Error("다른 화면이 쓰는 중이라고 입력 막대가 꺼졌다");
    // 열 때의 맞추기 판단(Term data-fit)이 나고(연결 전 off가 아니다) 크기 바뀜 뒤 남은 맞추기가 없을 때까지. 크기는 그 판단 안에서
    // 바로 보내므로, 그 판단이 「다른 화면이 쓰는 중」이고 그 뒤 보낸 것에 resize·take가 없으면 이 화면은 걸지 않았다(sleep으로 어림하지 않는다)
    const opTerm = op.getByTestId("terminal");
    const fit = async () => ({ fit: await opTerm.getAttribute("data-fit"), pending: await opTerm.getAttribute("data-fit-pending") });
    await until("열 때 맞추기 판단", async () => {
      const f = await fit();
      return f.fit !== null && f.fit !== "off" && f.pending === null;
    }).catch(async (e) => {
      throw new Error(`${e.message}: ${JSON.stringify(await fit())}`);
    });
    const decided = (await fit()).fit;
    const claimed = (await sentFrames(op, opSent)).filter((f) => f.t === "resize" || f.t === "take");
    if (decided !== "other" || claimed.length || t1size() !== held || holder() !== first)
      throw new Error(`열기만 했는데 크기·쓰는 화면이 바뀌었다: ${held} → ${t1size()}, 판단 ${decided}, 보낸 것 ${JSON.stringify(claimed)}`);
    // 1) 입력하면 바로 들어가고, 그 화면이 지금 쓰는 화면이 되어 자기 칸 크기로 맞춘다
    await op.getByLabel("보낼 글").fill("echo direct-$((3*7))");
    await op.getByRole("button", { name: "보내기" }).click();
    await until("다른 화면 입력", async () => (await screenText(page)).includes("direct-21"));
    await until("입력한 화면 크기", () => t1size() !== held && holder() !== first);
    const otherSize = t1size();
    await shot(page, "desktop-lease-held");
    // 2) 번갈아 입력하면 크기가 그때마다 바뀐다(지금 규칙): 원래 화면 터미널에 직접 친다
    await page.locator(".xterm-screen").click();
    await page.keyboard.type("echo back-$((2*5))\n");
    await until("원래 화면 입력", async () => (await screenText(page)).includes("back-10"));
    await until("원래 화면 크기로", () => t1size() === held && holder() === first);
    await op.getByLabel("보낼 글").fill("echo again-$((4*4))");
    await op.getByRole("button", { name: "보내기" }).click();
    await until("다시 다른 화면 크기", () => t1size() === otherSize);
    // 3) focus를 얻은 화면이 크기를 다시 건다(입력 없이)
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await until("focus 얻은 화면 크기", () => t1size() === held && holder() === first);
    // 3b) 「터미널의 aaa와 같이 쓰기」(herdr-web-ui releasePaneAway, 기본 꺼짐): 켜면 다른 창으로 옮기고 1초 뒤 크기를 놓고
    // 보기만 한다(띠). 남은 화면이 크기를 되찾고, 이 화면을 누르면 다시 이 화면 크기다
    await (await openSettings(op, "터미널")).getByRole("switch", { name: "터미널의 aaa와 같이 쓰기" }).click();
    await op.keyboard.press("Escape");
    await op.getByLabel("보낼 글").fill("echo away");
    await op.getByRole("button", { name: "보내기" }).click();
    await until("다른 화면 크기(같이 쓰기)", () => t1size() === otherSize);
    const beforeAway = opFrames.length;
    await op.evaluate(() => {
      document.hasFocus = () => false;
      window.dispatchEvent(new Event("blur"));
    });
    const blurAt = Date.now();
    await until("다른 창으로 옮기고 놓음", () => opFrames.slice(beforeAway).find((f) => f.t === "release"), 5000);
    const awayRelease = opFrames.slice(beforeAway).find((f) => f.t === "release");
    if (awayRelease.at - blurAt < 900) throw new Error(`1초를 기다리지 않고 놓았다(${awayRelease.at - blurAt}ms)`);
    await op.getByTestId("released-banner").waitFor();
    await shot(op, "desktop-released");
    await until("같이 쓰기로 놓은 뒤 남은 화면 크기", () => t1size() === held && holder() === first, 5000);
    await op.evaluate(() => delete document.hasFocus);
    await op.locator(".xterm-screen").click();
    await until("누르면 다시 이 화면 크기", () => t1size() === otherSize && holder() !== first);
    if ((await op.getByTestId("released-banner").count()) !== 0) throw new Error("되찾은 뒤에도 보기만 띠가 있다");
    // 4) 쓰던 화면이 가려지면 바로 놓고(참고 keepSize(hidden), 전에는 1초 뒤), 보이고 focus 있는 화면이 자기 크기로 맞춘다
    await op.getByLabel("보낼 글").fill("echo hide");
    await op.getByRole("button", { name: "보내기" }).click();
    await until("다른 화면 크기", () => t1size() === otherSize);
    const beforeHide = opFrames.length;
    await op.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    // 1초 타이머가 아니라 바로 놓는다(700ms 안에 release frame)
    await until("가려지자 바로 놓음", () => opFrames.slice(beforeHide).some((f) => f.t === "release"), 700);
    await until("가려진 화면이 놓음 → 남은 화면 크기", () => t1size() === held && holder() === first, 5000);
    if (other.errors.length) throw new Error(other.errors.join("; "));
    await other.page.context().close();
    if (await page.getByLabel("보낼 글").isDisabled()) throw new Error("입력 막대가 꺼져 있다");
  });


  await step("reader(휴대폰 크기): 보기만 된다", async () => {
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    const rp = rd.page;
    if ((await rp.getByRole("button", { name: "새 pane" }).count()) !== 0) throw new Error("reader에게 새 pane 버튼");
    await shot(rp, "mobile-list");
    await rp.getByTestId("pane-t1").click();
    await until("reader 화면", async () => (await screenText(rp)).includes("한글-8"));
    // 휴대폰 폭(13px)에서도 한글이 두 칸에 꼭 맞는다
    const cols = aaa("pane", "list").panes.find((x) => x.name === "t1").cols;
    console.log("     글꼴 측정(390×844)", JSON.stringify(await fontOk(rp, cols)));
    const banner = await rp.getByTestId("readonly-banner").innerText();
    if (!banner.includes("보기 권한")) throw new Error(`reader 배너: ${banner}`);
    if ((await rp.getByLabel("보낼 글").count()) !== 0) throw new Error("reader에게 입력 막대");
    await rp.getByTestId("terminal").click();
    // 키 입력은 그 키 사건 안에서 보낸다: 다 친 뒤 이 화면이 보낸 입력이 없으면 PTY에 갈 것이 없다(host 쪽 거부는 11 「그림 붙여 넣기」 5))
    const k = await sentCount(rp);
    await rp.keyboard.type("echo reader-$((1+1))\n");
    const typed = (await sentFrames(rp, k)).filter((f) => f.t === "input" || f.t === "bin" || f.t === "submit");
    if (typed.length) throw new Error(`reader 화면이 입력을 보냈다: ${JSON.stringify(typed)}`);
    const lines = aaa("pane", "read", "t1", "--source", "visible").text;
    if (lines.includes("reader-")) throw new Error("reader 입력이 PTY에 들어갔다");
    await shot(rp, "mobile-reader");
    if (rd.errors.length) throw new Error(rd.errors.join("; "));
    await rd.page.context().close();
    // 낮은 화면의 보기 권한: 데스크톱 grid가 칸보다 높으면 칸이 굴러가고 커서 줄을 보이게 따라간다(herdr-web-ui followCursor)
    // t1 화면은 뒤 단계가 본다: 따로 pane을 띄운다(80×40, 줄 80개 뒤 커서는 맨 아래)
    aaa("tab", "create", "--machine", "labA", "--label", "follow", "--cols", "80", "--rows", "40", "--", "/bin/sh", "-c", "seq 1 80; echo follow-$((6*7)); exec cat");
    const low = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 420 });
    await low.page.getByTestId("pane-follow").click();
    const followed = () =>
      low.page.evaluate(() => {
        const scroll = document.querySelector(".term-scroll");
        const box = scroll.getBoundingClientRect();
        // 보기만 하는 화면은 focus가 없어 커서 칸이 따로 없다: 커서 바로 위 줄(follow-42)을 본다
        const row = [...document.querySelectorAll(".xterm-rows > div")].find((r) => r.textContent.includes("follow-42"));
        const cursor = row?.getBoundingClientRect();
        return {
          text: row !== undefined,
          scrollTop: scroll.scrollTop,
          scrollHeight: scroll.scrollHeight,
          clientHeight: scroll.clientHeight,
          box: [Math.round(box.top), Math.round(box.bottom)],
          cursor: cursor ? [Math.round(cursor.top), Math.round(cursor.bottom)] : null,
        };
      });
    let last = null;
    await until("커서 줄을 따라감", async () => {
      last = await followed();
      return last.text && last.cursor && last.scrollTop > 0 && last.cursor[0] >= last.box[0] && last.cursor[1] <= last.box[1] + 1;
    }).catch((e) => {
      throw new Error(`${e.message}: ${JSON.stringify(last)}`);
    });
    await low.page.context().close();
    aaa("pane", "kill", "follow");
  });

  await step("controller: 새 pane 창(herdr-web-ui NewSessionDialog): 없는 폴더는 창 안 오류, 셸·폴더(이름은 폴더 이름), profile agent·최근 폴더·이름, Mod+Shift+O", async () => {
    const panes = () => aaa("pane", "list").panes;
    const before = panes().length;
    const dialog = page.getByRole("dialog", { name: "새 pane · labA" });
    const start = () => dialog.getByRole("button", { name: "시작", exact: true }).click();
    const hash = () => page.evaluate(() => decodeURIComponent(location.hash));
    await page.getByTestId("machine-labA").getByRole("button", { name: "labA에 새 pane" }).click();
    await dialog.waitFor();
    // 없는 폴더: agent가 거절한 글이 창 안에 보이고 창은 남는다(pane은 생기지 않는다)
    await dialog.getByRole("textbox", { name: "폴더" }).fill(`${root}/no-such-dir`);
    await start();
    const alert = dialog.getByRole("alert");
    await alert.waitFor();
    if (!(await alert.innerText()).includes("no-such-dir")) throw new Error(`없는 폴더 오류: ${await alert.innerText()}`);
    if (panes().length !== before) throw new Error("없는 폴더로 pane이 생겼다");
    // 셸: 이름을 비우면 폴더 이름(참고 이름 칸 placeholder), 만든 pane을 연다. 폴더는 저장소다: 실험실(/tmp)은 참고 passingFolder가
    // 임시 폴더로 보아 최근 폴더에 두지 않는다
    await dialog.getByRole("textbox", { name: "폴더" }).fill(repo);
    if ((await dialog.getByRole("textbox", { name: "이름" }).getAttribute("placeholder")) !== path.basename(repo)) throw new Error("이름 칸 placeholder가 폴더 이름이 아니다");
    await start();
    await dialog.waitFor({ state: "detached" });
    let shell;
    await until("셸 pane", () => (shell = panes().find((p) => p.name === path.basename(repo) && p.cwd === repo)) !== undefined);
    await until("셸 pane을 연다", async () => (await hash()) === `#pane=${shell.id}`);
    // profile agent: 이 머신 agent.toml의 fake, 최근 폴더 칩(방금 쓴 폴더), 이름
    await page.getByTestId("machine-labA").getByRole("button", { name: "labA에 새 pane" }).click();
    await dialog.waitFor();
    await dialog.getByRole("combobox").click();
    await page.getByRole("option", { name: "fake" }).click();
    await dialog.getByRole("button", { name: `${repo} 폴더 사용` }).click();
    if ((await dialog.getByRole("textbox", { name: "폴더" }).inputValue()) !== repo) throw new Error("최근 폴더 칩이 폴더 칸을 채우지 않았다");
    await dialog.getByRole("textbox", { name: "이름" }).fill("새-에이전트");
    await shot(page, "desktop-new-pane");
    await start();
    await dialog.waitFor({ state: "detached" });
    let agent;
    await until("profile pane", () => (agent = panes().find((p) => p.name === "새-에이전트")) !== undefined);
    if (!agent.argv.join(" ").includes("fake.sh") || agent.cwd !== repo) throw new Error(`profile pane: ${JSON.stringify(agent)}`);
    await until("profile pane을 연다", async () => (await hash()) === `#pane=${agent.id}` && (await screenText(page)).includes("fake>"));
    // 단축키(참고 new-session Mod+Shift+O): 고른 pane의 머신으로 열고, 마지막에 고른 agent를 기억한다. Esc로 닫는다
    await page.getByTestId("terminal").click();
    await page.keyboard.press("ControlOrMeta+Shift+O");
    await dialog.waitFor();
    // 기억한 agent는 이 머신 profile 목록이 온 뒤에 선다(없는 profile이면 셸로)
    await until("마지막에 고른 agent(fake)를 기억", async () => (await dialog.getByRole("combobox").innerText()).trim() === "fake");
    await page.keyboard.press("Escape");
    await dialog.waitFor({ state: "detached" });
    aaa("pane", "kill", agent.id);
    aaa("pane", "kill", shell.id);
    await page.getByTestId("pane-t1").click();
  });
}
