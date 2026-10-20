// 대화 정리(compaction) 중 감지·보내기 보류·표시(사용자 10-20 「compacting 중인 걸 감지해서 이때 동안은 보내지 못하게 하고 대화창에서도
// compacting 중이란 걸 보여줘」, plan §8.304). 가짜 Claude가 실제 Claude 2.1.304 정리 화면(spinner 「Compacting conversation… (3s)」,
// 요약 글이 오기 전이라 참고 규칙으로는 입력 상자의 쉼)을 그린다. agent 감지가 「정리 중」을 알리면 웹은 상단 표지·대화 끝 줄을 보이고,
// 그동안 보낸 글을 pane에 치지 않고 이 기기 보관함에 붙잡았다가 정리가 끝나면 차례대로 보낸다. Claude 대기 글의 「지금 보내기」는 끈다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 41-compacting(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, aaa, AA, step, inList, until, domIs, waitOutput, shot, webkit, sentFrames, sentCount } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0" };

// 가짜 Claude: 시작할 때 hook으로 대화 기록 자리를 알리고 받은 bytes를 남긴다. F7은 작업 중, F5는 정리 시작, F6은 정리 끝(쉼)이다.
// 쉬는 동안 Enter 글은 기록에 사람 말과 답으로 적는다. 작업·정리 중 Enter 글은 Claude처럼 대기열(기록에 enqueue)이고, 정리가 끝나면
// 차례대로 꺼내 사람 말로 적는다. 빈 입력의 Ctrl+X Ctrl+S(sendNow)는 작업 중에만 대기열을 지금 넣고, 정리 중에는 하지 않는다
// (Claude 2.1.304 대기열은 정리 중 `wait`)
const program = String.raw`
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
const [aa, transcript, bytes, session] = process.argv.slice(2);
execFileSync(aa, ['hook', '--agent', 'claude', JSON.stringify({ hook_event_name: 'SessionStart', session_id: session, source: 'startup', transcript_path: transcript })]);
fs.writeFileSync(bytes, '');
let draft = '', mode = 'idle', queue = [], count = 0, ctrlX = false;
const write = (line) => fs.appendFileSync(transcript, JSON.stringify(line) + '\n');
const said = (text) => { write({ type: 'user', message: { role: 'user', content: text } }); write({ type: 'assistant', message: { content: [{ type: 'text', text: '받음: ' + text }] } }); };
const dequeue = () => { for (const q of queue) { write({ type: 'queue-operation', operation: 'dequeue' }); said(q); } queue = []; };
process.stdin.setRawMode(true);
function draw() {
  let s = 'received ' + count + ' mode ' + mode + '\r\n';
  for (const q of queue) s += '❯ ' + q + '\r\n  \x1b[2mctrl+x ctrl+s to send now\x1b[0m\r\n';
  if (mode === 'compacting') s += '\r\n✻ Compacting conversation… (3s)\r\n  ⎿  Tip: Name your conversations with /rename\r\n';
  if (mode === 'working') s += '\r\n✻ Working… (1s · esc to interrupt)\r\n';
  s += '\r\n────────────────────────\r\n❯ ' + (draft || (queue.length ? '\x1b[2mPress up to edit queued messages\x1b[0m' : '')) + '\r\n────────────────────────\r\n  ? for shortcuts\r\n';
  process.stdout.write('\x1b[2J\x1b[H' + s);
}
// 정리 끝은 실제 Claude 2.1.304 기록 차례대로 적는다: compact_boundary → 요약(isCompactSummary 사람 줄) → 대기열 꺼냄
const compacted = () => {
  write({ type: 'system', subtype: 'compact_boundary', content: 'Conversation compacted' });
  write({ type: 'user', isCompactSummary: true, message: { role: 'user', content: 'This session is being continued from a previous conversation. 요약: 앞 대화를 줄였다' } });
};
const KEYS = { '\x1b[15~': () => { mode = 'compacting'; }, '\x1b[17~': () => { compacted(); mode = 'idle'; dequeue(); }, '\x1b[18~': () => { mode = 'working'; } };
process.stdin.on('data', (data) => {
  fs.appendFileSync(bytes, data); count += data.length;
  let text = data.toString().replaceAll('\x1b[200~', '').replaceAll('\x1b[201~', '');
  for (const [key, act] of Object.entries(KEYS)) if (text.includes(key)) { act(); text = text.replaceAll(key, ''); }
  for (const c of text) {
    if (c === '\x18') { ctrlX = true; continue; }
    if (c === '\x13' && ctrlX && mode === 'working') {
      for (const q of queue) { write({ type: 'queue-operation', operation: 'remove', content: q, reason: 'absorbed_mid_turn' }); write({ type: 'attachment', attachment: { type: 'queued_command', commandMode: 'prompt', origin: { kind: 'human' }, prompt: q } }); }
      queue = [];
    } else if (c === '\r') {
      if (draft === '') continue;
      if (mode === 'idle') said(draft);
      else { write({ type: 'queue-operation', operation: 'enqueue', content: draft }); queue.push(draft); }
      draft = '';
    } else if (c >= ' ') draft += c;
    ctrlX = false;
  }
  draw();
});
draw();
`;

const LABEL = "compact1";
const directory = `${root}/compacting`;
const transcript = `${root}/claude/projects/-lab-${LABEL}/s1.jsonl`;
const bytes = () => fs.readFileSync(`${directory}/${LABEL}.bytes`, "utf8");
/** 기록에 적힌 사람 말(차례대로, 정리 요약 빼고) */
const said = () => fs.readFileSync(transcript, "utf8").trim().split("\n").map((l) => JSON.parse(l)).filter((r) => r.type === "user" && !r.isCompactSummary).map((r) => r.message.content);
/** 대화 보기의 줄: 사람 말은 글, 정리된 자리는 「정리됨」 */
const rows = (p) => p.evaluate(() => [...document.querySelectorAll('[data-role="user"], [data-role="compact"]')].map((el) => el.dataset.role === "compact" ? "정리됨" : el.textContent.trim()));

/** 상단 상태 표지(AgentChip)가 정리 중인가·대화 끝 줄이 있는가 */
const shows = (p) => p.evaluate(() => ({
  chip: document.querySelector('[data-testid="agent-chip"]')?.textContent.trim() ?? null,
  compacting: document.querySelector('[data-testid="agent-chip"]')?.dataset.compacting === "true",
  row: document.querySelector('[data-testid="chat-compacting"]')?.textContent.trim() ?? null,
}));

async function openChat(p) {
  await inList(p, `pane-${LABEL}`);
  if ((await p.getByTestId("chat").count()) === 0) {
    await until("대화 단추", async () => (await p.getByTestId("chat-toggle").count()) === 1);
    await p.getByTestId("chat-toggle").click();
  }
  await p.getByTestId("chat-composer").waitFor();
}

/** F키를 pane에 보낸다(CLI: 웹 보내기 밖의 길이라 보류와 상관없다). 가짜가 그린 상태 줄을 host 화면에서 기다린다 */
async function key(name, mode) {
  aaa("pane", "send-keys", LABEL, name);
  await waitOutput(LABEL, `mode ${mode}`);
}

export default async function ({ page }) {
  await step("정리 준비: 가짜 Claude(실제 정리 화면) pane과 대화 보기", async () => {
    fs.mkdirSync(directory, { recursive: true });
    fs.mkdirSync(`${root}/claude/projects/-lab-${LABEL}`, { recursive: true });
    fs.writeFileSync(transcript, "");
    fs.writeFileSync(`${directory}/claude`, `#!${process.execPath}\n${program}`, { mode: 0o755 });
    aaa("tab", "create", "--machine", "labA", "--label", LABEL, "--cwd", directory, "--", `${directory}/claude`, AA, transcript, `${directory}/${LABEL}.bytes`,
      "4a1c2e3f-5b6d-4c7e-8f90-a1b2c3d4e5f6");
    await waitOutput(LABEL, "received 0 mode idle");
    await openChat(page);
    await domIs(page, "쉼", () => document.querySelector('[data-testid="agent-chip"]')?.classList.contains("idle"));
  }, { always: true });

  await step("Claude 대기 글의 「지금 보내기」(plan §8.271)는 정리 중에 까닭과 함께 꺼지고(누르면 10초 경고만 뜨던 운영 사건 10-20), 정리가 끝나면 Claude가 읽어 대기 말이 사라진다", async () => {
    await key("f7", "working");
    await domIs(page, "작업 중", () => document.querySelector('[data-testid="agent-chip"]')?.classList.contains("working"));
    const box = page.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" });
    await box.fill("작업 중 추가");
    await box.press("Enter");
    const steer = page.getByTestId("chat-pending-steer");
    await steer.waitFor();
    assert.equal(await steer.isDisabled(), false, "작업 중 대기 글은 지금 보낼 수 있다");
    await shot(page, "41-before-compacting-desktop");
    await key("f5", "compacting");
    await domIs(page, "정리 중 표지", () => document.querySelector('[data-testid="agent-chip"]')?.dataset.compacting === "true");
    assert.deepEqual(await shows(page), { chip: "정리 중", compacting: true, row: "대화 정리 중…" });
    await domIs(page, "대기 글 단추가 꺼짐", () => document.querySelector('[data-testid="chat-pending-steer"]')?.disabled === true);
    assert.match(await page.getByTestId("chat-pending-note").textContent(), /대화 정리가 끝나면 대기 글이 이어서 읽힙니다/);
    assert.equal(await steer.getAttribute("title"), "대화 정리가 끝나면 대기 글이 이어서 읽힙니다");
    await shot(page, "41-compacting-claude-queue-desktop");
    await key("f6", "idle");
    await domIs(page, "정리 끝: 대기 말이 사람 말로", () => !document.querySelector('[data-testid="chat-pending"]') && !document.querySelector('[data-testid="chat-compacting"]')
      && [...document.querySelectorAll('[data-role="user"]')].some((el) => el.textContent.trim() === "작업 중 추가"));
    assert.equal((await shows(page)).compacting, false);
  });

  await step("정리 중 보낸 글(Enter·Ctrl+Enter)은 pane에 치지 않고 이 기기 보관함에 붙잡혀 「정리가 끝나면 차례대로 보냅니다」이고, 휴대폰·패드도 같은 표지를 본다. 정리가 끝나면 보낸 차례대로 간다", async () => {
    await key("f5", "compacting");
    await domIs(page, "정리 중 표지", () => document.querySelector('[data-testid="agent-chip"]')?.dataset.compacting === "true");
    const before = bytes();
    const from = await sentCount(page);
    const box = page.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" });
    await box.fill("정리 뒤 첫째");
    await box.press("Enter");
    await box.fill("정리 뒤 둘째");
    await box.press("Control+Enter");
    const queue = page.getByTestId("chat-queue");
    await domIs(page, "보관함 둘", () => document.querySelectorAll('[data-testid="chat-queue"] li[data-after="compaction"]').length === 2);
    assert.match(await queue.textContent(), /대화 정리가 끝나면 차례대로 보냅니다/);
    for (const send of await queue.getByRole("button", { name: "지금 보내기" }).all()) assert.equal(await send.isDisabled(), true, "정리 중에는 지금 보내지 않는다");
    // 보내지 않았다: 이 화면이 pane WebSocket으로 submit을 보내지 않았고(보관은 그 사건 안에서 끝난다) pane도 받지 않았다
    assert.deepEqual((await sentFrames(page, from)).filter((f) => f.t === "submit" || f.t === "input"), []);
    assert.equal(bytes(), before);
    await shot(page, "41-compacting-held-desktop");

    // 휴대폰(iPhone Safari)·패드: host 상태 표지는 모든 기기가 같다
    const wk = await webkit.launch();
    try {
      for (const profile of ["iphone", "ipad"]) {
        const { page: p } = await pairDevice(wk, aaa("web", "pair").code, profile);
        await openChat(p);
        await domIs(p, `${profile} 정리 중`, () => document.querySelector('[data-testid="chat-compacting"]') !== null);
        const seen = await shows(p);
        assert.equal(seen.row, "대화 정리 중…", profile);
        if (seen.chip !== null) assert.equal(seen.chip, "정리 중", profile);
        await shot(p, `41-compacting-${profile}`);
        await p.context().close();
      }
    } finally {
      await wk.close();
    }

    await key("f6", "idle");
    await until("보낸 차례대로 기록", () => {
      const list = said();
      return list.includes("정리 뒤 둘째") && list;
    });
    const list = said();
    assert.ok(list.indexOf("정리 뒤 첫째") < list.indexOf("정리 뒤 둘째"), JSON.stringify(list));
    await domIs(page, "보관함이 비고 정리 줄이 사라짐", () => !document.querySelector('[data-testid="chat-queue"]') && !document.querySelector('[data-testid="chat-compacting"]'));
    assert.ok(bytes().slice(before.length).includes("정리 뒤 첫째"), "정리가 끝난 뒤 pane이 받았다");
    // 끝난 정리는 그 자리에 「대화 정리됨」 구분선으로 남고(herdr-web-ui chat-compact), 누르면 요약을 펼친다
    await domIs(page, "정리된 자리 둘", () => document.querySelectorAll('[data-role="compact"]').length === 2);
    assert.deepEqual(await rows(page), ["정리됨", "작업 중 추가", "정리됨", "정리 뒤 첫째", "정리 뒤 둘째"]);
    const divider = page.locator('[data-role="compact"]').last();
    assert.equal((await divider.locator("summary").textContent()).trim(), "대화 정리됨");
    await divider.locator("summary").click();
    await domIs(page, "요약 펼침", () => [...document.querySelectorAll('[data-role="compact"]')].at(-1)?.open === true);
    assert.match(await divider.locator(".chat-compact-text").textContent(), /앞 대화를 줄였다/);
    await shot(page, "41-after-compacting-desktop");
  });
}
