// 볼 자료와 답이 같은 카드에 있다. 기본 검수 질문·자료 미리보기·답한 판 보존까지 실제 경로로 본다.
import fs from "node:fs";
import { createHash } from "node:crypto";
import { root, aaa, until, step, domIs, shot, coordinatorPost, webkit } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";
const call = (page, req) => page.evaluate(async (req) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(req) })).json(), req);
const open = async (page, id, selector) => {
  await page.evaluate((id) => { location.hash = `question=${id}`; }, id);
  await domIs(page, "질문의 같은 카드", (selector) => !!document.querySelector(selector) && document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "true", selector);
};
const svg = (text) => `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="100"><rect width="400" height="100" fill="#c7d9f5"/><text x="20" y="55" font-size="24">${text}</text></svg>`;
export default async function ({ page }) {
  let report, request, thread, review, phone, wk;
  try {
  await step("coordinator 질문 안의 보고서·그림·음원을 같은 카드에서 열기", async () => {
    aaa("project", "create", "demo");
    const folder = `${root}/review-assets`; fs.mkdirSync(folder);
    fs.writeFileSync(`${folder}/index.html`, '<!doctype html><meta charset=utf-8><style>img{max-width:100%}</style><h1>카드 안 시안</h1><img src="preview.svg" alt="보고 시안">');
    fs.writeFileSync(`${folder}/preview.svg`, svg("시안 하나"));
    const wav = Buffer.alloc(46); wav.write("RIFF"); wav.writeUInt32LE(38, 4); wav.write("WAVEfmt ", 8); wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22); wav.writeUInt32LE(8000, 24); wav.writeUInt32LE(16000, 28); wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34); wav.write("data", 36); wav.writeUInt32LE(2, 40);
    fs.writeFileSync(`${folder}/theme.wav`, wav);
    report = aaa("library", "publish", folder, "--name", "review-assets").report_id;
    await coordinatorPost({ request: true, title: "시안과 음원 검수", body: `## Questions\n### 1. 이 안으로 진행할까\n[전체 보고서](aaa:report/${report})\n\n![시안](library/${report}/preview.svg)\n\n[음원](library/${report}/theme.wav)\n\n- A 좋음, 진행\n- B 고칠 점 있음\n` });
    request = (await call(page, { op: "asks" })).ok.requests.find((r) => r.title === "시안과 음원 검수");
    const selector = `[data-testid="request-ask-${request.id}"]`;
    await open(page, request.question_id, selector);
    await page.locator(`${selector} img[alt="시안"]`).evaluate((img) => img.decode());
    await page.getByTestId("thread-ask-pick-1A").click();
    await page.locator(selector).getByRole("button", { name: "보고서 열기 전체 보고서", exact: true }).click();
    await page.frameLocator('[data-testid="question-report-frame"]').getByRole("heading", { name: "카드 안 시안" }).waitFor();
    if ((await page.getByTestId("thread-ask-pick-1A").getAttribute("aria-checked")) !== "true") throw new Error("보고서를 열며 고른 답을 잊었다");
    if ((await page.evaluate(() => location.hash)) !== `#question=${request.question_id}`) throw new Error("자료를 열며 답 카드에서 나갔다");
    await page.locator(selector).getByRole("button", { name: "보고서 열기 음원", exact: true }).click();
    const audio = page.getByTestId("question-audio"); await audio.waitFor();
    const state = await audio.evaluate(async (a) => { const res = await fetch(a.src); return { paused: a.paused, preload: a.preload, type: res.headers.get("content-type"), bytes: (await res.arrayBuffer()).byteLength }; });
    if (!state.paused || state.preload !== "none" || state.type !== "audio/wav" || state.bytes !== wav.length) throw new Error(JSON.stringify(state));
    await shot(page, "74-review-audio");
    wk = await webkit.launch(); phone = await pairDevice(wk, aaa("web", "pair").code, "iphone", { hash: `#question=${request.question_id}` });
    for (const [name, p] of [["desktop", page], ["phone", phone.page]]) for (const theme of ["light", "dark"]) {
      await p.evaluate((theme) => { localStorage.setItem("aaa:appearance", JSON.stringify({ theme, palette: "amber" })); }, theme); await p.reload();
      await open(p, request.question_id, selector);
      await p.locator(selector).getByRole("button", { name: "보고서 열기 전체 보고서", exact: true }).click();
      await p.frameLocator('[data-testid="question-report-frame"]').getByRole("heading", { name: "카드 안 시안" }).waitFor();
      const frameImage = p.frameLocator('[data-testid="question-report-frame"]').getByRole("img", { name: "보고 시안" });
      await frameImage.waitFor();
      if (name === "desktop") await frameImage.evaluate((i) => i.decode());
      else {
        // E2E 59와 같은 headless WebKit 이중 sandbox 제약. 보안 정책을 바꾸지 않고 실제 그림은 앱 크게 보기에서 검증한다.
        await frameImage.tap();
        await p.getByTestId("lightbox-img").evaluate((i) => i.decode());
        await shot(p, `74-review-picture-phone-${theme}`);
        await p.keyboard.press("Escape");
        await p.getByTestId("lightbox").waitFor({ state: "detached" });
      }
      await domIs(p, "카드 가로 넘침 없음", () => document.documentElement.scrollWidth <= innerWidth);
      await shot(p, `74-review-${name}-${theme}`);
    }
    const guide = fs.readFileSync(`${root}/projects/demo/AGENTS.md`, "utf8");
    if (!guide.includes("볼 보고서·시안·음원 링크는 질문 안에 둔다")) throw new Error("coordinator Questions 안내 누락");
  });
  await step("Questions 없는 검수 대기는 기본 A/B 카드, 고칠 점은 글로 받고 해당 r 보고를 카드에서 본다", async () => {
    fs.writeFileSync(`${root}/task-review.md`, "검수 카드 시험\n");
    thread = aaa("thread", "start", "demo", "--title", "검수할 시안", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-review.md`);
    await until("첫 보고", () => aaa("thread", "show", thread.id).report_id);
    fs.unlinkSync(`${thread.work_dir}/library/index.html`);
    fs.writeFileSync(`${thread.work_dir}/library/review.svg`, svg("검수 당시 시안"));
    const body = "[검수 대기] 버튼 시안\n\n## Report\n버튼 시안을 검수한다.\n\n![검수 시안](library/review.svg)\n";
    fs.writeFileSync(`${thread.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    review = await until("자동 검수 질문", () => { const t = aaa("thread", "show", thread.id); return t.report_hash === hash && t.questions?.review && t; });
    const q = review.questions.items[0].id;
    if (review.questions.items.length !== 1 || fs.readFileSync(`${thread.work_dir}/report.md`, "utf8").includes("## Questions")) throw new Error("host가 원문을 바꿨거나 여러 질문을 만들었다");
    let rejected = ""; try { aaa("question", "answer", q, "B"); } catch (e) { rejected = String(e); }
    if (!rejected.includes("고칠 점을 글로")) throw new Error("글 없는 B를 받았다");
    const selector = `[data-testid="thread-ask-${thread.id}"]`;
    await open(page, q, selector);
    await page.locator(selector).getByRole("button", { name: `보고서 열기 ${review.report_id}`, exact: true }).click();
    await page.getByTestId("question-report-preview").getByText("버튼 시안을 검수한다.", { exact: true }).waitFor();
    await page.getByTestId("question-report-preview").locator('img[alt="검수 시안"]').evaluate((img) => img.decode());
    await page.getByTestId("thread-ask-pick-1B").click();
    if (!(await page.getByTestId("thread-ask-send").isDisabled())) throw new Error("고칠 점 없는 B를 보낼 수 있다");
    await shot(page, "74-review-thread-open");
    await page.getByTestId("thread-ask-text").fill("버튼 글씨를 더 크게");
    await page.getByTestId("thread-ask-send").click();
    await page.getByTestId("ask-done-card").waitFor();
    const done = (await call(page, { op: "asks" })).ok.done.find((r) => r.subject === thread.id);
    if (done.report_id !== review.report_id || done.answer.text !== "버튼 글씨를 더 크게" || done.answer.picks[0] !== 1) throw new Error(JSON.stringify(done));
    fs.writeFileSync(`${thread.work_dir}/library/review.svg`, svg("새 시안"));
    fs.writeFileSync(`${thread.work_dir}/report.md`, "[완료] 다음 판\n\n## Report\n새 결과\n");
    await until("다음 판", () => aaa("thread", "show", thread.id).report_id !== review.report_id);
    await open(page, q, '[data-testid="ask-done-card"]');
    const doneCard = page.getByTestId("ask-done-card");
    await doneCard.getByRole("button", { name: `보고서 열기 ${review.report_id}`, exact: true }).click();
    const img = doneCard.getByTestId("question-report-preview").locator('img[alt="검수 시안"]'); await img.evaluate((i) => i.decode());
    const original = await img.getAttribute("src");
    if (!(await page.evaluate(async (src) => (await fetch(src)).text(), original)).includes("검수 당시 시안")) throw new Error("답한 판이 새 시안으로 바뀌었다");
    await shot(page, "74-review-answered");
    if (phone.errors.length) throw new Error(phone.errors.join("; "));
    aaa("thread", "resolve", thread.id);
  });
  } finally {
    if (phone) await phone.page.context().close();
    if (wk) await wk.close();
  }
}
