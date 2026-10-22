// 실제 host·agent와 H.264 bytes로 질문·첨부 HTML·직접 mp4·답한 사본을 검사한다.
import fs from "node:fs";
import { createHash } from "node:crypto";
import { root, aaa, until, step, domIs, shot, coordinatorPost, webkit, base } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";
import { clickForPopup, observePopupLifecycle } from "../lib/popup.mjs";
const fixture = new URL("../fixtures/report-video.mp4", import.meta.url);
const call = (p, req) => p.evaluate(async (req) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(req) })).json(), req);
const ready = (video) => video.evaluate((v) => new Promise((resolve, reject) => {
  if (v.readyState >= 1) return resolve();
  const timer = setTimeout(() => reject(new Error(`영상 decode 시한: ${JSON.stringify({src:v.currentSrc,ready:v.readyState,network:v.networkState,error:v.error?.message})}`)), 15000);
  v.addEventListener("loadedmetadata", () => { clearTimeout(timer); resolve(); }, { once: true });
  v.addEventListener("error", () => { clearTimeout(timer); reject(new Error(`영상 오류 ${v.error?.code}: ${v.error?.message}`)); }, { once: true });
}));
const moved = (video) => video.evaluate((v) => new Promise((resolve, reject) => {
  if (v.currentTime > 0 && !v.paused && v.readyState >= 2) return resolve();
  const timer = setTimeout(() => reject(new Error(`재생 시간이 전진하지 않았다: ${JSON.stringify({time:v.currentTime,ready:v.readyState,paused:v.paused,ended:v.ended,error:v.error?.message})}`)), 15000);
  const on = () => { if (v.currentTime > 0 && v.readyState >= 2) { clearTimeout(timer); v.removeEventListener("timeupdate", on); resolve(); } };
  v.addEventListener("timeupdate", on);
}));
const seek = (video) => video.evaluate((v) => new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error(`탐색이 끝나지 않았다: ${JSON.stringify({time:v.currentTime,ready:v.readyState,seeking:v.seeking,error:v.error?.message})}`)), 15000);
  v.pause(); v.addEventListener("seeked", () => { clearTimeout(timer); resolve(); }, { once: true }); v.currentTime = 1.2;
}));
function observeVideo() {
  const events = [];
  window.__aaaVideoEvents = events;
  for (const type of ["click", "hashchange", "play", "pause", "loadedmetadata", "seeked", "error"]) addEventListener(type, (e) => {
    const el = e.target instanceof Element ? e.target.closest("button,a,video") : null;
    events.push({ type, tag: el?.tagName, label: el?.getAttribute("aria-label"), testid: el?.getAttribute("data-testid"), hash: location.hash });
    if (events.length > 60) events.shift();
  }, true);
}
export default async function ({ page }) {
  let request, report, wk, thread, current;
  const contexts = [];
  const timeline = [], observers = [];
  const record = (event, detail = {}) => timeline.push({ at: new Date().toISOString(), ms: Math.round(performance.now()), event, ...detail });
  let failed = false;
  try {
  await step("영상 자료·agent 안내와 기존 인증 오류 주소·UTF-8 원문", async () => {
    aaa("project", "create", "demo");
    const folder = `${root}/video`; fs.mkdirSync(folder); fs.copyFileSync(fixture, `${folder}/a.mp4`);
    fs.writeFileSync(`${folder}/index.html`, '<!doctype html><meta charset=utf-8><meta name=viewport content="width=device-width"><h1 id="A">영상 비교</h1><button onclick="document.querySelector(\'video\').play()">영상 재생</button><video controls muted playsinline preload="metadata" src="a.mp4" style="max-width:100%"></video>');
    report = aaa("library", "publish", folder, "--name", "video").report_id;
    await coordinatorPost({ request: true, title: "영상 비교 질문", body: `## Questions\n### 1. 동작\n![시안 영상](library/${report}/a.mp4)\n\n[기존 영상 링크](library/${report}/a.mp4)\n\n[비교 HTML](library/${report}/index.html#A)\n\n- A 적용한다\n  ![A 영상](library/${report}/a.mp4)\n- B 고친다\n` });
    request = (await call(page, { op: "asks" })).ok.requests.find((r) => r.title === "영상 비교 질문");
    if (!request.questions.files_base.startsWith("/v/")) throw new Error("질문 자료가 cookie 인증 경로에 남았다");
    const anonymous = await page.context().browser().newContext(); contexts.push(anonymous);
    const response = await anonymous.request.get(`${base}api/v1/projects/demo/requests/${request.id}/${request.questions.report_hash}/library/${report}/a.mp4`);
    if (response.status() !== 401 || response.headers()["content-type"] !== "application/json; charset=utf-8" || (await response.json()).err.message !== "pairing이 필요하다") throw new Error("옛 주소의 인증/한글 오류 계약");
    if (!fs.readFileSync(`${root}/projects/demo/AGENTS.md`, "utf8").includes("![시안 영상](library/preview/a.mp4)")) throw new Error("coordinator 영상 안내 누락");
    fs.writeFileSync(`${root}/video-task.md`, "영상 보고 시험");
    thread = aaa("thread", "start", "demo", "--title", "영상 보고", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/video-task.md`);
    await until("thread 첫 보고", () => aaa("thread", "show", thread.id).report_id);
    fs.unlinkSync(`${thread.work_dir}/library/index.html`); fs.copyFileSync(fixture, `${thread.work_dir}/library/a.mp4`);
    const body = "[결정 필요] 영상 질문\n\n## Report\n![보고 영상](library/a.mp4)\n\n## Questions\n### 1. 영상\n![thread 영상](library/a.mp4)\n\n- A 적용\n- B 수정\n";
    fs.writeFileSync(`${thread.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    current = await until("영상 질문 회수", () => { const t = aaa("thread", "show", thread.id); return t.report_hash === hash && t.questions?.items?.[0]?.id && t; });
    if (!fs.readFileSync(`${thread.work_dir}/brief.md`, "utf8").includes("![시안 영상](library/preview/a.mp4)")) throw new Error("thread 영상 안내 누락");
  });
  await step("Chromium·WebKit 데스크톱·휴대폰 카드 재생·탐색·HTML sandbox·새 탭·직접 mp4·보고 본문", async () => {
    wk = await webkit.launch();
    for (const [engine, browser] of [["chromium", page.context().browser()], ["webkit", wk]]) for (const profile of ["desktop", "iphone"]) {
      const paired = await pairDevice(browser, aaa("web", "pair").code, profile, { hash: `#question=${request.question_id}`, init: observeVideo });
      const p = paired.page; contexts.push(p.context());
      console.log(`영상 조합 ${engine}/${profile}`);
      let phase = "카드 선택";
      const note = (event, detail) => record(event, { engine, profile, phase, ...detail });
      observers.push(observePopupLifecycle(p, note));
      try {
        const card = p.getByTestId(`request-ask-${request.id}`); await card.waitFor();
        await p.getByTestId("thread-ask-pick-1B").click();
        const video = card.getByLabel("시안 영상", { exact: true }); await ready(video);
        const flags = await video.evaluate((v) => ({ muted: v.muted, inline: v.playsInline, controls: v.controls, paused: v.paused, autoplay: v.autoplay }));
        if (!flags.muted || !flags.inline || !flags.controls || !flags.paused || flags.autoplay) throw new Error(JSON.stringify(flags));
        const hash = await p.evaluate(() => location.hash);
        phase = "본문 재생·시간 전진·탐색";
        await card.getByRole("button", { name: "시안 영상 재생", exact: true }).click(); await moved(video); await seek(video);
        phase = "선택지 재생·시간 전진";
        const choice = card.getByLabel("A 영상", { exact: true }); await ready(choice);
        await card.getByRole("button", { name: "A 영상 재생", exact: true }).click(); await moved(choice); await choice.evaluate((v) => v.pause());
        phase = "선택·주소 보존";
        const picked = await p.getByTestId("thread-ask-pick-1B").getAttribute("aria-checked");
        if (picked !== "true") throw new Error(`재생으로 B 선택이 바뀌었다: ${picked}`);
        const afterHash = await p.evaluate(() => location.hash);
        if (afterHash !== hash) throw new Error(`재생으로 질문 주소가 바뀌었다: ${hash} → ${afterHash}`);
        await ready(card.getByLabel("기존 영상 링크", { exact: true }));
        const link = card.getByRole("link", { name: "비교 HTML", exact: true });
        const href = await link.getAttribute("href"); if (!href.startsWith("/v/") || !href.endsWith("#A")) throw new Error(`카드 링크 경로/조각 ${href}`);
        phase = "iframe 열기·metadata";
        await card.getByRole("button", { name: "보고서 열기 비교 HTML", exact: true }).click();
        const frame = p.frameLocator('[data-testid="question-report-frame"]');
        await frame.getByRole("heading", { name: "영상 비교" }).waitFor();
        const embedded = frame.locator("video"); console.log("iframe 영상 확인"); await ready(embedded);
        if ((await p.getByTestId("question-report-frame").getAttribute("sandbox")).includes("allow-same-origin")) throw new Error("sandbox 완화");
        phase = "iframe 재생·시간 전진·탐색";
        await frame.getByRole("button", { name: "영상 재생" }).click(); await moved(embedded); await seek(embedded);
        phase = "sandbox 경계";
        const boundary = await embedded.evaluate(async () => {
          let cookie = false; try { document.cookie; cookie = true; } catch {}
          let api = false; try { api = (await fetch('/api/v1/me')).ok; } catch {}
          let sibling = false; try { sibling = (await fetch('/v/other/a.mp4')).ok; } catch {}
          const own = await fetch('a.mp4', { credentials: 'omit', headers: { Range: 'bytes=0-1' } });
          return { cookie, api, sibling, own: own.status, bytes: (await own.arrayBuffer()).byteLength };
        });
        if (boundary.cookie || boundary.api || boundary.sibling || boundary.own !== 206 || boundary.bytes !== 2) throw new Error(JSON.stringify(boundary));
        phase = "카드 캡처";
        await shot(p, `88-video-${engine}-${profile}`);
        phase = "HTML 링크 클릭·popup";
        note("popup.begin");
        const tab = await clickForPopup(p, link, note);
        phase = "새 탭 재생";
        console.log("새 탭 영상 확인"); await ready(tab.locator("video")); await tab.getByRole("button", { name: "영상 재생" }).click(); await moved(tab.locator("video"));
        note("cleanup.tab.begin"); await tab.close();
        phase = "직접 mp4·Range·재생";
        const anonymous = await browser.newContext({ viewport: profile === "iphone" ? { width: 390, height: 844 } : { width: 1280, height: 800 } }); contexts.push(anonymous);
        const src = await video.getAttribute("src");
        const response = await anonymous.request.get(new URL(src, base).href, { headers: { Range: "bytes=0-1" } });
        if (response.status() !== 206 || response.headers()["content-type"] !== "video/mp4" || response.headers()["accept-ranges"] !== "bytes" || (await response.body()).length !== 2) throw new Error("cookie 없는 Range");
        const direct = await anonymous.newPage(); await direct.goto(new URL(src, base).href); console.log("직접 영상 확인"); await ready(direct.locator("video")); await direct.locator("video").evaluate((v) => { v.muted = true; return v.play(); }); await moved(direct.locator("video"));
        phase = "thread 질문·보고 본문";
        await p.evaluate((id) => { location.hash = `question=${id}`; }, current.questions.items[0].id);
        await p.getByTestId(`thread-ask-${thread.id}`).waitFor();
        await ready(p.getByTestId(`thread-ask-${thread.id}`).getByLabel("thread 영상", { exact: true }));
        await p.evaluate((id) => { location.hash = `report=demo%2F${id}`; }, current.report_id);
        const reportVideo = p.getByTestId("report-detail").getByLabel("보고 영상", { exact: true }); await ready(reportVideo);
        await p.getByTestId("report-detail").getByRole("button", { name: "보고 영상 재생", exact: true }).click(); await moved(reportVideo); await seek(reportVideo);
        await domIs(p, "영상 가로 넘침 없음", () => document.documentElement.scrollWidth <= innerWidth);
        await shot(p, `88-report-${engine}-${profile}`);
        note("cleanup.profile.begin");
        await p.context().close(); await anonymous.close();
      } catch (error) {
        failed = true;
        note("profile.failure", { error: error.message });
        const state = await Promise.all(p.frames().map((f) => f.evaluate(() => ({
          hash: location.hash, events: window.__aaaVideoEvents,
          picks: [...document.querySelectorAll('[role="radio"]')].map((b) => [b.getAttribute("data-testid"), b.getAttribute("aria-checked")]),
          videos: [...document.querySelectorAll("video")].map((v) => ({ label: v.getAttribute("aria-label"), time: v.currentTime, ready: v.readyState, paused: v.paused, error: v.error?.message })),
        })).catch((e) => ({ error: e.message }))));
        throw new Error(`${engine}/${profile} ${phase}: ${error.message} ‖ 영상 상태: ${JSON.stringify(state)}`, { cause: error });
      }
    }
  });
  await step("답한 영상은 원래 bytes를 보존하고 새 보고 뒤에도 재생한다", async () => {
    await page.evaluate((id) => { location.hash = `question=${id}`; }, current.questions.items[0].id);
    await page.getByTestId(`thread-ask-${thread.id}`).waitFor();
    await page.getByTestId("thread-ask-pick-1A").click(); await page.getByTestId("thread-ask-send").click();
    await page.getByTestId("ask-done-card").waitFor();
    fs.writeFileSync(`${thread.work_dir}/library/a.mp4`, "바뀐 영상"); fs.writeFileSync(`${thread.work_dir}/report.md`, "[완료] 다음 판\n\n## Report\n새 자료\n");
    await until("다음 판", () => aaa("thread", "show", thread.id).report_id !== current.report_id);
    await page.reload(); await page.getByTestId("ask-done-card").waitFor();
    const video = page.getByTestId("ask-done-card").getByLabel("thread 영상", { exact: true }); await ready(video);
    const bytes = await page.evaluate(async (url) => Array.from(new Uint8Array(await (await fetch(url)).arrayBuffer())), await video.getAttribute("src"));
    if (!Buffer.from(bytes).equals(fs.readFileSync(fixture))) throw new Error("답한 영상이 최신 파일로 바뀌었다");
    aaa("thread", "resolve", thread.id);
  });
  } catch (error) {
    failed = true;
    record("spec.failure", { error: error.message });
    throw error;
  } finally {
    record("cleanup.spec.begin");
    for (const c of contexts) await c.close();
    if (wk) await wk.close();
    for (const stop of observers) stop();
    if (failed) {
      fs.mkdirSync(`${root}/diagnostics`, { recursive: true });
      fs.writeFileSync(`${root}/diagnostics/video-popup.json`, JSON.stringify(timeline, null, 2));
      console.log(`영상 popup 사건: ${JSON.stringify(timeline)}`);
    }
  }
}
