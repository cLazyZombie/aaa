// 실제 host·agent·coordinator 자식 CLI. 화면은 Chromium·WebKit 데스크톱/휴대폰에서 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import crypto from "node:crypto";
import { root, AA, aaa, until, step, browser, webkit, pairPage, waitOutput, domIs, shot, start, stop } from "../lib/harness.mjs";

const quote = s => `'${s.replaceAll("'", "'\\''")}'`;
const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8").replace('[profiles.fake]\nagent = "custom"', '[profiles.fake]\nagent = "claude"'));
const transcript = `${root}/claude/projects/-lab-auto/auto.jsonl`;
fs.mkdirSync(`${root}/claude/projects/-lab-auto`, { recursive: true });
fs.writeFileSync(transcript, [
  { type: "user", message: { role: "user", content: "자리를 비울게. auto mode로 계속 진행해." } },
  { type: "assistant", message: { content: [{ type: "text", text: "설계를 확인했다. 질문의 근거를 살펴 진행한다." }] } },
].map(o => `${JSON.stringify(o)}\n`).join(""));
const hook = { hook_event_name: "SessionStart", session_id: "6a1f0c2e-3b4d-4e5f-8a9b-0c1d2e3f4a99", source: "startup", transcript_path: transcript };
fs.writeFileSync(`${root}/fake.sh`, `#!/bin/sh
if [ "$1" = "--version" ]; then printf 'Claude Code test\\n'; exit 0; fi
${quote(AA)} hook --agent claude ${quote(JSON.stringify(hook))} > ${quote(`${root}/auto-hook.json`)}
printf 'auto-ready> '
while IFS= read -r line; do
  case "$line" in post-probe\\ *) /bin/sh "\${line#post-probe }";; esac
  printf 'auto-ready> '
done
`);
const call = (p, req) => p.evaluate(async req => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(req) })).json(), req);
let coord;
async function run(args) {
  const id = crypto.randomBytes(8).toString("hex");
  const file = `${root}/auto-${id}`;
  fs.writeFileSync(`${file}.sh`, `${quote(AA)} --json ${args.map(quote).join(" ")} > ${quote(`${file}.json`)} 2>&1\nprintf 'auto-${id}-done\\n'\n`);
  aaa("pane", "run", coord, `post-probe ${file}.sh`);
  await waitOutput(coord, `auto-${id}-done`);
  return JSON.parse(fs.readFileSync(`${file}.json`, "utf8"));
}
async function showCoordinator(p) {
  await p.evaluate(id => { location.hash = `pane=${id}`; }, coord);
  await p.getByTestId("chat-toggle").waitFor();
  if (!(await p.getByTestId("chat").count())) await p.getByTestId("chat-toggle").click();
  await p.locator(".composer-surface.is-coordinator").waitFor();
}
async function menuToggle(p) {
  await p.locator('.term-head [data-testid="more"]').click();
  await p.getByTestId("auto-mode-menu").click();
}
export default async function () {
  let desktop, mirror, ids, safari;
  const pages = [];
  try {
  await step("auto mode: 메뉴 켜기·두 기기 방송·host 재시작 유지·coordinator hook", async () => {
    aaa("project", "create", "demo");
    coord = aaa("project", "open", "demo", "--profile", "fake").pane.id;
    await waitOutput(coord, "auto-ready>");
    desktop = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 844 }, { serviceWorkers: "block" })).page;
    mirror = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { serviceWorkers: "block" })).page;
    pages.push(desktop, mirror);
    await showCoordinator(desktop); await showCoordinator(mirror);
    assert.equal(await desktop.getByTestId("auto-mode-switch").count(), 0);
    await menuToggle(desktop);
    await desktop.getByTestId("auto-mode-switch").waitFor(); await mirror.getByTestId("auto-mode-switch").waitFor();
    const mode = aaa("project", "auto-mode");
    assert.equal(mode.on, true);
    await stop("host"); start("host", "host");
    await until("host와 agent 재연결", () => aaa("machine", "list").machines.some(m => m.name === "labA" && m.connection === "connected"));
    assert.deepEqual(aaa("project", "auto-mode"), mode);
    await desktop.reload(); await showCoordinator(desktop); await desktop.getByTestId("auto-mode-switch").waitFor();
    const context = await run(["context"]);
    assert.equal(context.auto_mode.on, true);
    assert.match(context.auto_mode_exceptions, /비밀값.*force push.*release/);
    // compaction hook은 현재 host 상태를 읽고 additionalContext에 싣는다.
    const file = `${root}/compact.sh`;
    fs.writeFileSync(file, `${quote(AA)} hook --agent claude ${quote(JSON.stringify({ ...hook, source: "compact" }))} > ${quote(`${root}/compact.json`)}\nprintf 'auto-compact-done\\n'\n`);
    aaa("pane", "run", coord, `post-probe ${file}`); await waitOutput(coord, "auto-compact-done");
    assert.match(fs.readFileSync(`${root}/compact.json`, "utf8"), /auto_mode: on/);
  });
  await step("auto mode: 실제 자동 답·예외 카드 유지·post 기록·외부 푸시 제외", async () => {
    fs.writeFileSync(`${root}/auto-questions.md`, "## Questions\n### 1. 글자 렌더 방식\n실측 결과를 비교해 고른다.\n- A Canvas (추천) — 기본안\n- B WebGL 유지 — 스크롤 보존\n### 2. [사람] 서비스 설치 승인\n사용자가 기기 변경을 정한다.\n- A 승인\n- B 보류\n");
    const posted = await run(["post", "user", "--request", "--needs-user", "--topic", "user-action", "--title", "렌더 방식과 사용자 승인", "--body-file", `${root}/auto-questions.md`]);
    assert.equal(posted.state, "accepted");
    const asks = (await call(desktop, { op: "asks" })).ok;
    ids = asks.requests[0].questions.items.map(q => q.id);
    fs.writeFileSync(`${root}/auto-reason.txt`, "실측에서 WebGL 보정으로 거칠기가 사라졌고 스크롤 성능도 유지됐다.");
    const result = await run(["question", "answer", ids[0], "B", "--auto", "--reason-file", `${root}/auto-reason.txt`]);
    assert.equal(result.auto_decisions[0].choice, "B");
    assert.equal(result.auto_decisions[0].decided_by, "coordinator(auto)");
    const denied = await run(["question", "answer", ids[1], "B", "--auto", "--reason-file", `${root}/auto-reason.txt`]);
    assert.match(JSON.stringify(denied), /사용자 전용/);
    assert.equal((await call(desktop, { op: "asks" })).ok.requests.length, 1);
    fs.writeFileSync(`${root}/auto-note.json`, JSON.stringify({ question: "다음 구현 순서", answer: "입력창부터", reason: "사용자 상태를 먼저 표시", action: "구현 순서를 정함", reversible: true }));
    assert.equal((await run(["post", "user", "--data", "--topic", "auto-decision", "--title", "구현 순서", "--body-file", `${root}/auto-note.json`])).state, "accepted");
    await domIs(desktop, "자동 결정 두 건과 예외 한 건", () => document.querySelector('[data-testid="auto-status"]')?.textContent.includes("자동 결정 2") && document.querySelector('[data-testid="auto-status"]')?.textContent.includes("예외 1 기다림"));
    // 실제 notify 큐의 자동 결정 제외는 Rust owner가 단언한다. UI 알림 수로 외부 푸시를 추정하지 않는다.
  });
  await step("auto mode: Chromium·WebKit 데스크톱·휴대폰 오로라와 결정 카드·다시 정하기", async () => {
    safari = await webkit.launch({ headless: true });
    for (const [engine, name] of [[browser, "chromium"], [safari, "webkit"]]) {
      for (const width of [1280, 390]) {
        const p = (await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { serviceWorkers: "block" })).page;
        pages.push(p); await showCoordinator(p); await p.getByTestId("auto-mode-switch").waitFor();
        const duration = await p.locator(".composer-glow").evaluate(el => getComputedStyle(el, "::before").animationDuration);
        assert.equal(duration, "2.6s");
        assert.equal(await p.getByRole("textbox", { name: "메시지", exact: true }).getAttribute("placeholder"), "coordinator에게 지시하기");
        assert.equal(await p.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
        assert.equal(await p.locator(".auto-lines").evaluate(el => [...el.querySelectorAll("button")].every(b => b.scrollWidth <= b.clientWidth + 1 && b.scrollHeight <= b.clientHeight + 1)), true, "자동 결정 줄은 휴대폰 폭에서 줄바꿈한다");
        await shot(p, `99-${name}-${width}-on`);
        await p.emulateMedia({ reducedMotion: "reduce" });
        assert.equal(await p.locator(".composer-glow").evaluate(el => getComputedStyle(el).display), "none");
        await p.emulateMedia({ reducedMotion: "no-preference" });
        await p.evaluate(id => { location.hash = `question=${id}`; }, ids[0]);
        await p.getByTestId("auto-decision-card").waitFor();
        assert.match(await p.getByTestId("auto-decision-card").textContent(), /coordinator가 고름/);
        assert.equal(await p.getByTestId("auto-decision-card").evaluate(el => el.scrollHeight <= el.clientHeight + 1), true, "카드 자체를 자르지 않고 본문이 굴러간다");
        await p.getByRole("button", { name: "다른 답으로 다시 정하기" }).scrollIntoViewIfNeeded();
        await shot(p, `99-${name}-${width}-decision`);
      }
    }
    await desktop.evaluate(id => { location.hash = `question=${id}`; }, ids[0]);
    await desktop.getByRole("button", { name: "다른 답으로 다시 정하기" }).click();
    await desktop.getByLabel("새 답", { exact: true }).fill("A를 다시 검토해라");
    await desktop.getByRole("button", { name: "새 답 보내기" }).click();
    await desktop.getByText("다시 정한 답: A를 다시 검토해라 · coordinator에 전달함").waitFor();
    const record = (await call(desktop, { op: "asks" })).ok.auto_decisions.find(d => d.question.id === ids[0]);
    assert.equal(record.choice, "B"); assert.equal(record.reviewed, true);
    assert.equal(record.reconsideration, "A를 다시 검토해라");
  });
  await step("auto mode: 즉시 끄기·한 번 요약·세 끄기 경로·Reader 거부·기록 보존", async () => {
    await showCoordinator(desktop);
    await desktop.getByTestId("auto-mode-switch").click();
    await desktop.getByTestId("auto-off-summary").waitFor();
    assert.match(await desktop.getByTestId("auto-off-summary").textContent(), /2개를 정했다/);
    await mirror.getByTestId("auto-mode-switch").waitFor({ state: "detached" });
    await shot(desktop, "99-desktop-off-summary"); await shot(mirror, "99-phone-off-summary");
    await desktop.getByRole("button", { name: "auto mode 요약 닫기" }).click();
    await desktop.reload(); await showCoordinator(desktop);
    assert.equal(await desktop.getByTestId("auto-off-summary").count(), 0);
    assert.equal((await call(desktop, { op: "asks" })).ok.auto_decisions.length, 2);
    await menuToggle(desktop); await desktop.getByTestId("auto-mode-switch").waitFor();
    await desktop.getByRole("button", { name: "눌러서 끄기", exact: true }).click(); await desktop.getByTestId("auto-mode-switch").waitFor({ state: "detached" });
    await menuToggle(desktop); await desktop.getByTestId("auto-mode-switch").waitFor();
    await menuToggle(desktop); await desktop.getByTestId("auto-mode-switch").waitFor({ state: "detached" });
    const reader = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, { serviceWorkers: "block" })).page;
    pages.push(reader);
    assert.ok((await call(reader, { op: "auto_mode_set", project: "demo", on: true })).err);
    assert.ok((await call(reader, { op: "question_auto_answer", project: "demo", id: ids[1], choice: "A", text: null, reason: "안 됨" })).err);
    assert.equal(aaa("project", "auto-mode").on, false);
  });
  await step("auto mode: 오래 켠 모드의 기록 페이지·옛 결정 직접 주소·재판단", async () => {
    aaa("project", "auto-mode", "on");
    // 질문 카드는 묶음당 9개까지다. 실제 제한 안에서 51개의 자동 답을 쌓는다.
    for (let offset = 0; offset < 51; offset += 9) {
      const size = Math.min(9, 51 - offset);
      fs.writeFileSync(`${root}/auto-many.md`, "## Questions\n" + Array.from({ length: size }, (_, n) => `### ${n + 1}. 기록 ${offset + n + 1}\n- A 먼저\n- B 나중\n`).join(""));
      const title = `오래 켠 모드 기록 ${offset}`;
      const posted = await run(["post", "user", "--request", "--needs-user", "--topic", "user-action", "--title", title, "--body-file", `${root}/auto-many.md`]);
      assert.equal(posted.state, "accepted");
      const request = (await call(desktop, { op: "asks" })).ok.requests.find(r => r.title === title);
      assert.equal(request.questions.items.length, size);
      for (const q of request.questions.items) {
        const result = await run(["question", "answer", q.id, "B", "--auto", "--reason-file", `${root}/auto-reason.txt`]);
        assert.equal(result.kind, "asks");
      }
    }
    const first = (await call(desktop, { op: "asks" })).ok;
    assert.equal(first.auto_decisions.length, 50); assert.ok(first.auto_next);
    await desktop.evaluate(() => { location.hash = "ask=a:"; }); await desktop.reload();
    await domIs(desktop, "최근 기록 50개", () => document.querySelector('[data-testid="asks-auto-count"]')?.textContent === "50");
    await desktop.getByRole("button", { name: "이전 자동 결정 더 보기" }).click();
    await domIs(desktop, "이전 기록까지 53개", () => document.querySelector('[data-testid="asks-auto-count"]')?.textContent === "53");
    await desktop.evaluate(id => { location.hash = `question=${id}`; }, ids[0]); await desktop.reload();
    await desktop.getByTestId("auto-decision-card").waitFor();
    assert.match(await desktop.getByTestId("auto-decision-card").textContent(), /글자 렌더 방식/);
    await desktop.getByRole("button", { name: "다른 답으로 다시 정하기" }).click();
    await mirror.evaluate(id => { location.hash = `question=${id}`; }, ids[0]); await mirror.reload();
    await mirror.getByTestId("auto-decision-card").waitFor();
    await desktop.getByLabel("새 답", { exact: true }).fill("오래된 결정도 A를 다시 검토해라");
    await desktop.getByRole("button", { name: "새 답 보내기" }).click();
    await desktop.getByText("다시 정한 답: 오래된 결정도 A를 다시 검토해라 · coordinator에 전달함").waitFor();
    await mirror.getByText("다시 정한 답: 오래된 결정도 A를 다시 검토해라 · coordinator에 전달함").waitFor();
    const old = (await call(desktop, { op: "auto_decisions", project: "demo", before: null, id: `auto-${ids[0]}` })).ok.decisions[0];
    assert.equal(old.choice, "B"); assert.equal(old.reviewed, true);
    aaa("project", "auto-mode", "off");
    for (const p of pages) await p.context().close();
  });
  } finally {
    await safari?.close();
  }
}
