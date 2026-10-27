import { listIds } from "../lib/list-ids.mjs";
// coordinator·thread 질문은 같은 카드다. 실제 host/agent·CLI로 질문 ID, 두 기기, 보기 권한, 답 보존과 평문 호환을 본다.
import fs from "node:fs";
import { root, aaa, until, step, webkit, pairPage, browser, inList, domIs, shot, coordinatorPost, AA, drawerSettled } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const call = (page, data) => page.evaluate(async (data) => {
  const res = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(data) });
  return res.json();
}, data);
const open = async (page, id, selector) => {
  await page.evaluate((id) => { location.hash = `question=${id}`; }, id);
  await domIs(page, "질문 ID의 카드", (selector) => document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "true" && !!document.querySelector(selector) && document.querySelector('[data-testid="activity-asks"]')?.getAttribute("aria-selected") === "true", selector);
};
const images = async (page, selector) => {
  const imgs = page.locator(`${selector} img`);
  for (const img of await imgs.all()) { await img.scrollIntoViewIfNeeded(); await img.evaluate((img) => img.decode()); }
  await page.getByTestId("asks-scroll").evaluate((el) => { el.scrollTop = 0; });
  return imgs.count();
};
const shape = (page, selector) => page.locator(`${selector} .thread-ask-q`).evaluateAll((qs) => qs.map((q) => ({ text: q.textContent.replace(/q-\d+/g, "q-ID").replace(/\s+/g, " "), classes: [...q.querySelectorAll("*")].map((el) => [el.tagName, el.className]) })));

// 실제 Claude 기록의 맨 q-ID·링크를 연 채 host 사건으로 상태·CSS가 바뀌는지 본다.
function chipChat(ids) {
  const dir = `${root}/claude/projects/-lab-q-colors`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/colors.jsonl`;
  const lines = [
    ...ids.map((id) => `[${id} 질문](#question=${id}) aaa:question/${id}`),
    `문장 속 ${ids.map((id) => `${id}에 답한다.`).join(" ")}`,
    `붙은 괄호(${ids.join("·")})도 질문이다.`,
    `쉼표 ${ids.join(",")}도 질문이다.`,
  ].join("\n\n");
  fs.writeFileSync(transcript, [
    { type: "user", message: { role: "user", content: "질문 상태" } },
    { type: "assistant", message: { content: [{ type: "text", text: lines }] } },
  ].map((x) => JSON.stringify(x) + "\n").join(""));
  aaa("tab", "create", "--machine", "labA", "--label", "q-colors", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: "6a1f0c2e-3b4d-4e5f-8a9b-0c1d2e3f4a52", source: "startup", transcript_path: transcript }));
}
async function chipState(page, ids, tone) {
  await inList(page, "pane-q-colors");
  await page.getByTestId("chat-toggle").waitFor();
  if (!(await page.getByTestId("chat").count())) await page.getByTestId("chat-toggle").click();
  await domIs(page, `q-ID 칩 ${tone}`, ({ ids, tone }) => ids.every((id) => {
    const chips = [...document.querySelectorAll(`[data-testid="chat"] [data-testid="app-ref"][data-target="${id}"]`)];
    // 기록 삭제 뒤 맨 ID는 기존 존재 검사대로 평문, 명시 참조 두 개는 중립 칩이다.
    return chips.length === (tone === "plain" ? 2 : 5) && chips.every((el) => el.classList.contains(`is-${tone}`) && el.dataset.tone === tone
      && el.querySelector("svg") && el.innerHTML === chips[0].innerHTML
      && (tone === "plain" ? !el.querySelector(".app-ref-state") : el.textContent.includes(tone === "ask" ? "답할 것" : "답함")));
  }), { ids, tone });
  // 상태 class만 맞고 색이 회색인 회귀도 잡는다. 밝음·어두움의 실제 토큰과 비교한다.
  const css = await page.evaluate(async ({ ids, tone }) => {
    const theme = document.documentElement.dataset.theme;
    const errors = [];
    const probe = document.createElement("span"); document.body.append(probe);
    for (const mode of ["light", "dark"]) {
      document.documentElement.dataset.theme = mode;
      for (const id of ids) {
        const chips = document.querySelectorAll(`[data-testid="chat"] [data-testid="app-ref"][data-target="${id}"]`);
        const token = tone === "ask" ? "blocked" : "done";
        probe.style.backgroundColor = tone === "plain" ? "var(--bg-elevated)" : `var(--status-${token}-tint)`;
        probe.style.color = `var(--status-${token})`;
        for (const chip of chips) {
          // button의 실제 색 전환 완료 사건을 기다린다. 테마를 바꾼 직후의 중간색을 비교하지 않는다.
          getComputedStyle(chip).backgroundColor;
          await Promise.all(chip.getAnimations({ subtree: true }).map((a) => a.finished));
          const actual = { bg: getComputedStyle(chip).backgroundColor, icon: getComputedStyle(chip.querySelector("svg")).color };
          const expected = { bg: getComputedStyle(probe).backgroundColor, icon: getComputedStyle(probe).color };
          if (actual.bg !== expected.bg || (tone !== "plain" && actual.icon !== expected.icon)) errors.push(JSON.stringify({ mode, id, tone, actual, expected }));
        }
      }
    }
    probe.remove(); document.documentElement.dataset.theme = theme;
    return errors;
  }, { ids, tone });
  if (css.length) throw new Error(`질문 칩 CSS: ${css.join(", ")}`);
  await shot(page, `69-q-chips-${tone}-${ids[0]}`);
}

export default async function ({ page }) {
  let t, q, req, post, phone, reader, wk;
  try {
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
    chipChat(ids);
    await chipState(page, ids, "ask");
    // 문장·붙은 괄호·쉼표 안의 각 칩도 정확히 그 q-ID와 카드로 이동한다.
    for (const [id, selector] of [
      ...q.items.map((q) => [q.id, `[data-testid="thread-ask-${t.id}"]`]),
      ...req.questions.items.map((q) => [q.id, `[data-testid="request-ask-${req.id}"]`]),
    ]) {
      for (const at of [2, 3, 4]) {
        await inList(page, "pane-q-colors");
        await page.locator(`[data-testid="chat"] [data-testid="app-ref"][data-target="${id}"]`).nth(at).click();
        await domIs(page, "맨 q-ID의 정확한 카드", ({ id, selector }) => location.hash === `#question=${id}`
          && document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "true" && !!document.querySelector(selector)
          // 질문 본문 render 뒤 effect가 사이드바도 「답할 것」으로 옮긴다. 이 이동이 끝나기 전에
          // 다음 inList가 「작업」을 열면 늦은 effect가 다시 숨겨 목록 누름과 경합한다.
          && document.querySelector('[data-testid="activity-asks"]')?.getAttribute("aria-selected") === "true"
          && !!document.querySelector('#sidebar-column [data-panel="asks"]:not([hidden])'), { id, selector });
      }
    }
    const listed = aaa("question", "list").questions;
    for (const id of ids) if (!listed.some((q) => q.id === id)) throw new Error(`목록에 ${id}가 없다`);
    wk = await webkit.launch();
    phone = await pairDevice(wk, aaa("web", "pair").code, "iphone", { hash: `#question=${req.question_id}` });
    reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1100, height: 800 }, { hash: `#question=${req.question_id}` });
  });
  await step("왼쪽 질문 목록: 모든 q-ID와 답한 기록의 번호를 밝음·어두움·좁은 화면에서 보인다", async () => {
    await open(page, req.question_id, `[data-testid="request-ask-${req.id}"]`);
    const ids = [...q.items, ...req.questions.items].map((q) => q.id);
    await listIds(page, '[data-testid="asks-panel"]', ids, "ids-question-desktop");
    const p = phone.page;
    await p.locator(".view [data-testid='drawer-toggle']").click();
    await p.locator("#workspace-drawer.is-open").waitFor();
    await drawerSettled(p);
    await listIds(p, '[data-testid="asks-panel"]', ids, "ids-question-webkit");
    const viewport = p.viewportSize();
    await p.setViewportSize({ width: 320, height: 844 });
    await listIds(p, '[data-testid="asks-panel"]', ids, "ids-question-webkit-320");
    await p.setViewportSize(viewport);
    await p.locator(".view [data-testid='drawer-toggle']").click();
    await p.locator("#workspace-drawer.is-open").waitFor({ state: "detached" });
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
        await shot(p, `69-thread-${label}-${theme}`);
        await open(p, req.question_id, requestCard);
        if (await images(p, requestCard) !== 3) throw new Error("post 그림이 없다");
        const b = await shape(p, requestCard);
        if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`두 질문 렌더링이 다르다: ${JSON.stringify({a,b})}`);
        await shot(p, `69-coordinator-${label}-${theme}`);
      }
    }
    if (!(await reader.page.getByTestId("thread-ask-send").isDisabled())) throw new Error("보기 권한이 답할 수 있다");
    for (const data of [
      { op: "ask_answer", item: req.id, body_hash: req.questions.report_hash, picks: [0, 0], text: null },
      { op: "question_reply", id: req.question_id, choice: "A", text: null },
    ]) if ((await call(reader.page, data)).err?.code !== "forbidden") throw new Error("보기 권한 요청이 거부되지 않았다");
  });
  await step("카드 답은 coordinator inbox에 선택·원문과 한 번 남고 post는 답함, 다른 기기도 보존한 같은 카드를 본다", async () => {
    await chipState(page, req.questions.items.map((q) => q.id), "ask");
    await phone.page.getByTestId("thread-ask-pick-1B").click();
    const direct = "눈 아이콘은 항상 보이게\n원래 색은 유지";
    await phone.page.getByTestId("thread-ask-text-2").fill(direct);
    await phone.page.getByTestId("thread-ask-send").click();
    await domIs(phone.page, "답한 post 카드", () => document.querySelector('[data-testid="ask-done-card"]')?.dataset.kind === "request");
    const data = (await call(page, { op: "asks" })).ok;
    const done = data.done.find((r) => r.subject === req.id);
    if (done?.summary !== "1B 2직접" || done.answer.text !== null || JSON.stringify(done.answer.picks) !== "[1,null]" || JSON.stringify(done.answer.comments) !== JSON.stringify([null, direct]) || done.items[0].id !== req.question_id) throw new Error(`답 사본: ${JSON.stringify(done)}`);
    const inbox = aaa("inbox", "list", "demo", "--peek").items.filter((i) => i.event === "question answered" && i.summary.includes(req.question_id));
    if (inbox.length !== 1 || !inbox[0].body.includes("가운데 큰 단추") || !inbox[0].body.includes(`2번 직접 쓴 답: ${direct}`) || !inbox[0].summary.includes("2 직접: 눈 아이콘은 항상 보이게")) throw new Error(`inbox 답: ${JSON.stringify(inbox)}`);
    if (!aaa("post", "status", "demo", post.id).handled) throw new Error("post가 답함이 아니다");
    await chipState(page, req.questions.items.map((q) => q.id), "done");
    await open(page, req.question_id, '[data-testid="ask-done-card"]');
    await domIs(page, "다른 기기의 같은 답", (direct) => {
      const text = document.querySelector('[data-testid="ask-done-text-2"]');
      return text?.textContent === `직접 쓴 답 ${direct}`;
    }, direct);
    const again = await call(page, { op: "ask_answer", item: req.id, body_hash: req.questions.report_hash, picks: [0,0], text: null });
    if (!again.err) throw new Error("두 번째 답을 받았다");
    fs.rmSync(`${previewPath()}/card.svg`); // publish 사본도 바꿔 원본과 보존본을 구별한다
    await images(page, '[data-testid="ask-done-card"]');
    await listIds(page, '[data-testid="asks-panel"] .ask-row.is-done', req.questions.items.map((q) => q.id), "ids-question-answered");
    await shot(page, "69-coordinator-answered");
  });
  await step("채팅 ID 답: thread answer q-ID A와 question answer q-ID B, post handled q-ID는 평문·구조화 post를 닫는다", async () => {
    await chipState(page, q.items.map((q) => q.id), "ask");
    aaa("thread", "answer", q.items[0].id, "A");
    const partial = aaa("thread", "show", "demo", t.id).questions;
    if (partial.answer !== null || partial.draft?.picks[0] !== 0) throw new Error(`일부 답: ${JSON.stringify(partial)}`);
    aaa("question", "answer", q.items[1].id, "B");
    await until("thread에 답 전달", () => aaa("thread", "show", "demo", t.id).questions?.answer?.summary === "1A 2B");
    await chipState(page, q.items.map((q) => q.id), "done");
    for (const [n, body] of [[1,"평문 요청"], [2,"## Questions\n### 1. 다음 진행\n- A 진행 (추천)\n- B 보류\n"]]) {
      const file = `${root}/handled-${n}.md`; fs.writeFileSync(file, body);
      const p = aaa("post", "demo", "user", "--request", "--needs-user", "--topic", "user-action", "--title", `채팅 답 ${n}`, "--body-file", file);
      const r = (await call(page, { op: "asks" })).ok.requests.find((r) => r.title === `채팅 답 ${n}`);
      aaa("post", "handled", r.question_id);
      if (!aaa("post", "status", "demo", p.id).handled) throw new Error("ID로 post를 닫지 못했다");
    }
    // 새 보고로 현재 질문을 비워도 보존한 답은 초록, 답 기록을 지우면 중립으로 돌아온다.
    fs.writeFileSync(`${t.work_dir}/report.md`, "[완료] 질문 답 반영\n\n## Report\n선택 반영 완료\n");
    await until("현재 보고의 질문 비움", () => !aaa("thread", "show", "demo", t.id).questions);
    await chipState(page, [...q.items, ...req.questions.items].map((q) => q.id), "done");
    const forgotten = await call(page, { op: "ask_forget", ids: [], all: true });
    if (forgotten.err) throw new Error(`답 기록 지우기: ${JSON.stringify(forgotten)}`);
    await chipState(page, [...q.items, ...req.questions.items].map((q) => q.id), "plain");
    for (const x of [phone, reader]) { if (x.errors.length) throw new Error(x.errors.join("; ")); await x.page.context().close(); }
    await wk.close(); wk = null;
    aaa("thread", "resolve", "demo", t.id);
  });
  } finally { if (wk) { await wk.close(); wk = null; } }
}
// 실험실 프로젝트의 publish된 사본. 운영 경로에는 닿지 않는다.
const previewPath = () => `${root}/projects/demo/library/preview`;
