// 웹 E2E thread와 알림: Web Push 구독·보고서·보고 알림·이 기기 알림·끝난 thread·줄 메뉴·사후 평가자.
// 쓰는 화면·쓰는 기기에 따른 push 판정은 17-push-in-use, 멈춘 쓰는 화면은 18-push-stalled.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 12-threads-push(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.
// 단계는 앞 단계가 남긴 것(구독·demo·t-0001·t-0002)에 기댄다: 뒤 단계 하나만 골라(--grep) 돌리면 그것이 없다(--spec으로 파일을 돈다)
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import {
  root, aaa, until, domIs, hashIs, screenText, shot, pairPage, pushes, pushPort, pushOrigin, b64u, pushDevice, nextPush, pushKey,
  step, browser, base, setPushKey, openSettings, focusIs, setInUse, pushInUse, coordinatorPost, inList, showPanel, drawerSettled,
} from "../lib/harness.mjs";

import { prepareThreadPages, threadPages } from "../lib/thread-pages.mjs";
import { threadBodyLifetime, threadFailureLifetime } from "../lib/thread-lifetime.mjs";

/**
 * 목록 줄의 오른쪽 끝(보이는 ⋯, 없으면 상태 칸)과 목록 오른쪽 끝의 거리가 목록 여백(--side-gutter)과 경계선 안이다(t-0370:
 * 휴대폰 ⋯ 뒤 빈 상태 칸). 고정 카드 안 줄은 카드 안쪽 여백만큼 더 들어간다. ⋯은 손가락 크기(--touch-target 40px)를 지킨다
 */
async function rowEndsNearEdge(p, what) {
  const rows = await p.evaluate(() => {
    const shell = document.querySelector(".sidebar-shell");
    const px = (el, ...props) => props.reduce((sum, prop) => sum + parseFloat(getComputedStyle(el).getPropertyValue(prop)), 0);
    const edge = shell.getBoundingClientRect().right - px(shell, "border-right-width");
    const shown = [...shell.querySelectorAll(".workspace-header")].filter((row) => row.getClientRects().length);
    return shown.map((row) => {
      const card = row.closest(".sidebar-pinned");
      const allowed = px(shell, "--side-gutter") + (card ? px(card, "padding-right", "border-right-width") : 0);
      const more = row.querySelector(":scope > .workspace-actions .row-menu-toggle");
      const end = more?.getClientRects().length ? more : row.querySelector(".sidebar-pane-meta");
      const r = end.getBoundingClientRect();
      return { name: row.querySelector(".workspace-name")?.textContent, more: end === more, gap: edge - r.right, allowed, width: r.width };
    });
  });
  const bad = rows.filter((r) => r.gap > r.allowed + 0.5 || (r.more && r.width < 40));
  if (!rows.length || bad.length) throw new Error(`${what}: 줄 끝이 목록 끝에서 멀다 ${JSON.stringify(bad.length ? bad : rows)}`);
  return rows;
}

export default async function ({ page }) {
  const pushDev = pushDevice("push/dev1");
  const goneDev = pushDevice("gone/dev2");
  await step("Web Push: 구독 검사 → 시험 알림이 암호화·서명돼 온다(알림을 띄우는 service worker는 01 「service worker 알림」)", async () => {
    const api = (path, body) =>
      page.evaluate(
        async ({ path, body }) => {
          const r = await fetch(path, body ? { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) } : {});
          return { status: r.status, json: await r.json() };
        },
        { path, body },
      );
    const info = await api("/api/v1/push");
    if (!info.json.ok?.enabled || info.json.ok.subscribed !== 0) throw new Error(JSON.stringify(info.json));
    setPushKey(info.json.ok.key);
    if (Buffer.from(pushKey, "base64url").length !== 65) throw new Error(`VAPID 공개키 ${pushKey}`);
    // 허용하지 않은 곳·loopback이라도 목록에 없는 이름·틀린 키는 받지 않는다
    for (const bad of [
      { ...pushDev.json, endpoint: "https://evil.example/push" },
      { ...pushDev.json, endpoint: `http://localhost:${pushPort}/x` },
      { ...pushDev.json, keys: { ...pushDev.json.keys, p256dh: b64u(Buffer.alloc(65, 1)).slice(0, 40) } },
    ]) {
      const r = await api("/api/v1/push/subscribe", bad);
      if (r.status !== 400) throw new Error(`거부하지 않았다: ${JSON.stringify(bad)} → ${r.status}`);
    }
    if ((await api("/api/v1/push/subscribe", pushDev.json)).status !== 200) throw new Error("구독 실패");
    const from = pushes.length;
    const t = await api("/api/v1/push/test", {});
    if (t.json.ok?.sent !== 1) throw new Error(JSON.stringify(t.json));
    const { msg, claims } = await nextPush(pushDev, from, "시험 알림");
    if (msg.title !== "aaa" || msg.body !== "알림이 켜졌다") throw new Error(JSON.stringify(msg));
    const now = Date.now() / 1000;
    if (claims.aud !== pushOrigin || claims.exp < now || claims.exp > now + 24 * 3600 || !/^(mailto:|https:)/.test(claims.sub)) {
      throw new Error(`claims ${JSON.stringify(claims)}`);
    }
    // 끄면 지워진다. 보고 알림을 받을 기기(dev1)와 브라우저가 버린 구독(dev2, 410)을 둔다.
    if (!(await api("/api/v1/push/unsubscribe", { endpoint: pushDev.json.endpoint })).json.ok?.removed) throw new Error("구독 해제");
    if ((await api("/api/v1/push")).json.ok.subscribed !== 0) throw new Error("해제 뒤에도 구독이 남았다");
    for (const d of [pushDev, goneDev]) {
      if ((await api("/api/v1/push/subscribe", d.json)).status !== 200) throw new Error("다시 구독 실패");
    }
    await inList(page, "pane-t1");
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  const pushFrom = pushes.length;

  await step("thread 보고서·결과물: Markdown 보고, sandbox 결과물, agent에 남은 큰 파일, ack", async () => {
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse(page)) === 0);
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task.md`, "웹 보기");
    const t = aaa("thread", "start", "demo", "--title", "웹 결과물", "--machine", "labA", "--profile", "fake", "--kind", "dir",
      "--task-file", `${root}/task.md`);
    await until("보고 회수", () => aaa("thread", "show", "demo", t.id).report_hash);
    const item = page.getByTestId(`thread-demo-${t.id}`);
    await until("thread 목록에 검토 대기", async () => (await item.innerText()).includes("검토 대기"));
    // thread 줄은 그 agent pane을 열고(차이 목록 L4, herdr-projects: thread는 자기 agent 줄) 보고서는 터미널 머리 「보고」로
    await item.click();
    await page.getByTestId("open-report").click();
    const report = page.getByTestId("report");
    await until("보고 Markdown", async () => (await report.locator("strong").first().innerText()) === "확인");
    // herdr-web-ui Markdown대로 그린다(차이 목록 R1): 표·굵게 안 code·링크·번호 목록 안 목록·code block
    const md = await report.locator(".markdown").evaluate((m) => ({
      firstCode: m.querySelector("li code")?.textContent,
      strongCode: m.querySelector("strong > code")?.textContent,
      link: [m.querySelector("a[href='https://example.com/doc']")?.textContent, m.querySelector("a[href='https://example.com/doc']")?.target],
      head: [...m.querySelectorAll(".markdown-table-wrap table thead th")].map((x) => x.textContent),
      cells: [...m.querySelectorAll(".markdown-table-wrap table tbody tr")].map((r) => [...r.children].map((c) => c.innerHTML.replace(/<\/?span>/g, ""))),
      nested: m.querySelector("ol > li > ul > li code")?.textContent,
      ol: [...m.querySelectorAll(":scope > ol > li")].length,
      fence: [m.querySelector(".markdown-code")?.dataset.language, m.querySelector(".markdown-code pre code")?.textContent],
      raw: /\*\*|\|/.test(m.textContent),
      // code block 안 code는 바탕을 따로 갖지 않는다(참고 `.markdown :not(pre) > code`만 칩)
      fenceBg: getComputedStyle(m.querySelector(".markdown-code pre code")).backgroundColor,
    }));
    const want = {
      firstCode: "index.html",
      strongCode: "a1b2c3d",
      link: ["참고 문서", "_blank"],
      head: ["ID", "판정", "근거"],
      // 경로로 읽히는 code는 그 thread 작업 폴더의 파일 단추다(참고 Markdown `codeIsFilePath`, plan §8.171)
      cells: [
        ["<code>T2</code>", "같음", '<button type="button" class="markdown-file is-code" title="prefs.rs 열기"><code>prefs.rs</code></button>'],
        ["R1", "<strong>일부</strong>", "표와 <code>code</code>"],
        ["Q9", "휴대폰 표 칸의 한글 낱말은 음절 사이에서 끊기지 않는다", "경우마다 메모를 남긴다"],
      ],
      nested: "하나",
      ol: 2,
      fence: ["sh", "cargo test -p aaa"],
      raw: false,
      fenceBg: "rgba(0, 0, 0, 0)",
    };
    if (JSON.stringify(md) !== JSON.stringify(want)) throw new Error(`보고 Markdown ${JSON.stringify(md)}`);
    // 보고 iframe의 첫 이동이 파일 보기의 pushState 뒤에 끼면 Back은 iframe 기록을 고르고
    // page.goBack은 주 문서 이동 없이 만료된다. 결과물 준비를 먼저 마친 뒤 파일 보기 기록을 검사한다.
    const frameEl = await page.getByTestId("library-frame").elementHandle();
    const frame = await until("결과물 frame", async () => frameEl && (await frameEl.contentFrame()));
    const probe = await until("결과물 HTML 실행", async () => {
      const d = await frame.evaluate(() => ({
        title: document.getElementById("t")?.textContent,
        pic: document.getElementById("pic")?.naturalWidth,
        ...document.body.dataset,
      }));
      // 제어 API 시험(api)이 그림을 다 읽기보다 먼저 끝날 수 있다: 그림까지 기다린다
      return d.api && d.pic ? d : null;
    });
    if (probe.title !== "결과 보고" || probe.pic !== 1) throw new Error(`결과물 표시: ${JSON.stringify(probe)}`);
    // opaque origin: 부모 문서·cookie·제어 API에 닿지 못한다
    if (probe.parent !== "blocked" || probe.cookie === "visible" || probe.api === "200") {
      throw new Error(`결과물이 격리되지 않았다: ${JSON.stringify(probe)}`);
    }
    // 보고서 안 파일 링크(1장 J 나머지, plan §8.171): 그 thread의 머신·worktree(agent가 시작한 폴더) 기준으로 찾아 앱 안 파일 보기
    // (`/f/` 읽기 경로)로 연다. 보고·결과물이 있는 `.aaa/…`(work_dir) 기준이 아니다
    const cwd = aaa("thread", "show", "demo", t.id).cwd;
    const fileViewer = page.getByTestId("file-viewer");
    await report.locator(".markdown-file", { hasText: "작업 메모" }).click();
    await until("작업 메모(worktree notes/plan.md)", async () => (await fileViewer.getByTestId("file-viewer-markdown").locator(".markdown-h1").textContent().catch(() => null)) === "작업 메모");
    const opened = await fileViewer.evaluate((v) => ({ path: v.querySelector(".file-viewer-path")?.textContent, newTab: v.querySelector('a[aria-label="새 탭에서 열기"]')?.getAttribute("href") }));
    if (!/^\/f\/[0-9a-f]+\/plan\.md$/.test(opened.newTab ?? "") || !opened.path?.includes(`${cwd.split("/").pop()}/notes`)) throw new Error(`작업 메모 ${JSON.stringify({ cwd, ...opened })}`);
    await page.goBack();
    await domIs(page, "작업 메모 보기 닫힘", () => !document.querySelector('[data-testid="file-viewer"]') && !!document.querySelector('[data-testid="report"]'));
    // 글 속 경로의 줄 참조는 소스로 그 줄을 보인다
    await report.locator(".markdown-file", { hasText: "src/app.ts:3" }).click();
    await until("줄 참조(app.ts 3번째 줄)", async () => (await fileViewer.locator(".hl-line.is-marked").textContent().catch(() => "")).includes("fixed"));
    await shot(page, "desktop-report-file-link");
    await page.goBack();
    await domIs(page, "줄 참조 보기 닫힘", () => !document.querySelector('[data-testid="file-viewer"]'));
    // 작업 폴더에 없는 파일은 까닭을 말한다(결과물 목록에서 찾지 않는다)
    await report.locator(".markdown-file", { hasText: "prefs.rs" }).click();
    await domIs(page, "없는 파일 알림", () => document.querySelector('[data-testid="file-viewer"] [role="alert"]')?.textContent === "이 경로에 읽을 수 있는 파일이 없습니다.");
    await page.goBack();
    await domIs(page, "없는 파일 보기 닫힘", () => !document.querySelector('[data-testid="file-viewer"]'));
    await page.getByTestId("report-copy-partial").waitFor();
    const missing = page.getByTestId("library-copy-missing");
    await missing.locator("summary").click();
    await missing.getByText("big.bin", { exact: true }).waitFor();
    const big = page.getByTestId("files").locator("li", { hasText: "big.bin" });
    if (!(await big.innerText()).includes("사본에 없음 · agent에서 읽음")) throw new Error(await big.innerText());
    await shot(page, "desktop-thread-library");
    // 결과물 파일은 새 탭 대신 앱 안 파일 보기로 연다(차이 목록 R1): 같은 보기 경로(/v/)로 읽고, 문서 안 그림·링크는 결과물 안에서 푼다
    const viewer = page.getByTestId("file-viewer");
    await page.getByTestId("files").getByTestId("library-file").filter({ hasText: "docs/notes.md" }).click();
    await until("결과물 Markdown 보기", async () => (await viewer.getByTestId("file-viewer-markdown").locator(".markdown-h1").textContent().catch(() => null)) === "결과 메모").catch(async (error) => {
      // locator의 null만 남기지 않고 보기의 오류·읽기 단계와 해당 자료의 요청 시간을 보존한다.
      const state = await page.evaluate(() => {
        const viewer = document.querySelector('[data-testid="file-viewer"]');
        return { visibility: document.visibilityState, focused: document.hasFocus(), present: !!viewer,
          title: viewer?.querySelector('h1, h2')?.textContent,
          heading: viewer?.querySelector('.markdown-h1')?.textContent,
          problem: viewer?.querySelector('[role="alert"]')?.textContent,
          note: viewer?.querySelector('.file-viewer-note')?.textContent,
          resources: performance.getEntriesByType("resource").filter((r) => new URL(r.name).pathname.endsWith("/docs/notes.md"))
            .map((r) => ({ type: r.initiatorType, duration: r.duration, bytes: r.transferSize, size: r.decodedBodySize })) };
      });
      throw new Error(`${error.message}: ${JSON.stringify(state)}`);
    });
    const pic = await until("Markdown 안 그림(../pic.png)", async () => {
      const w = await viewer.locator(".file-viewer-markdown img.markdown-image").evaluate((i) => (i.complete ? i.naturalWidth : 0)).catch(() => 0);
      return w > 0 ? w : null;
    });
    const head = await viewer.evaluate((v) => ({ path: v.querySelector(".file-viewer-path")?.textContent, newTab: v.querySelector('a[aria-label="새 탭에서 열기"]')?.getAttribute("href") }));
    if (pic !== 1 || head.path !== "·docs" || !/^\/v\/[0-9a-f]+\/docs\/notes\.md$/.test(head.newTab ?? "")) throw new Error(`결과물 Markdown ${JSON.stringify({ pic, ...head })}`);
    await shot(page, "desktop-library-viewer-markdown");
    // 문서의 링크(../index.html)는 같은 칸에서 그 결과물 페이지로: 보기 경로의 sandbox 그대로 opaque origin이다
    await viewer.locator(".markdown-file", { hasText: "결과 페이지" }).click();
    const pageEl = await until("결과물 페이지 frame", async () => (await viewer.locator("iframe.file-viewer-page").count()) === 1 && viewer.locator("iframe.file-viewer-page"));
    const inner = await until("결과물 페이지 실행", async () => {
      const f = await (await pageEl.elementHandle())?.contentFrame();
      const d = f && (await f.evaluate(() => ({ title: document.getElementById("t")?.textContent, pic: document.getElementById("pic")?.naturalWidth, ...document.body.dataset })).catch(() => null));
      return d?.api && d.pic ? d : null;
    });
    if (inner.title !== "결과 보고" || inner.pic !== 1 || inner.parent !== "blocked" || inner.cookie === "visible" || inner.api === "200") {
      throw new Error(`결과물 페이지 ${JSON.stringify(inner)}`);
    }
    await viewer.getByRole("button", { name: "소스" }).click();
    await until("결과물 페이지 소스", async () => (await viewer.getByTestId("file-viewer-code").textContent().catch(() => "")).includes('id=t>결과 보고'));
    // 내려받기는 attachment로 온다
    const dl = await viewer.locator('a[aria-label="내려받기"]').getAttribute("href");
    const got = await page.evaluate(async (u) => {
      const r = await fetch(u);
      return [r.status, r.headers.get("content-disposition")];
    }, dl);
    if (!dl.endsWith("/index.html?download=1") || got[0] !== 200 || got[1] !== "attachment; filename*=UTF-8''index.html") throw new Error(`내려받기 ${dl} ${JSON.stringify(got)}`);
    // 다른 파일을 골라도 기록 칸은 하나다: 뒤로 한 번에 닫히고 thread 보기로 돌아온다
    await page.goBack();
    await domIs(page, "결과물 파일 보기 닫힘", () => !document.querySelector('[data-testid="file-viewer"]') && !!document.querySelector('[data-testid="thread-view"]'));
    // agent에 남은 큰 binary: 내려받으라고 한다
    await page.getByTestId("files").getByTestId("library-file").filter({ hasText: "big.bin" }).click();
    await until("binary 안내", async () => (await viewer.locator(".file-viewer-note").textContent().catch(() => "")).includes("내려받아 여세요"));
    await viewer.getByRole("button", { name: "파일 닫기" }).click();
    await domIs(page, "binary 보기 닫힘", () => !document.querySelector('[data-testid="file-viewer"]'));
    // 휴대폰 폭: 넓은 표는 그 안에서만 옆으로 밀리고 화면은 밀리지 않는다(참고 .markdown-table-wrap)
    await page.setViewportSize({ width: 390, height: 844 });
    await report.locator(".markdown-table-wrap").scrollIntoViewIfNeeded();
    const wide = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (wide > 0) throw new Error(`휴대폰 폭에서 화면이 ${wide}px 옆으로 밀린다`);
    // 표 칸의 한글 낱말은 한 줄에 남는다(QA t-0337 Q9 「경/우」): 낱말마다 그 글자들이 그려진 줄이 하나다
    const split = await report.evaluate((el) => {
      const bad = [];
      for (const cell of el.querySelectorAll(".markdown th, .markdown td")) {
        const walk = document.createTreeWalker(cell, NodeFilter.SHOW_TEXT);
        for (let n = walk.nextNode(); n; n = walk.nextNode()) {
          for (const m of n.data.matchAll(/[가-힣]+/g)) {
            const r = document.createRange();
            r.setStart(n, m.index);
            r.setEnd(n, m.index + m[0].length);
            if (new Set([...r.getClientRects()].map((x) => Math.round(x.top))).size > 1) bad.push(m[0]);
          }
        }
      }
      return bad;
    });
    if (split.length) throw new Error(`휴대폰 폭 표 칸에서 한글 낱말이 갈렸다: ${split.join(", ")}`);
    await shot(page, "phone-report-markdown");
    await page.setViewportSize({ width: 1280, height: 800 });
    // thread 보고는 앱 안에 남는다. 외부 알림은 별도로 coordinator가 판단한 사용자 post다.
    await coordinatorPost();
    await until("coordinator 알림 보냄", () => pushes.slice(pushFrom).some((x) => x.path === new URL(pushDev.json.endpoint).pathname));
    // 보고서 공유 링크: CLI·웹은 같은 자동 발급 링크를 보고, 실제 clipboard 대신 기록하는 가짜를 쓴다.
    const shared = aaa("library", "link", "demo", t.id);
    if (aaa("library", "list", "--project", "demo").reports[0].link.id !== shared.id) throw new Error("링크 목록과 발급이 다르다");
    const linkRow = page.getByTestId("library-link");
    const sharedUrl = new URL(shared.base, base).href;
    if (await linkRow.getByRole("link", { name: "보고서 공유 링크" }).getAttribute("href") !== sharedUrl) throw new Error("웹 링크가 자동 발급 링크와 다르다");
    await page.evaluate(() => {
      window.__libraryCopies = [];
      Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text) => window.__libraryCopies.push(text) } });
    });
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 800 });
      const copiesBefore = await page.evaluate(() => window.__libraryCopies.length);
      await linkRow.getByRole("button", { name: "링크 복사" }).click();
      await until("이번 공유 링크 복사 기록", () => page.evaluate((n) => window.__libraryCopies.length === n + 1, copiesBefore));
      await until("공유 링크 복사", async () => (await linkRow.getByRole("status").textContent()) === "링크를 복사했다");
      if ((await page.evaluate(() => window.__libraryCopies.at(-1))) !== sharedUrl) throw new Error("다른 링크를 복사했다");
      await linkRow.getByRole("button", { name: "QR 코드", exact: true }).click();
      await linkRow.getByRole("img", { name: "보고서 공유 링크 QR 코드" }).waitFor();
      if (await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)) throw new Error("공유 링크가 휴대폰 폭을 넘는다");
      await shot(page, width === 390 ? "phone-library-link" : "desktop-library-link");
      await linkRow.getByRole("button", { name: "QR 코드", exact: true }).click();
    }
    // pairing한 cookie가 이미 있는 문맥에서도 보고서 JS의 fetch는 CSP·opaque origin에 막힌다.
    const pairedReport = await page.context().newPage();
    try {
      await pairedReport.goto(sharedUrl);
      const api = await until("공유 보고서 JS의 API 접근 판정", () => pairedReport.evaluate(() => document.body.dataset.api));
      if (api !== "blocked") throw new Error(`보고서 JS가 앱 API를 읽었다: ${api}`);
    } finally { await pairedReport.close(); }
    // 텔레그램처럼 cookie 없는 새 context에서 top-level로 연다. SW가 있는 앱 자리에서도 보고서가 그대로 열린다.
    const outside = await browser.newContext({ viewport: { width: 390, height: 844 } });
    try {
      const p = await outside.newPage();
      await p.goto(base);
      await until("공유 링크 시험 SW 준비", () => p.evaluate(() => !!navigator.serviceWorker.controller));
      const response = await p.goto(sharedUrl);
      const sharedHeaders = await response.allHeaders();
      if (sharedHeaders["set-cookie"]) throw new Error("보고서 링크 응답이 Set-Cookie로 세션을 만들었다");
      if (response.status() !== 200 || !sharedHeaders["content-security-policy"].includes("sandbox")) throw new Error("공유 보고서가 sandbox로 열리지 않았다");
      await until("공유 보고서 형제 그림", () => p.locator("#pic").evaluate((img) => img.naturalWidth === 1));
      const isolation = await p.evaluate(async () => {
        const result = {};
        for (const [name, read] of [["cookie", () => document.cookie], ["storage", () => localStorage.getItem("aaa")]]) {
          try { read(); result[name] = "open"; } catch { result[name] = "blocked"; }
        }
        try { await navigator.serviceWorker.register("/sw.js"); result.sw = "open"; } catch { result.sw = "blocked"; }
        return { ...result, title: document.getElementById("t")?.textContent, api: document.body.dataset.api };
      });
      if (isolation.title !== "결과 보고" || isolation.cookie !== "blocked" || isolation.storage !== "blocked" || isolation.sw !== "blocked" || isolation.api === "200") throw new Error(`공유 페이지 격리 ${JSON.stringify(isolation)}`);
      // 서명을 query·Authorization에 내도 장치 token으로 받아서는 안 된다.
      const signature = shared.base.split("/")[2];
      await outside.setExtraHTTPHeaders({ authorization: `Bearer ${signature}` });
      if ((await outside.request.get(`${base}api/v1/me?token=${signature}`)).status() !== 401) throw new Error("공유 링크가 장치 권한을 얻었다");
      if ((await outside.request.post(`${base}api/v1/call`, { headers: { origin: "null" }, data: { op: "panes" } })).status() !== 403) throw new Error("opaque origin의 API 변경을 허용했다");
      if ((await outside.request.post(`${base}api/v1/call`, { headers: { origin: base.slice(0, -1) }, data: { op: "panes" } })).status() !== 401) throw new Error("링크 문맥의 Front API가 pairing 없이 열렸다");
      for (const path of ["api/v1/events", "api/v1/front", `api/v1/panes/${t.pane}/ws`]) {
        const ws = await outside.request.get(`${base}${path}?token=${signature}`, { headers: {
          origin: base.slice(0, -1), connection: "Upgrade", upgrade: "websocket",
          "sec-websocket-version": "13", "sec-websocket-key": "dGhlIHNhbXBsZSBub25jZQ==",
        } });
        if (ws.status() !== 401) throw new Error(`링크 문맥 WebSocket이 ${ws.status()}: ${path}`);
      }
      if ((await outside.request.get(`${base}f/unknown/a.txt`)).status() !== 401) throw new Error("공유 페이지가 pane 파일을 읽었다");
      await shot(p, "phone-library-shared");
      await p.goto(`${base}?token=${signature}`);
      await p.getByTestId("token-gate").waitFor();
      if ((await outside.cookies()).length) throw new Error("보고서 링크가 브라우저에 cookie를 남겼다");
      // 장치 인증의 실제 입력(cookie)에 도장을 넣어도 장치 권한으로 바뀌지 않는다.
      await outside.addCookies([{ name: "aaa", value: signature, url: base }]);
      if ((await outside.request.get(`${base}api/v1/me`)).status() !== 401) throw new Error("서명 도장을 장치 cookie로 받았다");
      if ((await outside.request.post(`${base}api/v1/call`, { headers: { origin: base.slice(0, -1) }, data: { op: "panes" } })).status() !== 401) throw new Error("서명 cookie로 Front API가 열렸다");
      for (const path of ["api/v1/events", "api/v1/front", `api/v1/panes/${t.pane}/ws`]) {
        const ws = await outside.request.get(`${base}${path}`, { headers: {
          origin: base.slice(0, -1), connection: "Upgrade", upgrade: "websocket",
          "sec-websocket-version": "13", "sec-websocket-key": "dGhlIHNhbXBsZSBub25jZQ==",
        } });
        if (ws.status() !== 401) throw new Error(`서명 cookie로 WebSocket이 열렸다: ${path}`);
      }
      await outside.clearCookies();
      // /v/ SW 제외를 빼면 offline navigation이 앱 셸을 돌려줘 여기서 실패한다.
      await p.goto(base);
      await until("offline 공유 링크 시험 SW 준비", () => p.evaluate(() => !!navigator.serviceWorker.controller));
      await outside.setOffline(true);
      let intercepted = false;
      try { await p.goto(sharedUrl); intercepted = true; } catch { /* 네트워크로만 읽는 보고서는 offline에서 열리지 않는다 */ }
      if (intercepted) throw new Error("SW가 공유 링크를 앱 셸로 바꿨다");
      await outside.setOffline(false);
    } finally { await outside.close(); }
    // 휴대폰 폭의 폐기와 재발급. 앱 안 보기 경로는 그대로 열려 있다.
    await linkRow.getByRole("button", { name: "링크 폐기" }).click();
    await domIs(page, "공유 링크 폐기", () => document.querySelector('[data-testid="library-link"]')?.dataset.state === "revoked");
    if ((await fetch(sharedUrl)).status !== 401) throw new Error("폐기한 공유 링크가 열린다");
    const inApp = aaa("thread", "library", "demo", t.id);
    if ((await fetch(new URL(`${inApp.base}index.html`, base))).status !== 200) throw new Error("폐기로 앱 안 보기도 끝났다");
    await shot(page, "phone-library-link-revoked");
    await linkRow.getByRole("button", { name: "새 링크 만들기" }).click();
    await domIs(page, "공유 링크 재발급", () => document.querySelector('[data-testid="library-link"]')?.dataset.state === "active");
    if (aaa("library", "link", "demo", t.id).id === shared.id) throw new Error("폐기한 id를 다시 썼다");
    await page.setViewportSize({ width: 1280, height: 800 });
    // 보기 권한 기기 확인은 보고 알림이 나간 뒤에 한다: 짝짓기에서 누르고 친 그 화면이 쓰는 기기(보임·focus)가 되어, 그 사이 나갈 알림을
    // host가 건너뛴다(알림 A, plan §8.105 — 맞는 동작). 앞에 두면 짐 아래에서 「보고 알림 보냄」이 실패했다(t-0335 찾음)
    // 보기 권한 기기: 보고서의 경로는 단추가 아니고(이름 뒤 경로), host도 거부한다. 같은 요청이 조작 기기에는 열린다
    const threadFile = (p, path) =>
      p.evaluate(
        async ([project, thread, path]) =>
          (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "thread_file", project, thread, path }) })).json(),
        ["demo", t.id, path],
      );
    // 짝지은 뒤 주소로 thread를 연다. 목록(threads)이 그 주소를 앱이 읽기 전(hashchange 처리 전)에 와도 첫 고르기(마지막 pane·첫 줄)가
    // 주소를 덮지 않는다(주소가 이긴다). 짐 아래에서 나던 차례(t-0335 찾음)를 늘 만든다: threads 답을 주소를 넣을 때까지 붙잡고,
    // 앱의 hashchange 처리는 목록이 그려지고 그 뒤 effect가 돈 다음에 푼다
    let hashSet;
    const hashed = new Promise((done) => (hashSet = done));
    const rp = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 }, {
      route: ["**/api/v1/call", async (rt) => {
        if (rt.request().postDataJSON()?.op === "threads") await hashed;
        await rt.continue().catch(() => {});
      }],
      init: () => {
        const add = window.addEventListener.bind(window);
        const queue = [];
        window.__holdHash = false;
        window.__releaseHash = () => {
          window.__holdHash = false;
          for (const run of queue.splice(0)) run();
        };
        window.addEventListener = (type, fn, opts) =>
          add(type, type !== "hashchange" ? fn : (e) => (window.__holdHash ? queue.push(() => fn(e)) : fn(e)), opts);
      },
    })).page;
    await rp.evaluate((h) => {
      window.__holdHash = true;
      location.hash = h;
    }, `#thread=demo%2F${t.id}`);
    hashSet();
    await rp.getByTestId(`thread-demo-${t.id}`).waitFor({ state: "attached" });
    await rp.evaluate(() => new Promise((done) => requestAnimationFrame(() => setTimeout(done, 0))));
    await rp.evaluate(() => window.__releaseHash());
    const readerReport = rp.getByTestId("report");
    await domIs(rp, "주소로 연 thread 보기", () => !!document.querySelector('[data-testid="thread-view"]'));
    await until("reader 보고", async () => (await readerReport.locator("strong").first().textContent().catch(() => null)) === "확인");
    const readerFiles = await readerReport.evaluate((r) => ({ buttons: r.querySelectorAll(".markdown-file").length, shown: r.textContent.includes("작업 메모 (notes/plan.md)") }));
    if (await rp.getByTestId("library-link").count()) throw new Error("보기 권한 기기에 공유 링크 조작이 보인다");
    if (readerFiles.buttons !== 0 || !readerFiles.shown) throw new Error(`보기 권한 화면의 보고서 파일 링크: ${JSON.stringify(readerFiles)}`);
    const denied = await threadFile(rp, "notes/plan.md");
    if (denied.err?.code !== "forbidden") throw new Error(`reader thread_file: ${JSON.stringify(denied)}`);
    await rp.context().close();
    const allowed = await threadFile(page, "notes/plan.md");
    if (allowed.ok?.kind !== "pane_file" || allowed.ok.info?.name !== "plan.md") throw new Error(`조작 기기 thread_file: ${JSON.stringify(allowed)}`);
    await page.getByRole("button", { name: "확인(ack)" }).click();
    await until("ack 뒤 검토 대기 아님", async () => !(await item.innerText()).includes("검토 대기"));
    if (aaa("thread", "show", "demo", t.id).acked_hash !== aaa("thread", "show", "demo", t.id).report_hash) {
      throw new Error("ack가 기록되지 않았다");
    }
    // 다음 단계는 t1 터미널을 본다
    await inList(page, "pane-t1");
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("Web Push: coordinator 전체 완료가 기기에 오고 프로젝트로 연다, 버려진 구독(410)은 지운다(차이 목록 N6)", async () => {
    const { msg } = await nextPush(pushDev, pushFrom, "보고 알림");
    // thread 보고를 먼저 만들었어도 외부로 받은 첫 알림은 coordinator post다.
    // URL은 모든 thread(plan §8.291). 옛 `/#project=demo`도 웹이 그 화면으로 연다
    if (msg.title !== "demo" || msg.tag !== "aaa-demo" || msg.body !== "coordinator 맡긴 일 완료" || msg.url !== "/#threads") {
      throw new Error(JSON.stringify(msg));
    }
    await until("410 구독 정리", async () =>
      (await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.subscribed)) === 1,
    );
    const kinds = aaa("events", "demo").events.map((e) => e.kind);
    if (!kinds.includes("push_sent") || !kinds.includes("push_gone")) throw new Error(kinds.join(","));
    // 구독 주소(기기에 보내는 열쇠)는 기록에 남지 않는다
    if (JSON.stringify(aaa("events", "demo")).includes("dev1")) throw new Error("흐름 사건에 endpoint가 남았다");
    // 쓰는 화면이면 보내지 않고 앱 안 알림(차이 목록 N5)은 「보는 동안」·「두 기기」·「쓰는 기기」가 본다. 다음 단계는 본 화면을 쓴다
    await setInUse(page, true);
    await until("쓰는 화면", async () => (await pushInUse(page)) === 1);
  });

  await step("이 기기 알림(herdr-web-ui Droplet·alertSound·notifications·더 보기, 차이 목록 N1·N2·N8·N9): 새 보고에 앱 안 알림이 내려오고 누르면 그 thread, 소리는 켜면 미리 듣고 알림마다, 가려지면 탭 알림, 앱 안을 끄면 안 내려옴, 메뉴로 끄면 아무것도, 휴대폰에서도 화면 안", async () => {
    // 소리·탭 알림·권한·push 구독은 기록하는 것으로 바꾼다(실제 소리·알림·push 서비스 없이)
    const init = () => {
      const log = [];
      Object.defineProperty(window, "__alertLog", { value: log });
      class FakeNotification {
        static permission = "granted";
        static async requestPermission() {
          return "granted";
        }
        constructor(t) {
          log.push(`new ${t}`);
        }
        addEventListener() {}
      }
      Object.defineProperty(window, "Notification", { configurable: true, value: FakeNotification });
      ServiceWorkerRegistration.prototype.showNotification = async function (t, o) {
        log.push(`tab ${t}|${o.tag}|${o.data?.url}`);
      };
      PushManager.prototype.subscribe = async () => {
        throw new DOMException("시험에서는 push 서비스가 없다", "AbortError");
      };
      PushManager.prototype.getSubscription = async () => null;
      class FakeAudio {
        state = "suspended";
        destination = {};
        // 소리 하나가 울리는 동안은 같은 소리를 겹쳐 울리지 않는다(참고 chime): 시계가 흘러야 한다
        get currentTime() {
          return performance.now() / 1000;
        }
        constructor() {
          window.__evLog?.push("ac new");
        }
        async resume() {
          window.__evLog?.push(`ac resume ${this.state}`);
          this.state = "running";
        }
        createOscillator() {
          const o = {
            type: "",
            frequency: { value: 0 },
            connect: (g) => g,
            start: (at) => {
              log.push(`tone ${o.frequency.value}`);
              // 이 소리가 끝나는 때(참고 NOTE_LENGTH_S 0.3초)
              window.__chimeEnd = Math.max(window.__chimeEnd ?? 0, at + 0.3);
              window.__evLog?.push(`osc ${o.frequency.value} at ${at.toFixed(2)} now ${this.currentTime.toFixed(2)}`);
            },
            stop() {},
          };
          return o;
        }
        createGain() {
          return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect: (d) => d };
        }
      }
      Object.defineProperty(window, "AudioContext", { configurable: true, value: FakeAudio });
      // iPhone·iPad Safari의 소리 범주(Audio Session API, plan §8.83): 바꿀 때마다 남긴다
      let sessionType = "auto";
      const session = {
        get type() {
          return sessionType;
        },
        set type(v) {
          sessionType = v;
          log.push(`session ${v}`);
        },
      };
      Object.defineProperty(navigator, "audioSession", { configurable: true, value: session });
      // 진단: 상태 사건 WS의 열림·닫힘과 thread 사건(보고 hash)
      const ev = [];
      Object.defineProperty(window, "__evLog", { value: ev });
      new MutationObserver((ms) => {
        for (const m of ms) for (const n of m.addedNodes) if (n.classList?.contains("droplet")) ev.push(`drop ${n.dataset.kind}`);
      }).observe(document, { childList: true, subtree: true });
      addEventListener("pagehide", () => ev.push("pagehide"));
      const RealBC = window.BroadcastChannel;
      window.BroadcastChannel = class extends RealBC {
        postMessage(m) {
          ev.push(`bc ${m?.type} @${(performance.now() / 1000).toFixed(2)}`);
          super.postMessage(m);
        }
      };
      const Real = window.WebSocket;
      window.WebSocket = class extends Real {
        constructor(url, p) {
          super(url, p);
          if (!String(url).includes("/api/v1/events")) return;
          const t0 = performance.now();
          const at = () => Math.round(performance.now() - t0);
          this.addEventListener("open", () => ev.push(`open`));
          this.addEventListener("close", (e) => ev.push(`close ${e.code} ${at()}`));
          this.addEventListener("message", (m) => {
            try {
              const d = JSON.parse(m.data);
              if (d.ev === "thread") ev.push(`thr ${d.id} ${String(d.report_hash).slice(0, 6)} ${String(d.acked_hash).slice(0, 6)} @${(performance.now() / 1000).toFixed(2)}`);
            } catch {}
          });
        }
      };
    };
    const apOpen = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { init });
    const ap = apOpen.page;
    const log = () => ap.evaluate(() => window.__alertLog.join(","));
    const clear = () => ap.evaluate(() => void window.__alertLog.splice(0));
    // 기록을 기다린다. 실패하면 그때의 기록·고른 자리·소리 상태를 남긴다
    const waitLog = (what, ok) =>
      until(what, async () => ok(await log())).catch(async (e) => {
        const at = await ap.evaluate(() => ({ hash: location.hash, vis: document.visibilityState, sound: localStorage.getItem("aaa:alert-sound") }));
        const ev = await ap.evaluate(() => window.__evLog.slice(-16).join(","));
        throw new Error(`${e.message}: 기록 [${await log()}] ${JSON.stringify(at)} 앱 안 ${await droplet.count()} 오류 [${apOpen.errors.join("; ")}] 사건 [${ev}] 보고 ${String(aaa("thread", "show", "demo", t.id).report_hash).slice(0, 6)}`);
      });
    const droplet = ap.locator(".droplet");
    // 들어오는 움직임(참고 Droplet.css)이 끝난 모습
    const landed = (d) => d.evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished)));
    fs.writeFileSync(`${root}/task3.md`, "알림 시험");
    const t = aaa("thread", "start", "demo", "--title", "알림 시험", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task3.md`);
    const target = `#thread=demo%2F${t.id}`;
    // 첫 보고: 앱 안 알림(작업 완료). 누르면 그 thread로 가고 접힌다
    await until("앱 안 알림", async () => (await droplet.count()) === 1 && (await droplet.locator(".droplet-title").innerText()) === "알림 시험");
    if ((await droplet.getAttribute("data-kind")) !== "done" || (await droplet.locator(".droplet-detail").innerText()) !== "작업 완료") {
      throw new Error(`앱 안 알림 ${await droplet.getAttribute("data-kind")} ${await droplet.locator(".droplet-detail").innerText()}`);
    }
    await landed(droplet);
    await shot(ap, "desktop-droplet");
    await droplet.locator(".droplet-card").click();
    await until("누르면 그 thread", async () => (await ap.evaluate(() => location.hash)) === target);
    await until("접힘", async () => (await droplet.count()) === 0);
    const work = aaa("thread", "show", "demo", t.id).work_dir;
    let n = 0;
    // 새 보고를 낸다. 돌려주는 hash로 이 화면이 그 thread 사건을 받았는지 본다(evLog `thr`)
    const report = async (what, { chime = true } = {}) => {
      // 앞 소리가 울리는 동안 온 「완료」는 그 소리가 이미 알렸다고 보고 겹쳐 울리지 않는다(참고 alertSound.ts chime).
      // 알림마다 소리를 보려면 앞 소리가 끝난 뒤에 다음 보고를 낸다
      if (chime) await until("앞 소리 끝", () => ap.evaluate(() => performance.now() / 1000 > (window.__chimeEnd ?? 0)));
      const before = aaa("thread", "show", "demo", t.id).report_hash;
      fs.writeFileSync(`${work}/report.md`, `[완료] 맡긴 일 끝\n\n## Report\n- ${what} ${++n}\n`);
      return await until(`${what} 보고`, () => {
        const h = aaa("thread", "show", "demo", t.id).report_hash;
        return h !== before && h;
      });
    };
    // 이 화면이 그 보고 사건을 받아 처리하고(앱 처리기는 진단 기록 뒤에 돈다) 두 번 그린 뒤
    const handled = async (hash) => {
      await until("보고 사건 받음", () => ap.evaluate((h) => window.__evLog.some((e) => e.startsWith("thr ") && e.includes(` ${h.slice(0, 6)} `)), hash));
      await ap.evaluate(() => new Promise((r) => setTimeout(() => requestAnimationFrame(() => requestAnimationFrame(r)))));
    };
    const dropsSince = (i) => ap.evaluate((i) => window.__evLog.slice(i).filter((e) => e.startsWith("drop")).length, i);
    const evMark = () => ap.evaluate(() => window.__evLog.length);
    // 그 thread를 보고 있으면 띄우지 않는다: 다른 곳을 본다
    await inList(ap, "pane-t1");
    // 소리: 켜는 누름이 미리 듣기(작업 완료 음 880→660), 그 뒤 알림마다
    const dialog = await openSettings(ap, "알림");
    await dialog.getByRole("switch", { name: "소리" }).click();
    await waitLog("미리 듣기", (l) => l === "tone 880,tone 660");
    await ap.keyboard.press("Escape");
    await clear();
    await report("소리");
    await waitLog("알림 소리", (l) => l === "tone 880,tone 660");
    await until("소리와 함께 앱 안 알림", async () => (await droplet.count()) === 1);
    // 무음 모드에서도(사용자 보고 10-18 iPhone, plan §8.83): 끈 동안은 소리 범주를 건드리지 않는다(위 기록에 session이 없다).
    // 켜는 누름이 미리 듣기이고, 소리를 낼 때만 "playback"(무음 스위치를 무시)으로 두었다가 끝나면 "auto"로 되돌린다
    await openSettings(ap, "알림");
    const silent = dialog.getByRole("switch", { name: "무음 모드에서도" });
    await until("앞 소리 끝", () => ap.evaluate(() => performance.now() / 1000 > (window.__chimeEnd ?? 0)));
    await clear();
    await silent.click();
    await waitLog("무음에서도 미리 듣기", (l) => l === "session playback,tone 880,tone 660,session auto");
    await ap.keyboard.press("Escape");
    // 무음 모드에서 알림마다 울리는 소리는 15 「iPhone 소리」가 iOS 규칙(누른 직후·interrupted)과 함께 본다
    await openSettings(ap, "알림");
    await silent.click();
    await ap.keyboard.press("Escape");
    // 가려지면: 앱 안 알림은 없고, 소리와 탭 알림(이 기기에 push 구독이 없다)
    const hide = (h) =>
      ap.evaluate((h) => {
        Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (h ? "hidden" : "visible") });
        Object.defineProperty(document, "hidden", { configurable: true, get: () => h });
        document.dispatchEvent(new Event("visibilitychange"));
      }, h);
    await until("앞 알림 접힘", async () => (await droplet.count()) === 0);
    await clear();
    await hide(true);
    await report("가려짐");
    await waitLog("탭 알림", (l) => l.includes(`tab demo · 알림 시험|aaa-demo/${t.id}|/${target}`));
    // 소리는 탭끼리 누가 울릴지 정하는 짧은 틈(참고 alertTurns WINDOW_MS) 뒤에 울린다
    await waitLog("가려진 동안 소리", (l) => l.includes("tone 880,tone 660"));
    if ((await droplet.count()) !== 0) throw new Error(`가려진 동안 앱 안 알림 ${await log()}`);
    await hide(false);
    // 앱 안을 끄면 소리만
    await openSettings(ap, "알림");
    await dialog.getByRole("switch", { name: "앱 안에서" }).click();
    await ap.keyboard.press("Escape");
    await clear();
    const offMark = await evMark();
    await handled(await report("앱 안 끔"));
    await waitLog("앱 안 끈 뒤 소리", (l) => l === "tone 880,tone 660");
    if ((await dropsSince(offMark)) !== 0 || (await droplet.count()) !== 0) throw new Error("앱 안을 껐는데 내려왔다");
    // 더 보기 → 알림: 권한이 있고 구독이 없으면 「이 탭에서 켜짐」. 누르면 꺼지고 메뉴 단추에 점
    await ap.getByTestId("more").click();
    const item = ap.getByTestId("alerts-item");
    if (!(await item.innerText()).includes("이 탭에서 켜짐") || (await item.getAttribute("aria-checked")) !== "true") throw new Error(`알림 항목 ${await item.innerText()}`);
    if ((await ap.getByTestId("more").getAttribute("aria-label")) !== "더 보기") throw new Error("켜진 알림인데 점");
    await shot(ap, "desktop-more-menu");
    await item.click();
    await until("메뉴 단추 점", async () => (await ap.getByTestId("more").getAttribute("aria-label")) === "더 보기 · 알림 꺼짐");
    // 끄면 아무것도: 이 화면이 그 사건을 받아 처리한 뒤에도 소리·소리 차례·앱 안 알림·탭 알림이 없다
    await clear();
    const quietMark = await evMark();
    await handled(await report("끔"));
    const turns = await ap.evaluate((i) => window.__evLog.slice(i).filter((e) => e.startsWith("bc ")).length, quietMark);
    if ((await log()) !== "" || (await dropsSince(quietMark)) !== 0 || turns !== 0) throw new Error(`끈 뒤 [${await log()}] 소리 차례 ${turns}`);
    await ap.context().close();
    // 휴대폰(390×844): 앱 안 알림과 「더 보기」 메뉴가 화면 안, 머리 아래에 있다(겹침·잘림 없음)
    const pp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    const inside = (r, what) => {
      if (!r || r.left < 0 || r.right > 390 || r.top < 0 || r.bottom > 844) throw new Error(`${what}가 화면 밖 ${JSON.stringify(r)}`);
    };
    const rect = (l) => l.evaluate((e) => e.getBoundingClientRect().toJSON());
    await report("휴대폰", { chime: false });
    const pd = pp.locator(".droplet");
    await until("휴대폰 앱 안 알림", async () => (await pd.count()) === 1);
    await landed(pd);
    // 휴대폰 머리는 보는 화면(첫 화면은 pane)의 머리다. 목록 머리는 닫힌 drawer 안에 있다
    const head = await rect(pp.locator(".view header").first());
    const card = await rect(pd.locator(".droplet-card"));
    inside(card, "앱 안 알림");
    if (card.top < head.bottom) throw new Error(`앱 안 알림이 머리를 가린다 ${card.top} < ${head.bottom}`);
    await shot(pp, "phone-droplet");
    await pd.locator(".droplet-close").click();
    await until("휴대폰 알림 닫힘", async () => (await pd.count()) === 0);
    await inList(pp, "more");
    inside(await rect(pp.locator(".more-menu")), "더 보기 메뉴");
    const it = await rect(pp.getByTestId("alerts-item"));
    const menu = await rect(pp.locator(".more-menu"));
    if (it.bottom > menu.bottom) throw new Error(`알림 항목이 메뉴 밖으로 나온다 ${it.bottom} > ${menu.bottom}`);
    await shot(pp, "phone-more-menu");
    await pp.context().close();
    aaa("thread", "ack", "demo", t.id);
  });

  await step("끝난 thread 보기(herdr-projects popup.rs Section::Threads, 차이 목록 2장 L3·3장 C1, plan §8.291): 끝낸 thread는 목록에서 빠지고 「더 보기 → 모든 thread」(#threads)에서 묶음마다 보이며 끝남은 맨 아래 흐리게, 거르기, 고르면 보고(짧은 주소 #thread=<id>), 뒤로는 모든 thread, 옛 주소 #projects·#project=demo도 그 화면", async () => {
    const $ = (id) => `[data-testid="${id}"]`;
    if ((await page.locator($("thread-demo-t-0001")).count()) !== 1) throw new Error("t-0001이 목록에 없다");
    aaa("thread", "resolve", "demo", "t-0001");
    await domIs(page, "끝낸 thread가 목록에서 빠짐(sidebar.rs)", (q) => !document.querySelector(q), $("thread-demo-t-0001"));
    // 앞 단계들도 demo thread를 끝낸다: 끝난 수는 host가 말하는 그대로
    const ended = aaa("thread", "list", "demo").threads.filter((t) => t.group === "resolved").length;
    // 프로젝트 고르기는 없다(host는 프로젝트 하나): 더 보기가 바로 모든 thread를 연다
    await page.getByTestId("more").click();
    const toThreads = hashIs(page, "#threads");
    await page.getByTestId("threads-item").click();
    await toThreads;
    const view = page.getByTestId("all-threads");
    await domIs(page, "끝남 묶음", (q) => document.querySelector(q) !== null, `${$("all-threads")} ${$("project-group-resolved")}`);
    const done = view.getByTestId("project-group-resolved");
    if ((await done.locator("h3").innerText()) !== `끝남 (${ended})`) throw new Error(`끝남 ${ended}: ${await done.innerHTML()}`);
    // 묶음은 popup DISPLAY_ORDER대로, 끝남은 맨 아래
    const order = await view.locator("[data-testid^=project-group-]").evaluateAll((xs) => xs.map((x) => x.dataset.testid.slice("project-group-".length)));
    const rank = ["waiting_on_you", "ready_for_review", "landing", "working", "idle", "resolved"];
    if (order.at(-1) !== "resolved" || order.some((g, i) => i > 0 && rank.indexOf(g) < rank.indexOf(order[i - 1]))) throw new Error(order.join(","));
    // 끝난 줄은 흐리다(popup group_color: Resolved는 DarkGrey)
    const r = view.getByTestId("project-thread-t-0001");
    const dim = await r.locator(".project-threads-name").evaluate((n) => {
      const probe = document.createElement("span");
      probe.style.color = "var(--text-dim)";
      document.body.append(probe);
      const want = getComputedStyle(probe).color;
      probe.remove();
      return [getComputedStyle(n).color, want, n.textContent];
    });
    if (dim[0] !== dim[1] || dim[2] !== "t-0001 · 웹 결과물") throw new Error(`끝난 줄 ${JSON.stringify(dim)}`);
    await shot(page, "desktop-project-threads");
    // popup 「/」 거르기처럼 이름(id·제목)에 든 글로 고른다
    const rows = `${$("all-threads")} [data-testid^=project-thread-]`;
    const all = await page.locator(rows).count();
    const filter = view.getByRole("searchbox");
    await filter.fill("웹 결과");
    await domIs(page, "거른 줄 하나", (q) => document.querySelectorAll(q).length === 1, rows);
    await filter.fill("없는 이름");
    await domIs(page, "맞는 것 없음", (q) => document.querySelector(q)?.textContent === "맞는 thread 없음", `${$("all-threads")} [role=status]`);
    await filter.fill("");
    await domIs(page, "거르기 풂", ([q, n]) => document.querySelectorAll(q).length === n, [rows, all]);
    // 고르면 보고(popup detail, 주소는 프로젝트 없이), 뒤로는 모든 thread(popup: detail에서 목록으로)
    const toReport = hashIs(page, "#thread=t-0001");
    await r.click();
    await toReport;
    await domIs(page, "끝난 thread 보고", (q) => document.querySelector(q)?.textContent === "확인", `${$("report")} .markdown strong`);
    if ((await page.getByTestId("thread-back").getAttribute("aria-label")) !== "모든 thread로") throw new Error("뒤로 이름");
    const back = hashIs(page, "#threads");
    await page.getByTestId("thread-back").click();
    await back;
    // 옛 주소(이미 보낸 알림·저장한 링크)도 같은 화면이고 주소는 #threads로 바뀐다
    for (const old of ["#projects", "#project=demo"]) {
      await page.evaluate((h) => (location.hash = h), old);
      await until(`옛 주소 ${old}`, async () => (await page.evaluate(() => location.hash)) === "#threads" && (await view.count()) === 1);
    }
    // 휴대폰 폭: 목록 대신 이 화면, 옆으로 밀리지 않는다
    await page.setViewportSize({ width: 390, height: 844 });
    await domIs(page, "휴대폰 폭 모든 thread", (q) => document.querySelector(q)?.getBoundingClientRect().width > 0, $("all-threads"));
    const wide = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (wide > 0) throw new Error(`휴대폰 폭에서 화면이 ${wide}px 옆으로 밀린다`);
    await shot(page, "phone-project-threads");
    // 닫기(고른 것 없음)는 지금 칸을 바꾼다(replaceState, hashchange 없음, plan §8.301): 주소와 빈 화면을 본다
    await view.getByRole("button", { name: "닫기" }).click();
    await domIs(page, "닫은 뒤 주소 없음·빈 화면", () => location.hash === "" && document.querySelector(".empty-view") !== null);
    await page.setViewportSize({ width: 1280, height: 800 });
    await inList(page, "pane-t1");
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("줄 메뉴(herdr-web-ui RowMenu·Sidebar menuItems·AgentSidebar, 차이 목록 2장 L9·L10): 데스크톱은 오른쪽 클릭·휴대폰은 ⋯ sheet, 고정은 맨 앞 카드로 가고 다른 기기에도(host 공유 문서), pane 닫기·기록 지우기는 확인 창, 끝난 pane은 다시 열기, thread 줄에서 모든 thread", async () => {
    const pinWrote = (p) =>
      p.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "preferences_set").then((r) => r.request().postDataJSON());
    const opDone = (p, op) => p.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === op);
    const rowOf = (p, name) => p.locator(".machine-list li[data-pin-key]", { has: p.getByTestId(`pane-${name}`) });
    const menuItems = (p) => p.getByRole("menu").locator('[role="menuitem"]').allInnerTexts();
    // 참고대로 host가 고정을 나눈 적이 없으면 고정만 바꿔서는 보내지 않고 첫 설정 변경에 실려 간다(preferencesChange).
    // 이 단계는 다른 기기가 이미 고정을 나눈 host에서 시작한다: 빈 고정을 써 두고 화면을 다시 열어 그 문서를 읽힌다.
    await page.evaluate(() =>
      fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "preferences_set", settings: {}, pins: [] }) }),
    );
    const reread = opDone(page, "preferences");
    await page.reload();
    await reread;
    await page.getByTestId("pane-t1").waitFor();
    // 1. 데스크톱: 오른쪽 클릭 → 첫 항목에 focus, 차례는 참고대로 고정 → 닫기
    const key = await rowOf(page, "t1").getAttribute("data-pin-key");
    await page.getByTestId("pane-t1").click({ button: "right" });
    await focusIs(page, "메뉴 첫 항목", '[role="menu"] [role="menuitem"]');
    if (JSON.stringify(await menuItems(page)) !== JSON.stringify(["pane 고정", "pane 이름 바꾸기", "세션 끝내기", "pane 닫기"])) throw new Error(`메뉴 ${await menuItems(page)}`);
    if ((await page.locator(".machine-list .row-menu-toggle").first().isVisible())) throw new Error("마우스 데스크톱에 ⋯ 단추가 보인다(참고는 오른쪽 클릭)");
    await page.keyboard.press("Escape");
    await domIs(page, "Esc로 메뉴 닫힘", () => !document.querySelector('[role="menu"]'));
    // 고정: 맨 앞 카드에 핀 표시와 함께, host 공유 문서에 그 열쇠
    await page.getByTestId("pane-t1").click({ button: "right" });
    const sent = pinWrote(page);
    await page.getByRole("menuitem", { name: "pane 고정" }).click();
    if (!(await sent).pins?.includes(key)) throw new Error(`고정이 host로 가지 않았다: ${JSON.stringify(await sent)}`);
    await domIs(page, "고정 카드", (k) => document.querySelector(`.machine-list .sidebar-pinned [data-pin-key="${k}"] .sidebar-pin-mark`) !== null, key);
    await shot(page, "desktop-row-pinned");
    // 2. 휴대폰(다른 기기): 고정이 와 있고, ⋯ sheet에서 푼다 → 데스크톱에서도 풀린다
    const ph = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    await domIs(ph.page, "다른 기기의 고정 카드", (k) => document.querySelector(`.machine-list .sidebar-pinned [data-pin-key="${k}"]`) !== null, key);
    // ⋯은 drawer 오른쪽 끝 가까이(목록 여백 안): 마우스 데스크톱은 ⋯ 없이 상태 칸이 그 자리
    await rowEndsNearEdge(page, "데스크톱");
    await ph.page.locator(".view [data-testid='drawer-toggle']").click();
    await ph.page.locator("#workspace-drawer.is-open").waitFor();
    await drawerSettled(ph.page);
    if (!(await rowEndsNearEdge(ph.page, "휴대폰 drawer")).some((r) => r.more)) throw new Error("휴대폰 drawer에 ⋯ 줄이 없다");
    await inList(ph.page, ph.page.locator(`.machine-list .sidebar-pinned [data-pin-key="${key}"] .row-menu-toggle`));
    const sheet = ph.page.getByRole("dialog", { name: "t1" });
    await sheet.waitFor();
    // 아래에서 올라오는 움직임이 끝난 뒤 찍는다(움직임의 finished, 시간 기다림 없음)
    await sheet.evaluate((el) => Promise.all(el.closest(".modal-scrim").getAnimations({ subtree: true }).map((a) => a.finished)));
    await shot(ph.page, "phone-row-menu");
    const unsent = pinWrote(ph.page);
    await sheet.getByRole("button", { name: "pane 고정 해제" }).click();
    if ((await unsent).pins?.includes(key)) throw new Error("고정 해제가 host로 가지 않았다");
    await domIs(page, "데스크톱에서도 풀림", (k) => document.querySelector(`.machine-list .sidebar-pinned [data-pin-key="${k}"]`) === null && document.querySelector(`.machine-list [data-pin-key="${k}"]`) !== null, key);
    if (ph.errors.length) throw new Error(ph.errors.join("; "));
    await ph.page.context().close();
    // 이름 바꾸기(참고 PaneCanvas PaneRename): 창의 이름 칸에 focus, 앞뒤 공백은 떼어 host(agent spec.json)까지, 붙인 이름이 있으면
    // 메뉴에 「pane 이름 지우기」가 생기고 누르면 띄울 때 이름으로 돌아간다
    aaa("tab", "create", "--machine", "labA", "--label", "rowname", "--", "/bin/sh", "-c", "read x");
    await page.getByTestId("pane-rowname").click({ button: "right" });
    await page.getByRole("menuitem", { name: "pane 이름 바꾸기" }).click();
    const named = page.getByRole("dialog", { name: "pane 이름 바꾸기" });
    await focusIs(page, "이름 칸", '[role="dialog"] .pane-rename-form input');
    await named.getByRole("textbox").fill("  빌드 창  ");
    await shot(page, "desktop-pane-rename");
    const renamed = opDone(page, "pane_rename");
    await page.keyboard.press("Enter");
    const sentName = (await renamed).request().postDataJSON();
    if (sentName.label !== "빌드 창") throw new Error(`보낸 이름 ${JSON.stringify(sentName)}`);
    await domIs(page, "새 이름 줄", () => document.querySelector('[data-testid="pane-빌드 창"]') !== null && !document.querySelector('[role="dialog"]'));
    if (!aaa("pane", "list").panes.some((x) => x.name === "빌드 창" && x.label === "빌드 창")) throw new Error("붙인 이름이 host에 없다");
    await page.getByTestId("pane-빌드 창").click({ button: "right" });
    if (JSON.stringify(await menuItems(page)) !== JSON.stringify(["pane 고정", "pane 이름 바꾸기", "pane 이름 지우기", "세션 끝내기", "pane 닫기"]))
      throw new Error(`이름 붙은 pane 메뉴 ${await menuItems(page)}`);
    const cleared = opDone(page, "pane_rename");
    await page.getByRole("menuitem", { name: "pane 이름 지우기" }).click();
    if ((await cleared).request().postDataJSON().label !== null) throw new Error("지우기가 null을 보내지 않았다");
    await domIs(page, "띄울 때 이름으로", () => document.querySelector('[data-testid="pane-rowname"]') !== null);
    aaa("pane", "close", "rowname");
    await domIs(page, "rowname 닫힘", () => !document.querySelector('[data-testid="pane-rowname"]'));
    // 3. pane 닫기: 확인 창(취소에 focus) → 닫으면 줄이 사라진다
    aaa("tab", "create", "--machine", "labA", "--label", "rowclose", "--", "/bin/sh", "-c", "read x");
    await page.getByTestId("pane-rowclose").waitFor();
    await page.getByTestId("pane-rowclose").click({ button: "right" });
    await page.getByRole("menuitem", { name: "pane 닫기" }).click();
    const ask = page.getByRole("alertdialog").filter({ hasText: "rowclose pane을 닫을까요?" });
    await ask.waitFor();
    await focusIs(page, "확인 창 취소", '[role="alertdialog"] .modal-footer .btn:not(.btn-danger)');
    const closed = opDone(page, "pane_close");
    await ask.getByRole("button", { name: "pane 닫기" }).click();
    await closed;
    await domIs(page, "닫은 pane 줄 사라짐", () => !document.querySelector('[data-testid="pane-rowclose"]'));
    if (aaa("pane", "list").panes.some((x) => x.name === "rowclose")) throw new Error("닫은 pane이 host에 남았다");
    // 4. 끝난 pane: 다시 열기 → 돈다, 다시 끝내고 기록 지우기(확인 창) → 줄이 사라진다
    aaa("tab", "create", "--machine", "labA", "--label", "rowend", "--", "/bin/sh", "-c", "read x");
    await page.getByTestId("pane-rowend").waitFor();
    const endedRow = () => document.querySelector('li.is-ended:has([data-testid="pane-rowend"])') !== null;
    aaa("pane", "kill", "rowend");
    await domIs(page, "끝난 pane", endedRow);
    await page.getByTestId("pane-rowend").click({ button: "right" });
    if (JSON.stringify(await menuItems(page)) !== JSON.stringify(["pane 고정", "pane 이름 바꾸기", "다시 열기", "기록 지우기"])) throw new Error(`끝난 pane 메뉴 ${await menuItems(page)}`);
    const resumed = opDone(page, "pane_resume");
    await page.getByRole("menuitem", { name: "다시 열기" }).click();
    if (!(await (await resumed).json()).ok) throw new Error("다시 열기 실패");
    await domIs(page, "다시 연 pane이 돈다", () => document.querySelector('li:has([data-testid="pane-rowend"])') !== null && document.querySelector('li.is-ended:has([data-testid="pane-rowend"])') === null);
    aaa("pane", "kill", "rowend");
    await domIs(page, "다시 끝난 pane", endedRow);
    await page.getByTestId("pane-rowend").click({ button: "right" });
    await page.getByRole("menuitem", { name: "기록 지우기" }).click();
    const forget = page.getByRole("alertdialog").filter({ hasText: "rowend pane 기록을 지울까요?" });
    await forget.waitFor();
    const forgot = opDone(page, "pane_forget");
    await forget.getByRole("button", { name: "지우기" }).click();
    await forgot;
    await domIs(page, "기록 지운 pane 줄 사라짐", () => !document.querySelector('[data-testid="pane-rowend"]'));
    // 5. thread 줄: 보고·요약·고정·모든 thread(끝난 것까지, plan §8.89·§8.291·§8.434). 프로젝트 머리는 없다
    // t-0001은 앞 단계에서 끝냈으니 아직 열린 demo thread(「알림 시험」, agent pane이 돈다)의 줄에서. 고정 열쇠는 그 pane
    const other = aaa("thread", "list", "demo").threads.find((t) => t.id !== "t-0001" && t.role !== "evaluator").id;
    if ((await page.locator(`li[data-thread="demo/${other}"]`).getAttribute("data-pin-key")) === null) throw new Error(`${other} 줄에 고정 열쇠(agent pane)가 없다`);
    await page.getByTestId(`thread-demo-${other}`).click({ button: "right" });
    if (JSON.stringify(await menuItems(page)) !== JSON.stringify(["보고", "요약", "고정", "모든 thread"])) throw new Error(`thread 메뉴 ${await menuItems(page)}`);
    const toThreads = hashIs(page, "#threads");
    await page.getByRole("menuitem", { name: "모든 thread" }).click();
    await toThreads;
    // 모든 thread에서 연 보고의 「뒤로」는 그 화면, 다른 길(앱 안 알림·주소)로 간 thread는 목록으로. 짧은 주소(#thread=<id>)로도 간다
    const toReport = hashIs(page, "#thread=t-0001");
    await page.getByTestId("project-thread-t-0001").click();
    await toReport;
    if ((await page.getByTestId("thread-back").getAttribute("aria-label")) !== "모든 thread로") throw new Error("모든 thread에서 연 보고의 뒤로");
    const toOther = hashIs(page, `#thread=${other}`);
    await page.evaluate((h) => (location.hash = h), `#thread=${other}`);
    await toOther;
    await domIs(page, "짧은 주소로 연 보고", (id) => document.querySelector('[data-testid="thread-view"]')?.textContent.includes(id), other);
    await domIs(page, "다른 길로 간 보고의 뒤로는 목록", () => document.querySelector('[data-testid="thread-back"]')?.getAttribute("aria-label") === "목록으로");
    await inList(page, "pane-t1");
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("목록은 프로젝트 먼저(t-0352 2안, plan §8.195·§8.280): thread는 머신 묶음 없이, 프로젝트가 하나면 머리 없이 맨 위에, 둘째 줄에 상태 말과 머신 이름, thread에 딸리지 않은 pane은 「기타」에 줄마다 머신 이름, 「기타」 머리로 접고 편다", async () => {
    // 프로젝트가 demo 하나뿐이라 프로젝트 머리가 없다(머리 셈은 프로젝트가 여럿인 「heads」 단계에서 본다)
    if (await page.getByTestId("project-demo").count()) throw new Error("프로젝트가 하나인데 프로젝트 머리가 있다");
    // thread 둘째 줄: 왼쪽 상태 말, 오른쪽 머신 이름. 머신 칸에는 줄이 없다
    const machines = await page.locator('[data-testid^="thread-machine-demo-"]').allInnerTexts();
    if (machines.length === 0 || machines.some((m) => m !== "labA")) throw new Error(`thread 줄 머신: ${machines}`);
    const states = await page.locator('[data-testid^="thread-state-demo-"]').allInnerTexts();
    if (states.length !== machines.length || states.some((w) => !/^(확인 필요|검토 대기|병합 중|작업 중|쉼)/.test(w))) throw new Error(`thread 줄 상태 말: ${states}`);
    if (await page.locator(".machine-strip [data-thread], .machine-strip [data-pane]").count()) throw new Error("머신 칸 안에 줄이 있다");
    // 「기타」: thread pane이 아닌 셸 줄, 줄마다 머신 이름
    const other = page.getByTestId("other-group");
    const loose = ".machine-list .worktree-children li[data-pane]";
    const places = await page.locator(`${loose} .workspace-place`).allInnerTexts();
    if (places.length === 0 || places.some((w) => !w.startsWith("labA"))) throw new Error(`「기타」 줄 머신: ${places}`);
    const n = places.length;
    await other.locator(".project-select").click();
    await domIs(page, "「기타」 접힘", (q) => document.querySelectorAll(q).length === 0, loose);
    if ((await other.locator(".workspace-fold-count").innerText()) !== `+${n}`) throw new Error("접힌 「기타」의 줄 수");
    await other.locator(".project-select").click();
    await domIs(page, "「기타」 펼침", ([q, n]) => document.querySelectorAll(q).length === n, [loose, n]);
  });

  await step("줄 옮기기(herdr-web-ui Sidebar `reorderWorkspaces`·`moveVisible`·`dropOrder`·touchReorder.ts, 차이 목록 2장 L9): 데스크톱은 Alt+↑↓(focus가 따라감)와 끌기, 그 차례가 host 공유 설정 paneOrder로 가 다른 기기도 같은 차례, 휴대폰은 길게 눌러 끌어 옮기고 데스크톱도 따라온다", async () => {
    for (const n of ["ord1", "ord2", "ord3"]) aaa("tab", "create", "--machine", "labA", "--label", n, "--", "/bin/sh");
    // 머신 목록의 옮길 수 있는 줄(고정하지 않은 pane) 가운데 이 단계의 셋, 보이는 차례대로
    const order = (p) =>
      p.locator(".machine-list .workspace-list > li[data-workspace]").evaluateAll((els) =>
        els.map((e) => e.querySelector("[data-testid^='pane-']").dataset.testid.slice(5)).filter((n) => n.startsWith("ord")),
      );
    const orderIs = async (p, want, what) => {
      let last = null;
      await until(what, async () => JSON.stringify((last = await order(p))) === JSON.stringify(want)).catch((e) => {
        throw new Error(`${e.message}: ${JSON.stringify(last)}`);
      });
    };
    const keyOf = Object.fromEntries(aaa("pane", "list").panes.filter((p) => p.name.startsWith("ord")).map((p) => [`${p.machine_id}:${p.id}`, p.name]));
    // host 공유 설정에 간 차례(이 단계의 셋만). 단계가 먼저 실패해 화면이 닫히면 이 기다림은 거부된다: 처리되지 않은 거부로
    // 프로세스가 끝나 실패 글을 잃지 않게 빈 catch를 달아 둔다(await하는 곳은 그대로 거부를 받는다)
    const sentOrder = (p) => {
      const sent = p
        .waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "preferences_set" && r.request().postDataJSON().settings?.paneOrder !== undefined)
        .then((r) => r.request().postDataJSON().settings.paneOrder.map((k) => keyOf[k]).filter(Boolean));
      sent.catch(() => {});
      return sent;
    };
    await orderIs(page, ["ord1", "ord2", "ord3"], "처음은 host 차례");
    // 1. Alt+↑·Alt+↓: 이웃 줄 너머로, focus는 옮긴 줄에 남는다(↓는 그 줄 자신이 DOM에서 옮겨져 focus를 잃기 쉽다)
    const altMove = async (key, want) => {
      const sent = sentOrder(page);
      await page.keyboard.press(key);
      await orderIs(page, want, key);
      await focusIs(page, `${key} 뒤 옮긴 줄에 focus`, '[data-testid="pane-ord3"]');
      if (JSON.stringify(await sent) !== JSON.stringify(want)) throw new Error(`${key} 뒤 host로 간 차례: ${JSON.stringify(await sent)}`);
    };
    await page.getByTestId("pane-ord3").focus();
    await altMove("Alt+ArrowUp", ["ord1", "ord3", "ord2"]);
    await altMove("Alt+ArrowDown", ["ord1", "ord2", "ord3"]);
    // 2. 끌기: ord1을 ord2에 놓으면(아래로) ord2 뒤
    let sent = sentOrder(page);
    await page.getByTestId("pane-ord1").dragTo(page.getByTestId("pane-ord2"));
    await orderIs(page, ["ord2", "ord1", "ord3"], "끌어 놓기");
    if (JSON.stringify(await sent) !== JSON.stringify(["ord2", "ord1", "ord3"])) throw new Error(`host로 간 차례: ${JSON.stringify(await sent)}`);
    await shot(page, "desktop-row-reorder");
    // 3. 휴대폰(다른 기기): 같은 차례가 와 있다. 길게 눌러 들어 올린 ord1을 ord2 위에 놓으면(위로) ord2 앞
    const ph = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    const pp = ph.page;
    await pp.locator(".view [data-testid='drawer-toggle']").click();
    await pp.locator("#workspace-drawer.is-open").waitFor();
    await pp.evaluate(() => Promise.all((document.getElementById("workspace-drawer")?.getAnimations() ?? []).map((a) => a.finished.catch(() => null))));
    await orderIs(pp, ["ord2", "ord1", "ord3"], "다른 기기도 같은 차례");
    if (await pp.getByTestId("pane-ord1").getAttribute("draggable") === "true") throw new Error("터치 화면 줄이 브라우저 끌기를 켰다(참고는 길게 누르기)");
    const center = async (name) => {
      const b = await pp.getByTestId(`pane-${name}`).boundingBox();
      return { x: Math.round(b.x + b.width / 2), y: Math.round(b.y + b.height / 2) };
    };
    const from = await center("ord1");
    const to = await center("ord2");
    const cdp = await pp.context().newCDPSession(pp);
    sent = sentOrder(pp);
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [from] });
    // 누름이 붙잡으면(참고 HOLD_MS) 줄이 들린다: 그 표시를 기다린다
    await pp.locator(".machine-list li.is-lifted", { has: pp.getByTestId("pane-ord1") }).waitFor();
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: from.x, y: to.y }] });
    await pp.locator(".machine-list li[data-drop='before']", { has: pp.getByTestId("pane-ord2") }).waitFor();
    await shot(pp, "phone-row-lifted");
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await orderIs(pp, ["ord1", "ord2", "ord3"], "길게 눌러 끌기");
    if (JSON.stringify(await sent) !== JSON.stringify(["ord1", "ord2", "ord3"])) throw new Error(`휴대폰이 host로 보낸 차례: ${JSON.stringify(await sent)}`);
    await orderIs(page, ["ord1", "ord2", "ord3"], "데스크톱도 따라온다");
    await pp.context().close();
    for (const n of ["ord1", "ord2", "ord3"]) aaa("pane", "kill", n);
  });


  await step("프로젝트 멈춤(herdr-projects sidebar.rs project_line 「paused」, 차이 목록 4장 G #592): 멈추면 모든 thread 머리가 「멈춤」, 되살리면 돌아온다. host는 프로젝트 하나라(plan §8.285) 보관·삭제는 m13 project_lifecycle이 본다", async () => {
    // 머리 둘째 줄(묶음 셈 또는 「멈춤」): 멈춤은 host `instance`가 말한다(plan §8.291)
    const facts = '[data-testid="all-threads"] .project-threads-heading .project-threads-facts';
    const threadsScreen = async () => {
      await page.getByTestId("more").click();
      const toThreads = hashIs(page, "#threads");
      await page.getByTestId("threads-item").click();
      await toThreads;
    };
    const close = () => page.getByTestId("all-threads").getByRole("button", { name: "닫기" }).click();
    aaa("project", "pause", "demo");
    await threadsScreen();
    await domIs(page, "모든 thread 머리의 멈춤", (q) => document.querySelector(q)?.textContent.includes("멈춤"), facts);
    await shot(page, "desktop-project-paused");
    await close();
    aaa("project", "resume", "demo");
    await threadsScreen();
    await domIs(page, "되살린 프로젝트", (q) => document.querySelector(q) !== null && !document.querySelector(q).textContent.includes("멈춤"), facts);
    await close();
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("coordinator 열기(herdr-projects popup.rs Threads `c` → profile_picker, 차이 목록 4장 G #592): 모든 thread 머리 「coordinator」가 host 머신 profile 가운데 허용한 것을 기본부터 보이고, 고르면 띄워 그 pane을 열며 도는 것이 있으면 그것을 연다. 허용 목록 밖은 고를 것이 없다고 말하고, 고르는 사이 막히면 host 거부 글을 보인다", async () => {
    const $ = (id) => `[data-testid="${id}"]`;
    const toml = `${root}/c/host.toml`;
    const plain = fs.readFileSync(toml, "utf8");
    // safety는 host가 쓸 때마다 host.toml에서 다시 읽는다(사람만 바꾸는 값이라 시험은 파일을 바로 쓴다)
    const allowOnly = (names) => fs.writeFileSync(toml, `${plain}[safety.demo]\ncoordinator_profiles = ${JSON.stringify(names)}\n`);
    // 응답 몸은 {ok: …} 또는 {err: {code, message}}
    const openCall = () =>
      page.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "project_open").then((r) => r.json());
    const toDemoScreen = async () => {
      await page.getByTestId("more").click();
      const toThreads = hashIs(page, "#threads");
      await page.getByTestId("threads-item").click();
      await toThreads;
    };
    const button = page.getByTestId("project-coordinator");
    const fake = page.getByRole("menuitem", { name: /^fake/ });
    aaa("project", "set", "demo", "coordinator_profile", "fake");
    await toDemoScreen();
    // 1. 허용한 profile이 host 머신에 없으면 메뉴 대신 그 말(참고 「No profile is allowed here」)
    allowOnly(["nope"]);
    await button.click();
    await domIs(page, "허용된 profile 없음", (q) => document.querySelector(q)?.textContent.includes("(허용: nope)"), $("project-coordinator-error"));
    if ((await page.getByRole("menu").count()) !== 0) throw new Error("고를 것이 없는데 메뉴가 열렸다");
    // 2. 프로젝트 기본 coordinator profile이 맨 위(「기본」). 고르는 사이 허용 목록이 바뀌면 host가 거부하고 화면은 그대로
    fs.writeFileSync(toml, plain);
    await button.click();
    await focusIs(page, "메뉴 첫 항목", '[role="menu"] [role="menuitem"]');
    await domIs(page, "오류 줄 지움", (q) => !document.querySelector(q), $("project-coordinator-error"));
    const first = await page.getByRole("menu").locator('[role="menuitem"]').first().innerText();
    if (first.replace(/\s+/g, " ") !== "fake 기본") throw new Error(`첫 항목 ${JSON.stringify(first)}`);
    await shot(page, "desktop-coordinator-menu");
    allowOnly(["nope"]);
    const refused = openCall();
    await fake.click();
    if ((await refused).err?.code !== "forbidden") throw new Error(`거부가 아니다: ${JSON.stringify(await refused)}`);
    await domIs(page, "host 거부 글", (q) => document.querySelector(q)?.textContent.includes("coordinator에 허용되지 않았다"), $("project-coordinator-error"));
    if ((await page.evaluate(() => location.hash)) !== "#threads") throw new Error("거부됐는데 화면이 바뀌었다");
    fs.writeFileSync(toml, plain);
    // 3. 띄우기: 그 pane을 열고, host 기록은 역할 coordinator·이름 「demo coordinator」, workspace demo의 탭 coordinator
    await button.click();
    const started = openCall();
    await fake.click();
    const one = (await started).ok;
    if (!one.started || one.profile !== "fake" || one.workspace !== "demo") throw new Error(`띄우기 ${JSON.stringify(one)}`);
    await hashIs(page, `#pane=${encodeURIComponent(one.pane.id)}`);
    await until("coordinator 터미널", async () => (await screenText(page)).includes("fake>"));
    const mine = () => aaa("pane", "list").panes.filter((x) => x.name.startsWith("demo coordinator"));
    if (JSON.stringify(mine().map((x) => [x.id, x.role])) !== JSON.stringify([[one.pane.id, "coordinator"]])) throw new Error(`pane ${JSON.stringify(mine())}`);
    await inList(page, "pane-demo coordinator");
    await shot(page, "desktop-coordinator-pane");
    // 4. 도는 것이 있으면 띄우지 않고 그 pane(뒤로 가기는 모든 thread)
    const back = hashIs(page, "#threads");
    await page.goBack();
    await back;
    await button.click();
    const again = openCall();
    await fake.click();
    const two = (await again).ok;
    if (two.started || two.pane.id !== one.pane.id) throw new Error(`다시 열기 ${JSON.stringify(two)}`);
    await hashIs(page, `#pane=${encodeURIComponent(one.pane.id)}`);
    if (mine().length !== 1) throw new Error(`coordinator가 늘었다: ${JSON.stringify(mine())}`);
    // 5. 휴대폰 폭: 단추는 표지만, 머리가 옆으로 밀리지 않고 고르기는 아래 sheet
    await page.setViewportSize({ width: 390, height: 844 });
    const back2 = hashIs(page, "#threads");
    await page.goBack();
    await back2;
    // 표지는 16px 그대로(단추 padding에 눌리지 않는다)
    await domIs(page, "휴대폰 폭 단추는 표지만", (q) => { const b = document.querySelector(q)?.getBoundingClientRect(); const i = document.querySelector(`${q} svg`)?.getBoundingClientRect(); return b && b.width > 0 && b.width < 60 && i.width === 16; }, $("project-coordinator"));
    const wide = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (wide > 0) throw new Error(`휴대폰 폭에서 화면이 ${wide}px 옆으로 밀린다`);
    await button.click();
    const sheet = page.getByRole("dialog", { name: "coordinator 열기" });
    await sheet.waitFor();
    await shot(page, "phone-coordinator-menu");
    await sheet.getByRole("button", { name: "취소" }).click();
    await domIs(page, "sheet 닫힘", () => !document.querySelector('[role="dialog"]'));
    await page.setViewportSize({ width: 1280, height: 800 });
    aaa("pane", "close", one.pane.id);
    await page.getByTestId("all-threads").getByRole("button", { name: "닫기" }).click();
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });
  await step("빈 결과물의 자동 링크도 복사·QR·폐기가 보인다", async () => {
    // host는 프로젝트 하나(plan §8.285): 프로젝트 인자 없는 CLI로 demo에 시작한다
    fs.writeFileSync(`${root}/empty-task.md`, "빈 결과물 시험");
    const t = aaa("thread", "start", "--title", "빈 결과물", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/empty-task.md`);
    const first = await until("빈 결과물 시험의 첫 보고", () => aaa("thread", "show", t.id).report_hash);
    if (!t.work_dir.startsWith(`${root}/`)) throw new Error("실험실 밖 작업 폴더");
    for (const name of fs.readdirSync(`${t.work_dir}/library`)) fs.rmSync(`${t.work_dir}/library/${name}`, { recursive: true });
    fs.appendFileSync(`${t.work_dir}/report.md`, "\n빈 결과물을 회수한다\n");
    await until("빈 결과물 회수", () => aaa("thread", "show", t.id).report_hash !== first);
    const lib = aaa("thread", "library", t.id);
    if (lib.files.length !== 0 || !lib.link) throw new Error("빈 사본 자동 링크가 없다");
    await page.evaluate((id) => { location.hash = `#thread=demo%2F${id}`; }, t.id);
    const row = page.getByTestId("library-link");
    await row.getByRole("button", { name: "링크 복사" }).waitFor();
    await row.getByRole("button", { name: "QR 코드", exact: true }).click();
    await row.getByRole("img", { name: "보고서 공유 링크 QR 코드" }).waitFor();
    await row.getByRole("button", { name: "링크 폐기" }).click();
    await domIs(page, "빈 결과물 링크 폐기", () => document.querySelector('[data-testid="library-link"]')?.dataset.state === "revoked");
  });


  await step("coordinator 줄(plan §8.258·§8.285): 프로젝트가 하나면 머리 없이 트리 맨 위, 왼쪽 보라 막대, 그 아래 「thread n」 머리줄과 thread 줄, 좁은 목록에도 한 번만", async () => {
    const opened = aaa("coordinator", "open", "--profile", "fake");
    if (!opened.started) throw new Error("coordinator를 새로 열지 않았다");
    const row = page.getByTestId("pane-demo coordinator");
    await page.setViewportSize({ width: 1280, height: 800 });
    await inList(page, row);
    if (await page.locator('[data-panel="work"] [data-testid^="project-"].project-group').count()) throw new Error("프로젝트가 하나인데 프로젝트 머리가 있다");
    // demo thread는 저장소가 없어 폴더 없이 머리줄 「thread n」 아래에 선다(n은 그 아래 줄 수)
    await domIs(page, "thread 머리줄", () => {
      const head = document.querySelector('[data-panel="work"] [data-testid="project-thread-head-demo"]');
      let n = 0;
      for (let e = head?.nextElementSibling; e?.matches("li[data-thread]"); e = e.nextElementSibling) n++;
      return n > 0 && head.textContent.trim() === `thread ${n}`;
    });
    const look = await page.evaluate(() => {
      const coord = document.querySelector('[data-panel="work"] [data-testid="pane-demo coordinator"]').closest("li").querySelector(".workspace-header");
      const thread = document.querySelector('[data-panel="work"] [data-testid="project-thread-head-demo"]').nextElementSibling.querySelector(".workspace-header");
      const bar = getComputedStyle(coord.querySelector(".workspace-select"), "::before");
      return { coordinator: coord.closest("li").classList.contains("is-coordinator"), bar: bar.backgroundColor, width: bar.width, indent: getComputedStyle(coord).paddingLeft, threadIndent: getComputedStyle(thread).paddingLeft };
    });
    // 머리가 없으니 coordinator와 thread 줄은 같은 단계(같은 들여쓰기)다
    if (!look.coordinator || look.bar !== "rgb(185, 156, 242)" || look.width !== "2px" || look.indent !== look.threadIndent) throw new Error(`coordinator 구별·들여쓰기: ${JSON.stringify(look)}`);
    // 보라 막대는 줄 안이고 표지(Claude 별·셸 기호)를 덮지 않는다(사용자 10-19 「보라 세로선이 claude 아이콘을 가려. 핸드폰」, plan §8.286):
    // 머리가 없어 들여쓰기 0인 줄에서 머리 기준 막대가 표지 칸 왼쪽 8px 안으로 들어갔다
    const barClear = async (what) => {
      const m = await page.evaluate(() => {
        const select = document.querySelector('[data-panel="work"] [data-testid="pane-demo coordinator"]');
        const bar = getComputedStyle(select, "::before");
        const left = select.getBoundingClientRect().left + parseFloat(bar.left);
        const icon = select.querySelector(".sidebar-mark svg").getBoundingClientRect();
        return { inRow: left - select.closest(".workspace-header").getBoundingClientRect().left, gap: icon.left - (left + parseFloat(bar.width)) };
      });
      if (m.inRow < 0 || m.gap < 3) throw new Error(`${what} coordinator 막대와 표지: ${JSON.stringify(m)}`);
    };
    await barClear("데스크톱");
    await shot(page, "desktop-coordinator-project");
    await page.setViewportSize({ width: 390, height: 844 });
    await page.locator(".view [data-testid='drawer-toggle']").click();
    await drawerSettled(page);
    await barClear("휴대폰");
    await shot(page, "phone-coordinator-project");
    if ((await page.locator('[data-testid="pane-demo coordinator"]').count()) !== 1) throw new Error("coordinator 줄이 둘 이상이다");
    await page.locator(".view [data-testid='drawer-toggle']").click();
    await drawerSettled(page);
    await page.setViewportSize({ width: 1280, height: 800 });
  });

  await step("「작업」 트리 작업 공간 폴더(사용자 결정 10-19 안 A, plan §8.260·§8.280): worktree thread는 저장소 마지막 폴더 이름 폴더에, 저장소 없는 thread는 끝의 「작업 폴더」에. 폴더 머리는 프로젝트 머리 모양이고(프로젝트가 하나라 트리 맨 위) 자식 줄은 한 단계 더 8px, 둘째 줄은 상태 말·머신, 접으면 +n, 에이전트 패널은 없다", async () => {
    // 저장소 이름이 폴더 이름이다(경로 앞부분은 머신마다 다르다)
    const repo = `${root}/repos/rcgame`;
    fs.mkdirSync(repo, { recursive: true });
    const git = (...a) => execFileSync("git", ["-C", repo, ...a], { stdio: "ignore" });
    git("init", "-q", "-b", "main");
    git("-c", "user.name=e2e", "-c", "user.email=e2e@example.com", "commit", "-q", "--allow-empty", "-m", "init");
    fs.writeFileSync(`${root}/folder-task.txt`, "noreport\n");
    const start = (title, ...more) => aaa("thread", "start", "--title", title, "--profile", "fake", "--machine", "labA", ...more, "--task-file", `${root}/folder-task.txt`);
    const inRepo = start("저장소 일", "--repo", repo);
    const loose = start("폴더 일", "--kind", "dir");
    if (inRepo.repo !== repo || loose.repo !== undefined) throw new Error(`thread repo 칸: ${inRepo.repo} · ${loose.repo}`);
    await page.setViewportSize({ width: 1280, height: 800 });
    await showPanel(page, "work");
    if ((await page.getByTestId("activity-agents").count()) !== 0) throw new Error("에이전트 패널 단추가 남았다");
    const row = (t) => `[data-panel="work"] li[data-thread="demo/${t.id}"]`;
    await domIs(page, "두 thread 줄", ([a, b]) => document.querySelector(a) !== null && document.querySelector(b) !== null, [row(inRepo), row(loose)]);
    const look = await page.evaluate(([a, b]) => {
      const folderOf = (q) => document.querySelector(q).closest(".worktree-children")?.previousElementSibling?.dataset.testid;
      // 프로젝트가 하나라 머리 없이 트리 맨 위에 폴더가 선다
      const folders = [...document.querySelectorAll('[data-panel="work"] li[data-testid^="workspace-folder-"]')].map((el) => el.dataset.testid.slice(17));
      const r = document.querySelector(a);
      const pad = (q) => parseFloat(getComputedStyle(q).paddingLeft);
      return {
        folders,
        repo: folderOf(a),
        loose: folderOf(b),
        // 폴더 안 줄은 폴더 머리보다 한 단계(8px) 더
        head: pad(document.querySelector('[data-panel="work"] [data-testid="workspace-folder-rcgame"] .workspace-header')),
        indent: pad(r.querySelector(".workspace-header")),
        state: r.querySelector(".thread-place .thread-state")?.textContent,
        machine: r.querySelector(".thread-place .thread-machine")?.textContent,
      };
    }, [row(inRepo), row(loose)]);
    if (look.folders.join() !== "rcgame,작업 폴더" || look.repo !== "workspace-folder-rcgame" || look.loose !== "workspace-folder-작업 폴더") throw new Error(`작업 공간 폴더: ${JSON.stringify(look)}`);
    if (look.indent - look.head !== 8 || !/^(준비 중|확인 필요|검토 대기|병합 중|작업 중|쉼)/.test(look.state ?? "") || look.machine !== "labA") throw new Error(`폴더 안 줄: ${JSON.stringify(look)}`);
    await shot(page, "desktop-workspace-folders");
    const folder = page.getByTestId("workspace-folder-rcgame");
    await folder.locator(".project-select").click();
    await domIs(page, "접힌 작업 공간 폴더", (q) => document.querySelector(q) === null, row(inRepo));
    if ((await folder.locator(".workspace-fold-count").innerText()) !== "+1") throw new Error("접힌 폴더의 줄 수");
    // 접힘은 기기마다 새 이름으로 남는다
    if ((await page.evaluate(() => localStorage.getItem("aaa:workspace-collapsed:rcgame"))) !== "1") throw new Error("폴더 접힘 기억");
    await folder.locator(".project-select").click();
    await domIs(page, "펼친 작업 공간 폴더", (q) => document.querySelector(q) !== null, row(inRepo));
    // 휴대폰(터치 기기) drawer: 폴더 머리와 줄의 ⋯이 목록 끝 여백 안에 선다
    const ph = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    await ph.page.locator(".view [data-testid='drawer-toggle']").click();
    await ph.page.locator("#workspace-drawer.is-open").waitFor();
    await drawerSettled(ph.page);
    await showPanel(ph.page, "work");
    await ph.page.locator(row(inRepo)).waitFor();
    if (!(await rowEndsNearEdge(ph.page, "휴대폰 작업 공간 폴더")).some((r) => r.name === "rcgame" && r.more)) throw new Error("휴대폰 폴더 머리에 ⋯이 없다");
    await shot(ph.page, "phone-workspace-folders");
    await ph.page.context().close();
  });

  await step("thread 관측 수명: 헤더 뒤 문서 교체가 browser.close 없이 끝남", threadBodyLifetime);
  await step("thread 실패 수명: 떨어진 작업의 WebKit 자식도 자연 종료", threadFailureLifetime);
  let threadPageRecords;
  await step("thread 목록 규모: 끝난 2500건·CLI 거르기와 개수·커서", async () => { threadPageRecords = await prepareThreadPages(); });
  await step("thread 목록 규모: Chromium 전체 페이지·재연결·실제 DOM", () => threadPages(threadPageRecords, "chromium"));
  await step("thread 목록 규모: WebKit 전체 페이지·재연결·실제 DOM", () => threadPages(threadPageRecords, "webkit"));

}
