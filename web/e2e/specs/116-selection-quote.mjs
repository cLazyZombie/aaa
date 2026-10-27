// 고른 글을 출처와 함께 thread 입력 초안에 인용한다(t-1204, 참고 t3code 답 Cite·herdr-web-ui 터미널 Quote in message).
// 실제 host·agent·PTY의 thread pane(fake profile에 Claude hook을 걸어 대화 보기가 된다)에서
// 대화 답을 끌어 고르면 「메시지에 인용」이 뜨고, 누르면 쓰던 초안 뒤에 「인용: t-NNNN 대화 · Claude 답」과 `>` 줄이 붙는다.
// 터미널에서 끌어 고른 줄은 입력줄이 없으면 대화 입력창 초안에, 입력줄이 있으면 입력줄 초안에 「인용: t-NNNN 터미널 a~b줄」과
// 울타리로 붙는다. 보내지 않는다. 휴대폰(WebKit 390)은 고른 글 단추를 눌러도 입력창 focus(화면 키보드)를 가져가지 않는다.
// 완료는 DOM 사건·PTY 출력으로 보며 정한 시간을 자지 않는다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { aaa, step, browser, webkit, pairPage, domIs, root, AA, waitOutput, shot, sentFrames } from "../lib/harness.mjs";

const L = (o) => JSON.stringify(o) + "\n";
const ANSWER = "인용할 첫 문단이다.";
const TERMINAL = "```\nQUOTE-A 첫 줄\nQUOTE-B 둘째 줄\n```\n";

/** thread pane을 Claude 대화 pane으로 만들고 터미널에 고를 두 줄을 찍는다(fake.sh `post-probe`) */
async function quoteThread() {
  aaa("project", "create", "quote");
  fs.writeFileSync(`${root}/quote-task.md`, "인용 fixture");
  const thread = aaa("thread", "start", "quote", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--title", "인용", "--task-file", `${root}/quote-task.md`);
  await waitOutput(thread.pane, "fake>");
  const dir = `${root}/claude/projects/-lab-quote`;
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(`${dir}/s1.jsonl`, L({ type: "user", message: { role: "user", content: "앞 질문" } })
    + L({ type: "assistant", message: { content: [{ type: "text", text: `${ANSWER}\n\n둘째 문단은 고르지 않는다.` }] } }));
  const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "7f6e5d4c-3b2a-4190-8f7e-6d5c4b3a2918", source: "startup", transcript_path: `${dir}/s1.jsonl` });
  fs.writeFileSync(`${dir}/hook.sh`, `'${AA}' hook --agent claude '${hook}'\nprintf 'QUOTE-A 첫 줄\\nQUOTE-B 둘째 줄\\n'\n`);
  aaa("pane", "run", thread.pane, `post-probe ${dir}/hook.sh`);
  await waitOutput(thread.pane, "QUOTE-B 둘째 줄");
  return thread;
}

/** 보기를 고른다(대화 단추의 aria-pressed가 지금 보기) */
async function view(p, chat) {
  const toggle = p.getByTestId("chat-toggle");
  await toggle.waitFor();
  if ((await toggle.getAttribute("aria-pressed") === "true") !== chat) await toggle.click();
  if (chat) await p.getByTestId("chat-composer").waitFor();
  else await p.getByTestId("terminal").waitFor();
}

/** 대화 답 첫 문단 글자의 화면 범위 */
const answerBox = (p) => p.evaluate((text) => {
  const para = [...document.querySelectorAll('[data-testid="chat"] .chat-msg.assistant p')].find((el) => el.textContent === text);
  const range = document.createRange();
  range.selectNodeContents(para);
  const r = range.getBoundingClientRect();
  return { left: r.left, right: r.right, y: r.top + r.height / 2 };
}, ANSWER);

/** QUOTE-A 줄 처음에서 QUOTE-B 줄 글자 끝까지 끈다(xterm DOM 줄) */
async function dragTerminal(p) {
  const rows = await p.evaluate(() => {
    const lines = [...document.querySelectorAll('[data-testid="terminal"] .xterm-rows > div')];
    const box = (match) => {
      const row = lines.find((el) => el.textContent.startsWith(match));
      const range = document.createRange();
      range.selectNodeContents(row);
      const r = range.getBoundingClientRect();
      return { left: r.left, right: r.right, y: r.top + r.height / 2 };
    };
    return { a: box("QUOTE-A"), b: box("QUOTE-B") };
  });
  await p.mouse.move(rows.a.left + 1, rows.a.y);
  await p.mouse.down();
  await p.mouse.move(rows.b.right - 1, rows.b.y, { steps: 6 });
  await p.mouse.up();
}

export default async function () {
  const thread = await quoteThread();
  const hash = `#pane=${encodeURIComponent(thread.pane)}`;
  const chatHead = `인용: ${thread.id} 대화 · Claude 답\n> ${ANSWER}\n`;
  const terminalHead = new RegExp(`인용: ${thread.id} 터미널 (\\d+)~(\\d+)줄\\n`);

  await step("인용(t-1204): Chromium 1280 대화 답을 끌어 고르면 단추가 뜨고 쓰던 초안 뒤에 출처·> 줄로 붙는다. 바로 입력 터미널에서 끈 두 줄은 대화 입력창 초안에 줄·울타리로 붙고, 보내지 않는다", async () => {
    const { page: p, errors } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash });
    try {
      await view(p, true);
      const box = p.getByRole("textbox", { name: "메시지", exact: true });
      await p.locator('[data-testid="chat"] .chat-msg.assistant').getByText(ANSWER, { exact: true }).waitFor();
      await box.fill("쓰던 요청");
      const a = await answerBox(p);
      await p.mouse.move(a.left + 1, a.y);
      await p.mouse.down();
      await p.mouse.move(a.right - 1, a.y, { steps: 6 });
      // 끄는 동안(누른 채)은 띄우지 않는다
      assert.equal(await p.getByTestId("chat-quote").count(), 0, "누른 채 단추가 떴다");
      await p.mouse.up();
      await p.getByTestId("chat-quote").waitFor();
      await shot(p, "116-selection-quote-chat");
      await p.getByTestId("chat-quote").click();
      await domIs(p, "인용이 붙은 초안", (want) => document.querySelector('[aria-label="메시지"]')?.value === want, `쓰던 요청\n\n${chatHead}`);
      await domIs(p, "단추·고른 글이 사라지고 입력창 focus", () => !document.querySelector('[data-testid="chat-quote"]')
        && getSelection().isCollapsed && document.activeElement?.getAttribute("aria-label") === "메시지");

      await view(p, false);
      await dragTerminal(p);
      await p.getByTestId("terminal-quote").click();
      await p.getByTestId("clipboard-note").getByText("메시지 초안에 넣었다", { exact: true }).waitFor();
      await domIs(p, "터미널 인용 단추가 사라짐", () => !document.querySelector('[data-testid="terminal-quote"]'));
      await view(p, true);
      const draft = await box.inputValue();
      assert.ok(draft.startsWith(`쓰던 요청\n\n${chatHead}\n`), draft);
      const tail = draft.slice(`쓰던 요청\n\n${chatHead}\n`.length);
      const [head, start, end] = tail.match(terminalHead) ?? [];
      assert.ok(head && tail === `${head}${TERMINAL}`, tail);
      assert.equal(Number(end), Number(start) + 1, tail);
      assert.ok(!JSON.stringify(await sentFrames(p)).includes("QUOTE-A"), "인용을 PTY로 보냈다");
      assert.deepEqual(errors, []);
    } finally { await p.context().close(); }
  });

  await step("인용(t-1204): Chromium 1280 입력줄 터미널에서 끈 두 줄은 입력줄 초안의 쓰던 글 뒤에 붙고 입력줄로 focus가 간다", async () => {
    const { page: p, errors } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash, line: true });
    try {
      await view(p, false);
      const line = p.locator(".terminal-input-text");
      await line.fill("입력줄 요청");
      await dragTerminal(p);
      await p.getByTestId("terminal-quote").click();
      await domIs(p, "입력줄 focus·단추 사라짐", () => document.activeElement?.classList.contains("terminal-input-text")
        && !document.querySelector('[data-testid="terminal-quote"]'));
      const value = await line.inputValue();
      const [head, start, end] = value.match(terminalHead) ?? [];
      assert.ok(head && value === `입력줄 요청\n\n${head}${TERMINAL}`, value);
      assert.equal(Number(end), Number(start) + 1, value);
      assert.ok(!JSON.stringify(await sentFrames(p)).includes("QUOTE-A"), "인용을 PTY로 보냈다");
      assert.deepEqual(errors, []);
    } finally { await p.context().close(); }
  });

  await step("인용(t-1204): WebKit 390 휴대폰에서 대화 글을 고르고 단추를 누르면 빈 초안에 붙고 화면 키보드(입력창 focus)를 올리지 않는다", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      const { page: p, errors } = await pairPage(safari, aaa("web", "pair").code, { width: 390, height: 844 }, { hash });
      await view(p, true);
      await p.locator('[data-testid="chat"] .chat-msg.assistant').getByText(ANSWER, { exact: true }).waitFor();
      await p.evaluate((text) => {
        const para = [...document.querySelectorAll('[data-testid="chat"] .chat-msg.assistant p')].find((el) => el.textContent === text);
        getSelection().selectAllChildren(para);
      }, ANSWER);
      await p.getByTestId("chat-quote").waitFor();
      await shot(p, "116-selection-quote-phone");
      await p.getByTestId("chat-quote").tap();
      await domIs(p, "인용이 붙은 빈 초안", (want) => document.querySelector('[aria-label="메시지"]')?.value === want, chatHead);
      await domIs(p, "단추 사라짐·입력창 focus 없음", () => !document.querySelector('[data-testid="chat-quote"]')
        && document.activeElement?.getAttribute("aria-label") !== "메시지");
      assert.deepEqual(errors, []);
    } finally { await safari.close(); }
  });
}
