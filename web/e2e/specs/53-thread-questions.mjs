// 웹 E2E 「답할 것」과 앱 안 참조 칩(plan §8.329·§8.345, 사용자 결정 10-20 1A~4A·10-21 1B 2A 3A 4A 5A·1A 2B). thread가 보고
// `## Questions`에 남긴 `### N.` 질문은 사이드바 「답할 것」 목록의 한 줄과 본문 카드가 되고(본문 글·그림·표, 선택지마다 큰 그림),
// 활동 막대 「답할 것」에 빨강 수가 붙는다(보고서 단추는 안 본 수뿐). coordinator 대화의 `aaa:question/<thread>`·`aaa:report/<이름>`과
// 같은 host 링크 `<웹 주소>/#question=`·`#report=`는 칩이고 누르면 앱 안에서 그 카드·그 보고서를 연다. thread 대화 입력칸 위에는
// 그 질문의 칩 한 줄이 있다. 카드로 답하면 host가 한 번만 받아 thread에 「사용자 답(1B 2A)」를 보내고 답할 때의 원문·그림을
// 보존한다: 다른 기기도 답한 카드를 보고, 새 보고가 질문을 비워도 칩은 「답함 · 1B 2A」이고 누르면 그때 원문이 열린다.
// coordinator 확인 필요 post도 「답할 것」이고 「답함으로 표시」하면 coordinator inbox에 남는다. 답한 것은 지우면 모든 기기에서
// 사라진다. 보기 권한 기기는 답·처리·지우기를 할 수 없다. 알림 주소 `/#question=<thread>`(post push url)는 그 카드를 연다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 53-thread-questions(새 실험실).
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
/** 활동 막대 「답할 것」의 빨강 수와 보고서 단추의 안 본 수 */
const badge = (p) =>
  p.evaluate(() => {
    const a = document.querySelector('[data-testid="activity-badge-asks"]');
    const r = document.querySelector('[data-testid="activity-badge-reports"]');
    return `ask ${a?.classList.contains("is-ask") ? a.textContent : "-"} · unseen ${r?.textContent ?? "-"}`;
  });
/** 본문 카드의 그림이 다 떴는가(host 질문 파일 주소에서 받음) */
const pictures = (p, sel) =>
  p.evaluate((sel) => {
    const imgs = [...document.querySelectorAll(`${sel} img`)];
    return imgs.length > 0 && imgs.every((i) => i.complete && i.naturalWidth > 0) ? imgs.length : 0;
  }, sel);
const asksOf = (p) =>
  p.evaluate(async () => {
    const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "asks" }) });
    return r.json();
  });

export default async function ({ page }) {
  await step("「답할 것」과 앱 안 칩(plan §8.329·§8.345): ### 질문은 목록 줄·본문 카드(그림·표·선택지 그림)·빨강 배지, 대화의 aaa: 참조와 같은 host 링크는 앱 안 칩, 입력칸 위 칩 한 줄, 카드 답은 thread에 한 번 가고 원문을 보존(새 보고 뒤에도 칩 답함), 확인 필요 post 답함으로 표시, 답한 것 지우기, 보기 권한은 못 함", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task-cards.md`, "로그인 화면 다듬기\n질문 카드 시험");
    const t = aaa("thread", "start", "demo", "--title", "로그인 화면 다듬기", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-cards.md`);
    const v = await until("질문 보고", () => {
      const v = aaa("thread", "show", "demo", t.id);
      return v.questions?.items.length === 2 && v;
    });
    const q1 = v.questions.items[0];
    if (q1.text !== "로그인 단추 자리" || q1.choices.length !== 2 || !q1.choices[0].recommended || q1.choices[0].description !== "지금 화면과 같음" || v.questions.answer !== null
      || !q1.body?.includes("![지금 화면](library/pic.png)") || !q1.body.includes("| B | 누르기 쉬움 |") || q1.choices[1].body !== "![B안](library/pic.png)"
      || v.questions.items[1].body || v.questions.items[1].choices.length !== 2 || !v.questions.files_base)
      throw new Error(`읽은 질문: ${JSON.stringify(v.questions)}`);
    const input = `${root}/login-report`;
    fs.mkdirSync(input);
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>로그인 시안</h1></html>");
    aaa("library", "publish", input, "--name", "login-v2", "--to-user", "--title", "로그인 시안");
    // 터미널에서도 눌리게 coordinator가 쓰는 같은 host 링크(`aaa context`의 웹 주소)도 칩이다
    const origin = await page.evaluate(() => location.origin);
    chatPane([
      `${t.id} 질문 2개 · 추천: 1A 2A aaa:question/${t.id}`,
      "시안은 `aaa:report/login-v2`에 있다. 없는 것은 aaa:report/gone-1.",
      `터미널 링크: [${t.id} 질문](${origin}/#question=${t.id}) · [로그인 시안](${origin}/#report=login-v2) · [바깥](https://example.ts.net/#question=${t.id})`,
    ]);

    // 답할 것은 활동 막대 「답할 것」의 빨강 수다(3A). 보고서 단추는 안 본 보고서 수만 단다
    await until("빨강 배지", async () => (await badge(page)) === "ask 1 · unseen 1");
    // coordinator 대화: 질문 칩은 답할 것, 보고서 칩은 제목. 모르는 보고서도 칩이다(누르면 목록)
    await inList(page, `pane-${LABEL}`);
    if ((await page.getByTestId("chat").count()) === 0) {
      await until("대화 단추", async () => (await page.getByTestId("chat-toggle").count()) === 1);
      await page.getByTestId("chat-toggle").click();
    }
    const qchip = `question/${t.id} ask ${t.id} 질문 2개답할 것`;
    const want = [qchip, "report/login-v2 report 로그인 시안", "report/gone-1 report gone-1", qchip, "report/login-v2 report 로그인 시안"];
    await until("대화 칩", async () => JSON.stringify(await chips(page)) === JSON.stringify(want));
    // 다른 host 주소는 보통 링크다
    if ((await page.locator('[data-testid="chat"] a[href^="https://example.ts.net/"]').count()) !== 1) throw new Error("바깥 링크가 칩이 됐다");
    await shot(page, "53-chat-chips");
    // 같은 host 질문 링크: 「답할 것」 화면의 그 카드, 사이드바도 「답할 것」 목록(1B). 본문 그림·선택지 그림이 뜬다(2B)
    await page.locator(`[data-testid="chat"] [data-testid="app-ref"][data-kind="question"]`).nth(1).click();
    const card = `[data-testid="thread-ask-${t.id}"]`;
    await until("질문 카드", async () =>
      (await page.evaluate((a) => location.hash === `#question=${a.id}` && document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "true"
        && document.querySelector(".app")?.dataset.sidebar === "asks" && !!document.querySelector(`${a.card} [data-testid="thread-ask-send"]`)
        && document.querySelector('[data-testid="ask-row"].is-selected')?.dataset.key === `q:${a.id}`, { id: t.id, card })));
    await until("본문 그림", async () => (await pictures(page, `${card} [data-testid="ask-markdown"]`)) === 1);
    await until("선택지 그림", async () => (await pictures(page, `${card} .ask-choice-picture`)) === 2);
    if ((await page.locator(`${card} [data-testid="ask-markdown"] table td`).count()) !== 4) throw new Error("본문 표가 없다");
    if ((await page.getByTestId("asks-open-count").innerText()) !== "1") throw new Error(`답할 것 수: ${await page.getByTestId("asks-open-count").innerText()}`);
    await shot(page, "53-card-desktop");
    // 선택지 그림 「크게 보기」: 고르지 않고 크게 본다. Esc로 닫는다
    await page.getByTestId("ask-zoom").first().click();
    await until("크게 보기", async () => (await page.getByTestId("lightbox").count()) === 1);
    await shot(page, "53-card-zoom");
    await page.keyboard.press("Escape");
    await until("크게 보기 닫힘", async () => (await page.getByTestId("lightbox").count()) === 0);
    if ((await page.locator(`${card} [aria-checked="true"]`).count()) !== 0) throw new Error("크게 보기가 선택지를 골랐다");
    // 보고서 칩: 앱 안 보고서 화면(넓은 창도 새 탭이 아니다)
    await page.goBack();
    await until("대화로 돌아옴", async () => (await chips(page)).length === 5);
    await page.locator(`[data-testid="chat"] [data-testid="app-ref"][data-target="login-v2"]`).nth(1).click();
    await until("보고서 칩이 연 보고서", async () => (await page.evaluate(() => location.hash)) === "#report=demo%2Flogin-v2" && (await page.getByTestId("report-detail").count()) === 1);
    await page.goBack();
    await until("대화로 다시", async () => (await chips(page)).length === 5);

    // thread 대화 입력칸 위 칩 한 줄(4A)
    await page.evaluate((id) => (location.hash = `pane=${encodeURIComponent(id)}`), t.pane);
    await until("입력칸 위 질문 칩", async () =>
      (await page.evaluate(() => document.querySelector('[data-testid="dock-question"] [data-testid="app-ref"]')?.dataset.tone)) === "ask");
    await shot(page, "53-dock-chip");

    // 휴대폰(조작)은 알림 주소로 연다: 그 카드가 펼쳐진다. 보기 권한 기기는 카드를 보지만 답할 수 없고 host도 거절한다
    const phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { hash: `#question=${t.id}` });
    const pp = phone.page;
    await until("휴대폰의 카드", async () => (await pp.locator(`${card} [data-testid="thread-ask-send"]`).count()) === 1);
    await until("휴대폰 선택지 그림", async () => (await pictures(pp, `${card} .ask-choice-picture`)) === 2);
    const reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1100, height: 760 }, { hash: `#question=${t.id}` });
    const rp = reader.page;
    await until("보기 권한의 카드", async () => (await rp.locator(`${card} [data-testid="thread-ask-send"]`).count()) === 1);
    if (!(await rp.getByTestId("thread-ask-send").isDisabled()) || !(await rp.getByTestId("thread-ask-pick-1A").isDisabled())) throw new Error("보기 권한 카드의 단추가 눌린다");
    const denied = await rp.evaluate(async (a) => {
      const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "thread_answer", thread: a.id, report_hash: a.hash, picks: [0, 0], text: null }) });
      return (await r.json()).err?.code ?? "ok";
    }, { id: t.id, hash: v.questions.report_hash });
    if (denied !== "forbidden") throw new Error(`보기 권한 답: ${denied}`);

    // 휴대폰에서 1B만 고르고 「추천대로 보내기」: 2번은 추천(A)으로 채워 thread에 한 번 간다
    await pp.getByTestId("thread-ask-pick-1B").click();
    await shot(pp, "53-card-phone");
    if ((await pp.getByTestId("thread-ask-send").innerText()).trim() !== "추천대로 보내기") throw new Error(`보내기 단추: ${await pp.getByTestId("thread-ask-send").innerText()}`);
    await pp.getByTestId("thread-ask-send").click();
    await waitOutput(t.pane, "사용자 답(1B 2A)");
    const answered = aaa("thread", "show", "demo", t.id).questions.answer;
    if (answered?.summary !== "1B 2A" || JSON.stringify(answered.picks) !== "[1,0]") throw new Error(`기록한 답: ${JSON.stringify(answered)}`);
    // 모든 기기가 답함: 휴대폰·보기 권한은 답할 때의 원문(고른 답 표시, 그림 사본)을 보고, 데스크톱 입력칸 위 칩·배지도 답함이다
    for (const [name, p] of [["휴대폰", pp], ["보기 권한", rp]]) {
      await until(`${name} 답한 카드`, async () =>
        (await p.evaluate(() => [...document.querySelectorAll('[data-testid="ask-done-card"][data-kind="question"] [data-testid="ask-done-picked"]')].map((x) => x.textContent.slice(0, 1)).join(""))) === "BA");
      await until(`${name} 답한 카드 그림`, async () => (await pictures(p, '[data-testid="ask-done-card"] .ask-choice-picture')) === 2);
    }
    const rec = await until("답한 것 기록", async () => (await asksOf(pp)).ok?.done?.find((r) => r.kind === "question" && r.subject === t.id));
    if (rec.summary !== "1B 2A" || rec.items.length !== 2 || rec.items[0].body !== q1.body || !rec.files_base?.includes("/asks/")) throw new Error(`답한 것 기록: ${JSON.stringify(rec)}`);
    await until("입력칸 위 칩 답함", async () =>
      (await page.evaluate(() => document.querySelector('[data-testid="dock-question"] [data-testid="app-ref"]')?.textContent)) === `${t.id} 질문답함 · 1B 2A`);
    // 처음 배지는 안 본 보고서(login-v2)가 있어도 답할 수였다. 그 보고서는 칩으로 열어 봤다: 새 보고서 하나면 안 본 수 1로 돌아간다
    await until("답한 뒤 배지 없음", async () => (await badge(page)) === "ask - · unseen -");
    aaa("library", "publish", input, "--name", "login-v3", "--to-user", "--title", "로그인 시안 2");
    await until("배지는 안 본 보고서 수", async () => (await badge(page)) === "ask - · unseen 1");
    await shot(pp, "53-answered-phone");
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
    // 질문 없는 새 보고가 질문·답을 비워도(사용자 화면 지적 10-21) 대화 칩은 보존한 기록으로 「답함」이고, 누르면 그때 원문이 열린다.
    // 그 사이 thread library의 그림이 없어져도 답한 카드는 host 사본을 본다
    fs.rmSync(`${t.work_dir}/library/pic.png`);
    fs.writeFileSync(`${t.work_dir}/report.md`, "[진행 중] 단추를 옮기는 중\n\n## Report\n- 1B 2A로 한다\n");
    await until("질문이 비었다", () => aaa("thread", "show", "demo", t.id).questions == null);
    await inList(page, `pane-${LABEL}`);
    await until("대화 다시", async () => (await page.getByTestId("chat").count()) === 1);
    const doneChip = `question/${t.id} done ${t.id} 질문답함 · 1B 2A`;
    await until("새 보고 뒤 칩 답함", async () => (await chips(page))[0] === doneChip);
    await page.locator(`[data-testid="chat"] [data-testid="app-ref"][data-kind="question"]`).first().click();
    await until("그때 원문", async () => (await page.evaluate(() => document.querySelector('[data-testid="asks-view"]')?.dataset.key)) === `d:${rec.id}`);
    await until("그때 원문 그림(host 사본)", async () => (await pictures(page, '[data-testid="ask-done-card"] [data-testid="ask-markdown"]')) === 1);
    await page.getByTestId("ask-report-toggle").click();
    await until("그때 보고", async () => (await page.getByTestId("ask-report").innerText()).includes("로그인 단추 자리"));
    await shot(page, "53-done-desktop");
    // 새 보고가 질문을 실으면 다시 읽는다: 카드는 다시 답할 것이고, 옛 보고(hash)에 맞춘 답은 거절한다
    fs.writeFileSync(`${t.work_dir}/report.md`, "[결정 필요] 질문 1개: 색\n\n## Questions\n1. 단추 색\n   - A 파랑 (추천)\n   - B 회색\n");
    const next = await until("새 질문 보고", () => {
      const q = aaa("thread", "show", "demo", t.id).questions;
      return q && q.report_hash !== v.questions.report_hash && q;
    });
    if (next.answer !== null || next.items.length !== 1) throw new Error(`새 보고의 질문: ${JSON.stringify(next)}`);
    await until("휴대폰 카드 다시 답할 것", async () => (await pp.locator(`${card}[data-state="open"]`).count()) === 1);
    await until("빨강 배지 다시", async () => (await badge(page)).startsWith("ask 1 "));
    const stale = await pp.evaluate(async (a) => {
      const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "thread_answer", thread: a.id, report_hash: a.hash, picks: [0], text: null }) });
      return (await r.json()).err?.message ?? "ok";
    }, { id: t.id, hash: v.questions.report_hash });
    if (!stale.includes("질문이 바뀌었다")) throw new Error(`옛 보고의 답: ${stale}`);

    // coordinator 확인 필요 post도 「답할 것」이다(3A). 「답함으로 표시」는 그 post를 끝내고 coordinator inbox에 남긴다(5A).
    // 보기 권한 기기는 처리·지우기를 할 수 없다
    const body = `${root}/request-body.md`;
    fs.writeFileSync(body, "배포 전에 **승인**이 필요하다.\n\n- 바뀐 것: 로그인 화면\n");
    const inboxBadge = () => page.evaluate(() => document.querySelector('[data-testid="activity-badge-inbox"]')?.textContent ?? "0");
    aaa("post", "demo", "user", "--request", "--needs-user", "--topic", "user-action", "--title", "배포 승인", "--body-file", body);
    await until("빨강 배지 둘", async () => (await badge(page)).startsWith("ask 2 "));
    // 같은 항목을 두 배지가 세지 않는다: 알림함은 그 요청을 「답할 것」으로 보낸다(plan §8.343·§8.345)
    await page.getByTestId("activity-inbox").click();
    await until("알림함의 답할 요청 수", async () => (await page.getByTestId("inbox-answers").innerText().catch(() => "")).includes("1건"));
    // 배지는 host가 아는 끝내지 않은 항목에서 이 요청 하나를 뺀 수다. 앞 단계 보고의 「확인 필요」 같은 항목이 늦게 와도
    // 둘이 함께 바뀐다(앞뒤 배지만 대조하면 그 항목이 사이에 와서 틀렸다: ci 부하에서 5 → 6)
    const hostOpen = () => {
      const r = aaa("inbox", "list", "demo", "--peek");
      return r.items.filter((i) => !i.done).length + (r.omitted_open ?? 0);
    };
    await until("알림함 배지가 답할 요청을 세지 않음", async () => Number(await inboxBadge()) === hostOpen() - 1).catch(async (e) => {
      throw new Error(`알림함 배지가 답할 요청을 셌다: 배지 ${await inboxBadge()}, host 끝내지 않은 항목 ${hostOpen()}(요청 1 포함) — ${e.message}`);
    });
    if (await page.locator('[data-testid="inbox-item"]', { hasText: "배포 승인" }).count()) throw new Error("알림함 목록에 답할 요청이 있다");
    await page.getByTestId("inbox-answers").getByRole("button", { name: "답할 것 열기" }).click();
    await until("답할 것으로", async () =>
      (await page.evaluate(() => document.querySelector(".app")?.dataset.sidebar === "asks" && document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "true")));
    const reqKey = await until("확인 필요 줄", () => page.evaluate(() => [...document.querySelectorAll('[data-testid="ask-row"]')].map((r) => r.dataset.key).find((k) => k.startsWith("r:"))));
    await page.locator(`[data-testid="ask-row"][data-key="${reqKey}"]`).click();
    await until("확인 필요 카드", async () => (await page.locator('[data-testid="ask-card"][data-kind="request"] strong').innerText().catch(() => "")) === "승인");
    await shot(page, "53-request-desktop");
    const refused = await rp.evaluate(async (item) => {
      const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "ask_handle", item }) });
      return (await r.json()).err?.code ?? "ok";
    }, reqKey.slice(2));
    if (refused !== "forbidden") throw new Error(`보기 권한 처리: ${refused}`);
    await page.getByTestId("ask-handle").click();
    await until("처리한 기록", async () => (await page.evaluate(() => document.querySelector('[data-testid="ask-done-card"]')?.dataset.kind)) === "request");
    await until("확인 필요 빠짐", async () => (await badge(page)).startsWith("ask 1 "));
    const handled = await until("inbox에 처리함", () => aaa("inbox", "list", "demo", "--peek").items.find((i) => i.event === "request handled"));
    if (!handled.summary.includes("배포 승인")) throw new Error(`inbox 처리함: ${JSON.stringify(handled)}`);
    const reqRec = (await asksOf(page)).ok.done.find((r) => r.kind === "request");
    if (reqRec?.title !== "배포 승인" || !reqRec.body?.includes("**승인**")) throw new Error(`처리한 기록: ${JSON.stringify(reqRec)}`);
    // 답한 것 지우기는 모든 기기에서: 휴대폰 목록에서도 빠진다. 보기 권한은 지울 수 없다
    if ((await rp.getByTestId("ask-forget").count()) !== 0) throw new Error("보기 권한에 지우기 단추");
    await page.getByTestId("ask-forget").click();
    await page.locator(".modal-scrim").getByRole("button", { name: "지우기" }).click();
    await until("휴대폰 목록에서 빠짐", async () =>
      (await pp.evaluate((k) => !!document.querySelector('[data-testid="asks-panel"]') && !document.querySelector(`[data-testid="ask-row"][data-key="${k}"]`), `d:${reqRec.id}`)));
    if ((await asksOf(pp)).ok.done.some((r) => r.id === reqRec.id)) throw new Error("지운 기록이 남았다");
    for (const x of [phone, reader]) {
      if (x.errors.length) throw new Error(`페이지 오류: ${x.errors.join("; ")}`);
      await x.page.context().close();
    }
    aaa("thread", "resolve", "demo", t.id);
  });
}
