// thread 작업 요약(사용자 10-19 「thread에 mouse over를 하면 작업 내용이 정리된 게 뜨면 좋겠어」, plan §8.259): 프로젝트 패널
// thread 줄의 hover 카드(마우스 기기만, 6줄 안팎)와 thread 화면 머리 「요약」 단추(데스크톱 팝오버·휴대폰 아래 시트, 접힌 절).
// 요약 뒤 지시는 단추 점과 접힌 절로 보인다. 휴대폰에는 hover 대안이 없다(사용자 결정).
import fs from "node:fs";
import { root, aaa, step, browser, pairPage, inList, domIs, until, shot } from "../lib/harness.mjs";

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
    const lines = [...card.querySelectorAll(".summary-goal, .summary-now li, .summary-latest, .summary-status")];
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
    aaa("thread", "message", "demo", id, "--text", "들여쓰기는 8px로");
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
    await until("펼친 지시 첫 줄", async () => (await fold.innerText()).includes("들여쓰기는 8px로"));
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

  await step("thread 줄 hover 카드(데스크톱): 150ms 뒤 줄 오른쪽에 6줄 안팎(목표·지금 3줄·최근 바뀐 것·상태)만, 줄을 늘리지 않고, 떠나면·⋯ 메뉴면 닫힌다", async () => {
    const row = page.getByTestId(`thread-demo-${id}`);
    if ((await row.getAttribute("title")) !== null) throw new Error("hover 카드가 title tooltip을 대신한다");
    await row.hover();
    await page.getByTestId("summary-hover").waitFor();
    await domIs(page, "카드 자리", () => document.querySelector('[data-testid="summary-hover"]')?.style.visibility !== "hidden");
    const card = await cardOf(page);
    // 목표 1 + 지금 3 + 최근 바뀐 것 1 + 상태 1. 정한 것·끝남·처음은 카드에 없다
    if (card.lines !== 6 || card.wrapped.length > 0 || card.height > 220) throw new Error(`카드 ${JSON.stringify(card)}`);
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
}
