// 아이디어(t-1260, 기획 t-1255 r-4673): 실제 host·agent·PTY의 가짜 thread profile로 노트·전용 thread·할 일 넘기기를 본다.
// 넓은 창: 활동 막대 「아이디어」 목록 → 노트 고치기(textarea·그림 붙여 넣기)와 판, 노트 글·그림 의견을 모아 「입력창에 넣기」,
// 대화 입력창으로 말하면 host가 host 머신에 전용 thread를 띄워 「(노트 고침: …)」을 붙여 보내고 thread가 판을 주고 노트를 고친다.
// 전용 thread는 작업 목록·할 일 화면에 없고 실행 한도에 세지 않는다. 보류한 thread는 아이디어 화면을 열면 알림 없이 잇는다.
// 「할 일로 넘기기」는 초안 둘을 할 일로 만들고(메모 첫 줄 아이디어 링크) 아이디어를 잠근다. 그 할 일의 thread는 노트와 그림을
// 작업 폴더 attachments/로 받는다. 휴대폰(WebKit 390)은 「노트 | 대화」를 쓸어 넘기고 대화 단추에 모은 의견 수가 보인다.
// 시간은 기다리지 않는다: host 기록·PTY 출력·DOM 표지를 본다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { root, aaa, step, webkit, pairPage, domIs, until, waitOutput, shot } from "../lib/harness.mjs";

const NOTE = "# 알림 묶기\n\n## 목표\n- 밤에 오는 알림을 묶는다\n\n## 결정\n- 하루 한 번 묶어 보낸다\n";

/** WebKit·Chromium 같은 touch 생성(98과 같은 꼴): 손가락 아래 요소에 낸다 */
function stroke({ type, x, y }) {
  const target = window.__ideaTouchTarget = type === "touchstart" ? document.elementFromPoint(x, y) : window.__ideaTouchTarget;
  const init = { identifier: 121, target, clientX: x, clientY: y, pageX: x, pageY: y, screenX: x, screenY: y };
  const touch = typeof document.createTouch === "function" ? document.createTouch(window, target, 121, x, y, x, y) : new Touch(init);
  const ended = type === "touchend";
  const list = (items) => typeof document.createTouchList === "function" ? document.createTouchList(...items) : items;
  const touches = list(ended ? [] : [touch]);
  target.dispatchEvent(new TouchEvent(type, { touches, targetTouches: touches, changedTouches: list([touch]), bubbles: true, cancelable: true }));
}

async function swipe(p, from, to, y = 400) {
  await p.evaluate(stroke, { type: "touchstart", x: from, y });
  for (const x of [from + (to - from) / 3, from + (2 * (to - from)) / 3, to]) await p.evaluate(stroke, { type: "touchmove", x, y });
  await p.evaluate(stroke, { type: "touchend", x: to, y });
}

/** 노트 칸에서 그 글을 고른다(보고 글 의견과 같은 띠가 뜬다) */
const pick = (p, text) => p.evaluate((text) => {
  const li = [...document.querySelectorAll('[data-testid="idea-note"] li')].find((el) => el.textContent === text);
  getSelection().selectAllChildren(li);
}, text);

export default async function ({ page }) {
  let idea, thread;
  await step("아이디어(t-1260): 목록·노트 고치기·그림·의견 모음·말하면 전용 thread가 노트를 판으로 고친다", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    aaa("project", "set", "demo", "max_parallel_threads", "1");
    idea = aaa("idea", "new", "알림 묶기").note.idea.short_id;
    assert.equal(idea, "n-0001");
    await page.evaluate((h) => (location.hash = h), `#idea=${idea}`);
    await page.getByTestId("idea-title").getByText("알림 묶기").waitFor();
    assert.equal(await page.getByRole("tab", { name: "아이디어" }).getAttribute("aria-selected"), "true");
    await page.getByTestId("ideas-panel").locator(`[data-id="${idea}"]`).getByText("알림 묶기").waitFor({ state: "visible" });

    // 직접 고치기: textarea 저장이 판 2, 그림 붙여 넣기와 저장이 판 3
    await page.getByTestId("idea-edit").click();
    await page.getByTestId("idea-editor-text").fill(NOTE);
    await page.getByTestId("idea-editor-save").click();
    await domIs(page, "판 2", () => document.querySelector('[data-testid="idea-note"]')?.dataset.rev === "2");
    assert.ok(aaa("idea", "show", idea).note.text === NOTE, "host 노트");
    await page.getByTestId("idea-edit").click();
    const png = [...fs.readFileSync(`${root}/pic.png`)];
    await page.getByTestId("idea-editor-text").evaluate((el, bytes) => {
      el.setSelectionRange(el.value.length, el.value.length);
      const dt = new DataTransfer();
      dt.items.add(new File([new Uint8Array(bytes)], "pic.png", { type: "image/png" }));
      el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    }, png);
    await domIs(page, "그림 줄", () => /!\[그림\]\([0-9a-f]{16}\.png\)/.test(document.querySelector('[data-testid="idea-editor-text"]')?.value ?? ""));
    await page.getByTestId("idea-editor-save").click();
    await domIs(page, "그림이 그려진 판 3", () => {
      const img = document.querySelector('[data-testid="idea-note"] img');
      return document.querySelector('[data-testid="idea-note"]')?.dataset.rev === "3" && img?.complete && img.naturalWidth > 0;
    });

    // 노트 글과 그림에 의견: 모았다가 「입력창에 넣기」로 한 번에 붙이고 모음을 비운다
    await pick(page, "하루 한 번 묶어 보낸다");
    await page.getByTestId("report-note").click();
    await page.getByTestId("quote-note").getByRole("textbox", { name: "의견" }).fill("주말은 빼자");
    await page.getByTestId("quote-note-submit").click();
    await page.getByTestId("idea-media-note").click();
    await page.getByTestId("quote-note").getByRole("textbox", { name: "의견" }).fill("휴대폰 모양도");
    await page.getByTestId("quote-note-submit").click();
    await domIs(page, "의견 둘", () => document.querySelectorAll('[data-testid="idea-tray-item"]').length === 2);
    await shot(page, "121-ideas-tray");
    await page.getByTestId("idea-tray-insert").click();
    const box = page.getByTestId("idea-chat").getByRole("textbox", { name: "메시지", exact: true });
    await domIs(page, "의견이 붙은 입력창", () => {
      const v = document.querySelector('[data-testid="idea-chat"] [aria-label="메시지"]')?.value ?? "";
      return v.includes("인용: n-0001 노트 「결정」\n> 하루 한 번 묶어 보낸다\n의견: 주말은 빼자") && /인용: n-0001 노트 「결정」\n> 그림 그림 \([0-9a-f]{16}\.png\)\n의견: 휴대폰 모양도/.test(v);
    });
    if (await page.getByTestId("idea-tray").count()) throw new Error("넣은 의견이 모음에 남았다");

    // 말하기: 전용 thread가 host 머신에 뜨고(실행 한도 1을 쓰지 않는다), 앞의 사람 고침 요약이 붙어 가고, thread가 판을 주고 저장한다
    await box.fill(`노트에 한 줄 더해 ${idea}`);
    await box.press("Enter");
    thread = await until("전용 thread", () => aaa("idea", "list").ideas[0].thread, 30_000);
    const t = await until("thread pane", () => { const v = aaa("thread", "show", "demo", thread); return v.pane && v.brief === "delivered" && v; }, 30_000);
    assert.equal(t.idea, idea);
    assert.equal(t.machine, "labA", "host 머신(로컬 agent)");
    await waitOutput(t.pane, "idea-saved", "recent", 30_000);
    const out = aaa("pane", "read", t.pane).text;
    assert.match(out, /\(노트 고침: 판 2 [^;]*; 판 3 「결정」 1줄 더함\)/, "사람이 고친 판 요약이 말 앞에 붙는다");
    await domIs(page, "thread가 고친 판 4", () => document.querySelector('[data-testid="idea-note"]')?.dataset.rev === "4"
      && document.querySelector('[data-testid="idea-note"]')?.textContent.includes("thread가 더한 줄"));
    assert.equal(aaa("idea", "show", idea).note.history[0].by, "thread");
    // 작업 목록·할 일 화면에 없다. 실행 한도 1에서도 일반 thread를 하나 더 띄울 수 있다
    if (await page.locator(`[data-testid="thread-demo-${thread}"]`).count()) throw new Error("작업 목록에 아이디어 thread");
    assert.ok(!aaa("task", "list").unlinked_threads.some((x) => x.id === thread), "할 일 화면에 아이디어 thread");
    fs.writeFileSync(`${root}/idea-other.md`, "다른 일");
    const other = aaa("thread", "start", "demo", "--title", "다른 일", "--kind", "dir", "--task-file", `${root}/idea-other.md`).id;
    await until("다른 thread 시작", () => aaa("thread", "show", "demo", other).status === "open", 30_000);
    await shot(page, "121-ideas-wide");
  });

  await step("아이디어(t-1260): 보류한 전용 thread는 화면을 열면 알림 없이 잇는다", async () => {
    aaa("thread", "park", "demo", thread, "--now");
    await until("보류", () => aaa("thread", "show", "demo", thread).status === "parked");
    const items = aaa("inbox", "list", "--peek");
    assert.ok(!JSON.stringify(items).includes(`${thread}: 보류`), "아이디어 thread 보류가 알림함에 남았다");
    await page.evaluate(() => (location.hash = "#threads"));
    await page.evaluate((h) => (location.hash = h), `#idea=${idea}`);
    await until("화면 열기로 잇기", () => aaa("thread", "show", "demo", thread).status === "open", 30_000);
  });

  let tasks;
  await step("아이디어(t-1260): 할 일로 넘기면 초안마다 할 일이 생기고 아이디어는 잠기며, 그 thread는 노트와 그림을 받는다", async () => {
    await page.getByTestId("idea-handoff").click();
    const dialog = page.getByTestId("idea-handoff-dialog");
    assert.equal(await dialog.getByTestId("idea-draft-title").first().inputValue(), "알림 묶기");
    await dialog.getByTestId("idea-draft-add").click();
    await dialog.getByTestId("idea-draft-title").nth(1).fill("알림 문서");
    await dialog.getByTestId("idea-handoff-submit").click();
    await dialog.waitFor({ state: "detached" });
    await page.getByTestId("idea-status").getByText(/^완료/).waitFor();
    tasks = aaa("idea", "show", idea).note.idea.tasks;
    assert.equal(tasks.length, 2);
    const board = aaa("task", "list");
    for (const d of tasks) assert.equal(board.tasks.find((x) => x.short_id === d).notes[0], `[아이디어 ${idea}](ideas/${idea})`);
    if (await page.getByTestId("idea-edit").count()) throw new Error("잠긴 아이디어에 고치기");
    await page.getByTestId("idea-copy").waitFor();
    // 할 일 thread: 메모의 링크가 노트(내용 hash 이름)와 그림을 attachments/로 넘긴다
    const started = aaa("thread", "start", "--from-task", tasks[0], "--machine", "labA", "--kind", "dir", "--over-limit");
    const shown = await until("할 일 thread 작업 폴더", () => { const v = aaa("thread", "show", "demo", started.id); return v.work_dir && v; }, 30_000);
    const att = `${shown.work_dir}/attachments`;
    await until("첨부", () => fs.existsSync(att) && fs.readdirSync(att).length === 2, 30_000);
    const files = fs.readdirSync(att).sort();
    const md = files.find((f) => f.endsWith(".md"));
    assert.ok(fs.readFileSync(`${att}/${md}`, "utf8").includes("thread가 더한 줄"), "노트");
    assert.ok(files.some((f) => /^[0-9a-f]{16}\.png$/.test(f)), `그림 ${files}`);
    await page.getByTestId("idea-copy").click();
    await page.getByTestId("idea-title").getByText("알림 묶기").waitFor();
    await domIs(page, "복사한 새 아이디어", () => location.hash === "#idea=n-0002");
    await shot(page, "121-ideas-done");
  });

  await step("아이디어(t-1260): WebKit 390 휴대폰은 노트 ← 대화, 대화 → 노트로 쓸어 넘기고 대화 단추에 모은 의견 수가 보인다", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      const { page: p, errors } = await pairPage(safari, aaa("web", "pair").code, { width: 390, height: 844 }, { hash: "#idea=n-0002" });
      const screen = p.getByTestId("idea-view").filter({ has: p.getByTestId("idea-note") });
      await p.getByTestId("idea-note").waitFor();
      await domIs(p, "노트 칸", () => document.querySelector('[data-testid="idea-view"][data-shown="true"]')?.dataset.page === "note");
      // 복사한 아이디어도 그림을 자기 폴더에서 그린다
      await domIs(p, "복사한 노트의 그림", () => { const img = document.querySelector('[data-testid="idea-note"] img'); return img?.complete && img.naturalWidth > 0; });
      await pick(p, "하루 한 번 묶어 보낸다");
      await p.getByTestId("report-note").tap();
      await p.getByTestId("quote-note").getByRole("textbox", { name: "의견" }).fill("휴대폰 의견");
      await p.getByTestId("quote-note-submit").tap();
      await p.getByTestId("idea-badge").getByText("1").waitFor();
      await p.evaluate(() => getSelection().removeAllRanges());
      await swipe(p, 300, 120);
      await domIs(p, "대화 칸", () => document.querySelector('[data-testid="idea-view"][data-shown="true"]')?.dataset.page === "chat");
      await p.getByTestId("idea-tray").waitFor();
      await shot(p, "121-ideas-phone-chat");
      await swipe(p, 100, 300);
      await domIs(p, "다시 노트 칸", () => document.querySelector('[data-testid="idea-view"][data-shown="true"]')?.dataset.page === "note");
      assert.equal(await p.locator("#workspace-drawer.is-open").count(), 0, "대화 → 노트 쓸기가 목록을 열었다");
      await p.getByTestId("idea-tab-chat").tap();
      await domIs(p, "단추로 대화", () => document.querySelector('[data-testid="idea-view"][data-shown="true"]')?.dataset.page === "chat");
      assert.ok(await screen.count() > 0);
      assert.deepEqual(errors, []);
    } finally { await safari.close(); }
  });
}
