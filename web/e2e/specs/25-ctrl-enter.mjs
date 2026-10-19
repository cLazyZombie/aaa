// Ctrl+Enter: host의 같은 감지 판정을 웹 하드웨어·터치 키가 사용한다. TUI는 PaneCtrlEnter 요청을 공유한다.
// steering만 하고 줄바꿈은 넣지 않는다(사용자 10-19 「스티어링만 남기고 싶어」, plan §8.239 후속). 줄바꿈은 Shift+Enter다.
// agent가 작업 중일 때만 동작한다(사용자 10-19 15:1x 정정): 쉬는 Claude·Codex는 글이 있어도 보내지 않는다.
import assert from "node:assert/strict";
import fs from "node:fs";
import {
  root, aaa, waitOutput, domIs, shot, pairPage, step, browser, inList, sentCount, sentFrames,
} from "../lib/harness.mjs";

export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0" };

export default async function ({ page }) {
  const directory = `${root}/ctrl-enter`;
  const received = (name) => fs.readFileSync(`${directory}/${name}.bytes`);
  // 가짜 harness는 친 글을 입력 상자에 그리고, 받은 bytes를 기록한 뒤 화면에 누적 길이를 알린다.
  // 출력 표지를 wait-output으로 기다려 transport와 host의 화면 사본이 모두 처리된 뒤 검증한다.
  const program = String.raw`
import fs from 'node:fs';
// agent가 앞에 붙이는 Codex 옵션과 fixture 인자를 --로 구분한다.
const separator = process.argv.indexOf('--', 2);
if (separator < 0) throw new Error('fixture arguments need --');
const [mode, file] = process.argv.slice(separator + 1);
let draft = '', count = 0;
fs.writeFileSync(file, '');
process.stdin.setRawMode(true);
function draw() {
  let screen = 'received ' + count + '\r\n';
  if (mode.startsWith('pending')) screen += '• Working (2s • esc to interrupt)\r\n• Messages to be submitted after next tool call (press esc to interrupt and send immediately)\r\n  ↳ change course\r\n›\r\n\r\n  GPT-6.1 low · /workspace\r\n  ? for shortcuts\r\n';
  else if (mode === 'unknown') screen += '✻ Working… (1s · esc to interrupt)\r\ninput not visible\r\n';
  // Codex 입력 줄(›, 화면 셋째 줄 또는 Working 줄 아래 둘째 줄)에 친 글을 그리고 커서를 글 끝에 둔다
  else if (mode === 'working' || mode === 'idle') screen += (mode === 'working' ? '• Working (2s • esc to interrupt)\r\n' : '') + '› ' + draft + '\r\n\r\n  GPT-6.1 low · /workspace\r\n  ? for shortcuts\r\n';
  // Claude 입력 상자. work-로 시작하면 그 위에 작업 상태 줄(감지가 작업 중으로 본다)
  else screen += (mode.startsWith('work') ? '✻ Working… (1s · esc to interrupt)\r\n\r\n' : '') + '────────────────────────\r\n❯ ' + (mode.endsWith('grey') ? '\x1b[2mTry something\x1b[0m' : draft) + '\r\n────────────────────────\r\n  ? for shortcuts\r\n';
  const at = mode === 'pending' ? '\x1b[5;3H' : mode === 'working' ? '\x1b[3;' + (3 + draft.length) + 'H' : mode === 'idle' ? '\x1b[2;' + (3 + draft.length) + 'H' : '';
  process.stdout.write('\x1b[2J\x1b[H' + screen + at);
}
process.stdout.write('\x1b[>4;2m');
process.stdin.on('data', data => {
  fs.appendFileSync(file, data); count += data.length;
  if (![...data].some(c => c < 32 || c === 127)) draft += data.toString();
  draw();
});
draw();
`;
  await step("Ctrl+Enter 준비: Claude·Codex 감지 화면과 입력 bytes 기록", async () => {
    fs.mkdirSync(directory);
    for (const harness of ["claude", "codex"]) fs.writeFileSync(`${directory}/${harness}`, `#!${process.execPath}\n${program}`, { mode: 0o755 });
    // [이름, harness, 화면, 감지가 볼 상태(작업 중 / 쉼)]
    for (const [name, harness, mode, working] of [
      ["ce-empty", "claude", "work-empty", true], ["ce-text", "claude", "work-empty", true],
      ["ce-grey", "claude", "work-grey", true], ["ce-idle", "claude", "empty", false], ["ce-unknown", "claude", "unknown", true],
      ["ce-codex", "codex", "pending", true], ["ce-codex-away", "codex", "pending-away", true],
      ["ce-codex-typed", "codex", "working", true], ["ce-codex-idle", "codex", "idle", false],
    ]) {
      aaa("tab", "create", "--machine", "labA", "--label", name, "--cwd", directory, "--", `${directory}/${harness}`, "--", mode, `${directory}/${name}.bytes`);
      await waitOutput(name, "received 0");
      await page.getByTestId(`agent-${name}`).waitFor({ state: "attached" });
      assert.equal(aaa("pane", "list").panes.find(p => p.name === name)?.agent?.harness, harness);
      // 판정은 host가 아는 agent 상태를 본다: 감지가 그 상태를 알린 뒤에 시험한다(쉼은 「쉼」이거나 「끝남」)
      await domIs(page, `${name} 상태`, ([name, working]) => {
        const status = document.querySelector(`[data-testid="agent-${name}"]`)?.dataset.status;
        return working ? status === "working" : status === "idle" || status === "done";
      }, [name, working]);
    }
  }, { always: true });

  const open = async (pg, name) => {
    await inList(pg, `pane-${name}`);
    const toggle = pg.getByTestId("chat-toggle");
    if (await toggle.getAttribute("aria-pressed") === "true") await toggle.click();
    await domIs(pg, "터미널에 현재 pane 출력", () => document.querySelector('.xterm-rows')?.textContent.includes('received'));
    await pg.getByTestId("terminal").click();
  };
  const expectKey = async (pg, name, act, expected) => {
    const before = received(name);
    await act();
    await waitOutput(name, `received ${before.length + Buffer.byteLength(expected)}`);
    assert.equal(received(name).subarray(before.length).toString(), expected);
  };
  // 보내지 않았다: 같은 pane WebSocket의 뒤따르는 글자 하나만 닿았는지로 본다(host가 그 연결의 요청을 차례로 처리한다)
  const expectNothing = async (pg, name, act) => {
    const before = received(name);
    await act();
    await pg.keyboard.type("z");
    await waitOutput(name, `received ${before.length + 1}`);
    assert.equal(received(name).subarray(before.length).toString(), "z");
  };
  await step("Claude Ctrl+Enter: 작업 중이면 글이 있든 비었든 sendNow(Ctrl+X Ctrl+S), 회색 추천문도 같고, 쉬는 중(글이 있어도)·입력 상자를 모르면 보내지 않는다(줄바꿈 없음)", async () => {
    await open(page, "ce-text");
    await page.keyboard.type("draft");
    await waitOutput("ce-text", "❯ draft");
    await expectKey(page, "ce-text", () => page.keyboard.press("Control+Enter"), "\x18\x13");
    await shot(page, "ctrl-enter-draft-desktop");
    for (const name of ["ce-empty", "ce-grey"]) {
      await open(page, name);
      await expectKey(page, name, () => page.keyboard.press("Control+Enter"), "\x18\x13");
    }
    await open(page, "ce-idle");
    await expectNothing(page, "ce-idle", () => page.keyboard.press("Control+Enter"));
    await page.keyboard.type("draft");
    await waitOutput("ce-idle", "❯ zdraft");
    await expectNothing(page, "ce-idle", () => page.keyboard.press("Control+Enter"));
    await open(page, "ce-unknown");
    await expectNothing(page, "ce-unknown", () => page.keyboard.press("Control+Enter"));
    await shot(page, "ctrl-enter-unknown-desktop");
  });
  await step("Codex Ctrl+Enter: 대기 steering 패널의 빈 입력은 Esc, 작업 중 친 글은 Enter(steer), 쉬는 중·커서가 입력 밖이면 보내지 않는다", async () => {
    await open(page, "ce-codex");
    await expectKey(page, "ce-codex", () => page.keyboard.press("Control+Enter"), "\x1b");
    const sent = await sentCount(page);
    await page.locator('.xterm-helper-textarea').evaluate(el => el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', keyCode: 13, ctrlKey: true, repeat: true, bubbles: true })));
    assert.equal((await sentFrames(page, sent)).filter(f => ['input', 'ctrl_enter'].includes(f.t)).length, 0);
    await open(page, "ce-codex-away");
    await expectNothing(page, "ce-codex-away", () => page.keyboard.press("Control+Enter"));
    for (const [name, expected] of [["ce-codex-typed", "\r"], ["ce-codex-idle", null]]) {
      await open(page, name);
      await page.keyboard.type("steer");
      await waitOutput(name, "› steer");
      if (expected) await expectKey(page, name, () => page.keyboard.press("Control+Enter"), expected);
      else await expectNothing(page, name, () => page.keyboard.press("Control+Enter"));
    }
  });
  await step("터치 Ctrl+Enter도 같은 판정: Ctrl 래치+Enter, 입력줄 초안은 보내지도 줄을 바꾸지도 않고 알리며, IME 확정은 steering하지 않음", async () => {
    const mobile = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true, line: true })).page;
    await open(mobile, "ce-empty");
    const bar = mobile.locator('.key-bar');
    await bar.locator('[data-key="Control"]').click();
    await expectKey(mobile, "ce-empty", () => bar.locator('[data-key="Enter"]').click(), "\x18\x13");
    await bar.locator('[data-key="Control"]').click();
    const line = mobile.getByLabel("보낼 글");
    await line.fill("휴대폰 초안");
    const sent = await sentCount(mobile);
    await line.press("Control+Enter");
    await mobile.locator(".terminal-input-note", { hasText: "줄바꿈은 Shift+Enter" }).waitFor();
    assert.equal(await line.inputValue(), "휴대폰 초안");
    assert.equal((await sentFrames(mobile, sent)).filter(f => ['input', 'submit', 'ctrl_enter'].includes(f.t)).length, 0);
    await line.fill("");
    await expectKey(mobile, "ce-empty", () => line.press("Control+Enter"), "\x18\x13");
    const start = await sentCount(mobile);
    await line.evaluate(el => el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, isComposing: true, keyCode: 229, bubbles: true })));
    assert.equal((await sentFrames(mobile, start)).filter(f => ['input', 'submit', 'ctrl_enter'].includes(f.t)).length, 0);
    await shot(mobile, "ctrl-enter-phone");
    await mobile.context().close();
  });
}
