// 웹 E2E agent pane: 확인창·끝남(안 봄)·대화 기록·그림 붙여 넣기.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 11-agent-panes(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import {
  AA, root, repo, aaa, waitOutput, until, domIs, screenText, pasteFile, bannerPath, shot, pairPage, step, browser,
  claudeSh, clearScreen, claudeAsk, claudeWorking, claudeIdle, openSettings, inList, askClaude,
} from "../lib/harness.mjs";

// 화면 감지 시작 유예 0(기본 3초, aaa-agent detect/state.rs startup_grace): 가짜 harness 화면은 뜨자마자 그대로다.
// 기본값 길은 15-ios-rules가 돈다
export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0" };

export default async function ({ ctl, page }) {
  await step("확인창 질문: 카드·상태 표시 → reader는 못 답함 → 예", async () => {
    // 세션 안에서 Claude 권한 확인창을 그린다(화면 감지가 질문을 연다). 답(Enter)을 받으면 일하는 화면으로 바뀐다.
    const script = `stty -echo; ${claudeAsk("make test")}; read a; ${claudeWorking}; exec cat > /dev/null`;
    const q1 = aaa("tab", "create", "--machine", "labA", "--label", "q1", "--", claudeSh, "-c", script).tabs.at(-1).root.pane;
    // 목록에는 그 줄의 빨간 ?만(차이 목록 Q1·S2, 참고 Sidebar.tsx StatusBadge): 카드는 그 pane을 열면 터미널 아래에
    const state = (pg) => pg.getByTestId("agent-q1");
    await state(page).and(page.locator('[data-status="blocked"]')).waitFor();
    if ((await state(page).locator(".sidebar-status-mark").innerText()) !== "?") throw new Error("질문 줄에 ? 표시가 없다");
    if ((await state(page).getAttribute("aria-label")) !== "agent 답을 기다림") throw new Error(`상태 말: ${await state(page).getAttribute("aria-label")}`);
    await inList(page, "pane-q1");
    // 질문 본문은 화면에서 물음표가 있는 마지막 줄이다(host `modal_of`·agent `question_of`)
    const q = aaa("question", "list").questions.find((x) => x.pane === q1);
    const card = page.getByTestId(`dock-question-${q.id}`);
    await card.waitFor();
    if (!(await card.innerText()).includes("Do you want to proceed?")) throw new Error(`카드 본문: ${await card.innerText()}`);
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    await state(rd.page).and(rd.page.locator('[data-status="blocked"]')).waitFor({ state: "attached" });
    await inList(rd.page, "pane-q1");
    const rcard = rd.page.getByTestId(`dock-question-${q.id}`);
    await rcard.waitFor();
    if ((await rcard.getByRole("button", { name: "예" }).count()) !== 0) throw new Error("reader에게 답 버튼");
    if (!(await rcard.innerText()).includes("보기 권한이라 답할 수 없다")) throw new Error(`reader 카드: ${await rcard.innerText()}`);
    await shot(page, "desktop-question");
    await card.getByRole("button", { name: "예" }).click();
    await card.waitFor({ state: "detached" });
    await rcard.waitFor({ state: "detached" });
    const all = aaa("question", "list", "--all").questions;
    if (all.length !== 1 || all[0].state !== "answered" || all[0].answer !== "yes") throw new Error(JSON.stringify(all));
    await state(page).and(page.locator('[data-status="working"]')).waitFor();
    if (rd.errors.length) throw new Error(rd.errors.join("; "));
    await rd.page.context().close();
  });

  await step("확인창 답이 실패하면 카드 안에 보인다(herdr-web-ui PromptCard.tsx pending·role=alert, QA t-0337 Q3): 휴대폰에서 답하는 동안 선택지가 막히고, host가 선택 창을 못 찾은 오류가 그 카드에 뜨며 다시 누를 수 있다", async () => {
    // 선택 창 끝 줄(「Esc to cancel」)이 없는 확인창: 화면 감지는 Claude 권한 창으로 보고(herdr bash_permission_prompt),
    // host는 MENU_WAIT 동안 선택 창 끝 줄을 찾다가 오류로 답한다
    const script = `${clearScreen}; printf '%s\\n' ' Bash command' '' '   make lint' '' ' Do you want to proceed?' ' ❯ 1. Yes' '   2. No'; stty -echo; exec cat > /dev/null`;
    const q2 = aaa("tab", "create", "--machine", "labA", "--label", "q2", "--", claudeSh, "-c", script).tabs.at(-1).root.pane;
    const ph = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    await ph.page.getByTestId("agent-q2").and(ph.page.locator('[data-status="blocked"]')).waitFor({ state: "attached" });
    await inList(ph.page, "pane-q2");
    const q = aaa("question", "list").questions.find((x) => x.pane === q2);
    const id = `dock-question-${q.id}`;
    const card = ph.page.getByTestId(id);
    await card.waitFor();
    const answered = ph.page.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "question_answer");
    await card.getByRole("button", { name: "예" }).click();
    // 답이 가는 동안: 카드는 aria-busy, 선택지는 막힘(누른 순간의 모습을 본다. 답은 MENU_WAIT 뒤에 온다)
    const busy = await card.evaluate((c) => [c.getAttribute("aria-busy"), [...c.querySelectorAll(".q-actions button")].map((b) => b.disabled)]);
    if (JSON.stringify(busy) !== JSON.stringify(["true", [true, true]])) throw new Error(`답하는 동안 카드 ${JSON.stringify(busy)}`);
    const res = await (await answered).json();
    if (res.ok !== undefined) throw new Error(`실패해야 할 답이 성공했다: ${JSON.stringify(res)}`);
    await domIs(ph.page, "카드 안 오류", (sel) => document.querySelector(sel)?.textContent.includes("터미널에서 직접 답한다"), `[data-testid="${id}"] [role=alert]`);
    const after = await card.evaluate((c) => [c.getAttribute("aria-busy"), [...c.querySelectorAll(".q-actions button")].map((b) => b.disabled)]);
    if (JSON.stringify(after) !== JSON.stringify(["false", [false, false]])) throw new Error(`오류 뒤 카드 ${JSON.stringify(after)}`);
    // 오류는 카드에만(휴대폰은 pane을 보는 동안 목록 머리 띠가 숨는다)
    if (!(await card.getByRole("alert").isVisible())) throw new Error("휴대폰에서 카드 오류가 보이지 않는다");
    await shot(ph.page, "phone-question-error");
    if (ph.errors.length) throw new Error(ph.errors.join("; "));
    await ph.page.context().close();
    aaa("pane", "kill", "q2");
  });

  await step("화면 선택 창 카드(herdr-web-ui PromptCard, 차이 목록 Q2): 선택지 설명·추천·번호, 터미널에서 옮긴 커서에서 세어 고름, 그 사이 바뀐 창은 거절하고 새 창으로, 직접 쓰기, 여러 질문(하나 → 여러 개 체크 → 검토)", async () => {
    const opened = async (pane, not = []) => {
      const q = await until("열린 선택 창 질문", () => aaa("question", "list").questions.find((x) => x.pane === pane && x.menu && !not.includes(x.menu.id)));
      const card = page.getByTestId(`dock-question-${q.id}`).and(page.locator(`[data-menu="${q.menu.id}"]`));
      await card.waitFor();
      return { q, card };
    };
    const said = (pane, text) => until(`화면에 「${text}」`, () => aaa("pane", "read", pane, "--source", "recent").text.includes(text));
    const p1 = aaa("tab", "create", "--machine", "labA", "--label", "ask1", "--", askClaude, "one").tabs.at(-1).root.pane;
    await inList(page, "pane-ask1");
    let { q, card } = await opened(p1);
    const shown = await card.evaluate((c) => ({
      title: c.querySelector("h2")?.textContent,
      question: c.querySelector(".prompt-card-question")?.textContent,
      options: [...c.querySelectorAll(".prompt-card-option")].map((o) => [
        o.querySelector(".prompt-card-number [aria-hidden]")?.textContent,
        o.querySelector(".prompt-card-option-label")?.textContent,
        o.querySelector(".prompt-card-option-description")?.textContent ?? null,
      ]),
      custom: c.querySelector(".prompt-card-custom input")?.getAttribute("placeholder"),
    }));
    const want = {
      title: "저장소",
      question: "결과를 어디에 둘까요?",
      options: [["1", "SQLite 추천", "한 파일, 설치 없음"], ["2", "Postgres", "서버가 필요하다"], ["3", "파일 몇 개", null]],
      custom: "답을 입력",
    };
    if (JSON.stringify(shown) !== JSON.stringify(want)) throw new Error(`선택 창 카드 ${JSON.stringify(shown)}`);
    // 설명이 붙은 줄은 그만큼 자라고 다음 줄과 겹치지 않는다(앱 button 기본의 고정 높이·한 줄을 쓰지 않는다)
    const overlap = await card.evaluate((c) => {
      const rows = [...c.querySelectorAll(".prompt-card-option")].map((o) => [o.getBoundingClientRect(), o.querySelector(".prompt-card-option-description")?.getBoundingClientRect()]);
      return rows.flatMap(([r, d], i) => [
        ...(d && d.bottom > r.bottom + 0.5 ? [`${i + 1} 설명이 줄 밖`] : []),
        ...(i + 1 < rows.length && r.bottom > rows[i + 1][0].top + 0.5 ? [`${i + 1}·${i + 2} 겹침`] : []),
      ]);
    });
    if (overlap.length) throw new Error(`선택지 줄: ${overlap.join(", ")}`);
    await shot(page, "desktop-prompt-card");
    // 터미널에서 커서를 둘째로 옮겨도 같은 창이다: 카드의 셋째는 거기서 한 칸 아래다
    aaa("pane", "send-keys", "ask1", "down");
    await until("커서가 둘째 줄", () => aaa("pane", "read", "ask1", "--source", "recent").text.includes("❯ 2. Postgres"));
    // 그 사이 터미널에서 창이 바뀌었다(선택지 이름): 낡은 카드의 답은 거절되고 카드는 새 창으로 다시 그린다. 키는 가지 않는다
    aaa("pane", "send-text", "ask1", "r");
    await until("바뀐 선택지", () => aaa("pane", "read", "ask1", "--source", "recent").text.includes("2. MySQL"));
    const refused = page.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "question_answer");
    await card.getByRole("button", { name: /Postgres/ }).click();
    const res = await (await refused).json();
    if (!JSON.stringify(res).includes("화면의 질문이 바뀌었다")) throw new Error(`낡은 카드 답: ${JSON.stringify(res)}`);
    ({ q, card } = await opened(p1, [q.menu.id]));
    if (!(await card.innerText()).includes("MySQL")) throw new Error(`새 창 카드: ${await card.innerText()}`);
    await card.getByRole("button", { name: /파일 몇 개/ }).click();
    await said("ask1", "answer: 파일 몇 개");
    // 다음 질문: 직접 쓰기(한글, 글자를 그 줄에 친 뒤 Enter)
    ({ q, card } = await opened(p1, [q.menu.id]));
    if ((await card.locator("h2").textContent()) !== "이름") throw new Error(`둘째 질문 ${await card.innerText()}`);
    await card.locator(".prompt-card-custom input").fill("한글 이름");
    await card.locator(".prompt-card-custom input").press("Enter");
    await said("ask1", "answer: custom 한글 이름");
    const done = aaa("question", "list", "--all").questions.filter((x) => x.pane === p1 && x.state === "answered").map((x) => x.answer);
    if (JSON.stringify(done) !== JSON.stringify(["3. 파일 몇 개", "직접: 한글 이름"])) throw new Error(`답한 질문 ${JSON.stringify(done)}`);

    // 여러 질문: 첫째 하나 고르기 → 둘째 여러 개 체크(제출 (n)) → 검토 → 보내기
    const p2 = aaa("tab", "create", "--machine", "labA", "--label", "ask2", "--", askClaude, "tabs").tabs.at(-1).root.pane;
    await inList(page, "pane-ask2");
    ({ q, card } = await opened(p2));
    if ((await card.locator("h2").textContent()) !== "범위 · 1/2") throw new Error(`첫 탭 ${await card.innerText()}`);
    await card.getByRole("button", { name: /웹/ }).click();
    ({ q, card } = await opened(p2, [q.menu.id]));
    if ((await card.locator("h2").textContent()) !== "대상 · 2/2" || (await card.locator("input[type=checkbox]").count()) !== 3) {
      throw new Error(`둘째 탭 ${await card.innerText()}`);
    }
    await card.getByText("TUI").click();
    await card.getByText("CLI").click();
    // 「제출」은 고르면 주 단추로 바뀐다(색·투명도 전환): 그 전환이 끝난 모습을 찍는다
    await card.locator(".prompt-card-submit").evaluate((b) => Promise.all(b.getAnimations().map((a) => a.finished)));
    await shot(page, "desktop-prompt-card-multi");
    await card.getByRole("button", { name: "제출 (2)" }).click();
    ({ q, card } = await opened(p2, [q.menu.id]));
    if ((await card.locator("h2").textContent()) !== "답 검토") throw new Error(`검토 ${await card.innerText()}`);
    await card.getByRole("button", { name: /Submit answers/ }).click();
    await said("ask2", "answers: 범위=웹; 대상=TUI, CLI");
    aaa("pane", "kill", "ask1");
    aaa("pane", "kill", "ask2");
  });

  await step("끝남(안 봄)(herdr PaneState::seen, 차이 목록 S1): 보는 화면 없이 턴이 끝나면 초록 점, 그 pane을 열면 사라지고, 보는 동안 끝난 턴은 남지 않으며, 가려진 동안 끝나면 다시 보일 때 사라진다", async () => {
    // 줄 하나를 받을 때마다 턴을 열고(일하는 화면) 다음 줄에 닫는다(입력 상자): 시험이 턴의 끝을 정한다
    const script = `stty -echo; ${claudeIdle}; while read a; do ${claudeWorking}; read b; ${claudeIdle}; done`;
    aaa("tab", "create", "--machine", "labA", "--label", "seen1", "--", claudeSh, "-c", script);
    const inUse = (p, on) =>
      p.evaluate((on) => {
        if (on) delete document.hasFocus;
        else document.hasFocus = () => false;
        window.dispatchEvent(new Event(on ? "focus" : "blur"));
      }, on);
    const state = page.getByTestId("agent-seen1");
    const is = (s) => state.and(page.locator(`[data-status="${s}"]`)).waitFor();
    const using = async () => (await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.in_use));
    const turn = async (end, inUseNow) => {
      aaa("pane", "run", "seen1", "go");
      await is("working");
      // presence는 턴을 열기 전에 보냈고 host를 한 바퀴 돈 working이 왔다: host가 이미 안다(기다리지 않고 확인만 한다)
      const n = await using();
      if ((n > 0) !== inUseNow) throw new Error(`host가 아는 쓰는 화면 수 ${n}(쓰는 중 ${inUseNow})`);
      aaa("pane", "run", "seen1", "end");
      await is(end);
    };
    await inUse(page, true);
    // 1) 다른 pane(q1)을 보는 동안 끝났다: 끝남(안 봄), 초록 점과 그 말
    await turn("done", true);
    if ((await state.locator(".sidebar-status-dot").count()) !== 1) throw new Error("끝남에 점이 없다");
    if ((await state.getAttribute("aria-label")) !== "agent 끝남(안 봄)") throw new Error(`상태 말: ${await state.getAttribute("aria-label")}`);
    await shot(page, "desktop-unseen-done");
    // 2) 그 pane을 열면 봤다
    await inList(page, "pane-seen1");
    await is("idle");
    // 3) 보는 동안 끝난 턴은 끝남으로 남지 않는다
    await turn("idle", true);
    // 4) 가려진(쓰는 화면이 아닌) 동안 끝났다: 열어 둔 pane이라도 끝남, 다시 쓰는 화면이 되면 봤다
    await inUse(page, false);
    await turn("done", false);
    await inUse(page, true);
    await is("idle");
    aaa("pane", "kill", "seen1");
  });

  await step("대화 기록(읽기 전용): Claude 기록에서 사람·답·도구 줄만, 도구 결과는 빼고 생각은 설정 「생각 표시」일 때만(C4), 「채팅 글자 크기」(T4), 새 줄을 따라가고, 기록 폴더 밖은 거부", async () => {
    const dir = `${root}/claude/projects/-lab-chat`;
    fs.mkdirSync(dir, { recursive: true });
    const tr = `${dir}/s1.jsonl`;
    const L = (o) => JSON.stringify(o) + "\n";
    fs.writeFileSync(
      tr,
      L({ type: "user", message: { role: "user", content: "결제 지연을 줄여 줘" } }) +
        L({ type: "user", message: { role: "user", content: "<system-reminder>끼운 글</system-reminder>" } }) +
        L({
          type: "assistant",
          message: {
            content: [
              { type: "thinking", thinking: "속생각" },
              { type: "text", text: "캐시를 **넣었다**" },
              { type: "tool_use", name: "Bash", input: { command: "cargo test" } },
            ],
          },
        }) +
        L({ type: "user", message: { content: [{ type: "tool_result", content: "도구결과본문" }] } }),
    );
    const hookPane = (name, path) =>
      aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
        JSON.stringify({ hook_event_name: "SessionStart", session_id: "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b9a", source: "startup", transcript_path: path }));
    hookPane("chat1", tr);
    await inList(page, "pane-chat1");
    await until("대화 단추", async () => (await page.getByTestId("chat-toggle").count()) === 1);
    await page.getByTestId("chat-toggle").click();
    const chat = page.getByTestId("chat");
    const roles = async () => chat.locator(".chat-msg").evaluateAll((els) => els.map((e) => [e.dataset.role, e.textContent]));
    await until("대화 줄", async () => (await roles()).length === 3);
    const want = [["user", "결제 지연을 줄여 줘"], ["assistant", "캐시를 넣었다"], ["tool", "Bash: cargo test"]];
    if (JSON.stringify(await roles()) !== JSON.stringify(want)) throw new Error(JSON.stringify(await roles()));
    // 사람·답 말은 Markdown(참고 ChatView, plan §8.179): 굵게는 strong이고 「**」가 남지 않는다. 사람 말은 카드, 답은 카드 없이
    const look = await chat.evaluate((c) => {
      const user = c.querySelector('[data-role="user"]');
      const answer = c.querySelector('[data-role="assistant"]');
      return {
        strong: answer.querySelector(".markdown strong")?.textContent,
        raw: answer.textContent.includes("**"),
        userCard: getComputedStyle(user).borderTopStyle !== "none" && getComputedStyle(user).backgroundColor !== "rgba(0, 0, 0, 0)",
        answerBare: getComputedStyle(answer).borderTopStyle === "none" && getComputedStyle(answer).backgroundColor === "rgba(0, 0, 0, 0)",
      };
    });
    if (JSON.stringify(look) !== JSON.stringify({ strong: "넣었다", raw: false, userCard: true, answerBare: true })) throw new Error(`대화 Markdown ${JSON.stringify(look)}`);
    // 생각·도구 결과·끼운 글을 빼는 규칙은 agent chat.rs `claude_tail_filters`가 본다(위 세 줄이 그대로면 빠졌다)
    // 새 줄이 생기면 따라간다
    // 글 속 파일 경로는 그 pane의 파일 보기로 연다(plan §8.171, 참고 ChatView `OpenFileContext`)
    fs.writeFileSync(`${root}/files/chat-note.md`, "# 대화 메모\n");
    fs.appendFileSync(tr, L({ type: "assistant", message: { content: [{ type: "text", text: `시험도 통과했다. 기록은 ${root}/files/chat-note.md 에 남겼다` }] } }));
    await until("새 줄", async () => (await roles()).length === 4);
    await shot(page, "desktop-chat");
    // 설정 「생각 표시」(herdr-web-ui showThinking, 차이 목록 C4): 켜면 생각이 제자리에 접힌 「생각」 줄로, 누르면 글
    const thinkingSetting = async (on) => {
      const d = await openSettings(page, "채팅");
      const sw = d.getByRole("switch", { name: "생각 표시" });
      if ((await sw.getAttribute("aria-checked")) !== String(on)) await sw.click();
      await page.keyboard.press("Escape");
      await d.waitFor({ state: "detached" });
    };
    await thinkingSetting(true);
    await until("생각 줄", async () => JSON.stringify((await roles())[1]) === JSON.stringify(["thinking", "생각"]));
    await chat.getByRole("button", { name: "생각" }).click();
    await until("생각 글", async () => (await chat.locator('[data-role="thinking"]').innerText()).includes("속생각"));
    await shot(page, "desktop-chat-thinking");
    await thinkingSetting(false);
    await until("생각을 끄면 뺀다", async () => (await roles()).length === 4);
    // 설정 「채팅 글자 크기」(herdr-web-ui chatFontSize·--chat-scale, 차이 목록 T4): 한 칸 크게 하면 답 줄 글자가 커지고, 작게 하면 돌아온다
    const answerPx = () => chat.locator('[data-role="assistant"] .markdown').first().evaluate((e) => parseFloat(getComputedStyle(e).fontSize));
    const before = await answerPx();
    const chatFont = async (button) => {
      const d = await openSettings(page, "채팅");
      await d.getByRole("button", { name: button }).click();
      await page.keyboard.press("Escape");
      await d.waitFor({ state: "detached" });
    };
    await chatFont("채팅 글자 크게");
    const bigger = await answerPx();
    if (!(bigger > before)) throw new Error(`채팅 글자 크게: ${before} → ${bigger}`);
    await chatFont("채팅 글자 작게");
    if ((await answerPx()) !== before) throw new Error(`채팅 글자 작게로 돌아오지 않음: ${await answerPx()} ≠ ${before}`);
    // 글 속 파일 경로: 그 줄만 단추이고 누르면 앱 안 파일 보기, 뒤로 한 번이면 대화로 돌아온다
    const chatLinks = await chat.locator(".markdown-file").evaluateAll((els) => els.map((e) => [e.closest(".chat-msg")?.dataset.role, e.textContent]));
    if (JSON.stringify(chatLinks) !== JSON.stringify([["assistant", `${root}/files/chat-note.md`]])) throw new Error(`대화 파일 링크 ${JSON.stringify(chatLinks)}`);
    await chat.locator(".markdown-file").click();
    await until("대화에서 연 파일", async () => (await page.getByTestId("file-viewer-markdown").locator(".markdown-h1").textContent().catch(() => null)) === "대화 메모");
    await page.goBack();
    await domIs(page, "대화 파일 보기 닫힘", () => !document.querySelector('[data-testid="file-viewer"]') && !!document.querySelector('[data-testid="chat"]'));
    // 말 둘레(참고 ChatView, plan §8.182): 대화는 가운데 기둥(폭 --chat-w까지)
    const column = await chat.evaluate((c) => {
      const t = c.querySelector(".chat-transcript");
      const probe = document.createElement("div");
      probe.style.width = "var(--chat-w)";
      t.append(probe);
      const limit = probe.getBoundingClientRect().width;
      probe.remove();
      const box = t.getBoundingClientRect();
      const outer = c.getBoundingClientRect();
      const inner = outer.width - parseFloat(getComputedStyle(c).paddingLeft) - parseFloat(getComputedStyle(c).paddingRight);
      return { width: Math.abs(box.width - Math.min(inner, limit)) <= 1, centered: Math.abs(box.left - outer.left - (outer.right - box.right)) <= 1 };
    });
    if (!column.width || !column.centered) throw new Error(`대화 기둥 ${JSON.stringify(column)}`);
    // 복사: 사람 말은 「메시지 복사」, 답은 마크다운·일반 텍스트. 마우스면 말에 올렸을 때만 보인다. clipboard는 page 안에서 받는다(사람의 것을 덮지 않는다)
    await page.evaluate(() => {
      window.__copied = [];
      window.__copyFails = false;
      Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (t) => { if (window.__copyFails) throw new Error("거부"); window.__copied.push(t); } } });
      const exec = document.execCommand.bind(document);
      document.execCommand = (cmd, ...rest) => (cmd === "copy" ? false : exec(cmd, ...rest));
    });
    const agentTurn = chat.locator(".chat-turn-agent").first();
    const userTurn = chat.locator(".chat-turn-user").first();
    const metaShown = (turn) => turn.locator(".chat-turn-meta").evaluate((e) => getComputedStyle(e).opacity === "1");
    await page.mouse.move(0, 0);
    await until("마우스가 없으면 복사 단추를 감춘다", async () => !(await metaShown(agentTurn)) && !(await metaShown(userTurn)));
    await agentTurn.hover();
    await until("답에 올리면 복사 단추", () => metaShown(agentTurn));
    const copied = async (turn, label, want) => {
      // 이름이 「복사됨」으로 바뀌므로 누른 그 단추를 잡아 둔다(앞서 누른 단추도 잠깐 「복사됨」이다)
      const button = await turn.getByRole("button", { name: label }).elementHandle();
      await button.click();
      await until(`${label} 복사됨`, async () => (await button.getAttribute("aria-label")) === "복사됨");
      const got = await page.evaluate(() => window.__copied.at(-1));
      if (got !== want) throw new Error(`${label}: ${JSON.stringify(got)}`);
    };
    await copied(agentTurn, "마크다운으로 복사", "캐시를 **넣었다**");
    await copied(agentTurn, "일반 텍스트로 복사", "캐시를 넣었다");
    await userTurn.hover();
    await copied(userTurn, "메시지 복사", "결제 지연을 줄여 줘");
    // 복사가 안 되면 그 말 아래에 한 줄, 다시 되면 지운다
    await until("복사됨이 돌아온다", async () => (await userTurn.getByRole("button", { name: "메시지 복사" }).count()) === 1);
    await page.evaluate(() => (window.__copyFails = true));
    await userTurn.getByRole("button", { name: "메시지 복사" }).click();
    await until("복사 실패 줄", async () => (await userTurn.getByRole("alert").textContent().catch(() => "")).includes("복사하지 못했습니다"));
    await page.evaluate(() => (window.__copyFails = false));
    await userTurn.getByRole("button", { name: "메시지 복사" }).click();
    await until("복사 실패 줄을 지운다", async () => (await userTurn.getByRole("alert").count()) === 0);
    // 긴 사람 말은 접혀 몇 줄만, 「전체 메시지 보기」로 펼치고 「접기」로 다시. 짧은 말에는 단추가 없다
    const long = Array.from({ length: 30 }, (_, i) => `요청 ${i + 1}째 줄`).join("\n");
    fs.appendFileSync(tr, L({ type: "user", message: { role: "user", content: long } }));
    await until("긴 사람 말", async () => (await roles()).length === 5);
    const bubble = chat.locator('[data-role="user"]').nth(1);
    const fold = async () =>
      bubble.evaluate((b) => {
        const body = b.querySelector(".chat-bubble-body");
        const button = b.querySelector(".chat-bubble-fold");
        return { cls: [...b.classList].filter((c) => c.startsWith("is-")).join(" "), cut: body.scrollHeight > body.clientHeight + 1, button: button?.textContent ?? null, expanded: button?.getAttribute("aria-expanded") ?? null };
      });
    await until("긴 말이 접혔다", async () => JSON.stringify(await fold()) === JSON.stringify({ cls: "is-capped is-folded", cut: true, button: "전체 메시지 보기", expanded: "false" }));
    if ((await chat.locator('[data-role="user"]').first().locator(".chat-bubble-fold").count()) !== 0) throw new Error("짧은 말에 접기 단추");
    await shot(page, "desktop-chat-fold");
    await bubble.getByRole("button", { name: "전체 메시지 보기" }).click();
    await until("펼쳤다", async () => JSON.stringify(await fold()) === JSON.stringify({ cls: "", cut: false, button: "접기", expanded: "true" }));
    await bubble.getByRole("button", { name: "접기" }).click();
    await until("다시 접었다", async () => (await fold()).cls === "is-capped is-folded");
    // 터미널로 돌아간다
    await page.getByTestId("chat-toggle").click();
    if (await page.getByTestId("terminal").isHidden()) throw new Error("터미널로 돌아가지 않았다");
    // CLI도 같은 기록을 본다
    const c = aaa("pane", "chat", "chat1");
    if (c.messages.length !== 5 || c.harness !== "claude") throw new Error(JSON.stringify(c));
    const ct = aaa("pane", "chat", "chat1", "--thinking");
    if (ct.messages[1]?.role !== "thinking" || ct.messages[1].text !== "속생각") throw new Error(JSON.stringify(ct));
    // 기록 폴더 밖을 가리키는 hook(세션 안 누구나 보낼 수 있다)은 읽지 않는다
    fs.writeFileSync(`${root}/secret.jsonl`, L({ type: "user", message: { role: "user", content: "비밀" } }));
    hookPane("chat2", `${root}/secret.jsonl`);
    await until("chat2 harness", () => aaa("pane", "list").panes.find((p) => p.name === "chat2")?.agent?.harness === "claude");
    let refused = "";
    try {
      aaa("pane", "chat", "chat2");
    } catch (e) {
      // 오류 JSON은 stderr로 온다(herdr와 같다)
      refused = String(e.stderr);
    }
    if (!refused.includes("기록 폴더 밖") || refused.includes("비밀")) throw new Error(`밖의 파일: ${refused}`);
    for (const n of ["chat1", "chat2"]) aaa("pane", "kill", n);
    await inList(page, "pane-t1");
    await until("t1 터미널", async () => (await screenText(page)).includes("t1-42"));
  });

  await step("대화 기록 변화 push(herdr-web-ui conversation-watch·conversationRefresh.ts, 차이 목록 C3): 대화를 보는 화면에 agent가 기록 파일이 바뀐 것을 알려 바로 다시 읽고, 같은 pane을 보던 다른 화면이 그만둬도 계속 온다", async () => {
    const dir = `${root}/claude/projects/-lab-push`;
    fs.mkdirSync(dir, { recursive: true });
    const tr = `${dir}/s3.jsonl`;
    const L = (o) => JSON.stringify(o) + "\n";
    fs.writeFileSync(tr, L({ type: "user", message: { role: "user", content: "첫 말" } }));
    aaa("tab", "create", "--machine", "labA", "--label", "chat3", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
      JSON.stringify({ hook_event_name: "SessionStart", session_id: "6e5d4c3b-2a1f-4e0d-9c8b-7a6f5e4d3c2b", source: "startup", transcript_path: tr }));
    const pane = await until("chat3 harness", () => {
      const p = aaa("pane", "list").panes.find((x) => x.name === "chat3");
      return p?.agent?.harness === "claude" && p.id;
    });
    // 이 화면의 대화 읽기(pane_chat)와 그 pane의 chat_changed를 센다
    const reads = { started: 0, done: 0 };
    let changed = 0;
    const ph = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, {
      onPage: (p) => {
        p.on("request", (r) => {
          if (r.url().endsWith("/api/v1/call") && r.postData()?.includes('"op":"pane_chat"')) reads.started += 1;
        });
        p.on("requestfinished", (r) => {
          if (r.url().endsWith("/api/v1/call") && r.postData()?.includes('"op":"pane_chat"')) reads.done += 1;
        });
        p.on("websocket", (ws) => ws.on("framereceived", (f) => {
          if (typeof f.payload === "string" && f.payload.includes('"ev":"chat_changed"') && f.payload.includes(`"pane":"${pane}"`)) changed += 1;
        }));
      },
    });
    // 가짜 시계: 대화 보기의 2초 읽기·10초 backstop을 이 시계에 건다. 대화를 연 뒤 멈추면 그것들은 돌 수 없고, 읽기는 push(또는
    // 아래에서 시계를 민 것)로만 일어난다. 멈춘 동안은 frame을 기다리는 도우미(inList)를 쓰지 않는다
    await ph.page.clock.install();
    const chat = ph.page.getByTestId("chat");
    const roles = async () => chat.locator(".chat-msg").evaluateAll((els) => els.map((e) => [e.dataset.role, e.textContent]));
    await inList(ph.page, "pane-chat3");
    await ph.page.getByTestId("chat-toggle").click();
    await until("첫 말", async () => (await roles()).length === 1);
    await domIs(ph.page, "push가 된다", () => document.querySelector('[data-testid="chat"]')?.dataset.pushes === "true");
    await ph.page.clock.pauseAt(Date.now() + 1000);
    // 같은 pane을 다른 화면(앞 화면)도 열었다 닫는다: host는 청한 연결을 세므로 이 화면의 감시는 남는다
    await inList(page, "pane-chat3");
    await page.getByTestId("chat-toggle").click();
    await page.getByTestId("chat").and(page.locator('[data-pushes="true"]')).waitFor();
    await page.getByTestId("chat-toggle").click();
    await page.getByTestId("chat").waitFor({ state: "detached" });
    // 켤 때 agent가 지금 값을 한 번 알린다. 그 읽기는 처음 읽기와 2초 안이라 미뤄졌다: 시계를 밀어 끝내고, 2초 한도도 지나게 둔다
    await until("켤 때 알림", () => changed >= 1);
    for (let i = 0; i < 2; i++) {
      await ph.page.clock.runFor(2500);
      await until("읽기 끝", () => reads.done === reads.started);
    }
    const before = { reads: reads.started, changed };
    fs.appendFileSync(tr, L({ type: "assistant", message: { content: [{ type: "text", text: "push로 온 답" }] } }));
    await until("push로 온 새 줄", async () => JSON.stringify((await roles()).at(-1)) === JSON.stringify(["assistant", "push로 온 답"]));
    if (changed <= before.changed) throw new Error("chat_changed 없이 읽었다");
    if (reads.started !== before.reads + 1) throw new Error(`push 한 번에 읽기 ${reads.started - before.reads}번`);
    if (ph.errors.length) throw new Error(ph.errors.join("; "));
    await ph.page.context().close();
    aaa("pane", "kill", "chat3");
    await inList(page, "pane-t1");
  });

  await step("그림 붙여 넣기·끌어 놓기(herdr-web-ui): 셸·harness 입력창에 따옴표 경로(Enter 없음), 글이 있으면 글, 여러 파일은 놓은 순서, 글 놓기, 확인창·형식·reader 거부", async () => {
    const png = fs.readFileSync(`${root}/pic.png`);
    const banner = page.getByTestId("attach-banner");
    // 1) 일반 셸(t1): 대상 머신 세션 폴더에 저장하고 셸에도 따옴표 경로와 빈칸을 넣는다(결정 20 D7, Enter 없음)
    await inList(page, "pane-t1");
    await until("t1 터미널", async () => (await screenText(page)).includes("$"));
    // 띠는 터미널 위에 뜬다(herdr-web-ui .terminal-banners): 흐름에 있으면 띠가 날 때 칸이 줄어 PTY 크기가 바뀐다
    const termBox = () => page.locator(".term-scroll").evaluate((el) => `${el.clientWidth}x${el.clientHeight}`);
    const boxBefore = await termBox();
    await pasteFile(page, png, "image/png");
    await until("셸 경로 배너", async () => (await banner.innerText()).includes("경로를 넣었다"));
    if ((await termBox()) !== boxBefore) throw new Error(`그림 띠가 터미널 칸을 바꿨다: ${boxBefore} → ${await termBox()}`);
    const p1 = bannerPath(await banner.innerText());
    if (!p1?.startsWith(`${root}/s/agent/sessions/`)) throw new Error(`저장 경로: ${await banner.innerText()}`);
    if (!fs.readFileSync(p1).equals(png)) throw new Error("저장한 그림이 다르다");
    // 긴 경로는 화면에서 줄이 접힌다: 접힌 줄을 이어 본다(herdr recent-unwrapped)
    await waitOutput("t1", `'${p1}'`, "recent-unwrapped");
    // Enter가 없었다: 셸은 실행하지 않았다. 그 줄을 지우고 표지를 실행한다: 셸은 입력을 차례로 처리하므로 경로를 실행했다면
    // 그 오류가 표지보다 먼저 나온다
    aaa("pane", "send-text", "t1", "\u0015");
    aaa("pane", "run", "t1", "echo paste-fence-$((2+2))");
    await waitOutput("t1", "paste-fence-4");
    // 붙인 파일은 있으니 실행했다면 「Permission denied」다(0600). 옛 단언은 「No such file」만 봐 이 결함을 잡지 못했다
    const ran = /No such file|Permission denied|command not found|cannot execute/.exec(aaa("pane", "read", "t1", "--source", "visible").text.replaceAll("\n", ""));
    if (ran) throw new Error(`셸이 경로를 실행했다: ${ran[0]}`);
    // 2) 형식이 다른 파일(SVG)은 host가 signature로 거부한다
    await pasteFile(page, Buffer.from("<svg xmlns='http://www.w3.org/2000/svg'/>"), "image/svg+xml", "x.svg");
    await until("형식 거부", async () => (await page.locator(".banner.err").innerText()).includes("PNG·JPEG·GIF·WebP"));
    if ((await termBox()) !== boxBefore) throw new Error(`오류 띠가 터미널 칸을 바꿨다: ${boxBefore} → ${await termBox()}`);

    // 3) harness pane(세션 ID hook 보냄, 입력 대기, Claude처럼 bracketed paste를 켬): 여러 조각(1.2MB) 그림의 경로를
    // bracketed paste로 넣고 Enter는 보내지 않는다(감싸기는 프로그램이 켠 모드를 따른다, herdr-web-ui xterm paste)
    const big = Buffer.concat([png.subarray(0, 8), Buffer.alloc(1_200_000, 7)]);
    const recv = `${root}/h1.in`;
    aaa("tab", "create", "--machine", "labA", "--label", "h1", "--", "/bin/sh", "-c",
      '"$0" hook --agent claude \'{"hook_event_name":"SessionStart","session_id":"h1","source":"startup"}\'; stty raw -echo; printf "\\033[?2004h\\342\\235\\257 \\n"; exec cat > "$1"', AA, recv);
    await until("h1 harness 상태", async () => (await page.getByTestId("agent-h1").count()) === 1);
    await inList(page, "pane-h1");
    await until("h1 터미널", async () => (await screenText(page)).includes("❯"));
    await pasteFile(page, big, "image/png");
    await until("입력창 배너", async () => (await banner.innerText()).includes("경로를 넣었다"));
    const p2 = bannerPath(await banner.innerText());
    if (!fs.readFileSync(p2).equals(big)) throw new Error("여러 조각 그림이 다르다");
    await until("PTY가 받은 경로", () => fs.readFileSync(recv, "latin1") === `\x1b[200~'${p2}' \x1b[201~`);
    await shot(page, "desktop-attach");
    // 받은 것 가운데 이 뒤에 온 것
    let seen = fs.readFileSync(recv, "latin1").length;
    const recvNext = () => fs.readFileSync(recv, "latin1").slice(seen);
    // 3a) 클립보드에 글과 그림이 같이 있으면 글이다(복사한 경로도 글로 남는다). 프로그램이 bracketed paste를 켰으니 xterm이 감싼다
    await page.evaluate(
      ({ b64 }) => {
        const dt = new DataTransfer();
        dt.setData("text/plain", "mixed-text");
        dt.items.add(new File([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], "m.png", { type: "image/png" }));
        document.querySelector('[data-testid="terminal"] textarea').dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
      },
      { b64: png.toString("base64") },
    );
    await until("글만 붙여 넣음", () => recvNext() === "\x1b[200~mixed-text\x1b[201~");
    // 그림까지 올렸다면 그 경로는 다음 3b의 올리기보다 먼저 온다(올리기는 한 줄로 선다, Term.tsx fileQueueRef). 그래서 받은 글
    // 길이만큼만 넘긴다: 늦게 온 경로는 3b의 「두 경로」 앞에 남아 그 단언이 실패한다
    seen += "\x1b[200~mixed-text\x1b[201~".length;
    // 3b) 파일 여러 개를 끌어 놓으면 놓은 순서대로 올리고 경로를 차례로 넣는다
    const pa = Buffer.concat([png.subarray(0, 8), Buffer.alloc(300_000, 1)]);
    const pb = Buffer.concat([png.subarray(0, 8), Buffer.alloc(2_000, 2)]);
    const drop = (files, text) =>
      page.evaluate(
        ({ files, text }) => {
          const dt = new DataTransfer();
          for (const f of files) dt.items.add(new File([Uint8Array.from(atob(f.b64), (c) => c.charCodeAt(0))], f.name, { type: "image/png" }));
          if (text) dt.setData("text/plain", text);
          const at = document.querySelector('[data-testid="terminal"] .xterm-screen');
          const over = new DragEvent("dragover", { dataTransfer: dt, bubbles: true, cancelable: true });
          at.dispatchEvent(over);
          at.dispatchEvent(new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true }));
          // dropEffect는 볼 수 없다: 지어낸 DataTransfer는 끌기 중이 아니라 dropEffect 대입을 버린다(Chromium에서 확인)
          return { prevented: over.defaultPrevented };
        },
        { files: files.map(([name, b]) => ({ name, b64: b.toString("base64") })), text },
      );
    const over = await drop([["a.png", pa], ["b.png", pb]]);
    if (!over.prevented) throw new Error(`끌어 오기를 받지 않았다 ${JSON.stringify(over)}`);
    const two = await until("두 경로", () => {
      const m = /^\x1b\[200~'([^']+)' \x1b\[201~\x1b\[200~'([^']+)' \x1b\[201~$/.exec(recvNext());
      return m ? [m[1], m[2]] : null;
    }).catch((e) => {
      throw new Error(`${e.message}: 받은 것 ${JSON.stringify(recvNext())}`);
    });
    if (!fs.readFileSync(two[0]).equals(pa) || !fs.readFileSync(two[1]).equals(pb)) throw new Error(`놓은 순서가 아니다: ${two}`);
    seen = fs.readFileSync(recv, "latin1").length;
    // 3c) 글을 끌어 놓으면 붙여 넣는다(Enter 없음)
    await drop([], "dropped-text");
    await until("놓은 글", () => recvNext() === "\x1b[200~dropped-text\x1b[201~");

    // 4) 확인창이 떠 있는 harness pane에는 넣지 않는다
    const recv2 = `${root}/h2.in`;
    aaa("tab", "create", "--machine", "labA", "--label", "h2", "--", claudeSh, "-c", `${claudeAsk("rm x")}; stty raw -echo; exec cat > "$1"`, AA, recv2);
    await page.getByTestId("agent-h2").and(page.locator('[data-status="blocked"]')).waitFor();
    await inList(page, "pane-h2");
    await until("h2 터미널", async () => (await screenText(page)).includes("Esc to cancel"));
    await pasteFile(page, png, "image/png");
    await until("확인창 배너", async () => (await banner.innerText()).includes("확인창이 떠 있어"));
    // host는 넣지 않기로 정한 뒤 답했다. 같은 host → agent 길로 표지 글을 보내 그것만 왔는지 본다(넣었다면 경로가 먼저 온다)
    aaa("pane", "send-text", "h2", "fence-h2");
    await until("h2 표지", () => fs.readFileSync(recv2, "latin1").includes("fence-h2"));
    if (fs.readFileSync(recv2, "latin1") !== "fence-h2") throw new Error(`확인창에 입력이 갔다: ${JSON.stringify(fs.readFileSync(recv2, "latin1"))}`);

    // 5) reader 연결은 화면 밖에서 직접 보내도 host가 거부한다
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    const h1id = aaa("pane", "list").panes.find((p) => p.name === "h1").id;
    const got = await rd.page.evaluate(
      async ({ id, b64 }) => {
        const ws = new WebSocket(`ws://${location.host}/api/v1/panes/${id}/ws`);
        return await new Promise((resolve) => {
          ws.onopen = () => ws.send(JSON.stringify({ t: "attach", offset: 0, total: 70, sha256: "0".repeat(64), data: b64 }));
          ws.onmessage = (m) => {
            if (typeof m.data !== "string") return;
            const msg = JSON.parse(m.data);
            if (msg.t === "error" || msg.t === "attached") resolve(msg);
          };
          setTimeout(() => resolve({ t: "timeout" }), 5000);
        });
      },
      { id: h1id, b64: png.toString("base64") },
    );
    if (got.t !== "error" || got.code !== "forbidden") throw new Error(`reader 첨부: ${JSON.stringify(got)}`);
    await rd.page.context().close();
    for (const n of ["h1", "h2"]) aaa("pane", "kill", n);
  });

  await step("Agents 정렬(herdr-web-ui AgentSidebar `byActivity`·sidebarOrder.ts `activityOrder`, 차이 목록 L8): 기본은 머신 목록 차례, 설정 「에이전트 정렬」 활동순이면 질문이 먼저·그다음 최근 바뀐 agent, 일을 시키면 위로, 공유 설정이라 다른 기기도 같은 차례", async () => {
    // 차례대로 띄우고 상태마다 기다린다(상태가 바뀐 시각이 차례다): o1 쉼(입력을 받으면 작업 중) → o2 작업 중 → o3 질문
    const run = (label, script) => aaa("tab", "create", "--machine", "labA", "--label", label, "--", claudeSh, "-c", script);
    const status = (pg, name, value) => pg.getByTestId(`agent-${name}`).and(pg.locator(`[data-status="${value}"]`)).waitFor();
    run("o1", `stty -echo; ${claudeIdle}; read a; ${claudeWorking}; exec cat > /dev/null`);
    await status(page, "o1", "idle");
    run("o2", `stty -echo; ${claudeWorking}; exec cat > /dev/null`);
    await status(page, "o2", "working");
    run("o3", `stty -echo; ${claudeAsk("make o3")}; exec cat > /dev/null`);
    await status(page, "o3", "blocked");
    const order = (pg) =>
      pg.evaluate(() =>
        [...document.querySelectorAll('[data-testid="agents"] [data-testid^="agent-row-"]')].map((b) => b.dataset.testid.slice(10)).filter((n) => /^o\d$/.test(n)),
      );
    const orderIs = async (pg, what, want) => {
      let last = null;
      await until(what, async () => JSON.stringify((last = await order(pg))) === JSON.stringify(want)).catch((e) => {
        throw new Error(`${e.message}: ${JSON.stringify(last)}`);
      });
    };
    await orderIs(page, "기본은 머신 목록 차례", ["o1", "o2", "o3"]);
    const other = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    await orderIs(other.page, "다른 기기도 머신 목록 차례", ["o1", "o2", "o3"]);
    const d = await openSettings(page, "모양");
    await d.getByRole("group", { name: "에이전트 정렬" }).getByRole("button", { name: "활동순" }).click();
    await page.keyboard.press("Escape");
    await d.waitFor({ state: "detached" });
    await orderIs(page, "활동순: 질문, 그다음 최근", ["o3", "o2", "o1"]);
    // 쉬던 o1에 일을 시키면 질문 다음으로 오른다
    await inList(page, "pane-o1");
    await until("o1 화면", async () => (await screenText(page)).includes("? for shortcuts"));
    await page.getByTestId("terminal").click();
    await page.keyboard.press("Enter");
    await status(page, "o1", "working");
    await orderIs(page, "일을 시킨 o1이 위로", ["o3", "o1", "o2"]);
    // 공유 설정: 다른 기기도 활동순으로 같은 차례
    await orderIs(other.page, "다른 기기도 활동순", ["o3", "o1", "o2"]);
    await shot(page, "desktop-agents-activity");
    if (other.errors.length) throw new Error(other.errors.join("; "));
    await other.page.context().close();
    for (const n of ["o1", "o2", "o3"]) aaa("pane", "kill", n);
  });

  await step("계획 n/m·배경 작업 수(herdr-web-ui AgentSidebar `PlanBadge`·`BackgroundBadge`, 차이 목록 S5): agent가 Claude 기록의 TaskCreate·TaskUpdate와 배경 명령·배경 하위 agent를 읽어 Agents 줄에 보이고, 끝남 알림이 오면 배경 수가 준다", async () => {
    const dir = `${root}/claude/projects/-lab-plan`;
    fs.mkdirSync(dir, { recursive: true });
    const tr = `${dir}/p1.jsonl`;
    fs.writeFileSync(tr, "");
    aaa("tab", "create", "--machine", "labA", "--label", "plan1", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
      JSON.stringify({ hook_event_name: "SessionStart", session_id: "5d4c3b2a-1f0e-4d9c-8b7a-6f5e4d3c2b1a", source: "startup", transcript_path: tr }));
    // 배경 작업은 지금 Claude 프로세스(SessionStart startup) 뒤에 띄운 것만 센다: hook이 닿은 뒤에 기록을 쓴다
    await until("plan1 harness", () => aaa("pane", "list").panes.find((p) => p.name === "plan1")?.agent?.harness === "claude");
    const at = () => new Date().toISOString();
    const L = (o) => JSON.stringify(o) + "\n";
    const call = (id, name, input) => L({ type: "assistant", timestamp: at(), message: { content: [{ type: "tool_use", id, name, input }] } });
    const answer = (id, text, result) => L({ type: "user", timestamp: at(), toolUseResult: result, message: { content: [{ type: "tool_result", tool_use_id: id, content: text }] } });
    const created = (id, n, subject, activeForm) => call(id, "TaskCreate", { subject, activeForm }) + answer(id, `Task #${n} created successfully: ${subject}`, { task: { id: String(n) } });
    const updated = (id, n, status) => call(id, "TaskUpdate", { taskId: String(n), status }) + answer(id, "Updated", { success: true });
    const note = (task, tool) => `<task-notification>\n<task-id>${task}</task-id>\n<tool-use-id>${tool}</tool-use-id>\n<status>completed</status>\n<summary>끝</summary>\n</task-notification>`;
    fs.appendFileSync(
      tr,
      created("c1", 1, "읽기", "읽는 중") + created("c2", 2, "고치기", "고치는 중") + created("c3", 3, "시험", "시험하는 중") +
        updated("u1", 1, "completed") + updated("u2", 2, "in_progress") +
        call("b1", "Bash", { command: "cargo test", run_in_background: true }) + answer("b1", "Command running in background with ID: bg1", { backgroundTaskId: "bg1" }) +
        call("a1", "Agent", { description: "찾기", run_in_background: true }) + answer("a1", "Async agent launched successfully.", { status: "async_launched", agentId: "ag1" }),
    );
    const row = page.getByTestId("agent-row-plan1");
    const plan = row.getByTestId("plan-progress");
    const bg = row.getByTestId("background-tasks");
    const textIs = async (loc, what, want) => {
      let last = null;
      await until(what, async () => (last = (await loc.count()) ? await loc.innerText() : null) === want).catch((e) => {
        throw new Error(`${e.message}: ${JSON.stringify(last)}`);
      });
    };
    await textIs(plan, "계획 1/3", "1/3");
    await textIs(bg, "배경 둘", "2");
    if ((await plan.getAttribute("title")) !== "계획 3단계 중 1단계 끝남 · 고치는 중") throw new Error(`계획 title: ${await plan.getAttribute("title")}`);
    // CLI·TUI도 같은 값을 본다(PaneView.agent)
    const a = aaa("pane", "list").panes.find((p) => p.name === "plan1").agent;
    if (JSON.stringify([a.plan, a.background]) !== JSON.stringify([{ done: 1, total: 3, current: "고치는 중" }, 2])) throw new Error(JSON.stringify(a));
    await shot(page, "desktop-agents-plan");
    // 끝남 알림: 배경 명령은 queue-operation으로, 배경 하위 agent는 사람 쪽 글로 온다(같은 알림이 여러 자리에 실린다)
    fs.appendFileSync(tr, L({ type: "queue-operation", operation: "enqueue", timestamp: at(), content: note("bg1", "b1") }));
    await textIs(bg, "배경 하나", "1");
    fs.appendFileSync(tr, L({ type: "user", timestamp: at(), message: { role: "user", content: note("ag1", "a1") } }));
    await textIs(bg, "배경 없음", null);
    fs.appendFileSync(tr, updated("u3", 2, "completed") + updated("u4", 3, "completed"));
    await textIs(plan, "계획 3/3", "3/3");
    if (!(await plan.getAttribute("class")).includes("is-done")) throw new Error("다 끝난 계획이 is-done이 아니다");
    aaa("pane", "kill", "plan1");
  });

  await step("배경 작업을 기다리는 쉼(herdr-web-ui server/background-wait.ts·status.ts `paneStatus`, 차이 목록 S1): 사람 입력 뒤 턴이 배경 명령을 남긴 채 끝나면 끝남 대신 BG(배경 작업 대기), 그 일이 끝나도 Claude가 이어 갈 동안(유예)은 BG, 다음 턴이 끝나면 BG가 아니다", async () => {
    const dir = `${root}/claude/projects/-lab-bgwait`;
    fs.mkdirSync(dir, { recursive: true });
    const tr = `${dir}/w1.jsonl`;
    fs.writeFileSync(tr, "");
    const sid = "6e5d4c3b-2a1f-4e0d-9c8b-7a6f5e4d3c2b";
    const hook = (o) => JSON.stringify({ session_id: sid, transcript_path: tr, ...o });
    // 기록 위치는 SessionStart hook으로, 턴은 화면(일하는 화면·입력 상자)으로, 사람 입력은 UserPromptSubmit hook으로 알린다. 줄 하나에 턴을 열고 다음 줄에 닫는다
    const script = `"$0" hook --agent claude "$1"; stty -echo; ${claudeIdle}; while read a; do "$0" hook --agent claude "$2"; ${claudeWorking}; read b; ${claudeIdle}; done`;
    aaa("tab", "create", "--machine", "labA", "--label", "bgw1", "--", claudeSh, "-c", script, AA,
      hook({ hook_event_name: "SessionStart", source: "startup" }), hook({ hook_event_name: "UserPromptSubmit", prompt: "시험 돌려 줘" }));
    const state = page.getByTestId("agent-bgw1");
    const is = async (s, what) => {
      let last = null;
      await until(what, async () => (last = await state.getAttribute("data-status").catch(() => null)) === s).catch((e) => {
        throw new Error(`${e.message}: ${last}`);
      });
    };
    const agent = () => aaa("pane", "list").panes.find((p) => p.name === "bgw1")?.agent;
    await until("bgw1 harness", () => agent()?.harness === "claude");
    await is("idle", "처음은 쉼");
    // 그 pane을 보지 않는 화면이 받는 앱 안 알림(host `notice`): 참고 alertStatus대로 BG 동안은 끝남이 아니고, 기다림이 끝난 턴에
    // 한 번이다. 알림은 한 줄로 차례대로 오므로 마지막 끝남을 받았을 때 앞 턴의 끝남이 있었다면 이미 와 있다
    const bgId = aaa("pane", "list").panes.find((p) => p.name === "bgw1").id;
    const notices = [];
    const nw = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, {
      onPage: (p) => p.on("websocket", (ws) => ws.on("framereceived", (f) => {
        if (typeof f.payload !== "string" || !f.payload.includes('"ev":"notice"')) return;
        const n = JSON.parse(f.payload);
        if (n.pane === bgId) notices.push(n);
      })),
    });
    const t1 = Date.now();
    const at = () => new Date().toISOString();
    const L = (o) => JSON.stringify(o) + "\n";
    // 1) 사람 입력 → 턴이 배경 명령을 띄우고 끝난다: 끝남 대신 BG(CLI도 같은 값)
    aaa("pane", "run", "bgw1", "go");
    await is("working", "턴");
    fs.appendFileSync(tr, L({ type: "assistant", timestamp: at(), message: { content: [{ type: "tool_use", id: "b1", name: "Bash", input: { command: "cargo test", run_in_background: true } }] } }) +
      L({ type: "user", timestamp: at(), toolUseResult: { backgroundTaskId: "bg1" }, message: { content: [{ type: "tool_result", tool_use_id: "b1", content: "Command running in background with ID: bg1" }] } }));
    await until("배경 하나", () => agent()?.background === 1);
    aaa("pane", "run", "bgw1", "end");
    await is("waiting", "배경을 기다리는 쉼");
    const said = [(await state.innerText()).trim(), await state.getAttribute("title")];
    if (JSON.stringify(said) !== JSON.stringify(["배경 작업 대기", "agent가 배경 작업을 기다린다"])) throw new Error(`상태 말: ${JSON.stringify(said)}`);
    if (agent()?.background_wait !== true) throw new Error(`CLI background_wait: ${JSON.stringify(agent())}`);
    await shot(page, "desktop-agents-bg");
    // 2) 그 일이 끝났다(알림): Claude가 이어 갈 동안(유예)은 그대로 BG
    fs.appendFileSync(tr, L({ type: "queue-operation", operation: "enqueue", timestamp: at(), content: "<task-notification>\n<task-id>bg1</task-id>\n<tool-use-id>b1</tool-use-id>\n<status>completed</status>\n<summary>끝</summary>\n</task-notification>" }));
    await until("배경 없음", () => agent()?.background == null);
    await page.getByTestId("agent-row-bgw1").getByTestId("background-tasks").waitFor({ state: "detached" });
    await is("waiting", "일이 끝나도 유예 동안 BG");
    // 3) 다음 턴(작업 중)이 끝나면 BG가 아니다(이 화면이 그 pane을 보는지에 따라 끝남·쉼)
    const t2 = Date.now();
    aaa("pane", "run", "bgw1", "go");
    await is("working", "다음 턴");
    aaa("pane", "run", "bgw1", "end");
    await until("다음 턴이 끝나면 BG가 아니다", async () => ["done", "idle"].includes(await state.getAttribute("data-status")));
    if (agent()?.background_wait) throw new Error(`끝난 뒤 background_wait: ${JSON.stringify(agent())}`);
    // 끝남 알림은 둘째 턴이 끝날 때 하나, 그 턴은 첫 턴부터 BG를 지나 이어졌다
    await until("끝남 알림", () => notices.length > 0);
    const told = notices.map((n) => [n.kind, n.turn_ms === null ? null : n.turn_ms >= t2 - t1]);
    if (JSON.stringify(told) !== JSON.stringify([["finished", true]])) throw new Error(`BG 동안의 끝남 알림: ${JSON.stringify(notices)}`);
    if (nw.errors.length) throw new Error(nw.errors.join("; "));
    await nw.page.context().close();
    aaa("pane", "kill", "bgw1");
  });

  await step("처음 보기(herdr-web-ui settings `defaultView`·paneLens.ts, 차이 목록 C1): 자동이면 터치 화면의 agent pane은 대화, 데스크톱·셸은 터미널, pane에서 바꾼 보기는 그 pane에 남고, 설정 「패널 기본 화면」을 바꾸면 그것이 모두에게", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "lens1", "--", claudeSh, "-c", `stty -echo; ${claudeIdle}; exec cat > /dev/null`);
    await page.getByTestId("agent-lens1").and(page.locator('[data-status="idle"]')).waitFor();
    const ph = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    const pp = ph.page;
    const lensIs = async (pg, what, chat) => {
      await pg.getByTestId("chat-toggle").and(pg.locator(`[aria-pressed="${chat}"]`)).waitFor();
      if ((await pg.getByTestId("chat").count()) !== (chat ? 1 : 0)) throw new Error(`${what}: 대화 칸`);
    };
    // 다시 열기: 셸 t1을 거쳐 온다(휴대폰도 목록 drawer에서 고른다. drawer를 열어도 pane은 골라진 채다)
    const reopen = async (pg, name) => {
      await inList(pg, "pane-t1");
      await until(`${name} 떠나기`, async () => (await pg.getByTestId("chat-toggle").count()) === 0);
      await inList(pg, `pane-${name}`);
    };
    // 휴대폰(터치): agent pane은 대화, 셸은 터미널(대화 단추가 없다)
    await inList(pp, "pane-lens1");
    await lensIs(pp, "휴대폰 agent pane", true);
    await inList(pp, "pane-t1");
    await until("휴대폰 셸 터미널", async () => (await screenText(pp)).includes("$"));
    if ((await pp.getByTestId("chat").count()) !== 0) throw new Error("셸이 대화로 열렸다");
    // 데스크톱: 같은 agent pane이 터미널
    await inList(page, "pane-lens1");
    await lensIs(page, "데스크톱 agent pane", false);
    // 휴대폰에서 터미널로 바꾸면 그 pane은 다시 열어도 터미널
    await reopen(pp, "lens1");
    await lensIs(pp, "휴대폰 다시", true);
    await pp.getByTestId("chat-toggle").click();
    await lensIs(pp, "휴대폰 터미널로 바꿈", false);
    await reopen(pp, "lens1");
    await lensIs(pp, "고른 보기를 기억", false);
    // 설정 「패널 기본 화면」 채팅: pane마다 고른 보기를 잊고 모두 대화(이 기기만, 데스크톱은 그대로). 설정은 목록 바닥에 있다
    const d = await openSettings(pp, "채팅");
    await d.getByRole("group", { name: "패널 기본 화면" }).getByRole("button", { name: "채팅" }).click();
    await pp.keyboard.press("Escape");
    await d.waitFor({ state: "detached" });
    await inList(pp, "pane-lens1");
    await lensIs(pp, "기본 채팅", true);
    await shot(pp, "phone-chat-default");
    await reopen(page, "lens1");
    await lensIs(page, "데스크톱은 그대로", false);
    if (ph.errors.length) throw new Error(ph.errors.join("; "));
    await pp.context().close();
    aaa("pane", "kill", "lens1");
  });

  await step("대화 입력창(herdr-web-ui Composer·composer-queue, 차이 목록 C2): Enter로 보내면 터미널 입력줄과 같은 길로 프로그램에 가고, 「대기 메시지 자동 전송」을 끄면 작업 중에 쓴 글은 이 기기에 붙잡혀 「지금 보내기」로 가며, 빠른 답장은 누르면 간다", async () => {
    // 받은 줄을 파일에 적는 Claude 화면: 처음은 쉼, 한 줄을 받으면 작업 중으로 바뀌고 그 뒤 줄도 적는다
    const got = `${root}/composer.txt`;
    fs.writeFileSync(got, "");
    const lines = () => fs.readFileSync(got, "utf8").split("\n").filter(Boolean);
    aaa("tab", "create", "--machine", "labA", "--label", "cmp1", "--", claudeSh, "-c",
      `stty -echo; ${claudeIdle}; read a; printf '%s\\n' "$a" >> ${got}; ${claudeWorking}; while read a; do printf '%s\\n' "$a" >> ${got}; done`);
    await page.getByTestId("agent-cmp1").and(page.locator('[data-status="idle"]')).waitFor();
    await inList(page, "pane-cmp1");
    await page.getByTestId("chat-toggle").click();
    const composer = page.getByTestId("chat-composer");
    const box = composer.getByRole("textbox", { name: "메시지" });
    await box.fill("첫 메시지 c2");
    await box.press("Enter");
    await until("보낸 글이 프로그램에", async () => lines().includes("첫 메시지 c2"));
    if ((await box.inputValue()) !== "") throw new Error("보낸 뒤 입력창이 비지 않았다");
    await page.getByTestId("agent-cmp1").and(page.locator('[data-status="working"]')).waitFor();
    // 자동 전송을 끄면 작업 중에 쓴 글은 붙잡힌다
    const chatSettings = async (fn) => {
      const d = await openSettings(page, "채팅");
      await fn(d);
      await page.keyboard.press("Escape");
      await d.waitFor({ state: "detached" });
    };
    await chatSettings((d) => d.getByRole("switch", { name: "대기 메시지 자동 전송" }).click());
    await box.fill("붙잡을 글");
    await box.press("Enter");
    const queue = page.getByTestId("chat-queue");
    await queue.waitFor();
    if ((await queue.getByRole("textbox", { name: "대기 메시지 1" }).inputValue()) !== "붙잡을 글") throw new Error("붙잡은 글이 다르다");
    if (!(await queue.innerText()).includes("에이전트가 준비될 때까지 보관")) throw new Error(`붙잡은 칸 글: ${await queue.innerText()}`);
    if (lines().includes("붙잡을 글")) throw new Error("붙잡을 글이 프로그램에 갔다");
    await shot(page, "desktop-chat-composer-held");
    await queue.getByRole("button", { name: "지금 보내기" }).click();
    await until("지금 보내기로 간다", async () => lines().includes("붙잡을 글"));
    await queue.waitFor({ state: "detached" });
    // 빠른 답장: 켜면 입력창 위에 칩, 누르면 간다(작업 중이고 자동 전송이 꺼져 있어도 같은 규칙: 다시 켠다)
    await chatSettings(async (d) => {
      await d.getByRole("switch", { name: "대기 메시지 자동 전송" }).click();
      await d.getByRole("switch", { name: "입력창 위에 표시" }).click();
    });
    await composer.getByRole("group", { name: "빠른 답장" }).getByRole("button", { name: "yes", exact: true }).click();
    await until("빠른 답장이 간다", async () => lines().includes("yes"));
    await chatSettings((d) => d.getByRole("switch", { name: "입력창 위에 표시" }).click());
    // 보내지 않은 글은 pane마다 이 기기에 남는다(참고 composerDraft.ts): 다시 열어도(새 웹으로 다시 열기, plan §8.166) 그대로다
    await box.fill("남길 초안 c2");
    await page.reload();
    await until("다시 연 뒤 대화 입력창", () => box.isVisible());
    if ((await box.inputValue()) !== "남길 초안 c2") throw new Error(`다시 연 뒤 대화 초안: ${JSON.stringify(await box.inputValue())}`);
    await box.fill("");
    await page.getByTestId("chat-toggle").click();
    aaa("pane", "kill", "cmp1");
  });
  await step("Codex 폴더 신뢰: 데스크톱·휴대폰 카드가 실제 선택지에 답한다", async () => {
    const codexSh = `${root}/bin/codex`;
    fs.copyFileSync(claudeSh, codexSh);
    const fixture = `${repo}/crates/aaa-agent/src/detect/fixtures`;
    for (const phone of [false, true]) {
      const label = phone ? "codex-trust-phone" : "codex-trust-desktop";
      const script = `stty -echo; ${clearScreen}; cat '${fixture}/codex-trust.txt'; read a; ${clearScreen}; printf '%s\\n' 'codex-trust-accepted'; cat '${fixture}/codex-idle.txt'; exec cat > /dev/null`;
      const pane = aaa("tab", "create", "--machine", "labA", "--label", label, "--", codexSh, "-c", script).tabs.at(-1).root.pane;
      const device = phone ? await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }) : null;
      const pg = device?.page ?? page;
      await pg.getByTestId(`agent-${label}`).and(pg.locator('[data-status="blocked"]')).waitFor({ state: "attached" });
      await inList(pg, `pane-${label}`);
      const q = aaa("question", "list").questions.find((q) => q.pane === pane);
      if (q?.question_kind !== "trust") throw new Error(`신뢰 질문이 아니다: ${JSON.stringify(q)}`);
      const card = pg.getByTestId(`dock-question-${q.id}`);
      await card.waitFor();
      await shot(pg, label);
      // 읽은 화면 메뉴가 있으면 그 선택지, 예·아니오 카드면 같은 의미 답을 보낸다.
      await card.getByRole("button", { name: /^(예|.*Trust and continue.*)$/ }).click();
      await waitOutput(pane, "codex-trust-accepted", "recent");
      await card.waitFor({ state: "detached" });
      const answered = aaa("question", "list", "--all").questions.find((item) => item.id === q.id);
      if (answered?.state !== "answered") throw new Error(`답하지 못했다: ${JSON.stringify(answered)}`);
      if (device) {
        if (device.errors.length) throw new Error(device.errors.join("; "));
        await device.page.context().close();
      }
    }
  });

}
