// 입력칸 컨텍스트 게이지(plan §8.319, 사용자 10-20 「대화창에서 사용한 컨텍스트를 보여주고 싶어」·「%, over하면 실제 k」·
// 「타이핑시에는 보이지 않게」). 가짜 Claude가 대화 기록에 실제 Claude 2.1.304 꼴의 usage 줄·정리 줄을 적고, agent가 그 기록을
// 읽어 host 상태로 보낸 값을 웹이 보인다: 38% → 230k(창 넘음) → 84k(창 모름) → 86%(위험). 글을 치는 동안은 자리를 둔 채 숨고
// (입력칸 폭이 그대로), 80% 이상은 치는 중에도 보인다. 펼침의 「/compact 정리」는 쉴 때만 켜지고 pane에 /compact를 보내며,
// 입력칸은 전달로 풀리고(명령은 사람 글로 기록되지 않는다), 정리 중 표지 → 정리 뒤 값 14%와 「대화 정리됨 · 164k → 28k」로 이어진다. 휴대폰(iPhone Safari)도 같다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 49-context-gauge(새 실험실).
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, aaa, AA, step, inList, until, domIs, waitOutput, shot, webkit } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0" };

// 가짜 Claude: 시작할 때 hook으로 기록 자리를 알리고 받은 bytes를 남긴다. F7은 작업 중, F5는 작업 끝(쉼), F6은 정리 끝(정리 줄·요약을 적고 쉼)이다.
// 쉬는 동안 Enter 글은 기록에 사람 말로 적고, 「/compact」면 실제 Claude처럼 정리 화면을 그린다(spinner 「Compacting conversation…」)
const program = String.raw`
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
const [aa, transcript, bytes, session] = process.argv.slice(2);
execFileSync(aa, ['hook', '--agent', 'claude', JSON.stringify({ hook_event_name: 'SessionStart', session_id: session, source: 'startup', transcript_path: transcript })]);
fs.writeFileSync(bytes, '');
let draft = '', mode = 'idle';
const write = (line) => fs.appendFileSync(transcript, JSON.stringify(line) + '\n');
process.stdin.setRawMode(true);
function draw() {
  let s = 'mode ' + mode + '\r\n';
  if (mode === 'compacting') s += '\r\n✻ Compacting conversation… (3s)\r\n';
  if (mode === 'working') s += '\r\n✻ Working… (1s · esc to interrupt)\r\n';
  s += '\r\n────────────────────────\r\n❯ ' + draft + '\r\n────────────────────────\r\n  ? for shortcuts\r\n';
  process.stdout.write('\x1b[2J\x1b[H' + s);
}
const KEYS = {
  '\x1b[15~': () => { mode = 'idle'; },
  '\x1b[17~': () => {
    write({ type: 'system', subtype: 'compact_boundary', content: 'Conversation compacted', compactMetadata: { trigger: 'manual', preTokens: 163600, postTokens: 28400 } });
    write({ type: 'user', isCompactSummary: true, message: { role: 'user', content: 'This session is being continued from a previous conversation. 요약: 오류 처리를 정리했다' } });
    mode = 'idle';
  },
  '\x1b[18~': () => { mode = 'working'; },
};
process.stdin.on('data', (data) => {
  fs.appendFileSync(bytes, data);
  let text = data.toString().replaceAll('\x1b[200~', '').replaceAll('\x1b[201~', '');
  for (const [key, act] of Object.entries(KEYS)) if (text.includes(key)) { act(); text = text.replaceAll(key, ''); }
  for (const c of text) {
    if (c === '\r') {
      if (draft === '/compact') mode = 'compacting';
      else if (draft !== '') write({ type: 'user', message: { role: 'user', content: draft }, origin: { kind: 'human' } });
      draft = '';
    } else if (c >= ' ') draft += c;
  }
  draw();
});
draw();
`;

const LABEL = "ctx1";
const directory = `${root}/context`;
const transcript = `${root}/claude/projects/-lab-${LABEL}/s1.jsonl`;
const bytes = () => fs.readFileSync(`${directory}/${LABEL}.bytes`, "utf8");

/** 실제 Claude 2.1.304 assistant 줄 꼴: 컨텍스트는 input + cache 생성 + cache 읽기, 창은 requestedModel */
function answer(used, requestedModel = "claude-opus-5-5") {
  const line = { type: "assistant", isSidechain: false, message: { model: "claude-opus-5-5", content: [{ type: "text", text: `${used} 토큰째 답` }],
    usage: { input_tokens: 2, cache_creation_input_tokens: 998, cache_read_input_tokens: used - 1000, output_tokens: 40 } } };
  if (requestedModel) line.requestedModel = requestedModel;
  fs.appendFileSync(transcript, JSON.stringify(line) + "\n");
}

const gauge = (p) => p.evaluate(() => {
  const g = document.querySelector('[data-testid="context-gauge"]');
  const button = g?.querySelector("button");
  const box = document.querySelector('[data-testid="chat-composer"] textarea');
  return g && { text: g.querySelector('[data-testid="context-gauge-value"]').textContent, level: g.dataset.level, hidden: g.dataset.hidden === "true",
    compacting: g.dataset.compacting === "true", title: button.title, gaugeWidth: button.getBoundingClientRect().width,
    boxWidth: box.getBoundingClientRect().width, visibility: getComputedStyle(button).visibility };
});
/** 게이지 글이 그것이 될 때까지(agent가 2초마다 기록을 읽는다) */
const shows = (p, text) => domIs(p, `게이지 ${text}`, (t) => document.querySelector('[data-testid="context-gauge-value"]')?.textContent === t, text);
const box = (p) => p.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" });

async function openChat(p) {
  await inList(p, `pane-${LABEL}`);
  if ((await p.getByTestId("chat").count()) === 0) {
    await until("대화 단추", async () => (await p.getByTestId("chat-toggle").count()) === 1);
    await p.getByTestId("chat-toggle").click();
  }
  await p.getByTestId("chat-composer").waitFor();
}

async function key(name, mode) {
  aaa("pane", "send-keys", LABEL, name);
  await waitOutput(LABEL, `mode ${mode}`);
}

/** 글을 치는 동안: 보통 값이면 숨되 자리(게이지 폭·글 칸 폭)는 그대로다 */
async function typingHides(p, label) {
  const empty = await gauge(p);
  assert.equal(empty.hidden, false, label);
  await box(p).fill("오류 처리를 이어서");
  await domIs(p, `${label} 치는 중 숨김`, () => document.querySelector('[data-testid="context-gauge"]')?.dataset.hidden === "true");
  const typing = await gauge(p);
  assert.deepEqual([typing.gaugeWidth, typing.boxWidth], [empty.gaugeWidth, empty.boxWidth], `${label}: 치는 중에도 자리는 그대로`);
  return typing;
}

export default async function ({ page }) {
  await step("준비: 가짜 Claude pane과 첫 응답(76k / 200k)", async () => {
    fs.mkdirSync(directory, { recursive: true });
    fs.mkdirSync(`${root}/claude/projects/-lab-${LABEL}`, { recursive: true });
    fs.writeFileSync(transcript, JSON.stringify({ type: "user", message: { role: "user", content: "오류 처리를 정리해 줘" }, origin: { kind: "human" } }) + "\n");
    fs.writeFileSync(`${directory}/claude`, `#!${process.execPath}\n${program}`, { mode: 0o755 });
    aaa("tab", "create", "--machine", "labA", "--label", LABEL, "--cwd", directory, "--", `${directory}/claude`, AA, transcript, `${directory}/${LABEL}.bytes`,
      "7b2d4f6a-1c3e-4a5b-9d8f-0e1f2a3b4c5d");
    await waitOutput(LABEL, "mode idle");
    await openChat(page);
    // 첫 응답 전에는 게이지가 없다
    assert.equal(await page.getByTestId("context-gauge").count(), 0);
    answer(76_000);
    await shows(page, "38%");
    const g = await gauge(page);
    assert.deepEqual([g.level, g.title], ["normal", "컨텍스트 38% 사용 · 76k / 200k"]);
  }, { always: true });

  await step("창을 넘으면 실제 토큰 k(230k), 창을 모르면 k(84k), 80% 이상은 위험", async () => {
    answer(230_000);
    await shows(page, "230k");
    assert.deepEqual([(await gauge(page)).level, (await gauge(page)).title], ["over", "컨텍스트 230k 사용 · 창 200k를 넘음"]);
    answer(84_000, null);
    await shows(page, "84k");
    assert.equal((await gauge(page)).level, "unknown");
    answer(172_000);
    await shows(page, "86%");
    assert.equal((await gauge(page)).level, "high");
  });

  await step("치는 동안: 80% 이상은 그대로 보이고, 보통 값은 자리를 둔 채 숨었다가 입력칸이 비면 다시 보인다", async () => {
    await box(page).fill("정리할까");
    const high = await gauge(page);
    assert.deepEqual([high.hidden, high.visibility], [false, "visible"], "86%는 치는 중에도 보인다");
    await shot(page, "49-high-typing-desktop");
    await box(page).fill("");
    answer(76_000);
    await shows(page, "38%");
    await shot(page, "49-empty-desktop");
    await typingHides(page, "데스크톱");
    await shot(page, "49-typing-desktop");
    await box(page).fill("");
    await domIs(page, "빈 입력칸에서 다시 보임", () => document.querySelector('[data-testid="context-gauge"]')?.dataset.hidden === undefined);
  });

  await step("펼침: 사용·창, 작업 중에는 정리 단추가 까닭과 함께 꺼진다", async () => {
    await page.getByTestId("context-gauge").getByRole("button").click();
    const pop = page.getByTestId("context-pop");
    await pop.waitFor();
    assert.match(await pop.textContent(), /76k \/ 200k 사용/);
    assert.equal(await page.getByTestId("context-compact").isDisabled(), false, "쉬는 중에는 정리할 수 있다");
    await shot(page, "49-pop-desktop");
    await page.keyboard.press("Escape");
    await pop.waitFor({ state: "detached" });
    await key("f7", "working");
    await domIs(page, "작업 중", () => document.querySelector('[data-testid="agent-chip"]')?.classList.contains("working"));
    await page.getByTestId("context-gauge").getByRole("button").click();
    await pop.waitFor();
    assert.equal(await page.getByTestId("context-compact").isDisabled(), true);
    assert.match(await page.getByTestId("context-compact-blocked").textContent(), /작업 중에는 정리를 보내지 않는다/);
    await page.keyboard.press("Escape");
    await pop.waitFor({ state: "detached" });
  });

  await step("정리 단추 → pane에 /compact → 정리 중 표지와 게이지 → 정리 뒤 14%와 「대화 정리됨 · 164k → 28k」", async () => {
    await key("f5", "idle");
    await domIs(page, "쉼", () => document.querySelector('[data-testid="agent-chip"]')?.classList.contains("idle"));
    answer(164_000);
    await shows(page, "82%");
    const before = bytes();
    await page.getByTestId("context-gauge").getByRole("button").click();
    await page.getByTestId("context-compact").click();
    await waitOutput(LABEL, "mode compacting");
    assert.ok(bytes().slice(before.length).includes("/compact"), "pane이 /compact를 받았다");
    // harness는 명령을 사람 글로 기록하지 않는다(Claude `<command-name>` 줄, Codex 없음): host 전달로 입력칸이 풀린다
    await domIs(page, "입력칸 잠금 풀림", () => !document.querySelector(".composer-send-progress") && !document.querySelector('[data-testid="chat-composer"] textarea').readOnly);
    await domIs(page, "정리 중 게이지", () => document.querySelector('[data-testid="context-gauge"]')?.dataset.compacting === "true");
    assert.equal((await gauge(page)).text, "82%", "정리 중에는 마지막 값");
    await shot(page, "49-compacting-desktop");
    await key("f6", "idle");
    await shows(page, "14%");
    await domIs(page, "구분선 전후 크기", () => [...document.querySelectorAll('[data-testid="chat-compact-sizes"]')].map((e) => e.textContent).join("|") === " · 164k → 28k");
    await shot(page, "49-after-desktop");
  });

  await step("휴대폰(iPhone Safari): 같은 값, 치는 동안 숨고 비면 다시 보인다, 펼침", async () => {
    const wk = await webkit.launch();
    try {
      const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone");
      await openChat(p);
      await shows(p, "14%");
      await shot(p, "49-empty-iphone");
      await typingHides(p, "iPhone");
      await shot(p, "49-typing-iphone");
      await box(p).fill("");
      await domIs(p, "iPhone 다시 보임", () => document.querySelector('[data-testid="context-gauge"]')?.dataset.hidden === undefined);
      await p.getByTestId("context-gauge").getByRole("button").click();
      await p.getByTestId("context-pop").waitFor();
      assert.match(await p.getByTestId("context-pop").textContent(), /28k \/ 200k 사용/);
      await shot(p, "49-pop-iphone");
      await p.context().close();
    } finally {
      await wk.close();
    }
  });
}
