// 입력칸 「/」 목록과 harness 명령(plan §8.323, 사용자 10-20 「slash command (/compact 등)」, 기획 library/t-0502 §5·§6 2단계).
// 실험실 Claude 설정 폴더에 내 명령·skill을, pane 폴더(git root)에 프로젝트 명령을 두고 agent가 내장 표와 함께 읽는다. 가짜 Claude는
// 실측(Claude 2.1.304)대로 `/context`의 명령·출력 줄(색 코드 포함)을 기록에 적고, `/model`은 선택창을 그렸다가 Esc에 「Kept model」을
// 적고, `/clear`는 새 기록으로 SessionStart(clear) hook을 보낸다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 51-slash-commands(새 실험실).
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, aaa, AA, step, inList, until, domIs, inTime, waitOutput, shot, webkit } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0" };

// 가짜 Claude: F7 작업 중, F5 쉼. Enter 글: `/context`·`/model`·`/clear`는 실측 꼴로, 그 밖은 사람 말과 usage 있는 답으로 적는다
const program = String.raw`
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
const [aa, dir, bytes] = process.argv.slice(2);
let n = 1;
let transcript = dir + '/s1.jsonl';
const hook = (source) => execFileSync(aa, ['hook', '--agent', 'claude', JSON.stringify({ hook_event_name: 'SessionStart', session_id: '5c1d2e3f-4a5b-4c6d-8e7f-90a1b2c3d4e' + n, source, transcript_path: transcript })]);
fs.writeFileSync(transcript, '');
hook('startup');
fs.writeFileSync(bytes, '');
let draft = '', mode = 'idle';
const write = (line) => fs.appendFileSync(transcript, JSON.stringify(line) + '\n');
const echo = (name) => '<command-name>' + name + '</command-name>\n            <command-message>' + name.slice(1) + '</command-message>\n            <command-args></command-args>';
const local = (content) => write({ type: 'system', subtype: 'local_command', content, level: 'info', isMeta: false });
process.stdin.setRawMode(true);
function draw() {
  let s = 'session ' + n + ' mode ' + mode + '\r\n';
  if (mode === 'picker') s += '\r\n   Select model\r\n   ❯ 1. Haiku 5.5 ✔\r\n     2. Sonnet 5.5\r\n   Enter to set as default · s to use this session only · Esc to cancel\r\n';
  if (mode === 'working') s += '\r\n✻ Working… (1s · esc to interrupt)\r\n';
  s += '\r\n────────────────────────\r\n❯ ' + draft + '\r\n────────────────────────\r\n  ? for shortcuts\r\n';
  process.stdout.write('\x1b[2J\x1b[H' + s);
}
const KEYS = { '\x1b[15~': () => { mode = 'idle'; }, '\x1b[18~': () => { mode = 'working'; } };
process.stdin.on('data', (data) => {
  fs.appendFileSync(bytes, data);
  let text = data.toString().replaceAll('\x1b[200~', '').replaceAll('\x1b[201~', '');
  for (const [key, act] of Object.entries(KEYS)) if (text.includes(key)) { act(); text = text.replaceAll(key, ''); }
  if (mode === 'picker' && text === '\x1b') {
    local(echo('/model'));
    local('<local-command-stdout>Kept model as ' + String.fromCharCode(96) + 'Haiku 5.5' + String.fromCharCode(96) + '</local-command-stdout>');
    mode = 'idle';
    draw();
    return;
  }
  for (const c of text) {
    if (c !== '\r') { if (c >= ' ') draft += c; continue; }
    if (draft === '/context') {
      local(echo('/context'));
      local('<local-command-stdout> \x1b[1mContext Usage\x1b[22m\n\x1b[38;2;153;153;153m⛁ ⛀ ⛶ ⛶\x1b[39m  claude-haiku-5-5 · 43.9k/1m tokens (4%)</local-command-stdout>');
      write({ type: 'user', isMeta: true, message: { role: 'user', content: '## Context Usage' } });
    } else if (draft === '/model') {
      mode = 'picker';
    } else if (draft === '/clear') {
      n += 1;
      transcript = dir + '/s' + n + '.jsonl';
      fs.writeFileSync(transcript, '');
      hook('clear');
    } else if (draft !== '') {
      write({ type: 'user', message: { role: 'user', content: draft }, origin: { kind: 'human' } });
      write({ type: 'assistant', requestedModel: 'claude-haiku-4-5', message: { model: 'claude-haiku-4-5', content: [{ type: 'text', text: '받음: ' + draft }],
        usage: { input_tokens: 2, cache_creation_input_tokens: 998, cache_read_input_tokens: 75000, output_tokens: 10 } } });
    }
    draft = '';
  }
  draw();
});
draw();
`;

const LABEL = "slash1";
const project = `${root}/slash-project`;
const tdir = `${root}/claude/projects/-lab-${LABEL}`;
const bytes = () => fs.readFileSync(`${project}/${LABEL}.bytes`, "utf8");
const box = (p) => p.getByTestId("chat-composer").getByRole("textbox", { name: "메시지" });
/** 목록 줄: 이름과 표지 */
const menu = (p) => p.evaluate(() => {
  const m = document.querySelector('[data-testid="slash-menu"]');
  return m && {
    groups: [...m.querySelectorAll(".slash-group")].map((g) => g.textContent),
    items: [...m.querySelectorAll('[data-testid="slash-item"]')].map((e) => ({
      name: e.dataset.name, active: e.getAttribute("aria-selected") === "true",
      tags: [...e.querySelectorAll(".slash-tag")].map((t) => t.textContent),
    })),
  };
});
const menuShown = (p) => domIs(p, "「/」 목록", () => document.querySelector('[data-testid="slash-item"]') !== null);
const menuGone = (p) => domIs(p, "목록 닫힘", () => document.querySelector('[data-testid="slash-menu"]') === null);
/** 커서를 끝에 두고 친다(fill은 select 사건을 내지 않을 수 있어 키로 친다) */
async function typeIn(p, text) {
  await box(p).fill("");
  await box(p).pressSequentially(text);
}

/** 다음 frame을 사건으로 보류한다. 사용자 편집 뒤 실제 RAF를 모두 실행하고 끝났음을 기다린다. */
async function holdFrames(p, edit) {
  await p.evaluate(() => {
    const raf = window.requestAnimationFrame.bind(window);
    const cancel = window.cancelAnimationFrame.bind(window);
    const pending = new Map();
    let id = -1;
    window.requestAnimationFrame = (cb) => { const key = id--; pending.set(key, cb); return key; };
    window.cancelAnimationFrame = (key) => { if (key < 0) pending.delete(key); else cancel(key); };
    window.releaseComposerFrames = () => {
      window.requestAnimationFrame = raf;
      window.cancelAnimationFrame = cancel;
      return new Promise((done) => raf((now) => {
        for (const [key, cb] of pending) { pending.delete(key); cb(now); }
        done();
      }));
    };
  });
  try {
    return await edit();
  } finally {
    await inTime(p.evaluate(() => window.releaseComposerFrames()), 20_000, "보류한 frame 완료");
  }
}

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

export default async function ({ page }) {
  await step("준비: 내 명령·skill·프로젝트 명령과 가짜 Claude pane, 첫 답(게이지)", async () => {
    const write = (file, text) => { fs.mkdirSync(file.slice(0, file.lastIndexOf("/")), { recursive: true }); fs.writeFileSync(file, text); };
    write(`${root}/claude/commands/hello.md`, "---\ndescription: 인사를 건넨다\nargument-hint: \"[이름]\"\n---\n$ARGUMENTS에게 인사한다");
    write(`${root}/claude/skills/finish/SKILL.md`, "---\nname: finish\ndescription: 리뷰하고 마무리한다\n---\n");
    write(`${root}/claude/skills/inner/SKILL.md`, "---\nname: inner\nuser-invocable: false\n---\n");
    write(`${project}/.claude/commands/deploy.md`, "# 실험실에 배포한다\n");
    fs.mkdirSync(`${project}/.git`, { recursive: true });
    fs.mkdirSync(tdir, { recursive: true });
    fs.writeFileSync(`${project}/claude`, `#!${process.execPath}\n${program}`, { mode: 0o755 });
    aaa("tab", "create", "--machine", "labA", "--label", LABEL, "--cwd", project, "--", `${project}/claude`, AA, tdir, `${project}/${LABEL}.bytes`);
    await waitOutput(LABEL, "session 1 mode idle");
    await openChat(page);
    await box(page).fill("오류 처리를 정리해 줘");
    // 입력칸은 pane WebSocket의 readonly 해제보다 먼저 그려질 수 있다. 꺼진 동안의 Enter는 초안을 두고 보내지 않는다.
    // 첫 글을 한 번만 보내려면 연결 준비 사건을 먼저 기다린다(전체 검증에서 가짜 agent 입력 0바이트로 재현).
    await domIs(page, "첫 글 보낼 연결 준비", () => {
      const send = document.querySelector('[data-testid="chat-composer"] .composer-send');
      return send !== null && !send.disabled;
    });
    await box(page).press("Enter");
    await domIs(page, "답과 게이지", () => document.querySelector('[data-testid="context-gauge-value"]')?.textContent === "38%");
  }, { always: true });

  await step("빈 「/」: 출처별 묶음, 끝내는 명령·사람이 부르지 않는 skill은 없음, 터미널·확인 표지", async () => {
    await typeIn(page, "/");
    await menuShown(page);
    const m = await menu(page);
    assert.deepEqual(m.groups, ["기본", "skill", "내 명령", "프로젝트"]);
    const names = m.items.map((i) => i.name);
    for (const hidden of ["exit", "logout", "inner"]) assert.ok(!names.includes(hidden), `${hidden}는 목록에 없다`);
    for (const shown of ["compact", "context", "clear", "model", "finish", "hello", "deploy"]) assert.ok(names.includes(shown), shown);
    const tags = (name) => m.items.find((i) => i.name === name).tags;
    assert.deepEqual([tags("model"), tags("clear"), tags("context")], [["터미널"], ["확인"], []]);
    assert.equal(m.items[0].active, true);
    await shot(page, "51-menu-desktop");
  });

  await step("걸러내기·↓·Tab 넣기·Esc 닫기, 줄 중간과 경로에서는 열지 않는다", async () => {
    await typeIn(page, "/co");
    // 이름 앞이 맞는 것이 먼저(다른 이름 `/cost`의 usage도), 같은 순위는 이름 차례
    await domIs(page, "「/co」", () => [...document.querySelectorAll('[data-testid="slash-item"]')].map((e) => e.dataset.name).slice(0, 4).join(",") === "compact,config,context,usage");
    await shot(page, "51-filter-desktop");
    await box(page).press("ArrowDown");
    await box(page).press("ArrowDown");
    await domIs(page, "셋째 고름", () => document.querySelector('[data-testid="slash-item"][aria-selected="true"]')?.dataset.name === "context");
    await box(page).press("Tab");
    await menuGone(page);
    assert.equal(await box(page).inputValue(), "/context ");
    assert.equal(bytes().includes("/context"), false, "넣기만 하고 보내지 않는다");
    await typeIn(page, "/he");
    await menuShown(page);
    await box(page).press("Escape");
    await menuGone(page);
    assert.equal(await box(page).inputValue(), "/he", "Esc는 글을 두고 목록만 닫는다");
    await box(page).press("l");
    await menuShown(page);
    for (const text of ["보자 /co", "/Users/me/a.txt", "/tmp/x"]) {
      await typeIn(page, text);
      assert.equal(await page.getByTestId("slash-menu").count(), 0, text);
    }
    await box(page).fill("");
  });

  await step("지연 frame: 명령 넣기·기록 부르기 뒤 새 선택·입력·focus를 덮지 않는다", async () => {
    const before = bytes();
    for (const source of ["command", "recall"]) {
      for (const action of ["select", "edit", "focus"]) {
        await typeIn(page, source === "command" ? "/conte" : "");
        if (source === "command") await menuShown(page);
        const value = source === "command" ? "/context " : "오류 처리를 정리해 줘";
        const expected = await holdFrames(page, async () => {
          await box(page).press(source === "command" ? "Tab" : "ArrowUp");
          assert.equal(await box(page).inputValue(), value);
          if (action === "select") {
            await box(page).press("ControlOrMeta+a");
          } else if (action === "edit") {
            await box(page).press("ArrowLeft");
            await box(page).press("ArrowLeft");
            await box(page).pressSequentially("x");
          } else {
            await page.getByRole("button", { name: "그림 첨부", exact: true }).focus();
          }
          return box(page).evaluate((el) => ({ value: el.value, start: el.selectionStart, end: el.selectionEnd, focused: document.activeElement === el }));
        });
        if (action === "select") assert.deepEqual([expected.start, expected.end], [0, value.length]);
        if (action === "edit") assert.equal(expected.value, `${value.slice(0, -2)}x${value.slice(-2)}`);
        if (action === "focus") assert.equal(expected.focused, false);
        assert.deepEqual(await box(page).evaluate((el) => ({ value: el.value, start: el.selectionStart, end: el.selectionEnd, focused: document.activeElement === el })), expected, `${source}/${action}: 늦은 커서 작업이 새 편집을 덮었다`);
        if (action === "select") {
          await box(page).press("Delete");
          assert.equal(await box(page).inputValue(), "", "전체 선택 뒤 Delete는 글을 비운다");
        }
      }
    }
    assert.equal(bytes(), before, "편집만 하고 pane에 보내지 않는다");
    await box(page).fill("");
  });

  await step("/context: 다 친 이름의 Enter는 보낸다, 대화에 명령 줄과 결과(색 코드 없이), 입력칸이 풀린다", async () => {
    const before = bytes();
    await typeIn(page, "/context");
    await menuShown(page);
    await box(page).press("Enter");
    await until("pane이 /context를 받음", () => bytes().slice(before.length).includes("/context"));
    await domIs(page, "명령 줄과 결과", () => document.querySelector('[data-testid="chat-command"]')?.textContent === "/context"
      && /Context Usage/.test(document.querySelector('[data-testid="chat-command-output"]')?.textContent ?? ""));
    const out = await page.getByTestId("chat-command-output").first().textContent();
    assert.ok(!out.includes("\u001b") && !out.includes("[38;2"), out);
    await domIs(page, "입력칸 풀림", () => !document.querySelector('[data-testid="chat-composer"] textarea').readOnly && !document.querySelector(".composer-send-progress"));
    await shot(page, "51-context-desktop");
  });

  await step("/model(인자 없이): 보낸 뒤 「선택창」 알림과 터미널 보기, Esc로 닫으면 결과 줄", async () => {
    await typeIn(page, "/model");
    await box(page).press("Enter");
    await waitOutput(LABEL, "mode picker");
    await domIs(page, "선택창 알림", () => /선택창이 떴습니다/.test(document.querySelector('[data-testid="chat-note"]')?.textContent ?? ""));
    assert.equal(await page.getByTestId("chat-note").getByRole("button", { name: "터미널 보기" }).count(), 1);
    await shot(page, "51-picker-note-desktop");
    await key("esc", "idle");
    await domIs(page, "Kept model", () => [...document.querySelectorAll('[data-testid="chat-command-output"]')].some((e) => e.textContent.includes("Kept model as `Haiku 5.5`")));
  });

  await step("확인창: 직접 친 /exit는 아니오면 보내지 않고, /clear는 보내기를 누르면 가고 새 기록으로 넘어간다", async () => {
    const before = bytes();
    await typeIn(page, "/exit");
    // 숨은 명령은 목록에 없고, 다 친 이름에는 빈 목록도 띄우지 않는다
    assert.equal(await page.getByTestId("slash-menu").count(), 0);
    await typeIn(page, "/ex");
    await menuShown(page);
    assert.ok(!(await menu(page)).items.some((i) => i.name === "exit"), "exit 줄은 없다");
    await typeIn(page, "/exit");
    await box(page).press("Enter");
    const dialog = page.getByRole("alertdialog");
    await dialog.waitFor();
    assert.match(await dialog.textContent(), /\/exit를 보낼까요\?/);
    await shot(page, "51-confirm-exit-desktop");
    await dialog.getByRole("button", { name: "취소" }).click();
    await dialog.waitFor({ state: "detached" });
    assert.equal(bytes(), before, "보내지 않았다");
    assert.equal(await box(page).inputValue(), "/exit", "글은 그대로");

    await typeIn(page, "/clear");
    await box(page).press("Enter");
    await dialog.waitFor();
    assert.match(await dialog.textContent(), /맥락이 비워집니다/);
    await dialog.getByRole("button", { name: "보내기" }).click();
    await waitOutput(LABEL, "session 2 mode idle");
    assert.ok(bytes().slice(before.length).includes("/clear"));
    // SessionStart(clear) hook의 새 기록: 앞 대화와 게이지가 사라진다
    await domIs(page, "새 기록", () => !document.querySelector('[data-testid="chat-command"]') && ![...document.querySelectorAll('[data-role="user"]')].length
      && !document.querySelector('[data-testid="context-gauge"]'));
  });

  await step("작업 중: 쉴 때만 쓰는 /compact는 목록에 없다", async () => {
    await key("f7", "working");
    await domIs(page, "작업 중", () => document.querySelector('[data-testid="agent-chip"]')?.classList.contains("working"));
    await typeIn(page, "/c");
    await menuShown(page);
    const names = (await menu(page)).items.map((i) => i.name);
    assert.ok(!names.includes("compact") && names.includes("context"), names.join(","));
    await box(page).fill("");
    await key("f5", "idle");
  });

  await step("휴대폰(iPhone Safari): 「/」 목록을 눌러 넣는다", async () => {
    const wk = await webkit.launch();
    try {
      const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone");
      await openChat(p);
      await typeIn(p, "/");
      await menuShown(p);
      await shot(p, "51-menu-iphone");
      await p.locator('[data-testid="slash-item"][data-name="hello"]').click();
      await menuGone(p);
      assert.equal(await box(p).inputValue(), "/hello ");
      await p.context().close();
    } finally {
      await wk.close();
    }
  });
}
