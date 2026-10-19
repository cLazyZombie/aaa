// Ctrl+Enter: host의 같은 감지 판정을 웹 하드웨어·터치 키가 사용한다. TUI는 PaneCtrlEnter 요청을 공유한다.
// steering만 하고 줄바꿈은 넣지 않는다(사용자 10-19 「스티어링만 남기고 싶어」, plan §8.239 후속). 줄바꿈은 Shift+Enter다.
// agent가 작업 중일 때만 동작한다(사용자 10-19 15:1x 정정): 쉬는 Claude·Codex는 글이 있어도 보내지 않는다.
// 웹 입력창(대화 입력창·터미널 입력줄)에 글이 있으면 Ctrl+Enter는 안내 줄 없이 그 글을 steering으로 보낸다(사용자 10-19 15:3x):
// 보내기와 같은 submit이고 host가 끝 키를 고른다(작업 중인 Claude는 sendNow, 그 밖은 Enter).
import assert from "node:assert/strict";
import fs from "node:fs";
import {
  root, aaa, waitOutput, domIs, shot, pairPage, step, browser, inList, sentCount, sentFrames, openSettings,
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
  else if (mode === 'modern') screen += '❯ queued message\r\n  ctrl+x ctrl+s to send now\r\n✻ Shimmying…\r\n\r\n────────────────────────\r\n❯ \x1b[2mPress up to edit queued messages\x1b[0m\r\n────────────────────────\r\n  paste again to expand\r\n';
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
      ["ce-modern", "claude", "modern", true], ["ce-empty", "claude", "work-empty", true], ["ce-text", "claude", "work-empty", true],
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
    for (const name of ["ce-modern", "ce-empty", "ce-grey"]) {
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
    const events = aaa("debug", "input", "--since", "1h").events.filter((e) => e.event === "ctrl_enter");
    if (!events.some((e) => e.detail.includes('keys=["ctrl+x", "ctrl+s"]') && e.detail.includes("working_line=true"))) throw new Error("Ctrl+Enter 성공 판정 로그 없음");
    if (!events.some((e) => e.detail.includes('keys=[]') && e.detail.includes("input_live=false"))) throw new Error("Ctrl+Enter 무반응 근거 로그 없음");
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
  await step("대화 입력창 Ctrl+Enter(사용자 10-19 15:3x): 안내 줄 없이 글을 steering으로 보낸다. 작업 중인 Claude는 글 뒤 sendNow(Ctrl+X Ctrl+S)이고 「대기 메시지 자동 전송」을 꺼도 붙잡지 않으며, 쉬는 Claude는 보통 보내기(글 + Enter), 작업 중 Codex는 글 + Enter(steer). 보내기와 같은 submit이라 전달 뒤 입력창이 풀린다. 빈 입력창은 작업 중이면 대기 메시지를 지금 보내고(이 기기에 붙잡은 것이 먼저, 없으면 harness 대기열), 쉬면 아무것도 보내지 않는다", async () => {
    const chat = async (name) => {
      await inList(page, `pane-${name}`);
      const toggle = page.getByTestId("chat-toggle");
      if (await toggle.getAttribute("aria-pressed") !== "true") await toggle.click();
      const box = page.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" });
      await box.waitFor();
      return box;
    };
    // 가짜 harness는 기록을 알리지 않는다: host 전달이 보내짐이고(plan §8.247) 그때 입력창이 비고 풀린다
    const unlocked = (what) => domIs(page, what, () => {
      const box = document.querySelector('[data-testid="chat-composer"] textarea');
      return box?.value === "" && !box.hasAttribute("readonly") && !document.querySelector('[data-testid="chat-send-state"]');
    });
    const frames = async (from) => (await sentFrames(page, from))
      .filter((f) => ["input", "submit", "ctrl_enter"].includes(f.t))
      .map((f) => (f.t === "submit" ? `submit:${f.data}:${f.steer}` : f.t));
    const autoSend = async () => {
      const d = await openSettings(page, "채팅");
      await d.getByRole("switch", { name: "대기 메시지 자동 전송" }).click();
      await page.keyboard.press("Escape");
      await d.waitFor({ state: "detached" });
    };
    let box = await chat("ce-modern");
    let modernFrom = await sentCount(page);
    await expectKey(page, "ce-modern", () => box.press("Control+Enter"), "\x18\x13");
    assert.deepEqual(await frames(modernFrom), ["ctrl_enter"]);
    box = await chat("ce-text");
    await autoSend();
    try {
      // 자동 전송을 끄면 작업 중 Enter는 이 기기에 붙잡고, Ctrl+Enter는 붙잡지 않고 지금 턴에 넣는다
      await box.fill("held-1");
      await box.press("Enter");
      const queue = page.getByTestId("chat-queue");
      await queue.waitFor();
      let from = await sentCount(page);
      await box.fill("steer-1");
      await expectKey(page, "ce-text", () => box.press("Control+Enter"), "steer-1\x18\x13");
      await unlocked("Ctrl+Enter 글을 전달한 뒤 입력창이 풀린다");
      assert.deepEqual(await frames(from), ["submit:steer-1:true"]);
      const events = aaa("debug", "input", "--pane", "ce-text").events;
      const sent = events.filter((e) => e.event === "received" && e.detail.includes("steer=true")).at(-1);
      assert.ok(sent && events.some((e) => e.id === sent.id && e.event === "ctrl_enter" && e.detail.includes('keys=["ctrl+x", "ctrl+s"]') && e.detail.includes("input_live=true")), "글 있는 Ctrl+Enter의 같은 ID에 선택 키와 화면 근거를 기록한다");
      assert.equal(await queue.getByRole("textbox", { name: "대기 메시지 1" }).inputValue(), "held-1");
      await shot(page, "ctrl-enter-chat-held");
      // 빈 입력창: 이 기기에 붙잡은 맨 앞을 steering으로 지금 보낸다
      from = await sentCount(page);
      await expectKey(page, "ce-text", () => box.press("Control+Enter"), "held-1\x18\x13");
      await queue.waitFor({ state: "detached" });
      assert.deepEqual(await frames(from), ["submit:held-1:true"]);
    } finally {
      await autoSend();
    }
    // 붙잡은 것이 없으면 harness 대기열을 host 판정으로 지금 보낸다(Claude sendNow)
    let from = await sentCount(page);
    await expectKey(page, "ce-text", () => box.press("Control+Enter"), "\x18\x13");
    assert.deepEqual(await frames(from), ["ctrl_enter"]);
    // 쉬는 Claude: 글은 보통 보내기, 빈 입력창은 아무것도 보내지 않는다
    box = await chat("ce-idle");
    from = await sentCount(page);
    await box.fill("idle-1");
    await expectKey(page, "ce-idle", () => box.press("Control+Enter"), "idle-1\r");
    await unlocked("쉬는 Claude에 보낸 뒤");
    assert.deepEqual(await frames(from), ["submit:idle-1:false"]);
    const idleEvents = aaa("debug", "input", "--pane", "ce-idle").events;
    const idleSend = idleEvents.filter((e) => e.event === "received").at(-1);
    assert.ok(idleSend && idleEvents.some((e) => e.id === idleSend.id && e.event === "ctrl_enter" && e.detail.includes('keys=["enter"]') && e.detail.includes("working=false")), "쉬는 pane의 글 Ctrl+Enter도 보통 Enter 선택 근거를 기록한다");
    from = await sentCount(page);
    await box.press("Control+Enter");
    assert.deepEqual(await frames(from), ["ctrl_enter"]);
    // 작업 중 Codex: 글 + Enter(정식 steer)
    box = await chat("ce-codex-typed");
    from = await sentCount(page);
    await box.fill("codex-1");
    await expectKey(page, "ce-codex-typed", () => box.press("Control+Enter"), "codex-1\r");
    await unlocked("Codex에 보낸 뒤");
    assert.deepEqual(await frames(from), ["submit:codex-1:true"]);
    // 「Ctrl+Enter는 …」 안내 줄은 없다
    assert.equal(await page.locator(".composer-note, .terminal-input-note", { hasText: "Ctrl+Enter" }).count(), 0);
    await shot(page, "ctrl-enter-chat-desktop");
    await page.getByTestId("chat-toggle").click();
  });
  await step("대기 메시지 「지금 보내기」 host 판정(plan §8.270): 글을 다시 보내지 않고 작업 중 빈 입력의 대기 표지(Claude `to send now` 줄, Codex steering 패널) 위에서만 키를 쓰고, 그 밖은 쓰지 않고 까닭을 답한다. 보기 기기는 보내지 못한다", async () => {
    // pane WebSocket을 가로채 steer_queued frame을 넣고 답을 받는다(단추 화면은 spec 23). 연결은 주소의 pane id로 찾는다
    const ids = Object.fromEntries(aaa("pane", "list").panes.map((p) => [p.name, p.id]));
    const conns = new Map();
    const waiting = new Map();
    const wsRoute = [/\/api\/v1\/panes\/[^/]+\/ws/, (socket) => {
      const pane = decodeURIComponent(new URL(socket.url()).pathname.split("/")[4]);
      const server = socket.connectToServer();
      const conn = { server, replies: new Map() };
      server.onMessage((raw) => {
        if (typeof raw === "string") {
          const m = JSON.parse(raw);
          if (m.t === "front") {
            conns.set(pane, conn);
            waiting.get(pane)?.(conn);
          }
          if (m.t === "steer_queued") conn.replies.get(m.id)?.(m);
        }
        socket.send(raw);
      });
      socket.onMessage((raw) => server.send(raw));
    }];
    const connOf = (name) => conns.get(ids[name]) ?? new Promise((resolve) => waiting.set(ids[name], resolve));
    let next = 900;
    const steerFrame = async (name) => {
      const conn = await connOf(name);
      const id = next++;
      const reply = new Promise((resolve) => conn.replies.set(id, resolve));
      conn.server.send(JSON.stringify({ t: "steer_queued", id, trace: `e2e-steer-${id}` }));
      return reply;
    };
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { wsRoute });
    const p = client.page;
    for (const [name, expected] of [
      ["ce-modern", { keys: "\x18\x13" }], ["ce-text", { reason: "draft" }], ["ce-grey", { reason: "no_queue" }],
      ["ce-idle", { reason: "not_working" }], ["ce-unknown", { reason: "unknown_screen" }],
      ["ce-codex", { keys: "\x1b" }], ["ce-codex-typed", { reason: "draft" }], ["ce-codex-idle", { reason: "not_working" }],
      ["ce-codex-away", { reason: "unknown_screen" }],
    ]) {
      await open(p, name);
      let reply;
      if (expected.keys) await expectKey(p, name, async () => { reply = await steerFrame(name); }, expected.keys);
      else await expectNothing(p, name, async () => { reply = await steerFrame(name); });
      assert.deepEqual({ sent: reply.sent, reason: reply.reason ?? null }, { sent: !!expected.keys, reason: expected.reason ?? null }, name);
    }
    const events = aaa("debug", "input", "--since", "1h").events;
    for (const reason of ["Draft", "NoQueue", "NotWorking", "UnknownScreen"]) {
      if (!events.some((e) => e.event === "steer_queued" && e.detail.includes(`reason=Some(${reason})`))) throw new Error(`판정 진단 누락: ${reason}`);
    }
    for (const keys of ['["ctrl+x", "ctrl+s"]', '["esc"]']) {
      const decided = events.find((e) => e.event === "steer_queued" && e.detail.includes(`keys=${keys}`));
      if (!decided || !events.some((e) => e.id === decided.id && e.event === "steer_sent")) throw new Error(`보냄 진단이 같은 ID로 없다: ${keys}`);
    }
    await p.context().close();
    // 보기 기기: 화면 판정 전에 거절하고 아무것도 쓰지 않는다. 연결 표는 새 기기가 첫 화면 연결을 열기 전에 비운다
    conns.clear();
    const reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1100, height: 760 }, { wsRoute });
    await inList(reader.page, "pane-ce-modern");
    const before = received("ce-modern").length;
    const refused = await steerFrame("ce-modern");
    assert.equal(refused.sent, false);
    assert.equal(refused.reason, "failed");
    // 거절 답 뒤 CLI로 글자 하나를 보내 그 뒤 bytes가 그것뿐인지로 본다
    aaa("pane", "send-keys", "ce-modern", "z");
    await waitOutput("ce-modern", `received ${before + 1}`);
    assert.equal(received("ce-modern").subarray(before).toString(), "z");
    if (!aaa("debug", "input", "--pane", "ce-modern", "--since", "1h").events.some((e) => e.event === "steer_queued" && e.detail.includes("role=readonly"))) throw new Error("보기 기기 거절 진단 없음");
    await reader.page.context().close();
  });
  await step("터치 Ctrl+Enter도 같은 판정: Ctrl 래치+Enter, 입력줄 초안은 안내 줄 없이 그 글을 steering으로 보내고(작업 중 Claude는 글 뒤 sendNow), IME 확정은 steering하지 않음", async () => {
    const mobile = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true, line: true })).page;
    await open(mobile, "ce-empty");
    const bar = mobile.locator('.key-bar');
    await bar.locator('[data-key="Control"]').click();
    await expectKey(mobile, "ce-empty", () => bar.locator('[data-key="Enter"]').click(), "\x18\x13");
    await bar.locator('[data-key="Control"]').click();
    const line = mobile.getByLabel("보낼 글");
    await line.fill("휴대폰 초안");
    const sent = await sentCount(mobile);
    await expectKey(mobile, "ce-empty", () => line.press("Control+Enter"), "휴대폰 초안");
    await domIs(mobile, "보낸 입력줄이 빈다", () => document.querySelector('textarea[aria-label="보낼 글"]')?.value === "");
    assert.deepEqual((await sentFrames(mobile, sent)).filter(f => ['input', 'submit', 'ctrl_enter'].includes(f.t)).map(f => `${f.t}:${f.steer}`), ["submit:true"]);
    assert.equal(await mobile.locator(".terminal-input-note").count(), 0);
    await expectKey(mobile, "ce-empty", () => line.press("Control+Enter"), "\x18\x13");
    const start = await sentCount(mobile);
    await line.evaluate(el => el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, isComposing: true, keyCode: 229, bubbles: true })));
    assert.equal((await sentFrames(mobile, start)).filter(f => ['input', 'submit', 'ctrl_enter'].includes(f.t)).length, 0);
    await expectKey(mobile, "ce-empty", () => line.press("Control+Enter"), "\x18\x13");
    assert.ok(aaa("debug", "input", "--pane", "ce-empty").events.some((e) => e.event === "ui" && e.detail.includes("CtrlEnterBlocked") && e.detail.includes("Composing")), "IME Ctrl+Enter 차단 이유를 host에 기록한다");
    await shot(mobile, "ctrl-enter-phone");
    await mobile.context().close();
  });
}
