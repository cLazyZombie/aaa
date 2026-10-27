// 웹 E2E thread 변경 파일 카드와 diff 패널(t-0403 추천 3, plan §8.305). 실제 git 저장소 thread의 worktree를 바꾸고
// (branch 커밋·지움·새 파일·무시한 파일), thread 화면의 카드(파일 수·숫자·접힌 폴더)와 diff 패널(넓으면 옆 칸, 좁으면 덮는 sheet,
// 고른 파일이 펼쳐짐, 뒤로로 닫힘)을 데스크톱 Chromium·iPad·iPhone WebKit에서 본다. 큰 비교는 파일마다 읽는다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 42-thread-diff(새 실험실).

import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { aaa, until, step, webkit, domIs, shot, root, openSettings } from "../lib/harness.mjs";
import { tapContextButton } from "../lib/diff-context-tap.mjs";
import { pairDevice } from "../lib/devices.mjs";

let thread = "";
let cwd = "";
const hash = () => `#thread=demo%2F${thread}`;
const git = (dir, ...a) =>
  execFileSync("git", ["-C", dir, "-c", "user.name=e2e", "-c", "user.email=e2e@example.com", ...a], { encoding: "utf8" }).trim();

/** thread 화면을 열고 카드가 그 파일 수로 그려질 때까지 */
async function openThread(p, files) {
  await p.evaluate((h) => (location.hash = h), hash());
  await p.getByTestId("thread-view").waitFor();
  await domIs(p, `변경 파일 카드 ${files}개`, (n) => document.querySelector('[data-testid="changed-files"]')?.dataset.files === String(n), files);
}

/** 패널이 다 읽었다(제품 표지) */
const panelReady = (p) => domIs(p, "diff 패널 읽음", () => document.querySelector('[data-testid="diff-panel"]')?.dataset.diffStatus === "ready");

/** diff 패널 머리 한 줄과 닫기 단추 자리. 휴대폰 sheet는 아래에서 올라온다(modal.css sheet-in): 다 올라온 뒤 잰다.
 *  WebKit은 움직이는 동안 getBoundingClientRect 사이에도 움직임이 흘러 머리와 단추가 다른 자리로 잡힌다 */
const panelLook = (p) =>
  p.evaluate(async () => {
    const panel = document.querySelector('[data-testid="diff-panel"]');
    await Promise.all([panel, panel.closest(".modal-scrim")].flatMap((el) => el?.getAnimations() ?? []).map((a) => a.finished));
    const head = panel.querySelector(".diff-panel-head").getBoundingClientRect();
    const close = panel.querySelector(".diff-close").getBoundingClientRect();
    const box = panel.getBoundingClientRect();
    const kids = [...panel.querySelectorAll(".diff-panel-head > *")].map((e) => e.getBoundingClientRect()).filter((r) => r.width > 0);
    return {
      sheet: panel.classList.contains("sheet"),
      width: Math.round(box.width),
      left: Math.round(box.left),
      head: Math.round(head.height),
      oneLine: kids.every((r) => r.top >= head.top - 1 && r.bottom <= head.bottom + 1),
      close: { w: Math.round(close.width), h: Math.round(close.height), inside: close.right <= innerWidth && close.top >= 0 },
    };
  });

/** CodeView 머리마다 파일 이름과 펼침(머리는 shadow root 안, 접기 단추는 머리 slot) */
const fileHeads = (p) =>
  p.evaluate(() =>
    [...document.querySelectorAll('[data-testid="diff-panel"] .diff-file-toggle')].map((b) => ({
      name: b.getAttribute("aria-label").replace(/^(펼치기|접기) /, ""),
      open: b.getAttribute("aria-expanded") === "true",
    })),
  );

/** 처음에는 변경 주변만, 사용자가 누르면 네이티브 UI가 원문을 한 번 읽고 맥락을 펼친다. */
async function expandContext(p, name, refreshDuringKey = false) {
  let reads = 0;
  const requests = [];
  const record = (request) => {
    if (request.method() !== "POST") return;
    const body = request.postDataJSON();
    if (body?.op === "thread_diff") requests.push({ at: Date.now(), query: body.query });
    if (body?.op === "thread_diff" && body.query?.full_context) reads++;
  };
  p.on("request", record);
  try {
    const card = p.getByTestId("changed-files");
    if (await card.locator('[data-path="src/lib.rs"]').count() === 0) await card.getByRole("button", { name: "모든 폴더 펼치기" }).click();
    await card.locator('[data-path="src/lib.rs"]').click();
    await panelReady(p);
    const diff = p.locator("diffs-container").filter({ has: p.getByRole("button", { name: "접기 src/lib.rs", exact: true }) });
    const context = (i) => diff.locator('[data-line-type^="context"]').filter({ hasText: `fn f${i}()` }).first();
    const scroll = p.locator(".diff-code-view");
    await diff.getByRole("button", { name: "위로 20줄 펼치기", exact: true }).first().waitFor();
    if (await context(0).count() || await context(60).count() || reads) throw new Error("기본 맥락이 펼쳐졌거나 원문을 미리 읽었다");
    const touch = await p.evaluate(() => matchMedia("(pointer: coarse)").matches);
    const more = diff.getByRole("button", { name: "위로 20줄 펼치기", exact: true }).first();
    if (touch) {
      // 실제 재조회로 옛 단추를 제거한 뒤 tap이 받은 최종 DOM에서 44px를 검증한다.
      await tapContextButton(p, more);
      const diagnostics = process.env.AAA_E2E_ARTIFACTS ?? root;
      fs.mkdirSync(diagnostics, { recursive: true });
      fs.writeFileSync(`${diagnostics}/42-context-tap-${process.pid}.json`, JSON.stringify(await p.evaluate(() => ({
        touch: window.__contextTap.events,
        reflow: { before: window.__contextReflow.before, after: window.__contextReflow.after, events: window.__contextReflow.events },
      })), null, 2));
    } else if (refreshDuringKey) {
      const refresh = async () => {
        const response = p.waitForResponse((r) => {
          if (r.request().method() !== "POST") return false;
          const body = r.request().postDataJSON();
          return body?.op === "thread_diff" && !body.query?.full_context;
        });
        await p.evaluate(() => window.dispatchEvent(new Event("focus")));
        await (await response).finished();
        // 재조회 뒤 실제 렌더 frame까지 보내고, 새 단추에 다시 focus하지 않은 채 키를 누른다.
        await p.evaluate(() => new Promise(requestAnimationFrame));
      };
      const scope = p.getByLabel("비교 범위");
      await scope.focus();
      await refresh();
      if (!await scope.evaluate((el) => document.activeElement === el)) throw new Error("다른 조작의 focus를 빼앗았다");
      await more.focus();
      await refresh();
      if (!await more.evaluate((el) => el.getRootNode().activeElement === el)) throw new Error("맥락 단추의 키 입력 focus가 재조회에 사라졌다");
      await p.keyboard.press("Enter");
    } else await more.press("Enter");
    await context(60).waitFor();
    if (await context(0).count()) throw new Error("20줄 대신 전체가 펼쳐졌다");
    await shot(p, `42-${name}-context-20`);
    if (!touch) {
      await p.getByRole("button", { name: "나란히 보기" }).click();
      await diff.locator('[data-diff-type="split"]').waitFor();
      await context(60).waitFor();
      await shot(p, `42-${name}-context-split`);
      await p.getByRole("button", { name: "한 줄로 보기" }).click();
      await diff.locator('[data-diff-type="single"]').waitFor();
    }
    await diff.getByRole("button", { name: "모두 펼치기", exact: true }).first().click();
    await context(0).waitFor();
    await scroll.evaluate((el) => { el.scrollTop = el.scrollHeight; });
    await diff.getByRole("button", { name: "모두 펼치기", exact: true }).last().click();
    await diff.locator('[data-expand-all-button]').first().waitFor({ state: "detached" });
    await scroll.evaluate((el) => { el.scrollTop = el.scrollHeight; });
    await context(159).waitFor();
    if (reads !== 1) throw new Error(`원문 읽기 ${reads}회`);
    await shot(p, `42-${name}-context-all`);
    await p.getByRole("button", { name: "diff 닫기" }).click();
    await domIs(p, "맥락 diff 닫힘", () => !document.querySelector('[data-testid="diff-panel"]'));
  } catch (error) {
    await p.screenshot({ path: `${root}/context-failure.png` });
    fs.writeFileSync(`${root}/context-failure.json`, JSON.stringify({ reads, requests, ...await p.evaluate(() => ({
      touch: window.__contextTap?.events,
      reflow: window.__contextReflow,
      alert: document.querySelector('[role="alert"]')?.textContent,
      buttons: [...document.querySelectorAll('diffs-container')].flatMap((host) => [...host.shadowRoot.querySelectorAll('[data-expand-button]')].map((button) => {
        const r = button.getBoundingClientRect();
        const hit = [];
        let el = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
        while (el) {
          hit.push([el.tagName, el.className, el.getAttribute('style')]);
          const next = el.shadowRoot?.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
          if (next === el) break;
          el = next;
        }
        const ancestors = [];
        for (let el = button; el; el = el.parentElement ?? el.getRootNode().host) ancestors.push([el.tagName, getComputedStyle(el).pointerEvents, el.getAttribute('style')]);
        return { label: button.getAttribute('aria-label'), box: r.toJSON(), hit, ancestors };
      })),
      shadow: [...document.querySelectorAll('diffs-container')].map((e) => e.shadowRoot?.innerHTML),
    })) }, null, 2));
    throw error;
  } finally {
    p.off("request", record);
  }
}

export default async function ({ page }) {
  await step("준비: main에 커밋한 저장소, 그 저장소 thread의 worktree에 branch 커밋·지움·새 파일·무시한 파일", async () => {
    const repo = `${root}/repos/diffgame`;
    fs.mkdirSync(`${repo}/src`, { recursive: true });
    fs.mkdirSync(`${repo}/docs`, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    fs.writeFileSync(`${repo}/README.md`, "# 게임\n\n처음 글\n");
    fs.writeFileSync(`${repo}/src/lib.rs`, Array.from({ length: 160 }, (_, i) => `fn f${i}() -> u32 { ${i} }\n`).join(""));
    fs.writeFileSync(`${repo}/docs/old.md`, "옛 문서\n");
    fs.writeFileSync(`${repo}/.gitignore`, "*.log\n");
    git(repo, "add", "-A");
    git(repo, "commit", "-qm", "first");
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/diff-task.md`, "변경 보기\n");
    thread = aaa("thread", "start", "demo", "--title", "변경 보기", "--machine", "labA", "--profile", "fake", "--repo", repo, "--task-file", `${root}/diff-task.md`).id;
    await until("보고 회수", () => aaa("thread", "show", "demo", thread).report_hash);
    cwd = aaa("thread", "show", "demo", thread).cwd;
    // 가짜 agent가 보고 뒤 worktree에 쓰는 새 파일 둘(notes/plan.md, src/app.ts)
    await until("가짜 agent의 새 파일", () => fs.existsSync(`${cwd}/src/app.ts`));
    const lib = fs.readFileSync(`${cwd}/src/lib.rs`, "utf8").replace("fn f3() -> u32 { 3 }", "fn f3() -> u32 { 33 }");
    fs.writeFileSync(`${cwd}/src/lib.rs`, lib);
    git(cwd, "commit", "-qam", "branch change");
    fs.rmSync(`${cwd}/docs/old.md`);
    fs.writeFileSync(`${cwd}/README.md`, "# 게임\n\n고친 글\n더한 줄\n");
    fs.mkdirSync(`${cwd}/assets/sprites/hero`, { recursive: true });
    fs.writeFileSync(`${cwd}/assets/sprites/hero/walk.txt`, "1\n2\n3\n");
    fs.writeFileSync(`${cwd}/debug.log`, "무시\n");
    // main이 앞으로 가도 갈라진 곳 대비다
    git(repo, "commit", "-q", "--allow-empty", "-m", "main moves");
  });

  // README +2 -1, src/lib.rs +1 -1, docs/old.md -1, walk.txt +3, notes/plan.md +3, src/app.ts +3
  const FILES = 6;
  await step("데스크톱 카드: 「파일 6개 +12 -3 · main 대비」, 폴더는 접힌 채 시작하고 모두 펼치기를 기억, 파일을 누르면 옆 칸 diff가 그 파일을 펼친다, 뒤로 닫힌다", async () => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await openThread(page, FILES);
    const card = page.getByTestId("changed-files");
    const look = await card.evaluate((c) => ({
      count: c.querySelector('[data-testid="changed-files-count"]').textContent,
      total: c.querySelector(".changed-files-title .diff-stat").textContent,
      base: c.querySelector(".changed-files-base").textContent,
      rows: [...c.querySelectorAll(".changed-files-row")].map((r) => r.textContent),
      // 카드 폭 24rem 위에서는 「diff 열기」 글이 보인다
      open: c.querySelector(".changed-files-open-word").getBoundingClientRect().width > 0,
    }));
    const want = {
      count: "파일 6개",
      total: "+12-3",
      base: "main 대비",
      // 폴더 먼저(한 자식 폴더는 한 줄), 접힌 채
      rows: ["assets/sprites/hero+3-0", "docs+0-1", "notes+3-0", "src+4-1", "README.md+2-1"],
      open: true,
    };
    if (JSON.stringify(look) !== JSON.stringify(want)) throw new Error(`카드 ${JSON.stringify(look)}`);
    await shot(page, "42-desktop-card");
    await card.getByRole("button", { name: "모든 폴더 펼치기" }).click();
    await domIs(page, "모든 폴더 펼침", () => document.querySelectorAll('[data-testid="changed-files"] .changed-files-row.file').length === 6);
    if ((await page.evaluate((t) => localStorage.getItem(`aaa:diff-folders:demo/${t}`), thread)) !== "1") throw new Error("펼침 기억");
    await card.locator('.changed-files-row.file[data-path="src/lib.rs"]').click();
    await panelReady(page);
    await domIs(page, "src/lib.rs만 펼침", () => {
      const b = [...document.querySelectorAll('[data-testid="diff-panel"] .diff-file-toggle')];
      return b.length === 6 && b.filter((x) => x.getAttribute("aria-expanded") === "true").map((x) => x.getAttribute("aria-label")).join() === "접기 src/lib.rs";
    });
    const panel = await panelLook(page);
    if (panel.sheet || panel.width < 400 || panel.head > 44 || !panel.oneLine) throw new Error(`데스크톱 패널 ${JSON.stringify(panel)}`);
    // 바뀐 줄이 그려졌다(shadow root 안)
    await until("src/lib.rs의 바뀐 줄", () =>
      page.evaluate(() => [...document.querySelectorAll('[data-testid="diff-panel"] *')].some((el) => el.shadowRoot?.textContent.includes("{ 33 }"))),
    );
    await shot(page, "42-desktop-diff");
    // 나란히 보기(넓은 화면만)
    await page.getByRole("button", { name: "나란히 보기" }).click();
    await until("나란히", () => page.evaluate(() => [...document.querySelectorAll('[data-testid="diff-panel"] *')].some((el) => el.shadowRoot?.querySelector('[data-diff-type="split"]'))));
    await page.getByRole("button", { name: "한 줄로 보기" }).click();
    // 미커밋: branch 커밋(src/lib.rs)은 없다
    await page.getByLabel("비교 범위").selectOption("uncommitted");
    await panelReady(page);
    await domIs(page, "미커밋 파일 5개", () => document.querySelectorAll('[data-testid="diff-panel"] .diff-file-toggle').length === 5);
    if ((await fileHeads(page)).some((f) => f.name === "src/lib.rs")) throw new Error("미커밋에 branch 커밋이 보인다");
    await page.getByLabel("비교 범위").selectOption("branch");
    await panelReady(page);
    const before = await page.evaluate(() => location.hash);
    await page.goBack();
    await domIs(page, "뒤로 diff 닫힘", () => !document.querySelector('[data-testid="diff-panel"]') && !!document.querySelector('[data-testid="changed-files"]'));
    if ((await page.evaluate(() => location.hash)) !== before) throw new Error("뒤로가 thread 화면을 떠났다");
  }, { diagnose: async () => {
    await page.screenshot({ path: `${root}/diff-failure.png` });
    fs.writeFileSync(`${root}/diff-failure.json`, JSON.stringify(await page.evaluate(() => ({
      heads: [...document.querySelectorAll('.diff-file-toggle')].map((e) => [e.outerHTML, e.getBoundingClientRect().toJSON()]),
      panel: document.querySelector('[data-testid="diff-panel"]')?.outerHTML,
      shadow: [...document.querySelectorAll('diffs-container')].map((e) => e.shadowRoot?.innerHTML),
    })), null, 2));
  } });

  await step("데스크톱 맥락: 기본은 변경 주변, 키보드로 20줄·전체 펼치기와 원문 한 번 읽기", async () => {
    // 기존 카드 단계의 변경 위치는 유지한다. 이 단계에서만 양쪽에 긴 맥락을 둔다.
    fs.writeFileSync(`${cwd}/src/lib.rs`, Array.from({ length: 160 }, (_, i) => `fn f${i}() -> u32 { ${i === 80 ? 33 : i} }\n`).join(""));
    await openThread(page, FILES);
    await expandContext(page, "desktop");
  });

  await step("데스크톱 맥락: 재조회 중 단추 focus와 다른 조작의 focus를 지킨다", async () => {
    await openThread(page, FILES);
    await expandContext(page, "desktop-focus-refresh", true);
  });

  await step("iPhone 맥락: 기본은 변경 주변, 20줄·전체 펼치기와 원문 한 번 읽기", async () => {
    fs.writeFileSync(`${cwd}/src/lib.rs`, Array.from({ length: 160 }, (_, i) => `fn f${i}() -> u32 { ${i === 80 ? 33 : i} }\n`).join(""));
    const wk = await webkit.launch();
    try {
      const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", {});
      await openThread(p, FILES);
      await expandContext(p, "iphone-390");
    } finally {
      await wk.close();
    }
  });

  await step("iPhone 390·430·iPad 820(WebKit): 카드 줄 40px, diff는 화면을 덮는 sheet, 머리 한 줄·닫기 44px, 닫기와 뒤로가 같은 칸", async () => {
    const wk = await webkit.launch();
    try {
    for (const [profile, width] of [["iphone", 390], ["iphone", 430], ["ipad", 820]]) {
      const { page: p } = await pairDevice(wk, aaa("web", "pair").code, profile, {});
      try {
        if (width !== 390 && profile === "iphone") await p.setViewportSize({ width, height: 932 });
        await openThread(p, FILES);
        const rowH = await p.locator('[data-testid="changed-files"] .changed-files-row').first().evaluate((r) => r.getBoundingClientRect().height);
        if (rowH < 40) throw new Error(`${profile} ${width} 카드 줄 ${rowH}px`);
        await p.getByTestId("changed-files").scrollIntoViewIfNeeded();
        await shot(p, `42-${profile}-${width}-card`);
        await p.getByRole("button", { name: "diff 열기" }).click();
        await panelReady(p);
        const look = await panelLook(p);
        const vw = p.viewportSize().width;
        if (!look.sheet || !look.oneLine || look.close.w < 44 || look.close.h < 44 || !look.close.inside) throw new Error(`${profile} ${width} sheet ${JSON.stringify(look)}`);
        if (profile === "iphone" && (look.width !== vw || look.left !== 0)) throw new Error(`휴대폰 sheet가 화면을 다 덮지 않는다 ${JSON.stringify(look)}`);
        // 「diff 열기」는 첫 파일을 펼친다(t3code `files[0]`)
        await domIs(p, "첫 파일 펼침", () => document.querySelector('[data-testid="diff-panel"] .diff-file-toggle')?.getAttribute("aria-expanded") === "true");
        if ((await p.getByRole("button", { name: "나란히 보기" }).count()) !== (width > 768 ? 1 : 0)) throw new Error(`${profile} ${width} 나란히 단추`);
        await shot(p, `42-${profile}-${width}-diff`);
        await p.getByRole("button", { name: "diff 닫기" }).click();
        await domIs(p, "닫기로 닫힘", () => !document.querySelector('[data-testid="diff-panel"]'));
        if ((await p.evaluate(() => history.state?.["aaa:thread-diff"] ?? null)) !== null) throw new Error("닫은 뒤 기록 칸이 남았다");
      } finally {
        await p.context().close();
      }
    }
    } finally {
      await wk.close();
    }
  });

  await step("밝은 테마(데스크톱 Chromium·iPhone 390 WebKit): 카드와 diff 패널·diff 줄 바탕이 밝은 색이고 더한 줄 색이 바탕과 구별된다", async () => {
    // 테마는 공유 설정이다(기본 어두움): 설정에서 밝게로 바꾸면 휴대폰에도 온다. 끝에 어둡게로 돌린다
    const theme = async (t) => {
      const d = await openSettings(page, "모양");
      await d.getByRole("group", { name: "테마" }).getByRole("button", { name: t === "light" ? "밝게" : "어둡게" }).click();
      await page.keyboard.press("Escape");
      await d.waitFor({ state: "detached" });
      await domIs(page, `테마 ${t}`, (x) => document.documentElement.dataset.theme === x, t);
    };
    // 줄 바탕은 CodeView shadow root 안이라 문서 CSS가 닿지 않는다: 앱 색 변수가 그 안까지 닿았는지 본다
    const light = (p) =>
      p.evaluate(() => {
        const lum = (c) => {
          const [r, g, b] = c.match(/\d+(\.\d+)?/g).map(Number);
          return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
        };
        const host = [...document.querySelectorAll('[data-testid="diff-panel"] *')].find((el) => el.shadowRoot?.querySelector("[data-line-type='change-addition']"));
        const add = host?.shadowRoot.querySelector("[data-line-type='change-addition']");
        const ctx = host?.shadowRoot.querySelector("[data-line-type='context']");
        return {
          card: lum(getComputedStyle(document.querySelector('[data-testid="changed-files"]')).backgroundColor),
          head: lum(getComputedStyle(document.querySelector(".diff-panel-head")).backgroundColor),
          line: ctx ? lum(getComputedStyle(ctx).backgroundColor) : null,
          added: add ? lum(getComputedStyle(add).backgroundColor) : null,
        };
      });
    const check = async (p, name) => {
      await openThread(p, FILES);
      await p.getByTestId("changed-files").scrollIntoViewIfNeeded();
      await shot(p, `42-${name}-light-card`);
      // 앞 단계에서 「모든 폴더 펼치기」를 기억한 화면이면 이미 펼쳐져 있다
      if ((await p.locator('[data-testid="changed-files"] .changed-files-row.file[data-path="src/lib.rs"]').count()) === 0)
        await p.getByRole("button", { name: "모든 폴더 펼치기" }).click();
      await p.locator('[data-testid="changed-files"] .changed-files-row.file[data-path="src/lib.rs"]').click();
      await panelReady(p);
      await until(`${name} 밝은 diff 줄`, async () => (await light(p)).added !== null);
      const l = await light(p);
      if (l.card < 0.8 || l.head < 0.8 || l.line < 0.8 || Math.abs(l.added - l.line) < 0.02) throw new Error(`${name} 밝은 테마 ${JSON.stringify(l)}`);
      await shot(p, `42-${name}-light-diff`);
    };
    await page.setViewportSize({ width: 1280, height: 800 });
    await theme("light");
    try {
      await check(page, "desktop");
      await page.goBack();
      await domIs(page, "밝은 테마 diff 닫힘", () => !document.querySelector('[data-testid="diff-panel"]'));
      const wk = await webkit.launch();
      try {
        const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", {});
        await domIs(p, "휴대폰에 온 밝은 테마", () => document.documentElement.dataset.theme === "light");
        await check(p, "iphone-390");
      } finally {
        await wk.close();
      }
    } finally {
      await theme("dark");
    }
  });

  await step("큰 비교: 전체 patch가 상한(120,000바이트)을 넘으면 목록만 받고 파일마다 읽는다, 아주 큰 파일은 「일부」", async () => {
    fs.writeFileSync(`${cwd}/big.txt`, `${"큰 파일 한 줄 ".repeat(20)}\n`.repeat(9000));
    await page.setViewportSize({ width: 1280, height: 800 });
    // 카드는 앞 단계의 것 그대로(보고가 바뀌지 않았다), 패널은 열 때 새로 읽는다
    await page.getByRole("button", { name: "diff 열기" }).click();
    await domIs(page, "파일마다 읽기", () => document.querySelector('[data-testid="diff-panel"]')?.dataset.diffLazy === "");
    // 바닥이 600px 안에 들면 다음 넷: 일곱 파일 모두 읽힌다
    await domIs(page, "일곱 파일 읽음", () => document.querySelector('[data-testid="diff-panel"]')?.dataset.diffLoaded === "7");
    await domIs(page, "big.txt 「일부」", () => [...document.querySelectorAll('[data-testid="diff-panel"] .diff-tag.warn')].some((t) => t.textContent === "일부"));
    await shot(page, "42-desktop-lazy");
    await page.goBack();
    await domIs(page, "닫힘", () => !document.querySelector('[data-testid="diff-panel"]'));
  });
}
