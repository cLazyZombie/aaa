// 설치 창의 OS 키 전달은 별도 실기 항목이다. 여기서는 standalone 조건을 주입하고
// Playwright 입력(Chromium CDP·WebKit protocol)으로 실제 앱·host·PTY의 경계를 검사한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { selectInputAndWait } from "../lib/input-selection.mjs";
import { saveInputScrollFailure } from "../lib/input-scroll-diagnostics.mjs";
import { root, AA, aaa, step, browser, webkit, pairPage, inList, openSettings, until, screenText, paneHash, sentCount, sentFrames, sentText, shot, waitOutput } from "../lib/harness.mjs";

async function chatFocusCase(engine, width) {
  const label = `${engine}-${width}`;
  const dir = `${root}/claude/projects/-lab-focus-${label}`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/focus.jsonl`;
  fs.writeFileSync(transcript, JSON.stringify({ type: "user", message: { content: "focus 복원 시험" } }) + "\n");
  const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "7f6e5d4c-3b2a-4918-87f6-e5d4c3b2a190", source: "startup", transcript_path: transcript });
  const names = ["a", "b", "c", "d"].map(s => `focus-${label}-${s}`);
  for (const name of names) aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA, hook);
  const ids = await until("focus 대화 pane 준비", () => {
    const panes = aaa("pane", "list").panes;
    const found = names.map(name => panes.find(p => p.name === name && p.agent?.harness === "claude")?.id);
    return found.every(Boolean) && found;
  });
  const wk = engine === "webkit" ? await webkit.launch({ headless: true }) : null;
  try {
    const { page: p, errors } = await pairPage(wk ?? browser, aaa("web", "pair").code, { width, height: 844 }, {
      hash: `#pane=${ids[0]}`,
      init: () => {
        localStorage.setItem("aaa:default-view", "chat");
        const media = window.matchMedia.bind(window);
        window.matchMedia = query => { const value = media(query); if (query === "(display-mode: standalone)") Object.defineProperty(value, "matches", { value: true }); return value; };
        window.chatFocusLog = [];
        for (const type of ["focusin", "focusout", "select", "input", "keydown", "keyup", "pointerup", "scroll"]) document.addEventListener(type, e => {
          if (e.target.matches?.(".composer-text, .xterm-helper-textarea")) window.chatFocusLog.push({ type, pane: e.target.closest(".term")?.dataset.pane, kind: e.target.className, hidden: e.target.closest(".term").hidden, start: e.target.selectionStart, end: e.target.selectionEnd, top: e.target.scrollTop });
        }, true);
      },
    });
    try {
      const modal = p.getByTestId("recent-switcher");
      const input = () => p.locator('.term:not([hidden]) textarea.composer-text');
      const opened = async index => {
        await p.locator(`.term[data-pane="${ids[index]}"]:not([hidden]) [data-testid="chat"]`).waitFor();
        await p.locator('.term:not([hidden]) [data-testid="terminal"][data-readonly="false"]').waitFor({ state: "attached" });
      };
      const focused = async selector => p.waitForFunction(selector => document.activeElement?.matches(selector), selector);
      const go = async index => { await p.evaluate(id => { location.hash = `pane=${id}`; }, ids[index]); await opened(index); };
      const cycle = async () => { await p.keyboard.press("Control+Tab"); await modal.waitFor({ state: "detached" }); };
      const state = () => input().evaluate(el => ({ text: el.value, start: el.selectionStart, end: el.selectionEnd, direction: el.selectionDirection }));
      const select = async (start, end = start, direction = "forward") => input().evaluate(selectInputAndWait, [start, end, direction]);
      await opened(0);
      if (width === 1280) await focused(".composer-text");
      else assert.deepEqual(await p.evaluate(() => window.chatFocusLog), [], `${label}: 처음 대화를 열 때 자동 자판 focus`);
      await input().fill("첫 대화의 앞 중간 뒤"); await select(6);
      const first = await state();
      await go(1);
      if (width === 1280) await focused(".composer-text");
      await input().fill("둘째 대화의 선택 범위"); await select(3, 8, "backward");
      const second = await state();
      const from = await sentCount(p);
      await cycle(); await opened(0);
      assert.deepEqual(await state(), first, `${label}: 첫 대화의 초안·중간 커서`);
      if (width === 1280) {
        await focused(".composer-text");
        await p.keyboard.insertText("삽입");
        assert.equal(await input().inputValue(), first.text.slice(0, 6) + "삽입" + first.text.slice(6), "복귀 직후 커서 위치에서 입력");
      }
      await cycle(); await opened(1);
      assert.deepEqual(await state(), second, `${label}: 둘째 대화의 역방향 선택`);
      if (width === 1280) {
        await focused(".composer-text");
        await p.keyboard.insertText("교체");
        assert.equal(await input().inputValue(), second.text.slice(0, 3) + "교체" + second.text.slice(8), "복귀 직후 선택한 범위 교체");
        const edited = await state();
        // 같은 pane을 뒤에 둔 패널에서 복귀해도 active 변화 없이 focus를 돌린다.
        await inList(p, "activity-automations");
        await p.locator('#sidebar-panel-automations:not([hidden])').waitFor();
        await cycle(); await opened(1); await focused(".composer-text");
        assert.deepEqual(await state(), edited, "패널에서 같은 대화로 focus 복귀");
        // 패널 방문도 MRU에 들어간다. 뒤의 두 대화 왕복은 방문 순서를 다시 준비한다.
        await go(0); await go(1);
        assert.deepEqual(await state(), edited, "패널을 다녀온 뒤 연속 복귀 커서");
        // 입력칸 밖을 누른 사건을 마지막 focus 대상으로 잘못 기록하지 않는다.
        await p.getByTestId("chat").dispatchEvent("pointerup");
        await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
        // 선택 창이 실제 focus를 얻은 뒤 취소한다(빠른 keyup만 검사하지 않는다).
        await focused('[role="listbox"]');
        await p.keyboard.press("Escape"); await p.keyboard.up("Control");
        await modal.waitFor({ state: "detached" }); await focused(".composer-text");
        assert.deepEqual(await state(), edited, "Esc 취소 뒤 같은 커서");
        // 본문의 버튼을 마지막으로 썼다면 그 대상으로 돌아간다.
        const copy = p.locator('.term:not([hidden]) [aria-label="메시지 복사"]');
        await copy.focus();
        await cycle(); await opened(0); await cycle(); await opened(1);
        await focused('[aria-label="메시지 복사"]');
        // 마지막 대상이 사라지면 기본 입력으로 간다.
        await p.getByTestId("chat-find-open").click();
        await p.getByRole("searchbox", { name: "대화에서 찾기" }).waitFor();
        await focused(".chat-find-input");
        await cycle(); await opened(0); await cycle(); await opened(1);
        await focused(".composer-text");
        // 대화 DOM이 LRU에서 사라진 뒤에도 초안과 커서 숫자는 복원한다.
        await select(2, 5, "backward");
        const beforeEviction = await state();
        await go(0); await go(2); await go(3);
        await p.locator(`.term[data-pane="${ids[1]}"]`).waitFor({ state: "detached" });
        await go(1); await focused(".composer-text");
        assert.deepEqual(await state(), beforeEviction, "DOM 보관 종료 뒤 초안·선택 복원");
        await input().fill(Array.from({ length: 40 }, (_, i) => `${i}번째 작성 중인 줄`).join("\n"));
        await select(35, 50, "backward");
        await input().evaluate(el => { el.scrollTop = 37; });
        const longDraft = await state(), scroll = await input().evaluate(el => el.scrollTop);
        assert.ok(scroll > 0, "넘치는 입력창 스크롤 fixture");
        await cycle(); await opened(3); await cycle(); await opened(1); await focused(".composer-text");
        assert.deepEqual(await state(), longDraft, "긴 초안의 선택 복원");
        assert.equal(await input().evaluate(el => el.scrollTop), scroll, "입력창 내부 스크롤 복원");
      } else {
        // 터치로 다른 대화를 열고 돌아올 때 focus 사건이 없어야 한다.
        await input().blur();
        await p.evaluate(() => { window.chatFocusLog = []; });
        await inList(p, `pane-${names[0]}`); await opened(0);
        await inList(p, `pane-${names[1]}`); await opened(1);
        assert.deepEqual(await p.evaluate(() => window.chatFocusLog), [], `${label}: 터치 전환이 화면 자판 입력에 focus`);
      }
      assert.equal(sentText(await sentFrames(p, from)), "", `${label}: 전환 키·작성 중 글이 PTY로 전달됨`);
      assert.equal(await p.locator('.term[hidden]:not([inert])').count(), 0);
      if (errors.length) throw new Error(errors.join("; "));
    } catch (error) {
      await saveInputScrollFailure(root, p, `chat-focus-${label}`, error)
        .catch(cause => console.error("대화 focus 진단 저장 오류", String(cause)));
      throw error;
    } finally { await p.context().close(); }
  } finally { await wk?.close(); }
  for (const name of names) aaa("pane", "kill", name);
}

export default async function () {
  await step("PWA 대화 focus chromium-1280: 초안·커서·선택·복원·PTY 입력 차단", () => chatFocusCase("chromium", 1280));
  await step("PWA 대화 focus chromium-390: 초안·커서·선택·복원·PTY 입력 차단", () => chatFocusCase("chromium", 390));
  await step("PWA 대화 focus webkit-1280: 초안·커서·선택·복원·PTY 입력 차단", () => chatFocusCase("webkit", 1280));
  await step("PWA 대화 focus webkit-390: 초안·커서·선택·복원·PTY 입력 차단", () => chatFocusCase("webkit", 390));
  await step("PWA 최근 화면: 고정 MRU·역방향·확정·Esc·blur·종료·팔레트 공유·설정 끄기·터미널 입력 경계 (Chromium·WebKit)", async () => {
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, name, platform] of [[browser, "chromium", "Win32"], [wk, "webkit", "MacIntel"]]) {
        const names = ["a", "b", "c"].map((s) => `recent-${name}-${s}`);
        for (const label of names) aaa("tab", "create", "--machine", "labA", "--label", label, "--", process.execPath, `${root}/keys.js`);
        const ids = names.map((name) => aaa("pane", "list").panes.find((pane) => pane.name === name).id);
        const { page: p, errors } = await pairPage(engine, aaa("web", "pair").code, { width: 1100, height: 800 }, {
          platform, hash: paneHash(names[0]),
          init: () => {
            // 네이티브 예약 키를 우회하는 조건 주입이다. 실기 성공으로 세지 않는다.
            const media = window.matchMedia.bind(window);
            window.matchMedia = (query) => { const value = media(query); if (query === "(display-mode: standalone)") Object.defineProperty(value, "matches", { value: true }); return value; };
          },
        });
        try {
          const modal = p.getByTestId("recent-switcher");
          const selected = () => modal.getByRole("option", { selected: true }).locator("span").innerText();
          const history = () => p.evaluate(() => JSON.parse(localStorage.getItem("aaa:palette-recent") ?? "[]"));
          const address = () => p.evaluate(() => decodeURIComponent(location.hash));
          const at = async (index) => until(`최근 화면 ${names[index]}`, async () => (await address()) === `#pane=${ids[index]}`);
          const focus = async () => { await until("키 프로그램 준비", async () => (await screenText(p)).includes("keys>")); await p.locator(".xterm-screen").click(); };
          const open = async (index) => { await inList(p, `pane-${names[index]}`); await at(index); await focus(); };
          const noLeak = async (from) => assert.equal(sentText(await sentFrames(p, from)), "", `${name}: 전환 키가 PTY로 샜다`);
          await open(0); await open(1); await open(2);
          const original = ids.slice().reverse().map((id) => `pane:${id}`);
          assert.deepEqual(await history(), original);

          let from = await sentCount(p);
          await p.keyboard.down("Control"); await p.keyboard.press("Tab");
          await modal.waitFor();
          assert.equal(await selected(), names[1]); await at(2);
          await p.keyboard.press("Tab"); assert.equal(await selected(), names[0]);
          await p.keyboard.press("Shift+Tab"); assert.equal(await selected(), names[1]);
          assert.deepEqual(await history(), original);
          assert.equal(await modal.getByRole("option").evaluateAll((rows) => rows.every((row) => {
            const box = row.getBoundingClientRect();
            return [...row.children].every((child) => { const text = child.getBoundingClientRect(); return text.top >= box.top && text.bottom <= box.bottom; }) && row.scrollWidth <= row.clientWidth;
          })), true, "두 줄 항목의 글이 행 밖으로 넘친다");
          await shot(p, `recent-${name}`);
          await p.keyboard.press("Escape"); await p.keyboard.up("Control");
          await modal.waitFor({ state: "detached" }); await at(2);
          assert.deepEqual(await history(), original); await noLeak(from);
          assert.equal(await p.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")), true);

          // 확정한 것만 앞에 놓는다. 다음 짧은 Ctrl+Tab은 바로 전 화면으로 돌아간다.
          from = await sentCount(p);
          await p.keyboard.press("Control+Tab"); await at(1);
          assert.deepEqual(await history(), [`pane:${ids[1]}`, `pane:${ids[2]}`, `pane:${ids[0]}`]);
          await focus(); await p.keyboard.press("Control+Tab"); await at(2);
          assert.deepEqual(await history(), original); await noLeak(from);
          await focus(); await p.keyboard.press("Control+Shift+Tab"); await at(0);
          const afterReverse = [`pane:${ids[0]}`, `pane:${ids[2]}`, `pane:${ids[1]}`];
          assert.deepEqual(await history(), afterReverse);

          // Shift 뗌은 확정하지 않는다. 창을 벗어나면 취소하며 늦은 Ctrl 뗌도 무시한다.
          await focus(); from = await sentCount(p);
          await p.keyboard.down("Control"); await p.keyboard.press("Shift+Tab"); await modal.waitFor();
          assert.equal(await selected(), names[1]);
          await p.evaluate(() => window.dispatchEvent(new Event("blur")));
          await modal.waitFor({ state: "detached" }); await p.keyboard.up("Control");
          await at(0); assert.deepEqual(await history(), afterReverse); await noLeak(from);

          // 새로고침 후에도 이 기기의 기록을 쓴다. 팔레트도 확정된 순서를 공유한다.
          await p.reload(); await focus();
          assert.deepEqual(await history(), afterReverse);
          await p.keyboard.press(platform === "MacIntel" ? "Meta+Shift+K" : "Control+Shift+K");
          const palette = p.getByTestId("command-palette"); await palette.waitFor();
          // 현재 화면을 뺀 최근 두 줄이 먼저 보인다.
          const firstRows = palette.locator('[data-target]');
          assert.deepEqual(await firstRows.evaluateAll((rows) => rows.slice(0, 2).map((row) => row.dataset.target)), [`pane:${ids[2]}`, `pane:${ids[1]}`]);
          await p.keyboard.press("Control+Tab"); assert.equal(await modal.count(), 0); await at(0);
          await p.keyboard.press("Escape"); await palette.waitFor({ state: "detached" });

          // 설정에서 끄면 전환 창 대신 원래 xterm 경로로 보낸다. 다시 켜고 종료 경계를 검사한다.
          const settings = await openSettings(p, "단축키");
          const toggle = settings.getByRole("switch", { name: "최근 화면 전환", exact: true });
          await toggle.click(); assert.equal(await toggle.getAttribute("aria-checked"), "false");
          await settings.getByRole("button", { name: "설정 닫기" }).click(); await focus();
          from = await sentCount(p); await p.keyboard.press("Control+Tab");
          assert.equal(await modal.count(), 0); await at(0);
          const disabledBytes = sentText(await sentFrames(p, from));
          assert.notEqual(disabledBytes, "", "꺼진 키가 xterm에 전달되지 않았다");
          const settingsAgain = await openSettings(p, "단축키");
          await settingsAgain.getByRole("switch", { name: "최근 화면 전환", exact: true }).click();
          await settingsAgain.getByRole("button", { name: "설정 닫기" }).click(); await focus();

          const afterSettings = await history();
          assert.ok(afterSettings.includes("settings:"));
          from = await sentCount(p); await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
          for (let n = 0; n < afterSettings.length && await selected() !== names[2]; n++) await p.keyboard.press("Tab");
          assert.equal(await selected(), names[2]);
          aaa("pane", "kill", names[2]);
          await modal.waitFor({ state: "detached" }); await p.keyboard.up("Control");
          await at(0); assert.deepEqual(await history(), afterSettings); await noLeak(from);
          await focus(); await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
          assert.deepEqual(await modal.getByRole("option").locator("span").allTextContents(), [names[0], "설정", names[1]]);
          await p.keyboard.press("Escape"); await p.keyboard.up("Control");

          // 일반 탭에서도 똑같이 누른 키가 기존 xterm 경로로 간다(standalone 주입 없음).
          const normal = (await pairPage(engine, aaa("web", "pair").code, { width: 1100, height: 800 }, { platform, hash: paneHash(names[1]) })).page;
          try {
            await until("일반 탭 키 프로그램", async () => (await screenText(normal)).includes("keys>"));
            await normal.locator(".xterm-screen").click();
            const n = await sentCount(normal); await normal.keyboard.press("Control+Tab");
            assert.equal(await normal.getByTestId("recent-switcher").count(), 0);
            assert.equal(await normal.evaluate(() => decodeURIComponent(location.hash)), `#pane=${ids[1]}`);
            assert.equal(sentText(await sentFrames(normal, n)), disabledBytes);
          } finally { await normal.context().close(); }
          if (errors.length) throw new Error(errors.join("; "));
        } finally { await p.context().close(); }
      }
    } finally { await wk.close(); }
  });
  await step("PWA 앱 화면: 보고서·할 일·자동화·설정·답할 것·작업 공간 왕복과 coordinator 표시", async () => {
    aaa("project", "create", "demo");
    const coordinator = aaa("project", "open", "demo", "--profile", "fake").pane;
    await waitOutput(coordinator.id, "fake>");
    const input = `${root}/recent-report`; fs.mkdirSync(input);
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><meta charset=utf-8><h1>최근 보고서 본문</h1>");
    const report = aaa("library", "publish", input, "--name", "recent-report", "--to-user").report_id;
    aaa("task", "add", "최근 할 일");
    const wk = await webkit.launch({ headless: true });
    try {
      for (const engine of [browser, wk]) for (const width of [1280, 390]) {
        const name = `${engine.browserType().name()}-${width}`;
        const { page: p, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
          hash: `#pane=${coordinator.id}`,
          init: () => {
            const media = window.matchMedia.bind(window);
            window.matchMedia = query => { const value = media(query); if (query === "(display-mode: standalone)") Object.defineProperty(value, "matches", { value: true }); return value; };
          },
        });
        try {
          const modal = p.getByTestId("recent-switcher");
          // 휴대폰 시트의 닫힘 그림은 closed shadow root에 남는다. 실제 animationend의 제거 뒤에 촬영한다.
          const capture = async label => {
            await p.waitForFunction(() => document.querySelector('body > div[inert][aria-hidden="true"]') === null);
            await p.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
            await shot(p, label);
          };
          const address = () => p.evaluate(() => decodeURIComponent(location.hash));
          const history = () => p.evaluate(() => JSON.parse(localStorage.getItem("aaa:palette-recent") ?? "[]"));
          const go = async hash => { await p.evaluate(hash => { location.hash = hash; }, hash); await until("본문 주소", async () => await address() === `#${hash}`); };
          const switchTo = async (title, hash) => {
            const before = await address(), recent = await history();
            await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
            const rows = await modal.getByRole("option").locator(":scope > span").allTextContents();
            assert.ok(rows.includes(title), `${name}: ${title} 누락: ${rows}`);
            for (let n = 0; n < rows.length && await modal.getByRole("option", { selected: true }).locator(":scope > span").innerText() !== title; n++) await p.keyboard.press("Tab");
            assert.equal(await address(), before, "Ctrl 유지 중 주소 불변");
            assert.deepEqual(await history(), recent, "Ctrl 유지 중 MRU 불변");
            await p.keyboard.up("Control"); await modal.waitFor({ state: "detached" });
            if (hash === "automations") {
              await p.locator('#sidebar-panel-automations:not([hidden])').waitFor();
              assert.equal(await p.getByTestId("activity-automations").getAttribute("aria-selected"), "true");
              assert.equal(await address(), before, "자동화 목록은 보던 본문을 유지");
            }
            else if (hash === "settings") await p.getByRole("region", { name: "설정", exact: true }).waitFor();
            else await until(`Ctrl 해제 ${title}`, async () => await address() === `#${hash}` && await p.locator("#settings-screen").count() === 0);
          };
          await p.locator('.term:not([hidden]) > .term-head').waitFor();
          await inList(p, "activity-reports"); await p.getByTestId("reports-panel").waitFor();
          await inList(p, "activity-tasks"); await p.getByTestId("tasks-view").waitFor();
          await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
          await capture(`recent-app-initial-${name}`);
          assert.ok((await modal.getByRole("option").locator(":scope > span").allTextContents()).includes("보고서"), "방문한 보고서 목록 누락");
          await p.keyboard.press("Escape"); await p.keyboard.up("Control");
          await inList(p, "activity-automations"); await p.locator('#sidebar-panel-automations:not([hidden])').waitFor();
          await inList(p, "activity-asks"); await p.locator('#sidebar-panel-asks:not([hidden])').waitFor();
          await inList(p, "activity-workspace"); await until("작업 공간 주소", async () => (await address()).startsWith("#workspace"));
          const settings = await openSettings(p, "단축키");
          // 단축키 기록은 설정 화면에서도 우선한다.
          await settings.locator('.settings-shortcut-record').first().click();
          await p.keyboard.press("Control+Tab"); assert.equal(await modal.count(), 0);
          await p.keyboard.press("Escape");
          const screens = [["보고서", "reports"], ["할 일", "tasks"], ["자동화", "automations"], ["답할 것", "asks"], ["작업 공간", "workspace="], ["설정", "settings"]];
          for (const [title, hash] of screens) await switchTo(title, hash);
          // 목록뿐 아니라 개별 보고서 주소도 다시 연다.
          await switchTo("보고서", "reports");
          await go(`report=demo/${report}`);
          await p.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "최근 보고서 본문" }).waitFor();
          await inList(p, "activity-tasks");
          await switchTo(`보고서 · demo/${report}`, `report=demo/${report}`);
          await p.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "최근 보고서 본문" }).waitFor();
          await switchTo("할 일", "tasks");
          const taskRow = p.locator('.task-row[data-title="최근 할 일"]');
          const taskSelected = async () => {
            // 휴대폰은 상세를 열면 목록을 숨긴다. 보이는 상세와 보존한 선택을 각각 확인한다.
            await p.getByTestId("task-field-title").waitFor();
            assert.equal(await p.getByTestId("task-field-title").inputValue(), "최근 할 일");
            await p.locator('.task-row[data-title="최근 할 일"].is-selected').waitFor({ state: "attached" });
          };
          const taskId = await taskRow.getAttribute("data-short-id");
          // 넓은 화면의 목록 안 선택은 주소를 유지한다. 휴대폰의 주소 없는 상세 시트는 이동하면 닫히는 기존 동작이다.
          if (width >= 769) {
            await taskRow.locator('.task-pick').click();
            await taskSelected();
            await switchTo("보고서", "reports");
            await switchTo("할 일", "tasks");
            await taskSelected();
          }
          // 공유한 할 일 주소로 방문했을 때는 그 주소 자체를 복원한다.
          await go(`task=${taskId}`);
          await taskSelected();
          await switchTo("보고서", "reports");
          await switchTo(`할 일 · ${taskId}`, `task=${taskId}`);
          await taskSelected();
          await switchTo(coordinator.name, `pane=${coordinator.id}`);
          for (const theme of ["light", "dark"]) {
            await p.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
            const head = p.locator('.term:not([hidden]) > .term-head');
            assert.notEqual(await head.evaluate(el => getComputedStyle(el).boxShadow), "none");
            await capture(`coordinator-head-${name}-${theme}`);
            // 이미 선택된 작업 단추를 다시 누르면 접히므로, 현재 목록을 그대로 확인한다.
            if (width < 769) {
              await p.locator('.view [data-testid="drawer-toggle"]:visible').click();
              await p.locator("#workspace-drawer.is-open").waitFor();
            }
            const row = p.locator('.workspace[data-origin="coordinator"]');
            await row.locator('.coordinator-label').waitFor();
            assert.notEqual(await row.locator('.workspace-header').evaluate(el => getComputedStyle(el).boxShadow), "none");
            await capture(`coordinator-row-${name}-${theme}`);
            await inList(p, `pane-${coordinator.name}`);
            await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
            const option = modal.locator('[data-screen-role="coordinator"]');
            assert.equal(await option.locator('.coordinator-label').innerText(), "coordinator");
            assert.notEqual(await option.evaluate(el => getComputedStyle(el).boxShadow), "none");
            assert.equal(await option.evaluate(el => el.scrollWidth <= el.clientWidth), true);
            assert.equal(await head.evaluate(el => el.scrollWidth <= el.clientWidth), true);
            await capture(`recent-app-${name}-${theme}`);
            await p.keyboard.press("Escape"); await p.keyboard.up("Control");
          }
          if (errors.length) throw new Error(errors.join("; "));
        } catch (error) {
          console.log("앱 전환 실패 화면", name, await p.evaluate(() => ({ hash: location.hash, recent: localStorage.getItem("aaa:palette-recent"), detail: document.querySelector('[data-testid="task-detail"]')?.textContent?.slice(0, 300) })));
          await shot(p, `recent-failure-${name}`);
          throw error;
        } finally { await p.context().close(); }
      }
    } finally { await wk.close(); }
  });

}
