// 웹 E2E 터미널 기본: 키보드·크기·글꼴·글자 폭·입력 막대·두 controller·reader·새 셸.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 02-terminal(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import path from "node:path";
import {
  repo, root, sleep, aaa, until, screenText, shot, fontOk, pairPage, base, step, browser, openSettings, sentFrames,
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
      // 오른쪽에는 xterm 세로 scrollbar 자리(기본 14px)가 남는다
      const ok = m.sw <= m.w + 1 && m.w - m.sw < cw + 15 && m.sh <= m.h + 1 && m.h - m.sh < ch + 1;
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
    await op.getByTestId("pane-t1").click();
    await until("둘째 화면 터미널", async () => (await screenText(op)).includes("own-3"));
    // 「다른 화면이 크기를 정하는 중」 띠와 「화면에 맞추기」 단추는 참고에 없어 없앴다
    if ((await op.getByTestId("lease-banner").count()) !== 0) throw new Error("크기를 정하는 중 띠가 있다");
    if ((await op.getByRole("button", { name: "화면에 맞추기" }).count()) !== 0) throw new Error("화면에 맞추기 단추가 있다");
    // 입력 막대가 켜져 있다(보기 권한이 아니면 언제나 입력된다). 열기만 해서는 크기를 바꾸지 않는다
    if (await op.getByLabel("보낼 글").isDisabled()) throw new Error("다른 화면이 쓰는 중이라고 입력 막대가 꺼졌다");
    await sleep(600);
    if (t1size() !== held || holder() !== first) throw new Error(`열기만 했는데 크기·쓰는 화면이 바뀌었다: ${held} → ${t1size()}`);
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
    if ((await rp.getByRole("button", { name: "새 셸" }).count()) !== 0) throw new Error("reader에게 새 셸 버튼");
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
    const k = (await sentFrames(rp)).length;
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

  await step("controller: 새 셸", async () => {
    const before = aaa("pane", "list").panes.length;
    await page.getByTestId("machine-labA").getByRole("button", { name: "새 셸" }).click();
    await until("새 pane", () => aaa("pane", "list").panes.length === before + 1);
    await page.getByTestId("pane-t1").click();
  });
}
