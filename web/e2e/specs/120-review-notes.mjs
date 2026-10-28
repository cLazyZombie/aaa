// 보고 글·diff 줄 의견과 인용 위치로 돌아가기(t-1222, 참고 Orca diff·Markdown 의견 전달, t3code Cite 위치 복원).
// 실제 host·agent·PTY의 git 저장소 thread(fake profile에 Claude hook을 걸어 대화 보기가 된다)에서:
// thread 화면 보고 글을 고르면 「의견」, diff 줄 번호를 누르면 그 줄 아래 「의견」이 뜨고, 단 의견은 의견 모음에 쌓인다.
// 의견 모음의 「위치」는 그 줄을 다시 고르고, 「메시지에 넣기」는 pane 입력 초안에 모두 붙이고 모음을 비운다(보내지 않는다).
// 보낸 말 속 인용(보고·diff·대화·터미널)의 「돌아가기」는 그 자리를 다시 연다. 대화·터미널에서 고른 글에도 「의견」을 단다.
// 휴대폰(WebKit 390)은 보고 글 의견 단추를 tap으로 누른다. 완료는 DOM 사건·PTY 출력으로 보며 정한 시간을 자지 않는다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { aaa, step, browser, webkit, pairPage, domIs, root, AA, until, waitOutput, shot, sentFrames } from "../lib/harness.mjs";

const L = (o) => JSON.stringify(o) + "\n";
const ANSWER = "인용할 첫 문단이다.";
const git = (dir, ...a) =>
  execFileSync("git", ["-C", dir, "-c", "user.name=e2e", "-c", "user.email=e2e@example.com", ...a], { encoding: "utf8" }).trim();

/** main에 커밋한 저장소의 thread: branch에서 src/lib.rs 2줄을 바꾸고, pane을 Claude 대화 pane으로 만들고 터미널에 고를 줄을 찍는다 */
async function reviewThread() {
  const repo = `${root}/repos/notes`;
  fs.mkdirSync(`${repo}/src`, { recursive: true });
  git(repo, "init", "-q", "-b", "main");
  fs.writeFileSync(`${repo}/src/lib.rs`, "fn a() -> u32 { 1 }\nfn b() -> u32 { 2 }\nfn c() -> u32 { 3 }\n");
  git(repo, "add", "-A");
  git(repo, "commit", "-qm", "first");
  aaa("project", "create", "notes");
  fs.writeFileSync(`${root}/notes-task.md`, "의견 fixture");
  const thread = aaa("thread", "start", "notes", "--title", "의견", "--machine", "labA", "--profile", "fake", "--repo", repo, "--task-file", `${root}/notes-task.md`);
  await until("보고 회수", () => aaa("thread", "show", "notes", thread.id).report_id);
  const shown = aaa("thread", "show", "notes", thread.id);
  fs.writeFileSync(`${shown.cwd}/src/lib.rs`, "fn a() -> u32 { 1 }\nfn b() -> u32 { 22 }\nfn c() -> u32 { 3 }\n");
  git(shown.cwd, "commit", "-qam", "branch change");
  const sha = git(shown.cwd, "merge-base", "HEAD", "main").slice(0, 8);
  return { id: thread.id, pane: thread.pane, report: shown.report_id, sha };
}

/** 대화 기록(앞 질문·답, 네 인용을 보낸 사람 말)과 hook. 터미널에는 고를 두 줄을 찍는다 */
async function chatPane(t) {
  const dir = `${root}/claude/projects/-lab-notes`;
  fs.mkdirSync(dir, { recursive: true });
  const sent = [
    `인용: ${t.id} 보고 ${t.report}`, "> 결과 확인", "의견: 보고 의견", "",
    `인용: ${t.id} diff main 대비 ${t.sha} · src/lib.rs 새 2줄`, "```diff", "+fn b() -> u32 { 22 }", "```", "",
    `인용: ${t.id} 대화 · Claude 답`, `> ${ANSWER}`, "",
    `인용: ${t.id} 터미널 1줄`, "```", "QUOTE-A 첫 줄", "```",
  ].join("\n");
  fs.writeFileSync(`${dir}/s1.jsonl`, L({ type: "user", message: { role: "user", content: "앞 질문" } })
    + L({ type: "assistant", message: { content: [{ type: "text", text: `${ANSWER}\n\n둘째 문단은 고르지 않는다.` }] } })
    + L({ type: "user", message: { role: "user", content: sent } }));
  const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "5a4b3c2d-1e0f-4a9b-8c7d-6e5f4a3b2c1d", source: "startup", transcript_path: `${dir}/s1.jsonl` });
  fs.writeFileSync(`${dir}/hook.sh`, `'${AA}' hook --agent claude '${hook}'\nprintf 'QUOTE-A 첫 줄\\nQUOTE-B 둘째 줄\\n'\n`);
  aaa("pane", "run", t.pane, `post-probe ${dir}/hook.sh`);
  await waitOutput(t.pane, "QUOTE-B 둘째 줄");
}

/** 보기를 고른다(대화 단추의 aria-pressed가 지금 보기) */
async function view(p, chat) {
  const toggle = p.getByTestId("chat-toggle");
  await toggle.waitFor();
  if ((await toggle.getAttribute("aria-pressed") === "true") !== chat) await toggle.click();
  if (chat) await p.getByTestId("chat-composer").waitFor();
  else await p.getByTestId("terminal").waitFor();
}

/** thread 화면 보고 글에서 「결과 확인」 목록 줄을 고른다 */
const pickReport = (p) => p.evaluate(() => {
  const li = [...document.querySelectorAll('[data-testid="thread-view"] [data-testid="report"] .report-markdown li')].find((el) => el.textContent === "결과 확인");
  getSelection().selectAllChildren(li);
});

/** diff 칸 src/lib.rs의 새 2줄이 고른 줄인가(줄 번호 칸은 shadow root 안) */
const line2Selected = (p) => domIs(p, "diff 새 2줄을 고름", () => [...document.querySelectorAll('[data-testid="diff-panel"] diffs-container')]
  .some((c) => c.shadowRoot?.querySelector('[data-line-type="change-addition"][data-column-number="2"][data-selected-line]')));

export default async function () {
  const t = await reviewThread();
  await chatPane(t);
  const threadHash = `#thread=${encodeURIComponent(t.id)}`;
  const reportBlock = `인용: ${t.id} 보고 ${t.report}\n> 결과 확인\n의견: 보고 의견`;
  const diffBlock = `인용: ${t.id} diff main 대비 ${t.sha} · src/lib.rs 새 2줄\n\`\`\`diff\n+fn b() -> u32 { 22 }\n\`\`\`\n의견: 줄 의견`;

  await step("의견(t-1222): Chromium 1280 보고 글·diff 줄에 의견을 달면 의견 모음에 쌓이고, 「위치」는 그 줄을 다시 고르고, 「메시지에 넣기」는 pane 초안에 붙이고 모음을 비운다", async () => {
    const { page: p, errors } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash: threadHash });
    try {
      await p.getByTestId("report").locator(".report-markdown li", { hasText: "결과 확인" }).waitFor();
      await pickReport(p);
      await p.getByTestId("report-note").click();
      await p.getByTestId("quote-note").getByRole("textbox", { name: "의견" }).fill("보고 의견");
      await p.getByTestId("quote-note-submit").click();
      const tray = p.getByTestId("review-notes");
      await tray.getByTestId("review-note").first().waitFor();
      assert.equal(await tray.locator(".review-note-head code").first().textContent(), `보고 ${t.report}`);
      await domIs(p, "의견 단 글을 칠함", () => CSS.highlights.get("aaa-review-note")?.size === 1);

      const card = p.getByTestId("changed-files");
      await card.waitFor();
      if (await card.locator('[data-path="src/lib.rs"]').count() === 0) await card.getByRole("button", { name: "모든 폴더 펼치기" }).click();
      await card.locator('[data-path="src/lib.rs"]').click();
      await domIs(p, "diff 칸 읽음", () => document.querySelector('[data-testid="diff-panel"]')?.dataset.diffStatus === "ready");
      const diff = p.locator("diffs-container").filter({ has: p.getByRole("button", { name: "접기 src/lib.rs", exact: true }) });
      await diff.locator('[data-line-type="change-addition"][data-column-number="2"]').click();
      await p.getByTestId("diff-note").click();
      await p.getByTestId("diff-panel").getByRole("textbox", { name: "의견" }).fill("줄 의견");
      await p.getByTestId("diff-panel").getByTestId("quote-note-submit").click();
      await p.getByTestId("diff-line-note").getByText("줄 의견", { exact: true }).waitFor();
      await domIs(p, "의견 둘", () => document.querySelectorAll('[data-testid="review-note"]').length === 2);
      await shot(p, "120-review-notes");

      // 고른 줄을 풀고 의견 모음의 「위치」로 다시 고른다
      await p.evaluate(() => getSelection().removeAllRanges());
      await tray.getByRole("button", { name: "src/lib.rs 새 2줄 위치로" }).click();
      await line2Selected(p);

      await tray.getByTestId("review-notes-send").click();
      await p.getByTestId("terminal").waitFor();
      await view(p, true);
      await domIs(p, "의견이 붙은 초안", (want) => document.querySelector('[aria-label="메시지"]')?.value === want, `${reportBlock}\n\n${diffBlock}\n`);
      assert.equal(await p.evaluate((k) => localStorage.getItem(k), `aaa:review-notes:notes/${t.id}`), null, "넣은 의견이 모음에 남았다");
      assert.ok(!JSON.stringify(await sentFrames(p)).includes("보고 의견"), "의견을 PTY로 보냈다");
      assert.deepEqual(errors, []);
    } finally { await p.context().close(); }
  });

  await step("돌아가기(t-1222): Chromium 1280 보낸 말 속 대화·터미널·보고·diff 인용의 「돌아가기」가 그 자리를 다시 고르고, 대화에서 고른 글의 「의견」은 인용 아래 의견 줄로 붙는다", async () => {
    const { page: p, errors } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash: `#pane=${encodeURIComponent(t.pane)}` });
    try {
      await view(p, true);
      const returns = p.getByTestId("chat").getByTestId("chat-quote-return");
      await returns.first().waitFor();
      assert.deepEqual(await returns.allTextContents(), [`보고 ${t.report}`, "lib.rs 새 2줄", "대화", "터미널 1줄"]);

      await returns.nth(2).click();
      await domIs(p, "찾기 칸이 인용 첫 줄로 열리고 답의 그 글을 고름", (want) => {
        const active = CSS.highlights.get("aaa-chat-find-active");
        const range = active && [...active][0];
        return document.querySelector('[data-testid="chat-find"] input')?.value === want
          && range?.toString() === want && range.startContainer.parentElement?.closest(".chat-msg.assistant") !== null;
      }, ANSWER);
      await p.keyboard.press("Escape");

      // 대화에서 고른 글에 의견
      await p.evaluate((text) => {
        const para = [...document.querySelectorAll('[data-testid="chat"] .chat-msg.assistant p')].find((el) => el.textContent === text);
        getSelection().selectAllChildren(para);
      }, ANSWER);
      await p.getByTestId("chat-quote-note").click();
      await p.getByTestId("quote-note").getByRole("textbox", { name: "의견" }).fill("이 말 다시");
      await p.getByTestId("quote-note-submit").click();
      await domIs(p, "인용과 의견이 붙은 초안", (want) => document.querySelector('[aria-label="메시지"]')?.value === want,
        `인용: ${t.id} 대화 · Claude 답\n> ${ANSWER}\n의견: 이 말 다시\n`);

      await returns.nth(3).click();
      await p.getByTestId("terminal").waitFor();
      await p.getByTestId("terminal-quote").waitFor();

      await view(p, true);
      await returns.nth(0).click();
      await p.getByTestId("thread-view").waitFor();
      await domIs(p, "보고 글의 그 글을 고름", () => getSelection().toString().trim() === "결과 확인"
        && document.querySelector('[data-testid="report"]')?.contains(getSelection().anchorNode));

      await p.evaluate((h) => (location.hash = h), `#pane=${encodeURIComponent(t.pane)}`);
      await view(p, true);
      await returns.nth(1).click();
      await domIs(p, "diff 칸 읽음", () => document.querySelector('[data-testid="diff-panel"]')?.dataset.diffStatus === "ready");
      await line2Selected(p);
      assert.equal(await p.getByTestId("diff-focus-moved").count(), 0, "같은 줄인데 바뀌었다고 했다");
      assert.deepEqual(errors, []);
    } finally { await p.context().close(); }
  });

  await step("의견(t-1222): WebKit 390 휴대폰에서 보고 글을 고르고 「의견」·「의견 추가」를 tap하면 의견 모음에 쌓인다", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      const { page: p, errors } = await pairPage(safari, aaa("web", "pair").code, { width: 390, height: 844 }, { hash: threadHash });
      await p.getByTestId("report").locator(".report-markdown li", { hasText: "결과 확인" }).waitFor();
      await pickReport(p);
      await p.getByTestId("report-note").tap();
      await p.getByTestId("quote-note").getByRole("textbox", { name: "의견" }).fill("휴대폰 의견");
      await p.getByTestId("quote-note-submit").tap();
      await p.getByTestId("review-notes").getByText("휴대폰 의견", { exact: true }).waitFor();
      await shot(p, "120-review-notes-phone");
      assert.deepEqual(errors, []);
    } finally { await safari.close(); }
  });
}
