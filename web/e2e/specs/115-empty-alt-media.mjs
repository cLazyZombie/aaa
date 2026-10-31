// 실제 수집한 보고의 빈 alt가 카드·보고에서 같은 library 미디어를 읽어야 한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { root, aaa, until, step, browser, webkit, pairPage, shot } from "../lib/harness.mjs";

async function checkMedia(surface, mediaBase) {
  const images = surface.locator("img");
  await images.nth(1).waitFor();
  assert.equal(await images.count(), 2);
  for (const img of await images.all()) {
    await img.scrollIntoViewIfNeeded();
    await img.evaluate(img => img.decode());
    assert.ok(await img.evaluate(img => img.naturalWidth > 0));
  }
  assert.deepEqual(await images.evaluateAll(imgs => imgs.map(i => i.alt)), ["", "시안"]);
  const imageUrls = await images.evaluateAll(imgs => imgs.map(i => i.src));
  assert.equal(imageUrls[0], imageUrls[1]);
  assert.equal(new URL(imageUrls[0]).pathname, `${mediaBase}pic.png`);
  assert.ok((await surface.innerText()).includes("[](library/clip.mp4)"), "빈 일반 링크 원문을 남긴다");
  const videos = surface.locator("video");
  assert.equal(await videos.count(), 2, "빈 일반 링크는 재생기를 만들지 않는다");
  assert.deepEqual(await videos.evaluateAll(vs => vs.map(v => [v.getAttribute("src"), v.autoplay, v.loop, v.muted, v.playsInline])),
    [[null, false, false, true, true], [null, false, false, true, true]]);
  const urls = [];
  for (const label of ["시안 영상", "기존 영상"]) {
    const element = surface.locator(`video[aria-label="${label}"]`);
    await surface.getByRole("button", { name: `${label} 재생`, exact: true }).click();
    await element.evaluate(v => new Promise((resolve, reject) => {
      const done = () => {
        if (v.currentTime <= 0) return;
        v.removeEventListener("timeupdate", done);
        v.pause(); resolve();
      };
      if (v.currentTime > 0) return done();
      v.addEventListener("timeupdate", done);
      v.addEventListener("error", () => reject(new Error(`영상 오류 ${v.error?.code}`)), { once: true });
    }));
    assert.ok(await element.evaluate(v => v.readyState >= 2 && v.controls && v.currentTime > 0));
    urls.push(await element.getAttribute("src"));
  }
  assert.equal(urls[0], urls[1], "빈 alt도 같은 경로 resolver를 쓴다");
  assert.equal(urls[0], `${mediaBase}clip.mp4`);
}

export default async function () {
  let thread;
  await step("빈 alt 자료 준비: 실제 보고와 library를 host가 수집한다", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/empty-alt-task.md`, "빈 alt 미디어 회귀\n");
    thread = aaa("thread", "start", "demo", "--title", "빈 alt 미디어", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/empty-alt-task.md`);
    await until("첫 보고", () => aaa("thread", "show", thread.id).report_hash);
    // 기본 fake는 HTML 보고도 만든다. 이 시험은 Markdown 보고 화면을 연다.
    fs.unlinkSync(`${thread.work_dir}/library/index.html`);
    fs.copyFileSync(`${root}/pic.png`, `${thread.work_dir}/library/pic.png`);
    fs.copyFileSync(new URL("../fixtures/report-video.mp4", import.meta.url), `${thread.work_dir}/library/clip.mp4`);
    const body = `[결정 필요] 질문 1개: 미디어\n\n## Questions\n### 1. 시안\n![](library/pic.png)\n![시안](library/pic.png)\n\n![](library/clip.mp4)\n![기존 영상](library/clip.mp4)\n\n[](library/clip.mp4)\n\n- A 진행 (추천)\n- B 보류\n`;
    fs.writeFileSync(`${thread.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    await until("미디어 보고 수집", () => { const t = aaa("thread", "show", thread.id); return t.report_hash === hash && t.questions; });
  });
  const check = async (engine, width) => {
    const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { hash: `#question=${thread.id}` });
    try {
      const card = page.getByTestId(`thread-ask-${thread.id}`);
      await card.waitFor();
      await card.getByTestId("thread-ask-pick-1A").click();
      // 웹 응답은 CLI의 API 경로를 기기별 보기 권한 주소로 바꾼다(web attachment_views).
      const view = await page.evaluate(async thread => {
        const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" },
          body: JSON.stringify({ op: "thread_show", project: "demo", thread }) });
        return (await r.json()).ok;
      }, thread.id);
      assert.equal(view.kind, "thread");
      assert.match(view.questions.files_base, /^\/v\/[^/]+\/$/);
      await checkMedia(card, `${view.questions.files_base}library/`);
      assert.equal(await card.getByTestId("thread-ask-pick-1A").getAttribute("aria-checked"), "true");
      await shot(page, `115-card-${width}`);
      const opened = page.waitForResponse(r => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "library_open");
      await page.evaluate(id => { location.hash = `report=demo/${id}`; }, thread.id);
      const response = await (await opened).json();
      assert.equal(response.ok?.kind, "library");
      const report = page.getByTestId("report-detail");
      await report.getByRole("heading", { name: "1. 시안", exact: true }).waitFor();
      await checkMedia(report, response.ok.base);
      await shot(page, `115-report-${width}`);
      assert.deepEqual(errors, []);
    } finally { await page.context().close(); }
  };
  await step("Chromium 데스크톱: 카드·보고의 빈 alt 그림·영상·기존 alt·빈 링크", async () => {
    await check(browser, 1280);
  });
  await step("WebKit 휴대폰: 카드·보고의 빈 alt 그림·영상·기존 alt·빈 링크", async () => {
    const wk = await webkit.launch();
    try { await check(wk, 390); } finally { await wk.close(); }
  });
  aaa("thread", "resolve", thread.id);
}
