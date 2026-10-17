// 웹 E2E 파일 보기: 터미널의 파일 경로 링크로 여는 파일 보기(결정 20 D7, herdr-web-ui FileViewer·terminalFileLinks).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 16-file-viewer(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import { root, aaa, waitOutput, domIs, shot, pairPage, step, browser } from "../lib/harness.mjs";

/** 터미널에서 `text` 가운데 글자의 화면 좌표 */
async function textPoint(page, text) {
  const at = await page.evaluate((text) => {
    for (const row of document.querySelectorAll(".xterm-rows > div")) {
      const i = row.textContent.indexOf(text);
      if (i < 0) continue;
      const w = document.createTreeWalker(row, NodeFilter.SHOW_TEXT);
      const mid = i + Math.floor(text.length / 2);
      for (let n = w.nextNode(), seen = 0; n; seen += n.data.length, n = w.nextNode())
        if (mid < seen + n.data.length) {
          const r = document.createRange();
          r.setStart(n, mid - seen);
          r.setEnd(n, mid - seen + 1);
          const b = r.getBoundingClientRect();
          return { x: b.left + b.width / 2, y: b.top + b.height / 2, w: b.width };
        }
    }
    return null;
  }, text);
  if (!at) throw new Error(`터미널에 ${text}이 없다`);
  return at;
}

/** 링크 위에서 누르고 놓는다. 마우스를 올리면 xterm이 링크를 찾아 손가락 커서를 단다. xterm은 칸이 바뀔 때만 다시 찾고
 *  터미널 밖으로 나갔다 와도 마지막 칸을 기억하므로(같은 링크를 닫은 뒤 다시 누를 때) 옆 칸을 거쳐 온다 */
async function clickLink(page, text) {
  const { x, y, w } = await textPoint(page, text);
  await page.mouse.move(x - w, y);
  await page.mouse.move(x, y);
  await domIs(page, `${text} 링크`, () => document.querySelector(".xterm-screen").classList.contains("xterm-cursor-pointer"));
  await page.mouse.down();
  await page.mouse.up();
}

const viewer = (page) => page.getByTestId("file-viewer");
const closed = (page, what) => domIs(page, `${what} 닫힘`, () => !document.querySelector('[data-testid="file-viewer"]'));
const screenHas = (page, what, text) => domIs(page, what, (t) => document.querySelector(".xterm-rows")?.textContent.includes(t), text);
const codeIs = (page, what, text) => domIs(page, what, (t) => document.querySelector('[data-testid="file-viewer-code"]')?.textContent === t, text);
const callFile = (page, path) =>
  page.evaluate(
    async (path) =>
      (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "pane_file", pane: "files", path }) })).json(),
    path,
  );

export default async function ({ ctl, page }) {
  await step("파일 보기(결정 20 D7, herdr-web-ui FileViewer·terminalFileLinks, plan §8.125): 터미널의 상대 경로·줄 참조·OSC 8 file 링크를 누르면 앱 안에서 Markdown·소스·그림을 보고, 같은 이름이 여럿이면 고르고, 허용 폴더 밖은 열지 않으며, 보기 권한 기기에는 파일 링크가 없다", async () => {
    // 허용 폴더(harness agent.toml file_roots = <실험실>/files) 안 프로젝트와 밖 파일
    const proj = `${root}/files/proj`;
    for (const d of ["notes", "src", "shots", "a/logs", "b/logs"]) fs.mkdirSync(`${proj}/${d}`, { recursive: true });
    fs.mkdirSync(`${root}/outside`, { recursive: true });
    // 문서 폴더 기준의 그림·링크·code 경로(파일 보기 안 Markdown, 참고 markdownImages·resolveFromDocument)
    // 언어 없는 fence(로그) 안 경로는 열리고 언어가 있는 code 안은 글 그대로(참고 PLAIN_FENCE)
    const readme =
      "# 읽어보기\n\n파일 보기 시험.\n\n![점](../shots/dot.png)\n\n[코드](../src/app.ts)와 `../a/logs/run.log`\n\n" +
      "```\nsaved ../b/logs/run.log\n```\n\n```ts\nconst log = \"../b/logs/run.log\";\n```\n";
    fs.writeFileSync(`${proj}/notes/readme.md`, readme);
    const code = Array.from({ length: 30 }, (_, i) => `const line${String(i + 1).padStart(2, "0")} = ${i + 1};${i === 19 ? " // marked" : ""}`).join("\n") + "\n";
    fs.writeFileSync(`${proj}/src/app.ts`, code);
    // 1×1 PNG
    fs.writeFileSync(`${proj}/shots/dot.png`, Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==", "base64"));
    fs.writeFileSync(`${proj}/a/logs/run.log`, "run a\n");
    fs.writeFileSync(`${proj}/b/logs/run.log`, "run b\n");
    fs.writeFileSync(`${root}/outside/secret.txt`, "no\n");
    // 휴대폰(47칸)에서도 줄이 접히지 않게 짧게
    const lines = [
      "R notes/readme.md",
      "C src/app.ts:20:5",
      "L logs/run.log",
      `O ${root}/outside/secret.txt`,
      "W https://example.com/x",
    ];
    // OSC 8 링크(보이는 글은 「사진」, 주소는 file URI)는 한 줄 받으면 낸다: 화면이 붙은 뒤의 출력이어야 한다
    // (aaa-vt는 OSC 8을 화면 상태에 두지 않아 늦게 붙은 화면의 replay에는 링크가 없다, plan §8.125 남은 것)
    const osc8 = `P \x1b]8;;file://${proj}/shots/dot.png\x1b\\사진\x1b]8;;\x1b\\\r\nosc8> `;
    const prog = `process.stdout.write(${JSON.stringify(lines.join("\r\n") + "\r\nfiles> ")}); process.stdin.on("data", () => process.stdout.write(${JSON.stringify(osc8)}));`;
    aaa("tab", "create", "--machine", "labA", "--label", "files", "--cwd", proj, "--", process.execPath, "-e", prog);
    await waitOutput("files", "files>");

    // 데스크톱: 상대 경로 → Markdown 미리보기
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await dp.getByTestId("pane-files").click();
    await screenHas(dp, "파일 줄들", "files>");
    await clickLink(dp, "notes/readme.md");
    await domIs(dp, "Markdown 미리보기", () => document.querySelector('[data-testid="file-viewer-markdown"] .markdown-h1')?.textContent === "읽어보기");
    const title = await viewer(dp).locator(".modal-title").textContent();
    if (title !== "readme.md") throw new Error(`제목 ${title}`);
    // 그림은 그 pane의 파일 보기 길로 읽는다
    const pic = viewer(dp).locator("img.markdown-image");
    await pic.waitFor();
    const picWidth = await pic.evaluate((i) => i.decode().then(() => i.naturalWidth));
    if (picWidth !== 1 || !(await pic.getAttribute("src")).startsWith("/f/")) throw new Error(`Markdown 그림 ${picWidth} ${await pic.getAttribute("src")}`);
    await shot(dp, "desktop-file-markdown");
    // 소스 보기: 줄 번호는 CSS counter라 글(복사)에 섞이지 않는다
    await viewer(dp).getByRole("button", { name: "소스 보기" }).click();
    await codeIs(dp, "Markdown 소스", readme);
    // 뒤로(Android 뒤로 단추와 같은 길)가 파일 보기만 닫는다
    await dp.goBack();
    await closed(dp, "뒤로");
    await screenHas(dp, "뒤로 뒤 pane", "files>");
    // 문서 안 링크·code 경로는 문서 폴더 기준으로 열고, 기록 칸을 늘리지 않는다(뒤로 한 번이면 pane)
    await clickLink(dp, "notes/readme.md");
    await viewer(dp).getByRole("button", { name: "코드", exact: true }).click();
    await codeIs(dp, "문서 링크", code);
    await dp.goBack();
    await closed(dp, "링크 뒤 뒤로");
    await clickLink(dp, "notes/readme.md");
    await viewer(dp).getByRole("button", { name: "../a/logs/run.log", exact: true }).click();
    await codeIs(dp, "문서 code 경로", "run a\n");
    await dp.keyboard.press("Escape");
    await closed(dp, "code 경로 뒤 Escape");
    await clickLink(dp, "notes/readme.md");
    const fenced = viewer(dp).getByRole("button", { name: "../b/logs/run.log", exact: true });
    await fenced.first().waitFor();
    const fences = await fenced.evaluateAll((all) => all.map((b) => b.closest(".markdown-code")?.dataset.language));
    if (JSON.stringify(fences) !== "[\"\"]") throw new Error(`fence 안 경로 링크 ${JSON.stringify(fences)}(언어 없는 fence 하나만)`);
    // 언어가 있는 fence는 그 문법으로 칠한다(참고 CodeBlock languageForFence)
    const tsKeyword = await viewer(dp).locator('.markdown-code[data-language="ts"] .hl-keyword').allTextContents();
    if (tsKeyword.join() !== "const") throw new Error(`ts fence 색 ${JSON.stringify(tsKeyword)}`);
    await fenced.click();
    await codeIs(dp, "fence 안 경로", "run b\n");
    await dp.keyboard.press("Escape");
    await closed(dp, "fence 경로 뒤 Escape");

    // 줄 참조 → 소스, 20번째 줄 표시
    await clickLink(dp, "src/app.ts:20:5");
    await codeIs(dp, "소스", code);
    const marked = await dp.evaluate(() => {
      const all = [...document.querySelectorAll(".hl-line")];
      const m = document.querySelector(".hl-line.is-marked");
      return { index: all.indexOf(m) + 1, text: m?.textContent };
    });
    if (marked.index !== 20 || marked.text !== "const line20 = 20; // marked") throw new Error(`표시한 줄 ${JSON.stringify(marked)}`);
    // 짧은 소스는 그리면서 칠한다(참고 SYNC_HIGHLIGHT_LIMIT): 이름(.ts)으로 고른 문법의 낱말·주석
    const colors = await dp.evaluate(() => {
      const code = document.querySelector('[data-testid="file-viewer-code"]');
      const role = (cls, line) => [...code.querySelectorAll(`.hl-line[data-line="${line}"] .hl-${cls}`)].map((e) => e.textContent);
      return { keyword: role("keyword", 1), number: role("number", 1), comment: role("comment", 20) };
    });
    if (JSON.stringify(colors) !== JSON.stringify({ keyword: ["const"], number: ["1"], comment: ["// marked"] })) throw new Error(`소스 색 ${JSON.stringify(colors)}`);
    // 읽기 경로: Range로 앞부분, 글은 text/plain, sandbox, 저장 안 함. 모르는 cap은 401
    const url = await viewer(dp).getByRole("link", { name: "새 탭에서 열기" }).getAttribute("href");
    const got = await dp.evaluate(async (url) => {
      const r = await fetch(url, { headers: { range: "bytes=0-9" } });
      const bad = await fetch(url.replace(/\/f\/[0-9a-f]+\//, "/f/00/"));
      const h = (k) => r.headers.get(k);
      return { status: r.status, range: h("content-range"), type: h("content-type"), csp: h("content-security-policy"), cache: h("cache-control"), body: await r.text(), bad: bad.status };
    }, url);
    const want = { status: 206, range: `bytes 0-9/${code.length}`, type: "text/plain; charset=utf-8", body: "const line", bad: 401 };
    for (const k of Object.keys(want)) if (got[k] !== want[k]) throw new Error(`읽기 경로 ${k}: ${JSON.stringify(got)}`);
    if (!got.csp?.startsWith("sandbox") || !got.cache?.includes("no-store")) throw new Error(`읽기 경로 머리: ${JSON.stringify(got)}`);
    // 경로가 새어도 짝지은 기기가 아니면 못 읽는다
    const fileUrl = new URL(url, dp.url()).href;
    const anon = await browser.newContext();
    const bare = await anon.request.get(fileUrl);
    if (bare.status() !== 401) throw new Error(`cookie 없는 읽기 ${bare.status()}`);
    await anon.close();
    await shot(dp, "desktop-file-code");
    await viewer(dp).getByRole("button", { name: "파일 닫기" }).click();
    await closed(dp, "닫기 단추");

    // OSC 8 file URI → 그림
    aaa("pane", "run", "files", "p");
    await screenHas(dp, "OSC 8 줄", "osc8>");
    await clickLink(dp, "사진");
    const img = viewer(dp).locator("img.file-viewer-media");
    await img.waitFor();
    const width = await img.evaluate((i) => i.decode().then(() => i.naturalWidth));
    if (width !== 1) throw new Error(`그림 폭 ${width}`);
    await dp.keyboard.press("Escape");
    await closed(dp, "Escape");

    // 같은 끝의 파일이 여럿 → 고르기
    await clickLink(dp, "logs/run.log");
    const choices = viewer(dp).locator(".file-viewer-choices button");
    await choices.first().waitFor();
    const names = await choices.allTextContents();
    if (names.length !== 2 || !names.every((n) => n.endsWith("/logs/run.log"))) throw new Error(`후보 ${JSON.stringify(names)}`);
    await choices.filter({ hasText: "/b/logs/" }).click();
    await codeIs(dp, "고른 파일", "run b\n");
    await dp.keyboard.press("Escape");
    await closed(dp, "고른 뒤 Escape");

    // 허용 폴더 밖: 있어도 열지 않고 까닭을 말한다
    await clickLink(dp, `${root}/outside/secret.txt`);
    await domIs(dp, "허용 밖 알림", () => document.querySelector('[data-testid="file-viewer"] [role="alert"]')?.textContent.includes("허용한 폴더 밖"));
    await shot(dp, "desktop-file-outside");
    await dp.keyboard.press("Escape");
    await closed(dp, "허용 밖 Escape");

    // 휴대폰: 손가락으로 누른다. 화면을 다 덮는다
    const pp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true, deviceScaleFactor: 3 })).page;
    await pp.getByTestId("pane-files").click();
    await screenHas(pp, "휴대폰 파일 줄들", "files>");
    const at = await textPoint(pp, "notes/readme.md");
    await pp.touchscreen.tap(at.x, at.y);
    await domIs(pp, "휴대폰 Markdown", () => document.querySelector('[data-testid="file-viewer-markdown"] .markdown-h1')?.textContent === "읽어보기");
    // sheet가 다 올라온 뒤에 잰다(휴대폰은 창이 아래에서 올라온다, modal.css sheet-in)
    const landed = (p) => viewer(p).evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
    await landed(pp);
    const box = await viewer(pp).boundingBox();
    const full = (b, w, h) => [b.x, b.y, b.width, b.height].map(Math.round).join() === `0,0,${w},${h}`;
    if (!full(box, 390, 844)) throw new Error(`휴대폰 파일 보기가 화면을 다 덮지 않는다: ${JSON.stringify(box)}`);
    await shot(pp, "phone-file-markdown");
    await viewer(pp).getByRole("button", { name: "파일 닫기" }).tap();
    await closed(pp, "휴대폰 닫기");
    await pp.context().close();

    // iPad: 손가락 기기라 역시 화면 가득, 줄 참조
    const ip = (await pairPage(browser, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true, deviceScaleFactor: 2 })).page;
    await ip.getByTestId("pane-files").click();
    await screenHas(ip, "iPad 파일 줄들", "files>");
    const ipAt = await textPoint(ip, "src/app.ts:20:5");
    await ip.touchscreen.tap(ipAt.x, ipAt.y);
    await codeIs(ip, "iPad 소스", code);
    await landed(ip);
    const ipBox = await viewer(ip).boundingBox();
    if (!full(ipBox, 820, 1180)) throw new Error(`iPad 파일 보기가 화면을 다 덮지 않는다: ${JSON.stringify(ipBox)}`);
    await shot(ip, "ipad-file-code");
    await ip.context().close();

    // 보기 권한 기기: 파일 경로를 눌러도 열리지 않는다(뒤에 누른 웹 주소가 새 탭으로 열린 뒤에 본다). host도 거부한다
    const rp = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 })).page;
    await rp.getByTestId("pane-files").click();
    await screenHas(rp, "reader 파일 줄들", "files>");
    const fileAt = await textPoint(rp, "notes/readme.md");
    await rp.mouse.move(fileAt.x, fileAt.y);
    await rp.mouse.down();
    await rp.mouse.up();
    // 파일 보기가 열렸다면 그 덮개가 웹 주소를 가린다: 어느 쪽이 먼저인지로 가른다
    const webAt = await textPoint(rp, "example.com");
    await rp.mouse.move(webAt.x, webAt.y);
    const first = await Promise.race([
      domIs(rp, "웹 주소 링크", () => document.querySelector(".xterm-screen").classList.contains("xterm-cursor-pointer")).then(() => "link", () => "none"),
      viewer(rp).waitFor({ timeout: 20_000 }).then(() => "viewer", () => "none"),
    ]);
    if (first !== "link") throw new Error(first === "viewer" ? "보기 권한 화면에서 파일 경로가 파일 보기를 열었다" : "reader 화면에서 웹 주소가 링크가 아니다(대조군)");
    const popup = rp.context().waitForEvent("page");
    await rp.mouse.down();
    await rp.mouse.up();
    await (await popup).close();
    const opened = await rp.evaluate(() => ({ state: history.state?.["aaa:file-preview"] ?? null, viewer: document.querySelectorAll('[data-testid="file-viewer"]').length }));
    if (opened.state !== null || opened.viewer !== 0) throw new Error(`reader가 파일 보기를 열었다: ${JSON.stringify(opened)}`);
    const leaked = await rp.evaluate((u) => fetch(u).then((r) => r.status), fileUrl);
    if (leaked !== 403) throw new Error(`reader가 읽기 경로로 읽었다: ${leaked}`);
    const denied = await callFile(rp, "notes/readme.md");
    if (denied.err?.code !== "forbidden") throw new Error(`reader 파일 보기: ${JSON.stringify(denied)}`);
    // 같은 요청이 조작 기기에는 열린다(거부가 권한 때문임을 본다)
    const allowed = await callFile(dp, "notes/readme.md");
    if (allowed.ok?.kind !== "pane_file" || allowed.ok.info?.name !== "readme.md") throw new Error(`조작 기기 파일 보기: ${JSON.stringify(allowed)}`);
    await rp.context().close();
    await dp.context().close();
    aaa("pane", "kill", "files");
  });

  await step("파일 보기 HTML 페이지·동영상·소리·PDF(herdr-web-ui FileViewer·file-view.ts pageWithLocalFiles, plan §8.125): HTML은 origin도 network도 없는 sandbox에서 제 script로 그리고 제 폴더의 그림·style·script만 제 종류대로 넣으며, 소스로 바꿀 수 있다. 동영상·소리는 흘려 받아 틀고 PDF는 브라우저가 그린다", async () => {
    const proj = `${root}/files/media`;
    for (const d of ["site", "shots", "av"]) fs.mkdirSync(`${proj}/${d}`, { recursive: true });
    const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==", "base64");
    fs.writeFileSync(`${proj}/site/pic.png`, png);
    fs.writeFileSync(`${proj}/shots/up.png`, png);
    fs.writeFileSync(`${proj}/site/.env`, "KEY=secret\n");
    fs.writeFileSync(`${proj}/site/style.css`, "h1 { color: rgb(1, 2, 3); }\n");
    // 페이지 script: 돌았다, 이 앱 저장소가 없다(opaque origin), 밖으로 묻기는 CSP가 막는다(위반 사건)
    fs.writeFileSync(
      `${proj}/site/app.js`,
      [
        'document.addEventListener("securitypolicyviolation", (e) => { document.body.dataset.csp = e.violatedDirective; });',
        'document.addEventListener("DOMContentLoaded", () => {',
        '  try { localStorage.length; document.body.dataset.storage = "yes"; } catch { document.body.dataset.storage = "none"; }',
        '  document.body.dataset.ran = "yes";',
        '  fetch("https://example.com/x").catch(() => {});',
        "});",
      ].join("\n"),
    );
    const html = '<!doctype html><html><head><link rel="stylesheet" href="style.css"><script src="app.js"></script></head>\n<body><h1>보고</h1><img id="pic" src="pic.png"><img id="env" src=".env"><img id="up" src="../shots/up.png"></body></html>\n';
    fs.writeFileSync(`${proj}/site/index.html`, html);
    // 소리: 8kHz 16bit 한 채널 0.5초 WAV. 동영상은 틀지 않고 흘려 받는 길만 본다
    const wav = Buffer.alloc(44 + 8000);
    wav.write("RIFF", 0);
    wav.writeUInt32LE(36 + 8000, 4);
    wav.write("WAVEfmt ", 8);
    wav.writeUInt32LE(16, 16);
    wav.writeUInt16LE(1, 20);
    wav.writeUInt16LE(1, 22);
    wav.writeUInt32LE(8000, 24);
    wav.writeUInt32LE(16000, 28);
    wav.writeUInt16LE(2, 32);
    wav.writeUInt16LE(16, 34);
    wav.write("data", 36);
    wav.writeUInt32LE(8000, 40);
    fs.writeFileSync(`${proj}/av/tone.wav`, wav);
    fs.writeFileSync(`${proj}/av/clip.mp4`, Buffer.alloc(4096, 7));
    fs.writeFileSync(`${proj}/av/manual.pdf`, "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n");
    // 맨 이름은 링크가 아니다(참고: 폴더가 있어야 경로다)
    const lines = ["H site/index.html", "V av/clip.mp4", "A av/tone.wav", "D av/manual.pdf"];
    const prog = `process.stdout.write(${JSON.stringify(lines.join("\r\n") + "\r\nmedia> ")}); process.stdin.resume();`;
    aaa("tab", "create", "--machine", "labA", "--label", "media", "--cwd", proj, "--", process.execPath, "-e", prog);
    await waitOutput("media", "media>");
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await dp.getByTestId("pane-media").click();
    await screenHas(dp, "media 줄들", "media>");

    // HTML 페이지
    await clickLink(dp, "site/index.html");
    const frameEl = viewer(dp).locator("iframe.file-viewer-page");
    await frameEl.waitFor();
    if ((await frameEl.getAttribute("sandbox")) !== "allow-scripts") throw new Error("페이지 iframe sandbox");
    const frame = await (await frameEl.elementHandle()).contentFrame();
    await frame.locator("body[data-ran='yes'][data-csp]").waitFor();
    const inside = await frame.evaluate(() => ({
      pic: document.getElementById("pic").getAttribute("src").slice(0, 22),
      env: document.getElementById("env").getAttribute("src"),
      up: document.getElementById("up").getAttribute("src"),
      color: getComputedStyle(document.querySelector("h1")).color,
      storage: document.body.dataset.storage,
      csp: document.body.dataset.csp,
    }));
    const want = { pic: "data:image/png;base64,", env: ".env", up: "../shots/up.png", color: "rgb(1, 2, 3)", storage: "none", csp: "connect-src" };
    for (const k of Object.keys(want)) if (inside[k] !== want[k]) throw new Error(`페이지 안 ${k}: ${JSON.stringify(inside)}`);
    const pageHref = await viewer(dp).getByRole("link", { name: "새 탭에서 열기" }).getAttribute("href");
    if (!pageHref.endsWith("?render=1")) throw new Error(`페이지 새 탭 주소 ${pageHref}`);
    const served = await dp.evaluate((u) => fetch(u).then(async (r) => ({ type: r.headers.get("content-type"), csp: r.headers.get("content-security-policy"), body: await r.text() })), pageHref);
    if (served.type !== "text/html; charset=utf-8" || !served.csp?.startsWith("sandbox allow-scripts;") || !served.csp.includes("default-src 'none'") || !served.body.includes('src=".env"'))
      throw new Error(`페이지 응답: ${JSON.stringify({ ...served, body: served.body.slice(0, 200) })}`);
    await shot(dp, "desktop-file-page");
    // 소스로
    await viewer(dp).getByRole("button", { name: "소스" }).click();
    await codeIs(dp, "HTML 소스", html);
    await dp.keyboard.press("Escape");
    await closed(dp, "페이지 Escape");

    // 동영상: 흘려 받는 길(Range)
    await clickLink(dp, "av/clip.mp4");
    const video = viewer(dp).locator("video.file-viewer-media");
    await video.waitFor();
    const clip = await video.evaluate((v) => fetch(v.src, { headers: { range: "bytes=0-99" } }).then((r) => ({ status: r.status, type: r.headers.get("content-type"), range: r.headers.get("content-range") })));
    if (clip.status !== 206 || clip.type !== "video/mp4" || clip.range !== "bytes 0-99/4096") throw new Error(`동영상 읽기 ${JSON.stringify(clip)}`);
    await dp.keyboard.press("Escape");
    await closed(dp, "동영상 Escape");

    // 소리: 브라우저가 읽어 길이를 안다
    await clickLink(dp, "av/tone.wav");
    const audio = viewer(dp).locator("audio.file-viewer-audio");
    await audio.waitFor();
    const duration = await audio.evaluate(
      (a) =>
        new Promise((done, fail) => {
          if (a.readyState >= 1) return done(a.duration);
          a.addEventListener("loadedmetadata", () => done(a.duration));
          a.addEventListener("error", () => fail(new Error(`소리 오류 ${a.error?.code}`)));
        }),
    );
    if (duration !== 0.5) throw new Error(`소리 길이 ${duration}`);
    await dp.keyboard.press("Escape");
    await closed(dp, "소리 Escape");

    // PDF: 브라우저의 PDF 보기가 그린다(sandbox CSP를 받지 않는다). 이 앱만 frame에 넣는다(앱 기본 CSP는 frame을 막는다)
    await clickLink(dp, "av/manual.pdf");
    const pdf = viewer(dp).locator("iframe.file-viewer-pdf");
    await pdf.waitFor();
    const doc = await pdf.evaluate((f) => fetch(f.src).then((r) => ({ type: r.headers.get("content-type"), csp: r.headers.get("content-security-policy"), frame: r.headers.get("x-frame-options") })));
    if (doc.type !== "application/pdf" || doc.csp !== "frame-ancestors 'self'" || doc.frame !== "SAMEORIGIN") throw new Error(`PDF 읽기 ${JSON.stringify(doc)}`);
    await dp.keyboard.press("Escape");
    await closed(dp, "PDF Escape");
    await dp.context().close();
    aaa("pane", "kill", "media");
  });

  await step("파일 보기 문법 강조(herdr-web-ui HighlightedCode·highlight.ts·highlightOffThread, plan §8.125): 긴 소스는 worker가 칠하고 그동안 글 그대로이며, worker가 예산(2초) 안에 못 칠하는 소스는 글 그대로에 「너무 길어 강조 표시를 생략했습니다」를 단다", async () => {
    const proj = `${root}/files/code`;
    for (const d of ["src", "data"]) fs.mkdirSync(`${proj}/${d}`, { recursive: true });
    // 2KB(SYNC_HIGHLIGHT_LIMIT)를 넘는 소스: 그리는 동안이 아니라 worker가 칠한다
    const long = Array.from({ length: 200 }, (_, i) => `export const v${String(i).padStart(3, "0")} = "s"; // c${i}`).join("\n") + "\n";
    fs.writeFileSync(`${proj}/src/long.ts`, long);
    // YAML 대시 한 줄: 참고 시험의 tokenizer 제곱 꼴(이 크기는 30초쯤)
    const dashes = "-".repeat(200_000);
    fs.writeFileSync(`${proj}/data/dashes.yaml`, dashes);
    const lines = ["L src/long.ts", "Y data/dashes.yaml"];
    const prog = `process.stdout.write(${JSON.stringify(lines.join("\r\n") + "\r\ncode> ")}); process.stdin.resume();`;
    aaa("tab", "create", "--machine", "labA", "--label", "code", "--cwd", proj, "--", process.execPath, "-e", prog);
    await waitOutput("code", "code>");
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await dp.getByTestId("pane-code").click();
    await screenHas(dp, "code 줄들", "code>");

    await clickLink(dp, "src/long.ts");
    await codeIs(dp, "긴 소스", long);
    await domIs(dp, "worker가 칠한 긴 소스", () => {
      const k = document.querySelector('[data-testid="file-viewer-code"] .hl-line[data-line="200"] .hl-keyword');
      return k?.textContent === "export";
    });
    await dp.keyboard.press("Escape");
    await closed(dp, "긴 소스 Escape");

    // 글은 바로 보이고(worker를 기다리지 않음), 예산이 지나면 worker를 끝내고 글 그대로 알린다
    await clickLink(dp, "data/dashes.yaml");
    await codeIs(dp, "대시 소스", dashes);
    await domIs(dp, "너무 길어 알림", () => document.querySelector(".file-viewer-notice")?.textContent.includes("너무 길어 강조 표시를 생략했습니다"));
    const painted = await dp.evaluate(() => document.querySelectorAll('[data-testid="file-viewer-code"] [class^="hl-"]:not(.hl-line):not(.hl-code)').length);
    if (painted !== 0) throw new Error(`예산을 넘은 소스에 색 ${painted}`);
    await shot(dp, "desktop-file-too-long");
    await dp.keyboard.press("Escape");
    await closed(dp, "대시 소스 Escape");
    await dp.context().close();
    aaa("pane", "kill", "code");
  });
}
