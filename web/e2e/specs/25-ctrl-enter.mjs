// Ctrl+Enter: host의 같은 감지 판정을 웹 하드웨어·터치 키가 사용한다. TUI는 PaneCtrlEnter 요청을 공유한다.
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
// 실제 Codex가 앞에 --no-daemon, 뒤에 -c를 더하므로 fixture 인자는 --로 구분한다.
const separator = process.argv.indexOf('--');
if (separator < 0) throw new Error('fixture 인자 구분 누락');
const [mode, file] = process.argv.slice(separator + 1);
let draft = '', count = 0;
fs.writeFileSync(file, '');
process.stdin.setRawMode(true);
function draw() {
  let screen = 'received ' + count + '\r\n';
  if (mode.startsWith('pending')) screen += '• Working (2s • esc to interrupt)\r\n• Messages to be submitted after next tool call (press esc to interrupt and send immediately)\r\n  ↳ change course\r\n›\r\n\r\n  GPT-6.1 low · /workspace\r\n  ? for shortcuts\r\n';
  else if (mode === 'unknown') screen += '✻ Working… (1s · esc to interrupt)\r\ninput not visible\r\n';
  else screen += '────────────────────────\r\n❯ ' + (mode === 'grey' ? '\x1b[2mTry something\x1b[0m' : draft) + '\r\n────────────────────────\r\n  ? for shortcuts\r\n';
  process.stdout.write('\x1b[2J\x1b[H' + screen + (mode === 'pending' ? '\x1b[5;3H' : ''));
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
    for (const [name, harness, mode] of [
      ["ce-empty", "claude", "empty"], ["ce-text", "claude", "empty"],
      ["ce-grey", "claude", "grey"], ["ce-unknown", "claude", "unknown"], ["ce-codex", "codex", "pending"], ["ce-codex-away", "codex", "pending-away"],
    ]) {
      aaa("tab", "create", "--machine", "labA", "--label", name, "--cwd", directory, "--", `${directory}/${harness}`, "--", mode, `${directory}/${name}.bytes`);
      await waitOutput(name, "received 0");
      await page.getByTestId(`agent-${name}`).waitFor({ state: "attached" });
      assert.equal(aaa("pane", "list").panes.find(p => p.name === name)?.agent?.harness, harness);
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
  await step("Claude Ctrl+Enter: 글이 있으면 줄바꿈, 비면 Ctrl+X Ctrl+S, 회색 추천문은 빈 입력, 모름은 줄바꿈", async () => {
    await open(page, "ce-text");
    await page.keyboard.type("draft");
    await waitOutput("ce-text", "❯ draft");
    await expectKey(page, "ce-text", () => page.keyboard.press("Control+Enter"), "\x1b\r");
    await shot(page, "ctrl-enter-draft-desktop");
    for (const [name, expected] of [["ce-empty", "\x18\x13"], ["ce-grey", "\x18\x13"], ["ce-unknown", "\x1b\r"]]) {
      await open(page, name);
      await expectKey(page, name, () => page.keyboard.press("Control+Enter"), expected);
    }
    await shot(page, "ctrl-enter-unknown-desktop");
  });
  await step("Codex Ctrl+Enter: 현재 native steering 대기 패널의 빈 입력에서만 Esc", async () => {
    await open(page, "ce-codex");
    await expectKey(page, "ce-codex", () => page.keyboard.press("Control+Enter"), "\x1b");
    const sent = await sentCount(page);
    await page.locator('.xterm-helper-textarea').evaluate(el => el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', keyCode: 13, ctrlKey: true, repeat: true, bubbles: true })));
    assert.equal((await sentFrames(page, sent)).filter(f => ['input', 'ctrl_enter'].includes(f.t)).length, 0);
    await open(page, "ce-codex-away");
    await expectKey(page, "ce-codex-away", () => page.keyboard.press("Control+Enter"), "\x1b\r");
  });
  await step("터치 Ctrl+Enter도 같은 판정: Ctrl 래치+Enter, 입력줄 초안은 보내지 않고 줄바꿈, IME 확정은 steering하지 않음", async () => {
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
    assert.equal(await line.inputValue(), "휴대폰 초안\n");
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
