// 웹 E2E thread 질문 카드와 앱 안 참조 칩(plan §8.324, 사용자 결정 10-20 20:3x·1A·2A·3A·4A). thread가 보고 `## Questions`에 남긴
// 질문은 보고서 화면 위 「답할 것」의 카드가 되고, 활동 막대 보고서 단추에 빨강 수가 붙는다. coordinator 대화의
// `aaa:question/<thread>`·`aaa:report/<이름>`은 칩이고 누르면 앱 안에서 그 카드(그것만 펼침)·그 보고서를 연다. thread 대화 입력칸
// 위에는 그 질문의 칩 한 줄이 있다. 카드로 답하면 host가 한 번만 받아 thread에 「사용자 답(1B 2A)」를 보내고, 다른 기기의 카드·칩·
// 배지도 「답함」이 된다. 보기 권한 기기는 답할 수 없다. 알림 주소 `/#question=<thread>`(post push url)는 그 카드를 펼쳐 연다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 51-thread-questions(새 실험실).
import fs from "node:fs";
import { root, aaa, until, step, browser, pairPage, inList, waitOutput, shot, AA } from "../lib/harness.mjs";

const LABEL = "cq1";

/** coordinator처럼 질문·보고서 참조를 쓴 Claude 대화(hook으로 기록 자리를 알린다, 38-chat-report-close와 같은 길) */
function chatPane(lines) {
  const dir = `${root}/claude/projects/-lab-${LABEL}`;
  fs.mkdirSync(dir, { recursive: true });
  const tr = `${dir}/s1.jsonl`;
  const L = (o) => JSON.stringify(o) + "\n";
  let body = L({ type: "user", message: { role: "user", content: "진행 상황" } });
  for (const text of lines) body += L({ type: "assistant", message: { content: [{ type: "text", text }] } });
  fs.writeFileSync(tr, body);
  aaa("tab", "create", "--machine", "labA", "--label", LABEL, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: "6a1f0c2e-3b4d-4e5f-8a9b-0c1d2e3f4a51", source: "startup", transcript_path: tr }));
}

/** 칩의 종류·대상·색·글 */
const chips = (p) =>
  p.evaluate(() => [...document.querySelectorAll('[data-testid="chat"] [data-testid="app-ref"]')].map((b) => `${b.dataset.kind}/${b.dataset.target} ${b.dataset.tone} ${b.textContent}`));
const badge = (p) =>
  p.evaluate(() => {
    const b = document.querySelector('[data-testid="activity-badge-reports"]');
    return b ? `${b.dataset.kind} ${b.textContent}` : null;
  });

export default async function ({ page }) {
  await step("질문 카드와 앱 안 칩(plan §8.324): 보고 ## Questions는 「답할 것」 카드·빨강 배지, 대화의 aaa:question·aaa:report는 앱 안 칩, 입력칸 위 칩 한 줄, 카드 답은 thread에 한 번 가고 모든 기기가 답함, 보기 권한은 못 답함", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task-cards.md`, "로그인 화면 다듬기\n질문 카드 시험");
    const t = aaa("thread", "start", "demo", "--title", "로그인 화면 다듬기", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-cards.md`);
    const v = await until("질문 보고", () => {
      const v = aaa("thread", "show", "demo", t.id);
      return v.questions?.items.length === 2 && v;
    });
    const q1 = v.questions.items[0];
    if (q1.text !== "로그인 단추 자리" || q1.choices.length !== 2 || !q1.choices[0].recommended || q1.choices[0].description !== "지금 화면과 같음" || v.questions.answer !== null)
      throw new Error(`읽은 질문: ${JSON.stringify(v.questions)}`);
    const input = `${root}/login-report`;
    fs.mkdirSync(input);
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>로그인 시안</h1></html>");
    aaa("library", "publish", input, "--name", "login-v2", "--to-user", "--title", "로그인 시안");
    chatPane([`${t.id} 질문 2개 · 추천: 1A 2A aaa:question/${t.id}`, "시안은 `aaa:report/login-v2`에 있다. 없는 것은 aaa:report/gone-1."]);

    // 답할 것이 있으면 보고서 단추의 수는 빨강 답할 수다(3A)
    await until("빨강 배지", async () => (await badge(page)) === "ask 1");
    // coordinator 대화: 질문 칩은 답할 것, 보고서 칩은 제목. 모르는 보고서도 칩이다(누르면 목록)
    await inList(page, `pane-${LABEL}`);
    if ((await page.getByTestId("chat").count()) === 0) {
      await until("대화 단추", async () => (await page.getByTestId("chat-toggle").count()) === 1);
      await page.getByTestId("chat-toggle").click();
    }
    const want = [`question/${t.id} ask ${t.id} 질문 2개답할 것`, "report/login-v2 report 로그인 시안", "report/gone-1 report gone-1"];
    await until("대화 칩", async () => JSON.stringify(await chips(page)) === JSON.stringify(want));
    await shot(page, "51-chat-chips");
    // 질문 칩: 보고서 화면 「답할 것」의 그 카드만 펼친다(2A)
    await page.locator(`[data-testid="chat"] [data-testid="app-ref"][data-kind="question"]`).click();
    await until("질문 카드 펼침", async () =>
      (await page.evaluate((id) => location.hash === `#question=${id}` && document.querySelector(`[data-testid="thread-ask-${id}"]`)?.dataset.state === "open"
        && !!document.querySelector(`[data-testid="thread-ask-${id}"] [data-testid="thread-ask-send"]`), t.id)));
    await shot(page, "51-card-desktop");
    if ((await page.getByTestId("thread-asks-count").innerText()) !== "답할 것 1") throw new Error(`지금 줄: ${await page.getByTestId("thread-asks-count").innerText()}`);
    // 보고서 칩: 앱 안 보고서 화면(넓은 창도 새 탭이 아니다)
    await page.goBack();
    await until("대화로 돌아옴", async () => (await chips(page)).length === 3);
    await page.locator(`[data-testid="chat"] [data-testid="app-ref"][data-target="login-v2"]`).click();
    await until("보고서 칩이 연 보고서", async () => (await page.evaluate(() => location.hash)) === "#report=demo%2Flogin-v2" && (await page.getByTestId("report-detail").count()) === 1);
    await page.goBack();
    await until("대화로 다시", async () => (await chips(page)).length === 3);

    // thread 대화 입력칸 위 칩 한 줄(4A)
    await page.evaluate((id) => (location.hash = `pane=${encodeURIComponent(id)}`), t.pane);
    await until("입력칸 위 질문 칩", async () =>
      (await page.evaluate(() => document.querySelector('[data-testid="dock-question"] [data-testid="app-ref"]')?.dataset.tone)) === "ask");
    await shot(page, "51-dock-chip");

    // 휴대폰(조작)은 알림 주소로 연다: 그 카드가 펼쳐진다. 보기 권한 기기는 카드를 보지만 답할 수 없고 host도 거절한다
    const phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { hash: `#question=${t.id}` });
    const pp = phone.page;
    await until("휴대폰의 펼친 카드", async () => (await pp.locator(`[data-testid="thread-ask-${t.id}"] [data-testid="thread-ask-send"]`).count()) === 1);
    const reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1100, height: 760 }, { hash: `#question=${t.id}` });
    const rp = reader.page;
    await until("보기 권한의 카드", async () => (await rp.locator(`[data-testid="thread-ask-${t.id}"] [data-testid="thread-ask-send"]`).count()) === 1);
    if (!(await rp.getByTestId("thread-ask-send").isDisabled()) || !(await rp.getByTestId("thread-ask-pick-1A").isDisabled())) throw new Error("보기 권한 카드의 단추가 눌린다");
    const denied = await rp.evaluate(async (a) => {
      const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "thread_answer", thread: a.id, report_hash: a.hash, picks: [0, 0], text: null }) });
      return (await r.json()).err?.code ?? "ok";
    }, { id: t.id, hash: v.questions.report_hash });
    if (denied !== "forbidden") throw new Error(`보기 권한 답: ${denied}`);

    // 휴대폰에서 1B만 고르고 「추천대로 보내기」: 2번은 추천(A)으로 채워 thread에 한 번 간다
    await pp.getByTestId("thread-ask-pick-1B").click();
    await shot(pp, "51-card-phone");
    if ((await pp.getByTestId("thread-ask-send").innerText()).trim() !== "추천대로 보내기") throw new Error(`보내기 단추: ${await pp.getByTestId("thread-ask-send").innerText()}`);
    await pp.getByTestId("thread-ask-send").click();
    await waitOutput(t.pane, "사용자 답(1B 2A)");
    const answered = aaa("thread", "show", "demo", t.id).questions.answer;
    if (answered?.summary !== "1B 2A" || JSON.stringify(answered.picks) !== "[1,0]") throw new Error(`기록한 답: ${JSON.stringify(answered)}`);
    // 모든 기기가 답함: 휴대폰·보기 권한 카드, 데스크톱 입력칸 위 칩·배지(안 본 보고서 수로 돌아간다)
    for (const [name, p] of [["휴대폰", pp], ["보기 권한", rp]])
      await until(`${name} 카드 답함`, async () => (await p.locator(`[data-testid="thread-ask-${t.id}"][data-state="answered"]`).count()) === 1);
    await until("입력칸 위 칩 답함", async () =>
      (await page.evaluate(() => document.querySelector('[data-testid="dock-question"] [data-testid="app-ref"]')?.textContent)) === `${t.id} 질문답함 · 1B 2A`);
    // 처음 배지는 안 본 보고서(login-v2)가 있어도 답할 수였다. 그 보고서는 칩으로 열어 봤다: 새 보고서 하나면 안 본 수 1로 돌아간다
    await until("답한 뒤 배지 없음", async () => (await badge(page)) === null);
    aaa("library", "publish", input, "--name", "login-v3", "--to-user", "--title", "로그인 시안 2");
    await until("배지는 안 본 보고서 수", async () => (await badge(page)) === "unseen 1");
    await shot(pp, "51-answered-phone");
    // 같은 질문에 또 답하면 거절한다(다른 기기·CLI가 늦게 눌렀다)
    let again = null;
    try {
      aaa("thread", "answer", "demo", t.id, "1A", "2A");
    } catch (e) {
      again = e.message;
    }
    if (!again?.includes("이미 답함")) throw new Error(`두 번째 답: ${again}`);
    // 웹에서 답하면 coordinator inbox에 남는다(다시 묻거나 전하지 않게). host는 thread에 전한 뒤 남긴다
    const told = await until("inbox에 답함", () => aaa("inbox", "list", "demo", "--peek").items.find((i) => i.event === "question answered"));
    if (told.subject !== t.id || !told.summary.endsWith(": 1B 2A")) throw new Error(`inbox 답함: ${JSON.stringify(told)}`);
    // 새 보고가 오면 질문을 다시 읽고 답을 비운다: 카드는 다시 답할 것이고, 옛 보고(hash)에 맞춘 답은 거절한다
    fs.writeFileSync(`${t.work_dir}/report.md`, "[결정 필요] 질문 1개: 색\n\n## Questions\n1. 단추 색\n   - A 파랑 (추천)\n   - B 회색\n");
    const next = await until("새 질문 보고", () => {
      const q = aaa("thread", "show", "demo", t.id).questions;
      return q && q.report_hash !== v.questions.report_hash && q;
    });
    if (next.answer !== null || next.items.length !== 1) throw new Error(`새 보고의 질문: ${JSON.stringify(next)}`);
    await until("휴대폰 카드 다시 답할 것", async () => (await pp.locator(`[data-testid="thread-ask-${t.id}"][data-state="open"]`).count()) === 1);
    await until("빨강 배지 다시", async () => (await badge(page)) === "ask 1");
    const stale = await pp.evaluate(async (a) => {
      const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "thread_answer", thread: a.id, report_hash: a.hash, picks: [0], text: null }) });
      return (await r.json()).err?.message ?? "ok";
    }, { id: t.id, hash: v.questions.report_hash });
    if (!stale.includes("질문이 바뀌었다")) throw new Error(`옛 보고의 답: ${stale}`);
    for (const x of [phone, reader]) {
      if (x.errors.length) throw new Error(`페이지 오류: ${x.errors.join("; ")}`);
      await x.page.context().close();
    }
    aaa("thread", "resolve", "demo", t.id);
  });
}
