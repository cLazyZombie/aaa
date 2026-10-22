// thread 작업 요약(사용자 10-19 「thread에 mouse over를 하면 작업 내용이 정리된 게 뜨면 좋겠어」, plan §8.259): 프로젝트 패널
// thread 줄의 hover 카드(마우스 기기만, 6줄 안팎)와 thread 화면 머리 「요약」 단추(데스크톱 팝오버·휴대폰 아래 시트, 접힌 절).
// 요약 뒤 지시는 단추 점과 접힌 절로 보인다. 작업 목록 ⋯의 「요약」도 같은 상세를 연다(plan §8.434).
import fs from "node:fs";
import assert from "node:assert/strict";
import { root, AA, aaa, step, browser, webkit, pairPage, inList, domIs, until, inTime, focusIs, shot, claudeSh, claudeIdle, waitOutput } from "../lib/harness.mjs";

export const labEnv = { CODEX_HOME: `${root}/summary-codex` };
const hostConfig = `${root}/c/host.toml`;
fs.writeFileSync(hostConfig, fs.readFileSync(hostConfig, "utf8").replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "astra-high"]')
  + `\n[profiles.astra-high]\nagent = "custom"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`);

const SUMMARY = `# 목표
웹 왼쪽 패널 다듬기
처음: 머신 CPU·MEM 표시
## 지금
- Ctrl+Enter는 작업 중에만 보냄
- 패널 폭을 끌어서 바꾸기
- 특수 단추 자리 옮기기
## 정함
- 사용자 15:0x 들여쓰기 8px
## 바뀜
- ~~사용자 14:0x Ctrl+Enter 없앰~~
- 사용자 15:1x 쉬는 중엔 안 보냄
## 끝남
- CPU·MEM 표시
`;

/** 카드가 보인 것: 줄 글과 높이·넘침 */
const cardOf = (p) =>
  p.evaluate(() => {
    const card = document.querySelector('[data-testid="summary-hover"]');
    if (!card) return null;
    const box = card.getBoundingClientRect();
    const lines = [...card.querySelectorAll(".summary-goal, .summary-now li, .summary-latest, .summary-activity p, .summary-status")];
    // 줄마다 한 줄: 글이 넘치면 …로 자르고 줄을 늘리지 않는다
    const wrapped = lines.filter((l) => l.getBoundingClientRect().height > 30).map((l) => l.textContent);
    return { text: card.innerText, lines: lines.length, height: box.height, wrapped, visible: card.style.visibility !== "hidden" };
  });

export default async function ({ page }) {
  let id = "";
  await step("thread 요약: 요약 없이 지시가 쌓이면 단추에 점과 「받은 지시」, 요약을 쓰면 점이 사라지고 팝오버는 목표·지금만 펼치며 나머지는 접는다", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/summary-task.md`, "# 패널 다듬기\n\n사람을 기다림\n");
    id = aaa("thread", "start", "demo", "--title", "패널 다듬기", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/summary-task.md`).id;
    await until("brief 전달", () => aaa("thread", "show", "demo", id).brief === "delivered");
    const instruction = "들여쓰기는 8px로\n사용자 계획: 첫 배포는 수동으로 새 host와 agent를 설치하고, 그 다음 배포부터 새 명령으로 실행한다. 실패하면 문제 머신만 되돌리고 앞 머신은 보존한다. 지시 전문 마지막 표지";
    aaa("thread", "message", "demo", id, "--text", instruction);
    if (aaa("thread", "show", "demo", id).summary?.pending !== 1) throw new Error(`요약 표지 ${JSON.stringify(aaa("thread", "show", "demo", id).summary)}`);
    await inList(page, `thread-demo-${id}`);
    await page.getByTestId(`thread-demo-${id}`).click();
    const button = page.getByTestId("summary-open");
    await button.waitFor();
    await domIs(page, "반영 안 된 지시 점", () => document.querySelector('[data-testid="summary-open"]')?.classList.contains("is-stale"));
    await button.click();
    const pop = page.getByTestId("summary-pop");
    await until("요약 없음 팝오버", async () => (await pop.innerText()).includes("아직 요약이 없어")).catch(async (e) => {
      throw new Error(`${e.message}: ${await pop.innerText().catch(() => "(팝오버 없음)")}`);
    });
    // 요약이 없으면 목표 자리에 할 일 첫 줄, 받은 지시는 접힌 절
    if (!(await pop.locator(".summary-goal").innerText()).includes("패널 다듬기")) throw new Error("할 일 첫 줄");
    const fold = pop.locator("details.summary-fold.is-warn");
    if ((await fold.getAttribute("open")) !== null || !(await fold.innerText()).includes("받은 지시")) throw new Error("받은 지시는 접혀 있다");
    await fold.locator("summary").click();
    await until("펼친 지시 전문", async () => (await fold.locator(".summary-text").innerText()) === instruction);
    const overflowing = await fold.locator(".summary-text").evaluate((el) => el.scrollWidth > el.clientWidth + 1);
    if (overflowing) throw new Error("지시 전문이 창 밖으로 넘쳤다");
    await shot(page, "summary-instruction-full");
    await page.keyboard.press("Escape");
    await pop.waitFor({ state: "detached" });

    fs.writeFileSync(`${root}/summary.md`, SUMMARY);
    aaa("thread", "summary", "demo", id, "--file", `${root}/summary.md`);
    await domIs(page, "요약을 쓰면 점이 사라진다", () => {
      const b = document.querySelector('[data-testid="summary-open"]');
      return b !== null && !b.classList.contains("is-stale");
    });
    await button.click();
    await until("요약 팝오버", async () => (await pop.innerText()).includes("웹 왼쪽 패널 다듬기"));
    const folds = await pop.locator("details.summary-fold").evaluateAll((ds) => ds.map((d) => [d.querySelector("summary").textContent.replace(/\s+/g, ""), d.open]));
    const want = [["정한것1", false], ["바뀐것2", false], ["끝난것1", false]];
    if (JSON.stringify(folds) !== JSON.stringify(want)) throw new Error(`접힌 절 ${JSON.stringify(folds)}`);
    if (!(await pop.innerText()).includes("Ctrl+Enter는 작업 중에만 보냄")) throw new Error("지금 할 것은 펼친다");
    await shot(page, "summary-popover");
    await page.keyboard.press("Escape");
    await pop.waitFor({ state: "detached" });
  });

  await step("작업 목록 ⋯ → 요약: 데스크톱·휴대폰·Reader에서 같은 상세, 요약 없음과 화면 보존·뒤로 닫기", async () => {
    fs.writeFileSync(`${root}/summary-missing-task.md`, "# 요약 없는 작업\n");
    const missing = aaa("thread", "start", "demo", "--title", "요약 없는 작업", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/summary-missing-task.md`).id;
    const hash = await page.evaluate(() => location.hash);
    const wk = await webkit.launch({ headless: true });
    const devices = [];
    try {
      for (const [engine, width] of [[browser, 320], [wk, 390]]) {
        devices.push(await pairPage(engine, aaa("web", "pair", "--reader").code, { width, height: 844 }, { touch: true, hash }));
      }
      for (const p of [page, ...devices.map((d) => d.page)]) {
        const phone = (await p.evaluate(() => innerWidth)) <= 768;
        const tag = phone ? `phone-${await p.evaluate(() => innerWidth)}` : "desktop";
        for (const [thread, goal] of [[missing, "요약 없는 작업"], [id, "웹 왼쪽 패널 다듬기"]]) {
          const row = p.locator(`[data-thread="demo/${thread}"]`);
          // 데스크톱은 ⋯ 단추를 숨기고 오른쪽 누름으로 같은 메뉴를 연다.
          // 휴대폰 drawer를 열기 전에도 찾을 수 있도록 CSS locator로 단추를 잡는다.
          if (phone) await inList(p, row.locator(".row-menu-toggle"));
          else await p.getByTestId(`thread-demo-${thread}`).click({ button: "right" });
          const menu = phone ? p.locator('.row-sheet:not(.summary-sheet)') : p.getByRole("menu");
          const item = phone ? menu.getByRole("button", { name: "요약", exact: true }) : menu.getByRole("menuitem", { name: "요약", exact: true });
          await item.waitFor();
          if (await menu.locator(".summary-detail").count()) throw new Error("메뉴 안에 요약을 펼쳤다");
          await shot(p, `summary-menu-${tag}-${thread === missing ? "missing" : "saved"}`);
          if (!phone && thread === missing) {
            // RowMenu는 첫 항목 focus를 다음 frame에 준다. 초기 focus가 끝난 뒤
            // 요약으로 옮겨 Enter를 누른다(그 전에 옮기면 보고로 되돌아갈 수 있다).
            await inTime(menu.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)))), 20_000, "메뉴 초기 focus frame");
            await focusIs(p, "메뉴 첫 항목", '[role="menu"] > button:first-of-type');
            await item.focus();
            await p.keyboard.press("Enter");
          } else await item.click();
          await menu.waitFor({ state: "detached" });
          const detail = p.getByTestId(phone ? "summary-sheet" : "summary-pop");
          await until("메뉴에서 연 같은 작업 요약", async () => (await detail.locator(".summary-goal").innerText()).includes(goal));
          if (await detail.getAttribute("aria-label") !== `${thread} 작업 요약`) throw new Error("다른 thread 요약을 열었다");
          if (thread === missing && !(await detail.innerText()).includes("아직 요약이 없어")) throw new Error("요약 없는 작업의 안내 누락");
          if (await p.getByTestId("summary-hover").count()) throw new Error("상세 위에 hover 카드가 겹쳤다");
          if ((await p.evaluate(() => location.hash)) !== hash) throw new Error("요약을 열며 본문 화면을 바꿨다");
          await detail.evaluate((el) => Promise.all(el.closest(".modal-scrim")?.getAnimations({ subtree: true }).map((a) => a.finished) ?? el.getAnimations().map((a) => a.finished)));
          const box = await detail.evaluate((el) => ({ left: el.getBoundingClientRect().left, right: el.getBoundingClientRect().right, width: innerWidth, overflow: el.scrollWidth > el.clientWidth + 1 }));
          if (box.left < 0 || box.right > box.width + 1 || box.overflow) throw new Error(`요약 창 가로 넘침 ${JSON.stringify(box)}`);
          await shot(p, `summary-menu-detail-${tag}-${thread === missing ? "missing" : "saved"}`);
          if (phone) await p.evaluate(() => history.back());
          else await p.keyboard.press("Escape");
          await detail.waitFor({ state: "detached" });
          if ((await p.evaluate(() => location.hash)) !== hash) throw new Error("요약을 닫으며 본문 화면을 바꿨다");
        }
      }
    } finally {
      for (const d of devices) await d.page.context().close();
      await wk.close();
      aaa("thread", "resolve", "demo", missing);
    }
  });

  await step("thread 줄 hover 카드(데스크톱): 150ms 뒤 목표·지금·최근 변경·활동·시간·상태, 한 줄씩 보이고 떠나면·⋯ 메뉴면 닫힌다", async () => {
    const row = page.getByTestId(`thread-demo-${id}`);
    if ((await row.getAttribute("title")) !== null) throw new Error("hover 카드가 title tooltip을 대신한다");
    await row.hover();
    await page.getByTestId("summary-hover").waitFor();
    await domIs(page, "카드 자리", () => document.querySelector('[data-testid="summary-hover"]')?.style.visibility !== "hidden");
    const card = await cardOf(page);
    // 기존 여덟 줄에 모델 한 줄을 더한다. 정한 것·끝남·처음은 상세에서 읽는다.
    if (card.lines !== 9 || card.wrapped.length > 0 || card.height > 280) throw new Error(`카드 ${JSON.stringify(card)}`);
    assert.match(card.text, /현재 모델 미확인 · 시작 profile: fake/);
    if (!card.text.includes("지금 활동:") || !card.text.includes("시작 후") || !card.text.includes("활동")) throw new Error(`활동·시간 ${card.text}`);
    for (const word of ["웹 왼쪽 패널 다듬기", "특수 단추 자리 옮기기", "쉬는 중엔 안 보냄"]) {
      if (!card.text.includes(word)) throw new Error(`카드에 ${word}가 없다: ${card.text}`);
    }
    for (const word of ["들여쓰기 8px", "CPU·MEM 표시", "Ctrl+Enter 없앰"]) {
      if (card.text.includes(word)) throw new Error(`카드에 ${word}가 있다(단추 창 것): ${card.text}`);
    }
    const place = await page.evaluate((id) => {
      const r = document.querySelector(`[data-testid="thread-demo-${id}"]`).getBoundingClientRect();
      const c = document.querySelector('[data-testid="summary-hover"]').getBoundingClientRect();
      return { gap: Math.round(c.left - r.right), top: Math.round(c.top - r.top) };
    }, id);
    if (place.gap !== 4 || place.top !== 0) throw new Error(`카드 자리 ${JSON.stringify(place)}`);
    await shot(page, "summary-hover");
    await page.mouse.move(1000, 600);
    await page.getByTestId("summary-hover").waitFor({ state: "detached" });
    // ⋯ 메뉴(오른쪽 누름)가 열리면 카드는 없다
    await row.hover();
    await page.getByTestId("summary-hover").waitFor();
    await row.click({ button: "right" });
    await page.getByTestId("summary-hover").waitFor({ state: "detached" });
    await page.keyboard.press("Escape");
  });

  await step("「작업」 트리(plan §8.280): 고정한 thread 줄(맨 앞 카드)도 같은 요약 카드, 「기타」의 thread 없는 agent에는 카드 없음", async () => {
    const plain = aaa("tab", "create", "--machine", "labA", "--label", "일반 agent", "--",
      claudeSh, "-c", `stty -echo; ${claudeIdle}; echo summary-agent-ready; exec cat > /dev/null`).tabs.at(-1).root.pane;
    await waitOutput(plain, "summary-agent-ready");
    // thread 줄을 ⋯ 메뉴로 고정한다(예전 에이전트 패널에만 있던 thread 고정): 맨 앞 고정 카드로 간다
    const threadRow = page.getByTestId(`thread-demo-${id}`);
    await threadRow.click({ button: "right" });
    await page.getByRole("menuitem", { name: "고정", exact: true }).click();
    const row = page.locator(`.sidebar-pinned [data-thread="demo/${id}"] .pane-select`);
    await row.waitFor();
    const other = page.getByTestId("pane-일반 agent");
    await other.waitFor();
    // 가짜 시계로 hover 지연을 넘긴다. 요약 없는 줄이 카드를 예약하지 않는지도 같은 시간 창에서 본다.
    await page.clock.install();
    try {
      await other.hover();
      await page.clock.runFor(200);
      if (await page.getByTestId("summary-hover").count()) throw new Error("thread 없는 agent에 빈 카드");
      await row.hover();
      await page.clock.runFor(200);
      await page.getByTestId("summary-hover").waitFor();
      await domIs(page, "고정 카드 줄 요약 카드 자리", () => document.querySelector('[data-testid="summary-hover"]')?.style.visibility !== "hidden");
      if (!(await cardOf(page)).text.includes("웹 왼쪽 패널 다듬기")) throw new Error("고정한 줄의 thread 대응이 다르다");
      if (await row.getAttribute("title") !== null) throw new Error("고정한 줄에 title과 카드가 겹친다");
      await shot(page, "pinned-summary-hover");
      await row.click({ button: "right" });
      await page.getByTestId("summary-hover").waitFor({ state: "detached" });
      await page.getByRole("menuitem", { name: "고정 해제" }).click();
      await page.locator(".sidebar-pinned").waitFor({ state: "detached" });
    } finally {
      await page.clock.resume();
    }
    aaa("pane", "kill", plain);
  });

  await step("휴대폰: hover 카드 없이 thread 화면 머리 「요약」 단추(아이콘) → 아래 시트, 절은 접혀 있다", async () => {
    const pane = aaa("thread", "show", "demo", id).pane;
    const dev = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true, hash: `#pane=${encodeURIComponent(pane)}` });
    const p = dev.page;
    try {
      // 마우스가 없는 기기: hover 카드를 켜지 않으므로 줄은 전처럼 title을 갖는다(그 결정은 hover 조건에 있다)
      await p.getByTestId(`thread-demo-${id}`).waitFor({ state: "attached" });
      if ((await p.getByTestId(`thread-demo-${id}`).getAttribute("title")) === null) throw new Error("휴대폰에서 hover 카드를 켰다");
      const button = p.getByTestId("summary-open");
      await button.waitFor();
      if ((await p.getByTestId("summary-hover").count()) !== 0) throw new Error("휴대폰에 hover 카드가 떴다");
      if (await button.locator(".summary-btn-label").isVisible()) throw new Error("휴대폰 단추는 아이콘만");
      await button.click();
      const sheet = p.getByTestId("summary-sheet");
      await until("요약 시트", async () => (await sheet.innerText()).includes("웹 왼쪽 패널 다듬기"));
      if ((await sheet.locator("details[open]").count()) !== 0) throw new Error("시트의 절은 접혀 있다");
      if ((await p.getByTestId("summary-hover").count()) !== 0) throw new Error("휴대폰에 hover 카드가 떴다");
      await shot(p, "summary-sheet-phone");
      await sheet.getByRole("button", { name: "닫기" }).click();
      await sheet.waitFor({ state: "detached" });
    } finally {
      await p.context().close();
    }
  });

  await step("전체 진행 목록: 순서·완료 개수·실시간 갱신·새 지시 보존, Reader 휴대폰에서도 읽기 전용", async () => {
    // 공백 많은 29자는 270px 안에 들어갈 수 있다. 한글 29자로 실제 한 줄 너비를 넘긴다.
    const names = ["표시 조건 확인", "휴대폰에서만 숨김", "PC·휴대폰 확인", "이전 설정 보존", "작업목록에서긴항목도원래순서대로모두확인하고변경을적용하기"];
    const file = `${root}/progress-summary.md`;
    const set = (marks) => {
      fs.writeFileSync(file, `# 목표\n휴대폰 채팅 중 context 표시 숨기기\n## 진행\n${names.map((n, i) => `- [${marks[i]}] ${n}`).join("\n")}\n`);
      return aaa("thread", "summary", "demo", id, "--file", file);
    };
    const initial = set(["x", "~", " ", "x", " "]);
    if (initial.progress.length !== 5) throw new Error("실제 host가 전체 목록을 전달하지 않았다");
    const report = (marks) => {
      const t = aaa("thread", "show", "demo", id);
      fs.writeFileSync(`${t.work_dir}/report.md`, `[진행 중] 웹 확인\n## Report\n실제 보고 감시 시험\n## 진행\n${names.map((n, i) => `- [${marks[i]}] ${n}`).join("\n")}\n## Next\n- 검증을 마친다\n`);
    };
    // 실제 pane 안의 CLI 보고다. 같은 활동의 재보고는 host의 활동 시작 시각을 유지해야 한다.
    const activityFile = `${root}/summary-activity.sh`;
    const quote = (s) => `'${s.replaceAll("'", "'\\''")}'`;
    const activity = (percent) => {
      fs.writeFileSync(activityFile, `${quote(AA)} report --percent ${percent} --activity '웹 회귀 검증'\n`);
      aaa("thread", "message", "demo", id, "--text", `post-probe ${activityFile}`);
    };
    activity(65);
    const agent = await until("활동 보고 수신", () => {
      const a = aaa("thread", "show", "demo", id).agent;
      return a?.percent === 65 && a.progress === "웹 회귀 검증" ? a : null;
    });
    report(["x", "~", " ", "x", " "]);
    await page.getByTestId(`thread-demo-${id}`).hover();
    const hover = page.getByTestId("summary-hover");
    await hover.waitFor();
    const progress = hover.getByTestId("summary-progress");
    await progress.waitFor();
    const states = await progress.getByRole("listitem").evaluateAll((ls) => ls.map((l) => l.dataset.state));
    if (JSON.stringify(states) !== JSON.stringify(["done", "working", "pending", "done", "pending"])) throw new Error(`진행 순서 ${JSON.stringify(states)}`);
    const labels = await progress.locator(".summary-step-text").allTextContents();
    if (JSON.stringify(labels) !== JSON.stringify(names)) throw new Error(`전체 목록 ${JSON.stringify(labels)}`);
    if (!(await progress.innerText()).includes("2/5 완료")) throw new Error("완료 개수");
    if (await progress.getByRole("checkbox").count() || await progress.getByRole("button").count()) throw new Error("읽기 전용 목록에 편집 조작");
    const layout = await progress.locator("li").evaluateAll((ls) => ls.map((l) => ({ height: l.getBoundingClientRect().height, clipped: l.querySelector(".summary-step-text").scrollWidth > l.querySelector(".summary-step-text").clientWidth })));
    if (layout.some((l) => l.height > 30) || !layout.at(-1).clipped) throw new Error(`hover의 긴 이름은 한 줄 ${JSON.stringify(layout)}`);
    await shot(page, "summary-progress-hover");

    await domIs(page, "열린 카드의 실시간 활동", () => document.querySelector('[data-testid="summary-hover"] [data-testid="summary-activity"]')?.textContent.includes("웹 회귀 검증 · ~65%"));
    await page.clock.install({ time: agent.progress_since_ms + 10_000 });
    try {
      const time = hover.getByTestId("summary-time");
      await page.clock.runFor(1000);
      const before = await time.innerText();
      await page.clock.runFor(2000);
      if ((await time.innerText()) === before) throw new Error("열린 카드의 경과 시간이 멈췄다");
    } finally { await page.clock.resume(); }
    activity(70);
    await domIs(page, "같은 활동의 진행률 실시간 갱신", () => document.querySelector('[data-testid="summary-hover"] [data-testid="summary-activity"]')?.textContent.includes("웹 회귀 검증 · ~70%"));
    if (aaa("thread", "show", "demo", id).agent.progress_since_ms !== agent.progress_since_ms) throw new Error("정기 보고가 활동 시간을 초기화했다");

    // 카드를 열어 둔 채 report.md만 바꾼다. agent 감시→host 사본→카드 캐시·경고 갱신을 함께 본다.
    report(["x", "x", "~", "x", " "]);
    await domIs(page, "열린 카드 진행 갱신", () => document.querySelector('[data-testid="summary-hover"] [data-testid="summary-progress"]')?.textContent.includes("3/5 완료"));
    const updated = await progress.getByRole("listitem").evaluateAll((ls) => ls.map((l) => l.dataset.state));
    if (JSON.stringify(updated) !== JSON.stringify(["done", "done", "working", "done", "pending"])) throw new Error(`갱신 순서 ${JSON.stringify(updated)}`);
    await domIs(page, "새 진행 보고가 이전 지시 반영", () => !document.querySelector('[data-testid="summary-hover"] .summary-warn'));
    aaa("thread", "message", "demo", id, "--text", "태블릿 조건도 확인해 줘");
    await domIs(page, "진행 목록 옆 새 지시 경고", () => document.querySelector('[data-testid="summary-hover"] .summary-warn')?.textContent.includes("요약 뒤 지시 1개"));
    if (!(await progress.innerText()).includes("3/5 완료")) throw new Error("새 지시로 완료 개수가 바뀌었다");
    await shot(page, "summary-progress-pending");
    await page.mouse.move(1000, 600);
    await hover.waitFor({ state: "detached" });

    // 실제 Reader 권한의 Chromium과 iPhone WebKit. 네트워크에서 받은 목록을 같은 상세에 표시한다.
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, width] of [[browser, 320], [wk, 390]]) {
        const pane = aaa("thread", "show", "demo", id).pane;
        const dev = await pairPage(engine, aaa("web", "pair", "--reader").code, { width, height: 844 }, { touch: true, hash: `#pane=${encodeURIComponent(pane)}` });
        const p = dev.page;
        try {
          await p.getByTestId("summary-open").click();
          const sheet = p.getByTestId("summary-sheet");
          await sheet.getByTestId("summary-progress").waitFor();
          await sheet.evaluate((el) => Promise.all(el.closest(".modal-scrim").getAnimations({ subtree: true }).map((a) => a.finished)));
          const lines = await sheet.locator(".summary-step-text").allTextContents();
          if (JSON.stringify(lines) !== JSON.stringify(names)) throw new Error(`Reader 전체 목록 ${JSON.stringify(lines)}`);
          if (!(await sheet.innerText()).includes("3/5 완료") || !(await sheet.innerText()).includes("요약 뒤 지시")) throw new Error("Reader 완료 개수·새 지시");
          const activity = await sheet.getByTestId("summary-activity").innerText();
          if (!activity.includes("웹 회귀 검증 · ~70%") || !activity.includes("시작 후") || !activity.includes("활동")) throw new Error(`Reader 활동·시간 ${activity}`);
          const box = await sheet.evaluate((el) => ({ width: innerWidth, left: el.getBoundingClientRect().left, right: el.getBoundingClientRect().right, overflow: el.scrollWidth > el.clientWidth + 1, lastHeight: el.querySelector("li:last-child .summary-step-text").getBoundingClientRect().height }));
          if (box.left < 0 || box.right > width + 1 || box.overflow || box.lastHeight <= 20) throw new Error(`휴대폰 줄바꿈·넘침 ${JSON.stringify(box)}`);
          const refusal = await p.evaluate(async ([thread]) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "thread_summary_set", project: "demo", thread, text: "# 목표\n보기 기기 쓰기 금지" }) })).json(), [id]);
          if (refusal.err?.code !== "forbidden") throw new Error(`Reader가 요약을 썼다: ${JSON.stringify(refusal)}`);
          if (aaa("thread", "summary", "demo", id).goal !== "휴대폰 채팅 중 context 표시 숨기기") throw new Error("Reader 쓰기 거부 뒤 원본 변경");
          await shot(p, `summary-progress-reader-${width}`);
        } finally { await p.context().close(); }
      }
    } finally { await wk.close(); }

    // 모두 완료해도 목록을 없애거나 재정렬하지 않는다. 다음 진행 보고가 이전 지시를 반영한다.
    report(["x", "x", "x", "x", "x"]);
    await page.getByTestId("summary-open").click();
    const pop = page.getByTestId("summary-pop");
    await domIs(page, "모든 단계 완료", () => document.querySelector('[data-testid="summary-pop"] [data-testid="summary-progress"]')?.textContent.includes("5/5 완료"));
    if ((await pop.locator('.summary-step[data-state="done"]').count()) !== 5 || await pop.locator(".summary-warn").count()) throw new Error("완료 목록 보존·반영한 지시");
    await shot(page, "summary-progress-done");
    // coordinator는 목표·결정을 고친다. 진행 목록을 생략해도 thread가 보고한 완료 상태를 보존한다.
    fs.writeFileSync(file, "# 목표\n바뀐 목표\n");
    aaa("thread", "summary", "demo", id, "--file", file);
    await domIs(page, "목표만 편집한 요약", () => document.querySelector('[data-testid="summary-pop"] .summary-goal')?.textContent.includes("바뀐 목표"));
    if ((await pop.locator('.summary-step[data-state="done"]').count()) !== 5) throw new Error("목표 편집이 보고한 완료 목록을 덮었다");
    await page.keyboard.press("Escape");
    await pop.waitFor({ state: "detached" });
  });

  await step("모델: 기록의 sol high → astra medium을 열린 카드·상세에 갱신하고 재시작 profile과 미확인을 구별한다", async () => {
    const sid = "2f4b7c8d-1a2e-4f6a-8b9c-0d1e2f3a4b5c";
    const dir = `${root}/summary-codex/sessions/2026/10/22`;
    fs.mkdirSync(dir, { recursive: true });
    const transcript = `${dir}/rollout-summary-${sid}.jsonl`;
    fs.writeFileSync(transcript, `${JSON.stringify({ type: "session_meta", payload: { id: sid } })}\n`);
    const turn = (model, effort) => fs.appendFileSync(transcript, `${JSON.stringify({ type: "turn_context", timestamp: new Date().toISOString(), payload: { model, effort } })}\n`);
    turn("gpt-6-sol", "high");
    const quote = (s) => `'${s.replaceAll("'", "'\\''")}'`;
    const probe = `${root}/summary-model.sh`;
    // 실제 pane 안에서 hook을 보내고 실제 agent reader·link·host 상태·웹 사건을 거친다.
    // harness만 기록을 대신 쓰며 제품의 모델 판정·상태 전달·표시는 대체하지 않는다.
    fs.writeFileSync(probe, `${quote(AA)} hook --agent codex ${quote(JSON.stringify({ type: "agent-turn-complete", "thread-id": sid }))}\n`);
    aaa("thread", "message", "demo", id, "--text", `post-probe ${probe}`);
    await page.getByTestId(`thread-demo-${id}`).hover();
    const hover = page.getByTestId("summary-hover");
    const shows = (p, text) => domIs(p, text, (s) => document.querySelector('[data-testid="summary-model"]')?.textContent === s, text);
    await shows(page, "최근 확인 모델: sol high");
    // /model 선택만으로 새 값이 기록되었다고 추측하지 않는다. 다음 turn_context부터 갱신한다.
    aaa("thread", "message", "demo", id, "--text", "/model gpt-6-astra");
    turn("gpt-6-astra", "medium");
    await shows(page, "최근 확인 모델: astra medium");
    assert.match(await hover.getByTestId("summary-model").getAttribute("title"), /gpt-6-astra.*medium/);

    const theme = async (p, value) => {
      if (await p.evaluate(() => document.documentElement.dataset.theme) !== value) {
        await p.keyboard.press("ControlOrMeta+Shift+K");
        const palette = p.getByTestId("command-palette");
        await palette.locator('input[type="search"]').fill(">테마");
        await palette.getByRole("option", { name: "테마 전환" }).click();
        await palette.waitFor({ state: "detached" });
      }
      await domIs(p, `${value} 테마`, (t) => document.documentElement.dataset.theme === t, value);
    };
    for (const value of ["light", "dark"]) {
      await theme(page, value);
      await page.getByTestId(`thread-demo-${id}`).hover();
      await hover.waitFor();
      await shows(page, "최근 확인 모델: astra medium");
      await shot(page, `summary-model-hover-${value}`);
      await page.mouse.move(1000, 600);
      await hover.waitFor({ state: "detached" });
      await page.getByTestId("summary-open").click();
      await shows(page, "최근 확인 모델: astra medium");
      await shot(page, `summary-model-detail-${value}`);
      await page.keyboard.press("Escape");
      await page.getByTestId("summary-pop").waitFor({ state: "detached" });
    }
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, width] of [[browser, 320], [wk, 390]]) {
        const pane = aaa("thread", "show", "demo", id).pane;
        const dev = await pairPage(engine, aaa("web", "pair", "--reader").code, { width, height: 844 }, { touch: true, hash: `#pane=${encodeURIComponent(pane)}` });
        try {
          const p = dev.page;
          for (const value of ["light", "dark"]) {
            await theme(p, value);
            await p.getByTestId("summary-open").click();
            const sheet = p.getByTestId("summary-sheet");
            await shows(p, "최근 확인 모델: astra medium");
            await sheet.evaluate((el) => Promise.all(el.closest(".modal-scrim").getAnimations({ subtree: true }).map((a) => a.finished)));
            const bounds = await sheet.evaluate((el) => ({
              viewport: innerWidth,
              left: el.getBoundingClientRect().left,
              right: el.getBoundingClientRect().right,
              closeRight: el.querySelector('[aria-label="닫기"]').getBoundingClientRect().right,
            }));
            assert.equal(bounds.viewport, width, JSON.stringify(bounds));
            assert.ok(bounds.left >= 0 && bounds.right <= width + 1 && bounds.closeRight <= width, JSON.stringify(bounds));
            assert.equal(await sheet.evaluate((el) => el.scrollWidth > el.clientWidth + 1), false);
            assert.equal(await sheet.getByTestId("summary-model").evaluate((el) => el.scrollWidth > el.clientWidth + 1), false);
            await shot(p, `summary-model-reader-${width}-${value}`);
            await sheet.getByRole("button", { name: "닫기" }).click();
            await sheet.waitFor({ state: "detached" });
          }
        } finally { await dev.page.context().close(); }
      }
    } finally { await wk.close(); }

    // restart는 실행 중 pane의 종료를 먼저 요구한다. 실제 종료 요청을 마친 뒤 새 profile로 띄운다.
    const oldPane = aaa("thread", "show", "demo", id).pane;
    aaa("pane", "kill", oldPane);
    await until("재시작 전 실제 pane 종료", () => aaa("pane", "get", oldPane).state !== "running");
    aaa("thread", "restart", "demo", id, "--profile", "astra-high");
    await inList(page, `thread-demo-${id}`);
    await page.getByTestId(`thread-demo-${id}`).click();
    await page.getByTestId("summary-open").click();
    await shows(page, "현재 모델 미확인 · 시작 profile: astra-high");
    assert.equal(aaa("thread", "show", "demo", id).agent?.model_observation ?? null, null);
    await shot(page, "summary-model-restarted");
    await page.keyboard.press("Escape");
    await page.getByTestId("summary-pop").waitFor({ state: "detached" });
  });
}
