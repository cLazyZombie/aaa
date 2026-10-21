// coordinator·thread 질문은 같은 카드다. 실제 host/agent·CLI로 질문 ID, 두 기기, 보기 권한, 답 보존과 평문 호환을 본다.
import fs from "node:fs";
import { root, aaa, until, step, webkit, pairPage, browser, inList, domIs, shot, coordinatorPost } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const call = (page, data) => page.evaluate(async (data) => {
  const res = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(data) });
  return res.json();
}, data);
const open = async (page, id, selector) => {
  await page.evaluate((id) => { location.hash = `question=${id}`; }, id);
  await domIs(page, "질문 ID의 카드", (selector) => document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "true" && !!document.querySelector(selector), selector);
};
const images = async (page, selector) => {
  const imgs = page.locator(`${selector} img`);
  for (const img of await imgs.all()) { await img.scrollIntoViewIfNeeded(); await img.evaluate((img) => img.decode()); }
  await page.getByTestId("asks-scroll").evaluate((el) => { el.scrollTop = 0; });
  return imgs.count();
};
const shape = (page, selector) => page.locator(`${selector} .thread-ask-q`).evaluateAll((qs) => qs.map((q) => ({ text: q.textContent.replace(/q-\d+/g, "q-ID").replace(/\s+/g, " "), classes: [...q.querySelectorAll("*")].map((el) => [el.tagName, el.className]) })));

export default async function ({ page }) {
  let t, q, req, post, phone, reader, wk;
  await step("준비: thread 질문·coordinator 구조화 post는 같은 문법, 질문마다 영속 q ID", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task-cards.md`, "로그인 화면 다듬기\n질문 카드 시험");
    t = aaa("thread", "start", "demo", "--title", "로그인 화면", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-cards.md`);
    q = await until("thread 질문 ID", () => aaa("thread", "show", "demo", t.id).questions);
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="600" height="160" viewBox="0 0 600 160"><rect width="600" height="160" rx="12" fill="#293743"/><rect x="26" y="25" width="220" height="12" rx="6" fill="#a4b6c5"/><rect x="26" y="58" width="548" height="36" rx="6" fill="#f1f5f8"/><rect x="454" y="109" width="120" height="32" rx="8" fill="#f2bb69"/><text x="494" y="130" font-size="16">로그인</text></svg>';
    fs.writeFileSync(`${t.work_dir}/library/card.svg`, svg);
    let body = fs.readFileSync(`${t.work_dir}/report.md`, "utf8").replaceAll("pic.png", "card.svg");
    fs.writeFileSync(`${t.work_dir}/report.md`, body);
    q = await until("새 보고의 그림과 ID", () => { const q = aaa("thread", "show", "demo", t.id).questions; return q?.items[0].body?.includes("card.svg") && q; });
    const preview = `${root}/preview`; fs.mkdirSync(preview);
    fs.writeFileSync(`${preview}/card.svg`, svg);
    aaa("library", "publish", preview, "--name", "preview");
    post = await coordinatorPost({ request: true, title: "로그인 화면 고르기", body: body.replaceAll("library/card.svg", "library/preview/card.svg") });
    const data = (await call(page, { op: "asks" })).ok;
    req = data.requests.find((r) => r.title === "로그인 화면 고르기");
    if (!req?.questions || req.questions.items.length !== 2) throw new Error(`구조화 post: ${JSON.stringify(req)}`);
    const ids = [...q.items, ...req.questions.items].map((q) => q.id);
    if (!ids.every((id) => /^q-\d{4,}$/.test(id)) || new Set(ids).size !== 4) throw new Error(`질문 ID: ${ids}`);
    const listed = aaa("question", "list").questions;
    for (const id of ids) if (!listed.some((q) => q.id === id)) throw new Error(`목록에 ${id}가 없다`);
    wk = await webkit.launch();
    phone = await pairDevice(wk, aaa("web", "pair").code, "iphone", { hash: `#question=${req.question_id}` });
    reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1100, height: 800 }, { hash: `#question=${req.question_id}` });
  });
  await step("같은 카드·본문·표·그림·선택지·추천: 데스크톱과 WebKit 휴대폰, 밝음·어두움 캡처", async () => {
    await open(page, "q-999999", '[data-testid="asks-missing"]');
    if (await page.locator(".thread-ask-card").count()) throw new Error("없는 질문 ID로 다른 질문 카드가 열렸다");
    for (const [label, p] of [["desktop", page], ["phone", phone.page]]) {
      for (const theme of ["light", "dark"]) {
        await p.evaluate((theme) => { localStorage.setItem("aaa:appearance", JSON.stringify({ theme, palette: "amber" })); }, theme);
        await p.reload();
        await domIs(p, "테마 적용", (theme) => document.documentElement.dataset.theme === theme, theme);
        const threadCard = `[data-testid="thread-ask-${t.id}"]`;
        const requestCard = `[data-testid="request-ask-${req.id}"]`;
        await open(p, q.items[0].id, threadCard);
        if (await images(p, threadCard) !== 3) throw new Error("thread 그림이 없다");
        const a = await shape(p, threadCard);
        await shot(p, `68-thread-${label}-${theme}`);
        await open(p, req.question_id, requestCard);
        if (await images(p, requestCard) !== 3) throw new Error("post 그림이 없다");
        const b = await shape(p, requestCard);
        if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`두 질문 렌더링이 다르다: ${JSON.stringify({a,b})}`);
        await shot(p, `68-coordinator-${label}-${theme}`);
      }
    }
    if (!(await reader.page.getByTestId("thread-ask-send").isDisabled())) throw new Error("보기 권한이 답할 수 있다");
    for (const data of [
      { op: "ask_answer", item: req.id, body_hash: req.questions.report_hash, picks: [0, 0], text: null },
      { op: "question_reply", id: req.question_id, choice: "A", text: null },
    ]) if ((await call(reader.page, data)).err?.code !== "forbidden") throw new Error("보기 권한 요청이 거부되지 않았다");
  });
  await step("카드 답은 coordinator inbox에 선택·원문과 한 번 남고 post는 답함, 다른 기기도 보존한 같은 카드를 본다", async () => {
    await page.getByTestId("thread-ask-pick-1B").click();
    await page.getByTestId("thread-ask-text").fill("2 눈 아이콘은 항상 보이게");
    await page.getByTestId("thread-ask-send").click();
    await domIs(page, "답한 post 카드", () => document.querySelector('[data-testid="ask-done-card"]')?.dataset.kind === "request");
    await domIs(phone.page, "다른 기기의 같은 답", () => !!document.querySelector('[data-testid="ask-done-text"]'));
    const data = (await call(page, { op: "asks" })).ok;
    const done = data.done.find((r) => r.subject === req.id);
    if (done?.summary !== "1B 직접" || done.answer.text !== "2 눈 아이콘은 항상 보이게" || done.items[0].id !== req.question_id) throw new Error(`답 사본: ${JSON.stringify(done)}`);
    const inbox = aaa("inbox", "list", "demo", "--peek").items.filter((i) => i.event === "question answered" && i.summary.includes(req.question_id));
    if (inbox.length !== 1 || !inbox[0].body.includes("가운데 큰 단추") || !inbox[0].body.includes(done.answer.text)) throw new Error(`inbox 답: ${JSON.stringify(inbox)}`);
    if (!aaa("post", "status", "demo", post.id).handled) throw new Error("post가 답함이 아니다");
    const again = await call(page, { op: "ask_answer", item: req.id, body_hash: req.questions.report_hash, picks: [0,0], text: null });
    if (!again.err) throw new Error("두 번째 답을 받았다");
    fs.rmSync(`${previewPath()}/card.svg`); // publish 사본도 바꿔 원본과 보존본을 구별한다
    await images(page, '[data-testid="ask-done-card"]');
    await shot(page, "68-coordinator-answered");
  });
  await step("채팅 ID 답: thread answer q-ID A와 question answer q-ID B, post handled q-ID는 평문·구조화 post를 닫는다", async () => {
    aaa("thread", "answer", q.items[0].id, "A");
    const partial = aaa("thread", "show", "demo", t.id).questions;
    if (partial.answer !== null || partial.draft?.picks[0] !== 0) throw new Error(`일부 답: ${JSON.stringify(partial)}`);
    aaa("question", "answer", q.items[1].id, "B");
    await until("thread에 답 전달", () => aaa("thread", "show", "demo", t.id).questions?.answer?.summary === "1A 2B");
    for (const [n, body] of [[1,"평문 요청"], [2,"## Questions\n### 1. 다음 진행\n- A 진행 (추천)\n- B 보류\n"]]) {
      const file = `${root}/handled-${n}.md`; fs.writeFileSync(file, body);
      const p = aaa("post", "demo", "user", "--request", "--needs-user", "--topic", "user-action", "--title", `채팅 답 ${n}`, "--body-file", file);
      const r = (await call(page, { op: "asks" })).ok.requests.find((r) => r.title === `채팅 답 ${n}`);
      aaa("post", "handled", r.question_id);
      if (!aaa("post", "status", "demo", p.id).handled) throw new Error("ID로 post를 닫지 못했다");
    }
    for (const x of [phone, reader]) { if (x.errors.length) throw new Error(x.errors.join("; ")); await x.page.context().close(); }
    await wk.close(); wk = null;
    aaa("thread", "resolve", "demo", t.id);
  });
}
// 실험실 프로젝트의 publish된 사본. 운영 경로에는 닿지 않는다.
const previewPath = () => `${root}/projects/demo/library/preview`;
