// 긴 본문의 끝단에서 문서·왼쪽 목록으로 스크롤이 전달되지 않아야 한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { publishReport } from "../lib/report-file.mjs";
import { root, base, aaa, step, browser, webkit, pairPage, until, domIs, shot, openSettings, inTime, waitOutput } from "../lib/harness.mjs";

const frames = (p) => p.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
const navigate = async (p, hash, selector) => {
  await p.evaluate((hash) => { location.hash = hash; }, hash);
  await p.locator(selector).waitFor();
};

async function boundaries(p, selector, label, wheel) {
  const body = p.locator(selector);
  await body.waitFor();
  const geometry = await body.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { height: el.clientHeight, content: el.scrollHeight, top: r.top, bottom: r.bottom, width: el.clientWidth, contentWidth: el.scrollWidth, overflow: getComputedStyle(el).overflowY, viewport: innerHeight };
  });
  assert.ok(geometry.height > 0 && geometry.content > geometry.height + 80, `${label} 실제 긴 본문: ${JSON.stringify(geometry)}`);
  assert.ok(geometry.top >= -1 && geometry.bottom <= geometry.viewport + 1, `${label} 화면 안 높이: ${JSON.stringify(geometry)}`);
  assert.ok(geometry.contentWidth <= geometry.width + 1, `${label} 가로 넘침`);
  assert.match(geometry.overflow, /auto|scroll/, `${label} 본문 스크롤 허용`);
  // 머리 없는 headless WebKit이 OS 고무줄을 그리지 않아도 전달 경계의 누락을 잡는다.
  const isolation = await body.evaluate((el) => ({
    chain: getComputedStyle(el).overscrollBehaviorY,
    html: getComputedStyle(document.documentElement).overflowY,
    body: getComputedStyle(document.body).overflowY,
  }));
  assert.match(isolation.chain, /contain|none/, `${label} 본문 전달 차단`);
  assert.equal(isolation.html, "hidden", `${label} html 스크롤 잠금`);
  assert.equal(isolation.body, "hidden", `${label} body 스크롤 잠금`);
  const fixed = () => p.evaluate(() => {
    const rect = (s) => { const e = document.querySelector(s); if (!e) return null; const r = e.getBoundingClientRect(); return [r.top, r.bottom, e.scrollTop]; };
    return { x: scrollX, y: scrollY, app: rect(".app"), sidebar: rect("#sidebar-column"), list: rect("#sidebar-column .asks-panel"), view: rect(".view"), viewport: window.visualViewport && [visualViewport.offsetTop, visualViewport.offsetLeft] };
  });
  const before = await fixed();
  await body.evaluate((el) => { el.scrollTop = 0; });
  await frames(p);
  if (wheel) {
    const box = await body.boundingBox();
    await p.mouse.move(box.x + box.width / 2, box.y + Math.min(box.height / 2, 150));
    await body.evaluate((el) => {
      window.__bodyScrollMoved = new Promise((done) => el.addEventListener("scroll", () => done(), { once: true }));
    });
    await p.mouse.wheel(0, 120);
    await inTime(p.evaluate(() => window.__bodyScrollMoved), 20_000, "본문 scroll 사건");
  } else {
    // mobile WebKit은 Playwright wheel/swipe를 지원하지 않는다. 실제 overflow와 scroll 사건을 확인한다.
    await inTime(body.evaluate((el) => new Promise((done) => {
      el.addEventListener("scroll", () => done(), { once: true });
      el.scrollTop = 120;
    })), 20_000, "휴대폰 본문 scroll 사건");
  }
  assert.ok(await body.evaluate((el) => el.scrollTop > 0), `${label} 본문 이동`);
  assert.deepEqual(await fixed(), before, `${label} 본문 이동 중 바깥 고정`);
  for (const end of ["bottom", "top"]) {
    await body.evaluate((el, end) => { el.scrollTop = end === "bottom" ? el.scrollHeight : 0; }, end);
    await frames(p);
    if (wheel) {
      const box = await body.boundingBox();
      await p.mouse.move(box.x + box.width / 2, box.y + Math.min(box.height / 2, 150));
      // 실제 wheel 수신 사건 뒤 렌더링 프레임을 기다린다. 고정 sleep은 쓰지 않는다.
      await p.evaluate(() => { window.__bodyScrollWheel = new Promise((done) => window.addEventListener("wheel", () => requestAnimationFrame(() => requestAnimationFrame(done)), { once: true, capture: true })); });
      await p.mouse.wheel(0, end === "bottom" ? 900 : -900);
      await inTime(p.evaluate(() => window.__bodyScrollWheel), 20_000, "본문 wheel 사건");
    }
    assert.deepEqual(await fixed(), before, `${label} ${end}: 바깥 틀·문서·왼쪽 목록 고정`);
    assert.ok(await body.evaluate((el, end) => end === "bottom" ? Math.abs(el.scrollHeight - el.clientHeight - el.scrollTop) <= 2 : el.scrollTop === 0, end), `${label} ${end}: 본문 끝 유지`);
  }
  await body.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  await frames(p);
  await shot(p, `body-scroll-${label}`);
}

export default async function () {
  let thread, question, task;
  await step("긴 본문 준비: 실제 thread 질문·그림·표·보고서와 할 일", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/scroll-task.md`, "질문 카드 시험\n본문 끝단 스크롤 회귀");
    thread = aaa("thread", "start", "demo", "--title", "본문 스크롤", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/scroll-task.md`);
    // asked-cards는 printf가 끝나 report.md의 FD가 닫힌 뒤에 나온다.
    // 파일 존재나 마지막 글자만으로는 close를 보장하지 못한다(Windows는 열린 대상 교체가 실패한다).
    await waitOutput(thread.pane, "asked-cards");
    fs.writeFileSync(`${thread.work_dir}/library/scroll.svg`, '<svg xmlns="http://www.w3.org/2000/svg" width="800" height="300"><rect width="800" height="300" fill="#8a623a"/><circle cx="400" cy="150" r="110" fill="#bad8de"/></svg>');
    const content = Array.from({ length: 16 }, (_, i) => `#### 비교 ${i + 1}\n\n![비교 그림](library/scroll.svg)\n\n| 판단 | 내용 |\n| --- | --- |\n| 효과 | 긴 본문과 표를 끝까지 읽는다 |\n| 비용 | 스크롤 영역 안에서 확인한다 |\n`).join("\n");
    publishReport(`${thread.work_dir}/report.md`, `[결정 필요] 본문 스크롤\n\n## Report\n긴 보고서\n\n## Questions\n### 1. 긴 질문\n${content}\n- A 진행 (추천) — 끝까지 확인\n- B 보류 — 나중에 확인\n\n## Next\n\n## 진행\n- [ ] 본문 확인\n`);
    question = await until("긴 질문 수집", () => { const q = aaa("thread", "show", "demo", thread.id).questions; return q?.items[0]?.body?.includes("비교 16") && q.items[0].id; });
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, `## Backlog\n- [ ] 긴 할 일 (me)\n${Array.from({ length: 50 }, (_, i) => `  설명 ${i + 1}: 할 일 상세 본문 스크롤`).join("\n")}\n`);
    task = aaa("task", "list").tasks.find((t) => t.title === "긴 할 일").short_id;
  }, { always: true });
  const combination = async (engine, instance, name, viewport, touch, theme) => {
    const { page: p, errors } = await pairPage(instance, aaa("web", "pair", "--reader").code, viewport, { touch, hash: `#question=${question}` });
    try {
      await p.evaluate((theme) => localStorage.setItem("aaa:appearance", JSON.stringify({ theme, palette: "amber" })), theme);
      await p.reload();
      await domIs(p, "테마 적용", (theme) => document.documentElement.dataset.theme === theme, theme);
      const label = `${engine}-${name}-${theme}`;
      await p.locator('[data-testid="asks-scroll"] img').first().waitFor();
      // 그림은 lazy 로드된다. 각 그림을 실제로 보인 뒤 decode 완료를 기다린다.
      for (const img of await p.locator('[data-testid="asks-scroll"] img').all()) {
        await img.scrollIntoViewIfNeeded();
        await inTime(img.evaluate((img) => img.decode()), 20_000, "질문 그림 decode");
      }
      await boundaries(p, '[data-testid="asks-scroll"]', `${label}-question`, !touch);
      await navigate(p, `#report=demo%2F${thread.id}`, '[data-testid="report-detail"][data-status="ready"] .thread-body');
      await boundaries(p, '[data-testid="report-detail"] .thread-body', `${label}-report`, !touch);
      await navigate(p, `#thread=${thread.id}`, '[data-testid="thread-view"] .thread-body');
      await p.getByTestId("thread-view").getByText("긴 보고서", { exact: true }).waitFor();
      await boundaries(p, '[data-testid="thread-view"] .thread-body', `${label}-thread`, !touch);
      await navigate(p, `#task=${task}`, '[data-testid="task-detail"]');
      await boundaries(p, '[data-testid="task-detail"] .task-detail-body', `${label}-task`, !touch);
      if (touch) await p.getByTestId("task-detail-close").click();
      const settings = await openSettings(p, "단축키");
      await boundaries(p, '.settings-body:not([hidden])', `${label}-settings`, !touch);
      await settings.getByRole("button", { name: "설정 닫기", exact: true }).click();
      assert.deepEqual(errors, []);
    } finally { await p.context().close(); }
  };
  await step("인증 화면은 문서 스크롤 잠금을 받지 않는다", async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
    try {
      const p = await context.newPage();
      await p.goto(base);
      await p.getByTestId("token-gate").waitFor();
      assert.deepEqual(await p.evaluate(() => [document.documentElement, document.body].map((el) => getComputedStyle(el).overflowY)), ["visible", "visible"]);
    } finally { await context.close(); }
  });
  const desktop = { width: 1280, height: 720 }, phone = { width: 390, height: 844 };
  const wk = await webkit.launch();
  try {
  await step("본문 끝단 Chromium 데스크톱 밝음", () => combination("chromium", browser, "desktop", desktop, false, "light"));
  await step("본문 끝단 Chromium 데스크톱 어두움", () => combination("chromium", browser, "desktop", desktop, false, "dark"));
  await step("본문 끝단 Chromium 휴대폰 밝음", () => combination("chromium", browser, "phone", phone, true, "light"));
  await step("본문 끝단 Chromium 휴대폰 어두움", () => combination("chromium", browser, "phone", phone, true, "dark"));
  await step("본문 끝단 WebKit 데스크톱 밝음", () => combination("webkit", wk, "desktop", desktop, false, "light"));
  await step("본문 끝단 WebKit 데스크톱 어두움", () => combination("webkit", wk, "desktop", desktop, false, "dark"));
  await step("본문 끝단 WebKit 휴대폰 밝음", () => combination("webkit", wk, "phone", phone, true, "light"));
  await step("본문 끝단 WebKit 휴대폰 어두움", () => combination("webkit", wk, "phone", phone, true, "dark"));
  } finally { await wk.close(); }
}
