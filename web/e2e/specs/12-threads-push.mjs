// 고유 프로세스·전송 경계는 남기고 화면 모델과 겹치는 시나리오는 단위 owner에 맡긴다.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import {
  root, aaa, until, domIs, hashIs, screenText, shot, pairPage, pushes, pushPort, pushOrigin, b64u, pushDevice, nextPush, pushKey,
  step, browser, base, setPushKey, openSettings, focusIs, setInUse, pushInUse, coordinatorPost, inList, showPanel, drawerSettled,
} from "../lib/harness.mjs";
import { prepareThreadPages, threadPages } from "../lib/thread-pages.mjs";
import { threadBodyLifetime, threadFailureLifetime } from "../lib/thread-lifetime.mjs";

export default async function ({ page }) {
  const pushDev = pushDevice("push/dev1");
  const goneDev = pushDevice("gone/dev2");
  let threadPageRecords;
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
  await step("thread 관측 수명: 헤더 뒤 문서 교체가 browser.close 없이 끝남", threadBodyLifetime);
  await step("thread 실패 수명: 떨어진 작업의 WebKit 자식도 자연 종료", threadFailureLifetime);
  await step("thread 목록 규모: 끝난 2500건·CLI 거르기와 개수·커서", async () => { threadPageRecords = await prepareThreadPages(); });
  await step("thread 목록 규모: Chromium 전체 페이지·재연결·실제 DOM", () => threadPages(threadPageRecords, "chromium"));
  await step("thread 목록 규모: WebKit 전체 페이지·재연결·실제 DOM", () => threadPages(threadPageRecords, "webkit"));
}
