// 읽던 자리 지키기(사용자 10-19 16:5x 「내가 위를 보고 있으면 거기에 멈춰 있게 해줘. 제일 아래로 스크롤 한 상태에서만 계속 스크롤이
// 되게」·「입력 창을 두 줄 이상으로 했을 경우 입력줄에 의해 지금 창이 가려지면 안 돼. 스크롤은 입력창 위에서만」, plan §8.262).
// 대화 보기: agent는 기록을 끝 150개로 자른다. 위를 읽는 동안 새 말이 붙어 앞이 잘려도, 입력창이 커져도 보던 줄은 한 픽셀도 움직이지
// 않고(t3code timelineScrollAnchoring처럼 보던 줄 id + 거리), 맨 아래에 붙어 있을 때만 따라간다.
// 웹 터미널 입력줄: 여러 줄로 커지면 터미널 칸이 그 위에서 끝나고(겹침 없음) PTY 크기는 그대로, 칸 안에서 커서 줄로 굴린다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 29-scroll-hold(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.
import fs from "node:fs";
import { root, aaa, AA, step, inList, until, domIs, waitOutput, shot, pairPage, browser } from "../lib/harness.mjs";

const L = (o) => JSON.stringify(o) + "\n";
const user = (text) => L({ type: "user", message: { role: "user", content: text } });
const answer = (text) => L({ type: "assistant", message: { content: [{ type: "text", text }] } });

/** 대화 칸: 끝인가, 위 끝에 걸친 줄의 글과 그 줄 위에서 잰 거리, 입력창과 겹친 높이 */
const view = (p) =>
  p.evaluate(() => {
    const el = document.querySelector('[data-testid="chat"]');
    const box = el.getBoundingClientRect();
    const rows = [...el.querySelectorAll(":scope > .chat-transcript > :not(.chat-note):not(.banner)")];
    const row = rows.findLast((r) => r.getBoundingClientRect().top <= box.top) ?? rows[0];
    const composer = document.querySelector('[data-testid="chat-composer"]').getBoundingClientRect();
    return {
      atEnd: el.scrollHeight - el.scrollTop - el.clientHeight <= 40,
      row: row?.textContent.slice(0, 16),
      offset: Math.round((box.top - row.getBoundingClientRect().top) * 10) / 10,
      overlap: Math.max(0, Math.round(box.bottom - composer.top)),
    };
  });
/** 웹 터미널 칸과 입력줄: 겹친 높이, 입력줄 높이, prompt 줄이 칸 안(입력줄 위)에 보이는가 */
const termView = (p) =>
  p.evaluate(() => {
    const scroll = document.querySelector(".term-scroll").getBoundingClientRect();
    const input = document.querySelector(".terminal-input").getBoundingClientRect();
    const prompt = [...document.querySelectorAll(".xterm-rows > div")].find((r) => r.textContent.includes("prompt>"))?.getBoundingClientRect();
    return {
      overlap: Math.max(0, Math.round(scroll.bottom - input.top)),
      inputHeight: Math.round(input.height),
      promptVisible: prompt !== undefined && prompt.top >= scroll.top - 1 && prompt.bottom <= Math.min(scroll.bottom, input.top) + 1,
    };
  });
const frame = (p) => p.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

export default async function ({ page }) {
  await step("대화 위를 읽는 동안(기록 150개로 잘림): 새 말이 붙어 앞이 잘려도, 입력창이 여러 줄로 커져도 보던 줄이 그 거리 그대로이고 입력창은 대화를 덮지 않는다. 맨 아래에 붙어 있으면 새 말을 따라간다", async () => {
    const dir = `${root}/claude/projects/-lab-hold1`;
    fs.mkdirSync(dir, { recursive: true });
    const transcript = `${dir}/s1.jsonl`;
    let body = "";
    for (let i = 0; i < 160; i++) body += user(`질문 ${i}`) + answer(`답 ${i}\n\n${"긴 줄 ".repeat(30 + (i % 7) * 5)}`);
    fs.writeFileSync(transcript, body);
    aaa("tab", "create", "--machine", "labA", "--label", "hold1", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
      JSON.stringify({ hook_event_name: "SessionStart", session_id: "9c3d4e5f-6a7b-4c8d-9e0f-1a2b3c4d5e6f", source: "startup", transcript_path: transcript }));
    await inList(page, "pane-hold1");
    const toggle = page.getByTestId("chat-toggle");
    await until("대화 단추", async () => (await toggle.count()) === 1);
    if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
    await until("잘린 대화 기록", async () => (await page.locator('[data-testid="chat"] .chat-msg').count()) >= 140);
    const chat = page.getByTestId("chat");
    await chat.evaluate((el) => (el.scrollTop = Math.round(el.scrollHeight / 2)));
    await frame(page);
    const reading = await view(page);
    if (reading.atEnd) throw new Error("가운데를 읽는 중이어야 한다");

    // 새 말 둘이 붙고 앞 둘이 잘린다
    fs.appendFileSync(transcript, user("새 질문") + answer("새 답"));
    await domIs(page, "새 답이 그려짐", () => [...document.querySelectorAll('[data-testid="chat"] .chat-msg')].some((m) => m.textContent.includes("새 답")));
    await frame(page);
    const afterAppend = await view(page);
    if (afterAppend.row !== reading.row || Math.abs(afterAppend.offset - reading.offset) > 1)
      throw new Error(`새 말 뒤 보던 줄이 움직였다: ${JSON.stringify(reading)} → ${JSON.stringify(afterAppend)}`);

    // 입력창이 여러 줄로 커진다: 대화 칸만 줄고 보던 줄은 그대로, 겹침 없음
    const box = page.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" });
    await box.fill("한 줄\n두 줄\n세 줄\n네 줄\n다섯 줄\n여섯 줄");
    await frame(page);
    const tall = await view(page);
    if (tall.row !== reading.row || Math.abs(tall.offset - reading.offset) > 1 || tall.overlap > 0)
      throw new Error(`입력창이 커진 뒤: ${JSON.stringify(reading)} → ${JSON.stringify(tall)}`);
    await shot(page, "scroll-hold-chat-tall-composer");
    await box.fill("");

    // 맨 아래에 붙어 있으면 따라간다
    await chat.evaluate((el) => (el.scrollTop = el.scrollHeight));
    await frame(page);
    fs.appendFileSync(transcript, user("끝 질문") + answer("끝 답"));
    await domIs(page, "끝 답을 따라 끝", () => {
      const el = document.querySelector('[data-testid="chat"]');
      const last = [...el.querySelectorAll(".chat-msg")].at(-1);
      return last?.textContent.includes("끝 답") && el.scrollHeight - el.scrollTop - el.clientHeight <= 40;
    });
    await toggle.click();
    aaa("pane", "kill", "hold1");
  });

  await step("웹 터미널 입력줄이 여러 줄로 커지면 터미널 칸은 입력줄 위에서 끝나고(겹침 없음) PTY 크기는 그대로, 커서 줄이 칸 안에 보인다. 한 줄로 돌아오면 칸도 돌아온다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "hold2", "--", "/bin/sh", "-c", "i=1; while [ $i -le 80 ]; do echo line-$i; i=$((i+1)); done; printf 'prompt> '; exec cat");
    await waitOutput("hold2", "prompt>");
    await inList(page, "pane-hold2");
    await domIs(page, "터미널에 prompt", () => document.querySelector(".xterm-rows")?.textContent.includes("prompt>"));
    const size = () => {
      const p = aaa("pane", "list").panes.find((x) => x.name === "hold2");
      return `${p.cols}x${p.rows}`;
    };
    const before = size();
    const measure = () => termView(page);
    const one = await measure();
    const box = page.getByLabel("보낼 글");
    await box.fill("한 줄\n두 줄\n세 줄\n네 줄");
    await until("입력줄이 커짐", async () => (await measure()).inputHeight > one.inputHeight + 20);
    await domIs(page, "맞추기 판단이 끝남", () => !document.querySelector('[data-testid="terminal"]')?.dataset.fitPending);
    await frame(page);
    const tall = await measure();
    if (tall.overlap > 0 || !tall.promptVisible) throw new Error(`입력줄이 커진 뒤: ${JSON.stringify(one)} → ${JSON.stringify(tall)}`);
    if (size() !== before) throw new Error(`PTY 크기가 바뀌었다: ${before} → ${size()}`);
    await shot(page, "scroll-hold-terminal-tall-input");
    await box.fill("");
    await until("입력줄이 한 줄로", async () => (await measure()).inputHeight === one.inputHeight);
    await domIs(page, "맞추기 판단이 끝남", () => !document.querySelector('[data-testid="terminal"]')?.dataset.fitPending);
    const back = await measure();
    if (back.overlap > 0 || !back.promptVisible || size() !== before) throw new Error(`한 줄로 돌아온 뒤: ${JSON.stringify(back)} ${size()}`);
    aaa("pane", "kill", "hold2");
  });

  await step("휴대폰 폭(390px)에서도 대화 입력창·터미널 입력줄이 여러 줄로 커지면 보기는 그 위에서 끝난다(겹침 없음). 기록의 붙여넣기 꼬리표는 걷고 안의 글만 보인다", async () => {
    const dir = `${root}/claude/projects/-lab-hold3`;
    fs.mkdirSync(dir, { recursive: true });
    const transcript = `${dir}/s1.jsonl`;
    let body = "";
    for (let i = 0; i < 20; i++) body += user(`질문 ${i}`) + answer(`답 ${i}\n\n${"긴 줄 ".repeat(30)}`);
    // 웹 입력창 글은 Claude Code가 기록에 붙여넣기 꼬리표로 감싸 남긴다: 대화 보기는 안의 글만 보인다(pastedContent.ts)
    body += user('<pasted_content id="7f06">\n붙여 넣은 글\n둘째 줄\n</pasted_content id="7f06">');
    fs.writeFileSync(transcript, body);
    aaa("tab", "create", "--machine", "labA", "--label", "hold3", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
      JSON.stringify({ hook_event_name: "SessionStart", session_id: "0d4e5f6a-7b8c-4d9e-8f0a-1b2c3d4e5f6a", source: "startup", transcript_path: transcript }));
    aaa("tab", "create", "--machine", "labA", "--label", "hold4", "--", "/bin/sh", "-c", "i=1; while [ $i -le 80 ]; do echo line-$i; i=$((i+1)); done; printf 'prompt> '; exec cat");
    await waitOutput("hold4", "prompt>");
    const mobile = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true, line: true })).page;
    await inList(mobile, "pane-hold3");
    const toggle = mobile.getByTestId("chat-toggle");
    await until("대화 단추", async () => (await toggle.count()) === 1);
    if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
    await until("대화 기록", async () => (await mobile.locator('[data-testid="chat"] .chat-msg').count()) >= 40);
    const pasted = (await mobile.locator('[data-testid="chat"] .chat-msg.user').last().innerText()).trim();
    if (pasted !== "붙여 넣은 글\n둘째 줄") throw new Error(`붙여넣기 꼬리표가 보인다: ${JSON.stringify(pasted)}`);
    await mobile.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" }).fill("한 줄\n두 줄\n세 줄\n네 줄\n다섯 줄");
    await frame(mobile);
    const chat = await view(mobile);
    if (chat.overlap > 0) throw new Error(`휴대폰 대화 입력창이 대화를 덮는다: ${JSON.stringify(chat)}`);
    await shot(mobile, "scroll-hold-chat-phone");
    await inList(mobile, "pane-hold4");
    await domIs(mobile, "휴대폰 터미널에 prompt", () => document.querySelector(".xterm-rows")?.textContent.includes("prompt>"));
    const one = await termView(mobile);
    await mobile.getByLabel("보낼 글").fill("한 줄\n두 줄\n세 줄\n네 줄");
    await until("휴대폰 입력줄이 커짐", async () => (await termView(mobile)).inputHeight > one.inputHeight + 20);
    await domIs(mobile, "맞추기 판단이 끝남", () => !document.querySelector('[data-testid="terminal"]')?.dataset.fitPending);
    await frame(mobile);
    const tall = await termView(mobile);
    if (tall.overlap > 0 || !tall.promptVisible) throw new Error(`휴대폰 입력줄이 커진 뒤: ${JSON.stringify(one)} → ${JSON.stringify(tall)}`);
    await shot(mobile, "scroll-hold-terminal-phone");
    await mobile.getByLabel("보낼 글").fill("");
    await mobile.context().close();
    aaa("pane", "kill", "hold3");
    aaa("pane", "kill", "hold4");
  });
}
