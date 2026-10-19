// 대화 보기 읽기·보내기 다듬기(t-0403, plan §8.266, t3code 참고): 「맨 아래로」 단추(ChatView 「Scroll to end」, 150ms 뒤 보이고 바로
// 숨음), thread(pane)마다 읽던 자리 기억(timelineScrollAnchoring·MessagesTimeline: 끝을 따라가던 곳은 끝, 아니면 기억한 줄의 그 거리),
// 입력 보낸 뒤 맨 아래(사용자 결정 §8.250), 보내는 글의 보이지 않는 문자 빼기(promptLinks.ts), 작업 중 빈 입력창의 중단 단추(Esc,
// ComposerPrimaryActions), 빈 입력창 ↑·↓로 보낸 글 다시 부르기(composerPromptHistory.ts).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 30-chat-reading(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, aaa, AA, step, inList, until, domIs, waitOutput, shot } from "../lib/harness.mjs";

export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0" };

// 가짜 Claude: 시작할 때 hook으로 대화 기록 자리를 알리고, 받은 bytes를 남기며 입력 상자에 친 글을 그린다. Enter면 그 글을 기록에
// 사람 말로 적고 작업 중 화면이 되고, Esc면 입력 상자로 돌아온다(중단)
const program = String.raw`
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
const [aa, transcript, bytes, session] = process.argv.slice(2);
execFileSync(aa, ['hook', '--agent', 'claude', JSON.stringify({ hook_event_name: 'SessionStart', session_id: session, source: 'startup', transcript_path: transcript })]);
fs.writeFileSync(bytes, '');
let draft = '', working = false, count = 0;
process.stdin.setRawMode(true);
function draw() {
  const top = 'received ' + count + '\r\n';
  process.stdout.write('\x1b[2J\x1b[H' + top + (working ? '✻ Working… (1s · esc to interrupt)\r\n' : '────────────────────────\r\n❯ ' + draft + '\r\n────────────────────────\r\n  ? for shortcuts\r\n'));
}
process.stdin.on('data', (data) => {
  fs.appendFileSync(bytes, data); count += data.length;
  for (const c of data.toString()) {
    if (c === '\x1b') working = false;
    else if (c === '\r') {
      if (draft !== '') fs.appendFileSync(transcript, JSON.stringify({ type: 'user', message: { role: 'user', content: draft } }) + '\n');
      draft = ''; working = true;
    } else if (c >= ' ') draft += c;
  }
  draw();
});
draw();
`;

const directory = `${root}/chat-reading`;
/** 30턴 기록을 가진 가짜 Claude pane */
function readingPane(label, session) {
  // 기록은 실험실 Claude 기록 폴더 안이어야 agent가 읽는다(22-chat-surface와 같은 자리)
  const dir = `${root}/claude/projects/-lab-${label}`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/s1.jsonl`;
  const L = (o) => JSON.stringify(o) + "\n";
  let body = "";
  for (let i = 0; i < 30; i++)
    body += L({ type: "user", message: { role: "user", content: `질문 ${i}` } }) + L({ type: "assistant", message: { content: [{ type: "text", text: `답 ${i}\n\n${"긴 줄 ".repeat(40)}` }] } });
  fs.writeFileSync(transcript, body);
  aaa("tab", "create", "--machine", "labA", "--label", label, "--cwd", directory, "--", `${directory}/claude`, AA, transcript, `${directory}/${label}.bytes`, session);
}

/** 대화 칸의 굴림: 끝인가(t3code 40px), 위 끝에 걸친 줄의 글과 그 줄 위에서 잰 거리, 단추가 있는가 */
const view = (p) =>
  p.evaluate(() => {
    const el = document.querySelector('[data-testid="chat"]');
    const top = el.getBoundingClientRect().top;
    const rows = [...el.querySelectorAll(":scope > .chat-transcript > :not(.chat-note):not(.banner)")];
    const row = rows.findLast((r) => r.getBoundingClientRect().top <= top) ?? rows[0];
    return {
      atEnd: el.scrollHeight - el.scrollTop - el.clientHeight <= 40,
      scrollTop: el.scrollTop,
      row: row?.textContent.slice(0, 12),
      offset: Math.round(top - row.getBoundingClientRect().top),
      jump: document.querySelector('[data-testid="chat-jump"]') !== null,
    };
  });

async function openChat(p, label, count = 60) {
  await inList(p, `pane-${label}`);
  if ((await p.getByTestId("chat").count()) === 0) {
    await until("대화 단추", async () => (await p.getByTestId("chat-toggle").count()) === 1);
    await p.getByTestId("chat-toggle").click();
  }
  await p.getByTestId("chat-composer").waitFor();
  await until("대화 기록", async () => (await p.locator('[data-testid="chat"] .chat-msg').count()) >= count);
}

export default async function ({ page }) {
  await step("대화 「맨 아래로」와 thread마다 읽던 자리(t3code ChatView·timelineScrollAnchoring): 위로 굴리면 단추가 뜨고, 다른 pane에 갔다 오면 같은 줄의 같은 거리로 돌아오며 단추가 바로 있다. 단추를 누르면 끝으로 가 숨고, 그 뒤 오가면 끝이다. 처음 보는 pane은 끝이다", async () => {
    fs.mkdirSync(directory);
    fs.writeFileSync(`${directory}/claude`, `#!${process.execPath}\n${program}`, { mode: 0o755 });
    readingPane("cr1", "6f1e2d3c-4b5a-4987-8a6b-5c4d3e2f1a01");
    readingPane("cr2", "6f1e2d3c-4b5a-4987-8a6b-5c4d3e2f1a02");
    await waitOutput("cr1", "received 0");
    await waitOutput("cr2", "received 0");
    await openChat(page, "cr1");
    await domIs(page, "처음은 끝, 단추 없음", () => {
      const el = document.querySelector('[data-testid="chat"]');
      return el.scrollHeight - el.scrollTop - el.clientHeight <= 40 && !document.querySelector('[data-testid="chat-jump"]');
    });
    await page.getByTestId("chat").evaluate((el) => (el.scrollTop = Math.round(el.scrollHeight / 3)));
    await page.getByTestId("chat-jump").waitFor();
    const left = await view(page);
    assert.equal(left.atEnd, false);
    await shot(page, "chat-jump-desktop");

    await openChat(page, "cr2");
    await domIs(page, "처음 보는 pane은 끝", () => {
      const el = document.querySelector('[data-testid="chat"]');
      return el.scrollHeight - el.scrollTop - el.clientHeight <= 40 && !document.querySelector('[data-testid="chat-jump"]');
    });
    await openChat(page, "cr1");
    await until("읽던 줄의 같은 거리로", async () => {
      const back = await view(page);
      return back.jump && back.row === left.row && Math.abs(back.offset - left.offset) <= 2;
    }).catch(async (e) => {
      throw new Error(`${e.message}: 떠날 때 ${JSON.stringify(left)}, 지금 ${JSON.stringify(await view(page))}`);
    });

    await page.getByTestId("chat-jump").click();
    await domIs(page, "단추를 누르면 끝, 단추 숨음", () => {
      const el = document.querySelector('[data-testid="chat"]');
      return el.scrollHeight - el.scrollTop - el.clientHeight <= 40 && !document.querySelector('[data-testid="chat-jump"]');
    });
    await openChat(page, "cr2");
    await openChat(page, "cr1");
    await domIs(page, "끝에서 떠난 pane은 끝으로 돌아온다", () => {
      const el = document.querySelector('[data-testid="chat"]');
      return el.scrollHeight - el.scrollTop - el.clientHeight <= 40 && !document.querySelector('[data-testid="chat-jump"]');
    });
  });

  await step("대화 보내기(t3code promptLinks·ComposerPrimaryActions·composerPromptHistory): 위로 굴려 읽다가 보내도 끝으로 가고(§8.250), 보이지 않는 문자는 빼고 보내며 몇 개 뺐는지 알린다. 작업 중 빈 입력창은 보내기 자리가 중단(Esc)이다. 빈 입력창 ↑는 보낸 글을 새 것부터, ↓는 앞으로, 가장 새 것 너머는 빈 칸이고, 쓰던 글에서는 부르지 않는다", async () => {
    const received = () => fs.readFileSync(`${directory}/cr1.bytes`);
    const box = page.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" });
    // 처음 자동으로 끝에 간 scroll 사건보다 먼저 위로 이동한다. DOM commit의 관찰 사건으로 순서를 고정한다:
    // 끝 위치를 scroll 사건에서만 기억하면 합쳐진 첫 사건(0)에서 위로 간 것을 놓친다.
    await openChat(page, "cr2");
    await page.evaluate(() => {
      const previous = document.querySelector('[data-testid="chat"]');
      window.__earlyChatScroll = null;
      const observer = new MutationObserver(() => {
        const el = document.querySelector('[data-testid="chat"]');
        if (!el || el === previous || el.querySelectorAll('.chat-msg').length < 60 || el.scrollTop === 0) return;
        observer.disconnect();
        const before = el.scrollTop;
        el.addEventListener('scroll', () => { window.__earlyChatScroll = { before, after: el.scrollTop }; }, { once: true });
        el.scrollTop = 0;
      });
      observer.observe(document.body, { childList: true, subtree: true });
    });
    await openChat(page, "cr1");
    await page.waitForFunction(() => window.__earlyChatScroll !== null);
    const earlyScroll = await page.evaluate(() => window.__earlyChatScroll);
    console.log(`chat-early-scroll ${JSON.stringify(earlyScroll)}`);
    assert(earlyScroll.before > 0 && earlyScroll.after === 0, "첫 자동 굴림 사건 전에 위로 이동했다");
    await page.getByTestId("chat-jump").waitFor();
    const before = received().length;
    await box.fill("보낼​글⁠");
    await box.press("Enter");
    const sent = "보낼글\r";
    await waitOutput("cr1", `received ${before + Buffer.byteLength(sent)}`);
    assert.equal(received().subarray(before).toString(), sent);
    await domIs(page, "보낸 뒤 끝, 단추 숨음, 보낸 글이 기록에", () => {
      const el = document.querySelector('[data-testid="chat"]');
      const users = [...el.querySelectorAll('.chat-msg.user')].map((m) => m.textContent);
      return el.scrollHeight - el.scrollTop - el.clientHeight <= 40 && !document.querySelector('[data-testid="chat-jump"]') && users.at(-1)?.includes("보낼글");
    });
    const note = await page.getByTestId("chat-note").innerText();
    if (!note.includes("보이지 않는 문자 2개를 빼고 보냈습니다")) throw new Error(`숨은 문자 안내: ${note}`);

    // 작업 중 빈 입력창: 중단 단추가 Esc를 보내고 agent가 쉬면 사라진다
    await page.getByTestId("agent-cr1").and(page.locator('[data-status="working"]')).waitFor();
    const interrupt = page.getByTestId("chat-interrupt");
    await interrupt.waitFor();
    await shot(page, "chat-interrupt-desktop");
    const atStop = received().length;
    await interrupt.click();
    await waitOutput("cr1", `received ${atStop + 1}`);
    assert.equal(received().subarray(atStop).toString(), "\x1b");
    await interrupt.waitFor({ state: "detached" });
    await box.fill("x");
    if (await interrupt.count()) throw new Error("글이 있으면 중단 단추가 아니다");
    await box.fill("");

    // ↑·↓ 다시 부르기
    await box.focus();
    const recall = async (key, want) => {
      await box.press(key);
      await until(`${key} → 「${want}」`, async () => (await box.inputValue()) === want);
    };
    await recall("ArrowUp", "보낼글");
    await recall("ArrowUp", "질문 29");
    await recall("ArrowDown", "보낼글");
    await recall("ArrowDown", "");
    await box.fill("쓰던 글");
    await box.press("ArrowUp");
    assert.equal(await box.inputValue(), "쓰던 글");
    await box.fill("");
    aaa("pane", "kill", "cr1");
    aaa("pane", "kill", "cr2");
  });
}
