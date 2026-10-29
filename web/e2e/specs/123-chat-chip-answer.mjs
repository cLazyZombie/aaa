// 대화 칩에서 바로 답하기(t-1287, 사용자 결정 1B): coordinator 대화의 질문 링크 칩은 질문 글과 선택지 단추를 붙이고,
// 누르면 카드 답과 같은 host 경로로 답한다. 다른 기기의 칩·열린 카드·post 처리·inbox가 같은 답을 보고, 보기 권한과
// 문장 속 맨 ID에는 단추가 없다.
import fs from "node:fs";
import { root, aaa, until, step, pairPage, browser, inList, domIs, coordinatorPost, AA } from "../lib/harness.mjs";

const call = (page, data) => page.evaluate(async (data) => {
  const res = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(data) });
  return res.json();
}, data);

const BODY = `## Questions
### 1. 로그인 단추 자리
- A 오른쪽 아래 (추천) — 지금 화면과 같음
- B 가운데 큰 단추 — 휴대폰에서 누르기 쉬움
### 2. 비밀번호 눈 아이콘
- A 늘 보임 (추천) — 한 번에 확인
- B 누를 때만 — 화면이 덜 복잡
`;

// 실제 Claude 기록 형식의 coordinator 답: 질문마다 링크 칩, 문장 속 맨 ID 하나
function chat(ids) {
  const dir = `${root}/claude/projects/-lab-chip-answer`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/chip.jsonl`;
  const text = [`시안을 골라 줘: ${ids.map((id) => `[${id} 질문](#question=${id})`).join(" ")}`, `문장 속 ${ids[0]}은 단추가 없다.`].join("\n\n");
  fs.writeFileSync(transcript, [
    { type: "user", message: { role: "user", content: "시안 어때?" } },
    { type: "assistant", message: { content: [{ type: "text", text }] } },
  ].map((x) => JSON.stringify(x) + "\n").join(""));
  aaa("tab", "create", "--machine", "labA", "--label", "chip-answer", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: "6a1f0c2e-3b4d-4e5f-8a9b-0c1d2e3f4a91", source: "startup", transcript_path: transcript }));
}

async function showChat(p) {
  await inList(p, "pane-chip-answer");
  await p.getByTestId("chat-toggle").waitFor();
  if (!(await p.getByTestId("chat").count())) await p.getByTestId("chat-toggle").click();
  await domIs(p, "대화 칩", () => document.querySelectorAll('[data-testid="chat"] [data-testid="app-ref"]').length === 3);
}

/** 그 질문 칩들의 상태: 모든 칩의 tone·상태 글, 링크 칩의 질문 글·단추 글자 */
const chips = (p, id) => p.evaluate((id) => [...document.querySelectorAll(`[data-testid="chat"] [data-testid="app-ref"][data-target="${id}"]`)].map((chip) => {
  const ask = chip.closest('[data-testid="app-ref-ask"]');
  return { tone: chip.dataset.tone, state: chip.querySelector(".app-ref-state")?.textContent ?? null,
    text: ask?.querySelector(".app-ref-text")?.textContent ?? null,
    picks: ask ? [...ask.querySelectorAll('[data-testid="app-ref-pick"]')].map((b) => b.dataset.letter) : [] };
}), id);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

export default async function ({ page }) {
  let ids, req, post, phone, reader;
  await step("준비: 두 질문 post와 링크 칩·맨 ID가 든 coordinator 대화, 휴대폰·보기 권한 기기", async () => {
    aaa("project", "create", "demo");
    post = await coordinatorPost({ request: true, title: "로그인 화면 고르기", body: BODY });
    req = (await call(page, { op: "asks" })).ok.requests.find((r) => r.title === "로그인 화면 고르기");
    ids = req.questions.items.map((q) => q.id);
    chat(ids);
    await showChat(page);
    phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1100, height: 800 });
    await showChat(reader.page);
  });
  await step("열린 질문 링크 칩은 질문 글·선택지 단추, 맨 ID와 보기 권한은 단추가 없다", async () => {
    await domIs(page, "링크 칩 단추", (ids) => ids.every((id) => document.querySelectorAll(`[data-testid="chat"] [data-testid="app-ref-ask"][data-target="${id}"]`).length === 1), ids);
    const first = await chips(page, ids[0]);
    if (!same(first, [
      { tone: "ask", state: "답할 것", text: "로그인 단추 자리", picks: ["A", "B"] },
      { tone: "ask", state: "답할 것", text: null, picks: [] },
    ])) throw new Error(`첫 질문 칩: ${JSON.stringify(first)}`);
    if (await reader.page.locator('[data-testid="chat"] [data-testid="app-ref-pick"]').count()) throw new Error("보기 권한 대화에 단추가 있다");
  });
  await step("칩 누름은 그 질문만 답하고, 다른 기기의 열린 카드·칩이 같은 일부 답을 본다", async () => {
    await phone.page.evaluate((id) => { location.hash = `question=${id}`; }, ids[1]);
    const card = phone.page.locator(`[data-testid="request-ask-${req.id}"]`);
    await card.waitFor();
    await page.locator(`[data-testid="chat"] [data-testid="app-ref-pick"][data-question="${ids[0]}"][data-letter="B"]`).click();
    await domIs(page, "답한 칩", (id) => [...document.querySelectorAll(`[data-testid="chat"] [data-testid="app-ref"][data-target="${id}"]`)]
      .every((c) => c.dataset.tone === "done" && c.querySelector(".app-ref-state")?.textContent === "답함 · 1B"), ids[0]);
    // 칩에서 답하기 전에 연 카드도 다시 열지 않고 그 답을 보인다.
    await domIs(phone.page, "열린 카드의 일부 답", (id) => document.querySelector(`[data-testid="request-ask-${id}"] [data-testid="thread-ask-pick-1B"]`)?.getAttribute("aria-checked") === "true", req.id);
    if (!(await chips(page, ids[1])).every((c) => c.tone === "ask")) throw new Error("답하지 않은 둘째 질문이 답함이다");
    if (aaa("post", "status", "demo", post.id).handled) throw new Error("일부 답에서 post가 닫혔다");
  });
  await step("마지막 질문 칩 누름은 카드 답과 같이 post를 닫고 inbox에 한 번, 모든 칩·카드가 답함이다", async () => {
    await page.locator(`[data-testid="chat"] [data-testid="app-ref-pick"][data-question="${ids[1]}"][data-letter="A"]`).click();
    await until("post 답함", () => aaa("post", "status", "demo", post.id).handled);
    await domIs(phone.page, "휴대폰 카드 답함", () => document.querySelector('[data-testid="ask-done-card"]')?.dataset.kind === "request");
    const done = (await call(page, { op: "asks" })).ok.done.find((r) => r.subject === req.id);
    if (done?.summary !== "1B 2A" || JSON.stringify(done.answer.picks) !== "[1,0]") throw new Error(`답 사본: ${JSON.stringify(done)}`);
    const inbox = aaa("inbox", "list", "demo", "--peek").items.filter((i) => i.event === "question answered" && i.summary.includes(req.question_id) && !i.summary.includes("일부"));
    if (inbox.length !== 1) throw new Error(`inbox 답: ${JSON.stringify(inbox)}`);
    for (const p of [page, reader.page]) await domIs(p, "모든 칩 답함", (ids) => ids.every((id) => {
      const all = [...document.querySelectorAll(`[data-testid="chat"] [data-testid="app-ref"][data-target="${id}"]`)];
      return all.length > 0 && all.every((c) => c.dataset.tone === "done") && !document.querySelector(`[data-testid="chat"] [data-testid="app-ref-ask"][data-target="${id}"]`);
    }), ids);
    for (const x of [phone, reader]) { if (x.errors.length) throw new Error(x.errors.join("; ")); await x.page.context().close(); }
  });
}
