// 할 일 한 목록(plan §8.393, t-0611): 「## 목록」 머리 없이 예약은 맨 위에 host 예약 차례대로(보라 표지만), 그 밑에 일반 할 일.
// 실제 host 예약 기록을 정본으로 데스크톱 끌기(예약 차례·일반 → 예약·예약 → 풀기), 「예약」 칩·저장소 거르기(기기 기억), 휴대폰 「순서」 ▲▼와
// ⋯ 시트를 확인한다. 자리 한도를 채워 예약이 기동하지 않게 하고, DOM·host 기록으로 끝낸다(고정 sleep 없음).
import fs from "node:fs";
import { root, aaa, step, browser, webkit, domIs, inTime, shot } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const MD = `# Tasks

## Backlog
- [ ] 상위 일 (fake@labA) [작업 폴더]
  상위 설명
  - [ ] 하위 일 (fake@labA) [작업 폴더]
    하위 설명
- [ ] 첫 예약 (fake@labA) [작업 폴더]
  독립 작업을 끝내고 결과를 쓴다
- [ ] 둘째 예약 (fake@labA) [작업 폴더]
  독립 작업을 끝내고 결과를 쓴다
- [ ] rc 일 (fake@labA) [rc]
  예약 뒤 저장소를 빼 확인 필요가 된다
- [ ] 끌어 예약 (fake@labA) [작업 폴더]
  끌어서 예약한다
## Later
- [ ] 나중 일 [작업 폴더]
  일반 할 일
`;
const row = (p, title) => p.locator(`.task-row[data-title="${title}"]`);
const pick = (p, title) => row(p, title).locator(".task-pick").click();
const queue = () => aaa("task", "queue").entries;
const hostOrder = () => queue().filter((q) => q.state === "queued" || q.state === "check").sort((a, b) => a.position - b.position).map((q) => q.task.title).join("|");
/** 맨 위 묶음의 행(kind:제목, 숨은 예약 줄은 gap) */
const TOP_ROWS = () => [...document.querySelectorAll('[data-testid="tasks-reserved"] > li')].map((e) => (e.dataset.kind ? `${e.dataset.kind}:${e.dataset.title}` : e.dataset.testid === "task-gap" ? `gap:${e.textContent}` : "")).filter(Boolean).join("|");
const TOP = (want) => [...document.querySelectorAll('[data-testid="tasks-reserved"] > li')].map((e) => (e.dataset.kind ? `${e.dataset.kind}:${e.dataset.title}` : e.dataset.testid === "task-gap" ? `gap:${e.textContent}` : "")).filter(Boolean).join("|") === want;
const topIs = async (p, what, want) => {
  try {
    await domIs(p, what, TOP, want);
  } catch (e) {
    throw new Error(`${e.message}: 맨 위 ${await p.evaluate(TOP_ROWS)}`);
  }
};
const restOf = (p) => p.getByTestId("tasks-rest").locator(".task-row").evaluateAll((els) => els.map((e) => e.dataset.title));
/** 손잡이를 대상 행의 위(before)·아래(after) 절반에 놓는다 */
const dragTo = async (p, from, target, side) => {
  const box = await target.boundingBox();
  await row(p, from).locator(".task-grip").dragTo(target, { targetPosition: { x: 60, y: side === "before" ? 3 : box.height - 3 } });
};

/** 휴대폰 상세의 닫기는 비동기 history.back이다. 기록 복귀 전에 reload하면 새 문서 요청이 취소된다. */
const closeBeforeReload = async (p, device) => {
  // 주소는 양쪽 모두 #tasks다. 앞서 닫은 ⋯ 메뉴의 늦은 popstate는 상세의 복귀가 아니다.
  // 상세 DOM과 마지막 시트 기록이 모두 사라진 popstate까지 듣는다.
  const rewound = device === "iphone"
    ? inTime(p.evaluate(() => new Promise(done => {
      const on = () => {
        if (document.querySelector('[data-testid="task-detail-close"]') || history.state?.["aaa:sheet"]) return;
        removeEventListener("popstate", on);
        done();
      };
      addEventListener("popstate", on);
    })), 20_000, "상세 닫기의 마지막 시트 기록 복귀")
    : Promise.resolve();
  await Promise.all([rewound, p.getByTestId("task-detail-close").click()]);
};

export default async function ({ page }) {

  await step("한 목록: 예약은 맨 위 host 차례대로 보라 표지만, 상위 예약은 하위를 데리고, 특별한 까닭은 상세에만", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    aaa("project", "set", "demo", "max_parallel_threads", "1");
    fs.writeFileSync(`${root}/one-list-busy.md`, "사람을 기다림");
    aaa("thread", "start", "demo", "--title", "자리 보유", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/one-list-busy.md`);
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, MD);
    // rc 저장소가 있는 동안 예약하고 빼면 host가 그 예약을 「확인 필요」로 둔다(까닭은 상세에만)
    aaa("project", "set", "demo", "repos.add", "/srv/rc@labA");
    aaa("task", "list");
    for (const t of ["첫 예약", "둘째 예약", "상위 일", "rc 일"]) aaa("task", "reserve", t);
    aaa("project", "set", "demo", "repos.remove", "/srv/rc@labA");
    aaa("ticker", "tick");
    if (queue().find((q) => q.task.title === "rc 일")?.state !== "check") throw new Error("rc 예약이 확인 필요가 아니다");
    if (aaa("thread", "list", "demo").threads.length !== 1) throw new Error("한도가 찬 동안 예약을 기동했다");
    if (hostOrder() !== "첫 예약|둘째 예약|상위 일|rc 일") throw new Error(`host 차례 ${hostOrder()}`);
    await page.getByTestId("activity-tasks").click();
    await topIs(page, "맨 위 예약", "reserved:첫 예약|reserved:둘째 예약|reserved:상위 일|under:하위 일|reserved:rc 일");
    if ((await restOf(page)).join("|") !== "끌어 예약|나중 일") throw new Error(`아래 일반 할 일 ${await restOf(page)}`);
    // 목록 머리·「▼ 예약 N」 칸이 없다. 행에는 순번·까닭 글이 없다
    if ((await page.locator(".task-list-toggle").count()) !== 0) throw new Error("목록 머리나 예약 칸이 남았다");
    const words = await page.getByTestId("tasks-reserved").textContent();
    if (/예약 \d|자리 대기|project\.toml/.test(words)) throw new Error(`예약 행에 순번·까닭 글 ${words}`);
    if ((await row(page, "rc 일").locator(".task-reserved-dot.is-check").count()) !== 1) throw new Error("확인 필요 예약은 노란 표지");
    if ((await page.getByTestId("tasks-filter-reserved").textContent()) !== "예약4") throw new Error(await page.getByTestId("tasks-filter-reserved").textContent());
    await pick(page, "rc 일");
    const why = page.getByTestId("task-queue-why");
    await why.waitFor();
    if (!(await why.textContent()).includes("project.toml repos에 없는 저장소다: rc")) throw new Error(await why.textContent());
    await pick(page, "첫 예약");
    await page.getByTestId("task-queue-home").waitFor();
    if (!(await page.getByTestId("task-queue-home").textContent()).includes("끌어 예약 바로 위")) throw new Error(await page.getByTestId("task-queue-home").textContent());
    if ((await page.getByTestId("task-queue-why").count()) !== 0) throw new Error("그냥 자리 대기에 까닭 상자");
    await shot(page, "one-list-desktop");
    await page.getByTestId("task-detail-close").click();
  });

  await step("활성 정렬: 예약 차례·원문 보존, 속성 정렬 중 이동 차단, 기기 기억과 원래 차례, 두 엔진과 휴대폰", async () => {
    const beforeFile = fs.readFileSync(`${root}/projects/demo/TASKS.md`, "utf8");
    const beforeQueue = JSON.stringify(queue());
    const notes = "# 나중\n\n## UI\n- `하 항목`\n  하 메모\n- 가 항목\n  가 메모\n\n## 엔진\n- 나 항목\n";
    fs.writeFileSync(`${root}/projects/demo/later.md`, notes);
    aaa("project", "set", "demo", "task_notes.add", "later.md");
    aaa("ticker", "tick");
    const top = "reserved:첫 예약|reserved:둘째 예약|reserved:상위 일|under:하위 일|reserved:rc 일";
    const wk = await webkit.launch();
    try {
      for (const [name, engine] of [["chromium", browser], ["webkit", wk]]) {
        for (const device of ["desktop", "iphone"]) {
          const { page: p } = await pairDevice(engine, aaa("web", "pair").code, device, { hash: "#tasks" });
          const select = async (label, testId = "tasks-sort") => {
            await p.getByTestId(testId).click();
            if (device === "iphone") await p.locator(".row-sheet").getByRole("button", { name: new RegExp(`^${label}`) }).click();
            else await p.getByRole("menuitemcheckbox", { name: new RegExp(`^${label}`) }).click();
          };
          try {
            await topIs(p, "기본 손 차례", top);
            await shot(p, `active-sort-before-${name}-${device}`);
            // 휴대폰 순서 모드에서 속성 정렬을 골라도 이동 도구가 남지 않는다.
            if (device === "iphone") await p.getByTestId("tasks-order").click();
            for (const [label, value, rest] of [["번호", "number", "나중 일|끌어 예약"], ["제목", "title", "끌어 예약|나중 일"], ["상태", "state", "끌어 예약|나중 일"], ["모델", "profile", "끌어 예약|나중 일"], ["머신", "machine", "끌어 예약|나중 일"]]) {
              await select(label);
              await domIs(p, "속성 정렬", key => document.querySelector('[data-testid="tasks-list"]')?.dataset.sort === key, value);
              await topIs(p, "정렬 중 실제 예약 차례", top);
              if ((await restOf(p)).join("|") !== rest) throw new Error(`${label} 차례 ${await restOf(p)}`);
              if (await p.locator('.task-grip, [data-testid^="task-step-"], [data-testid="tasks-order"]').count()) throw new Error("속성 정렬에 이동 도구가 남음");
            }
            await row(p, "둘째 예약").locator(".task-pick").focus();
            await p.keyboard.press("Alt+ArrowUp");
            await topIs(p, "Alt 이동 차단", top);
            await row(p, "둘째 예약").locator('[data-testid^="task-qmenu-"]').click();
            const menu = device === "iphone" ? p.locator(".row-sheet") : p.getByRole("menu");
            if (/예약 위로|예약 아래로|예약 맨 앞으로/.test(await menu.textContent())) throw new Error("속성 정렬에 예약 이동 메뉴");
            if (!(await menu.textContent()).includes("예약 풀기")) throw new Error("예약 풀기 메뉴가 사라짐");
            await p.keyboard.press("Escape");
            await pick(p, "첫 예약");
            await p.getByTestId("task-queue-home").waitFor();
            await p.getByTestId("task-detail-more").click();
            const more = device === "iphone" ? p.locator(".row-sheet") : p.getByRole("menu");
            if (/예약 위로|예약 아래로|예약 맨 앞으로/.test(await more.textContent())) throw new Error("상세에 예약 위치 이동 도구");
            await p.keyboard.press("Escape");
            await more.waitFor({ state: "detached" });
            await closeBeforeReload(p, device);
            await p.reload();
            await domIs(p, "정렬 기억", () => document.querySelector('[data-testid="tasks-list"]')?.dataset.sort === "machine");
            await topIs(p, "새로고침 뒤 실제 예약 차례", top);
            for (const width of device === "iphone" ? [390, 320] : [1280]) {
              await p.setViewportSize({ width, height: 844 });
              const layout = await p.evaluate(() => {
                const sort = document.querySelector('[data-testid="tasks-sort"]').getBoundingClientRect();
                const filters = document.querySelector('.tasks-filters').getBoundingClientRect();
                return { left: sort.left, right: sort.right, filters: filters.left, wide: document.documentElement.scrollWidth > innerWidth + 1, width: innerWidth };
              });
              if (layout.wide || layout.left < 0 || layout.right > width || (width <= 640 && layout.left > layout.filters)) throw new Error(`정렬 칩 배치 ${JSON.stringify(layout)}`);
              await shot(p, `active-sort-after-${name}-${width}`);
            }
            await p.getByTestId("tasks-sort-reset").click();
            await domIs(p, "손 차례 복구", () => document.querySelector('[data-testid="tasks-list"]')?.dataset.sort === "manual");
            if ((await restOf(p)).join("|") !== "끌어 예약|나중 일") throw new Error("손 차례가 바뀜");
            if (device === "iphone") await p.getByTestId("tasks-order").waitFor();
            else await p.locator(".task-grip").first().waitFor();
            await p.getByTestId("tasks-tab-later").click();
            const file = p.getByTestId("tasks-notes-later.md");
            await file.locator("summary", { hasText: "UI" }).click();
            await select("제목", "tasks-later-sort");
            if ((await file.locator("summary .task-list-name").allTextContents()).join("|") !== "UI|엔진") throw new Error("나중 정렬이 절 순서를 바꿈");
            const firstItem = file.locator(".task-notes-item").first();
            if (!(await firstItem.textContent()).startsWith("가 항목")) throw new Error("나중 제목순");
            // 정렬 뒤에도 첫 행 메뉴는 같은 제목·메모를 옮긴다(원문 위치 index 보존).
            await firstItem.locator(".task-menu-button").click();
            if (device === "iphone") await p.locator(".row-sheet").getByRole("button", { name: /^할 일로 옮기기/ }).click();
            else await p.getByRole("menuitem", { name: /^할 일로 옮기기/ }).click();
            await p.getByTestId("task-field-title").waitFor();
            if (await p.getByTestId("task-field-title").inputValue() !== "가 항목") throw new Error("나중 메뉴의 옮길 제목이 다름");
            await closeBeforeReload(p, device);
            await p.reload();
            await p.getByTestId("tasks-list").waitFor();
            if (await p.getByTestId("tasks-list").getAttribute("data-sort") !== "manual") throw new Error("나중 정렬이 활성 정렬을 바꿈");
            await p.getByTestId("tasks-tab-later").click();
            if (await p.getByTestId("tasks-later").getAttribute("data-sort") !== "title") throw new Error("나중 정렬 기억");
            await file.locator("summary", { hasText: "UI" }).click();
            await shot(p, `later-sort-${name}-${device}`);
            await select("원문 차례", "tasks-later-sort");
            if (!(await file.locator(".task-notes-item").first().textContent()).startsWith("하 항목")) throw new Error("나중 원문 차례 복구");
          } catch (error) {
            throw new Error(`${name}/${device}: ${error.message}`, { cause: error });
          } finally { await p.context().close(); }
        }
      }
      if (fs.readFileSync(`${root}/projects/demo/TASKS.md`, "utf8") !== beforeFile || JSON.stringify(queue()) !== beforeQueue) throw new Error("보기 정렬이 host 원문/예약을 바꿈");
      if (fs.readFileSync(`${root}/projects/demo/later.md`, "utf8") !== notes) throw new Error("나중 정렬이 원문을 바꿈");
      aaa("project", "set", "demo", "task_notes.remove", "later.md");
    } finally { await wk.close(); }
  });

  await step("끌기: 예약끼리 차례, 일반 → 예약(내용 확인 뒤 그 자리), 예약 → 아래 영역(풀면 원래 자리)", async () => {
    // 둘째 예약을 첫 예약 위에
    await dragTo(page, "둘째 예약", row(page, "첫 예약"), "before");
    await topIs(page, "예약 차례 바꾸기", "reserved:둘째 예약|reserved:첫 예약|reserved:상위 일|under:하위 일|reserved:rc 일");
    if (hostOrder() !== "둘째 예약|첫 예약|상위 일|rc 일") throw new Error(`host 차례 ${hostOrder()}`);
    // 일반 「끌어 예약」을 상위 일 위에: 내용 확인이 열리고, 예약하면 그 자리에 든다
    await dragTo(page, "끌어 예약", row(page, "상위 일"), "before");
    const confirm = page.getByTestId("task-reserve-confirm");
    await confirm.waitFor();
    if (!(await confirm.textContent()).includes("예약 3번째 자리에")) throw new Error(await confirm.textContent());
    await shot(page, "one-list-drag-confirm");
    await page.getByTestId("task-reserve-go").click();
    await topIs(page, "끌어 예약", "reserved:둘째 예약|reserved:첫 예약|reserved:끌어 예약|reserved:상위 일|under:하위 일|reserved:rc 일");
    if (hostOrder() !== "둘째 예약|첫 예약|끌어 예약|상위 일|rc 일") throw new Error(`host 차례 ${hostOrder()}`);
    await page.getByTestId("task-detail-close").click();
    // 첫 예약을 아래 영역(나중 일)에 놓으면 예약을 풀고, 파일 차례의 원래 자리에서 잠깐 알린다
    await dragTo(page, "첫 예약", row(page, "나중 일"), "after");
    await row(page, "첫 예약").getByTestId("task-flash").waitFor();
    if ((await restOf(page)).join("|") !== "첫 예약|나중 일") throw new Error(`풀린 자리 ${await restOf(page)}`);
    if (queue().find((q) => q.task.title === "첫 예약")?.state !== "cancelled") throw new Error("host 예약이 풀리지 않았다");
    await shot(page, "one-list-unreserved");
  });

  await step("「예약」 칩과 저장소 거르기: 함께 걸리고, 숨은 예약은 흐린 한 줄, 거른 채 옮겨도 숨은 예약은 그대로, 기기에 기억", async () => {
    await page.getByTestId("tasks-filter-reserved").click();
    await topIs(page, "예약만", "reserved:둘째 예약|reserved:끌어 예약|reserved:상위 일|reserved:rc 일");
    if ((await page.getByTestId("tasks-rest").count()) !== 0) throw new Error("예약만 보는데 일반 할 일");
    // rc 일을 맨 앞으로(⋯)
    await row(page, "rc 일").locator("[data-testid^='task-qmenu-']").click();
    await page.getByRole("menuitem", { name: "예약 맨 앞으로", exact: true }).click();
    await topIs(page, "맨 앞으로", "reserved:rc 일|reserved:둘째 예약|reserved:끌어 예약|reserved:상위 일");
    // 저장소: 작업 폴더 → rc 예약은 흐린 한 줄
    await page.getByTestId("tasks-repo").click();
    await shot(page, "one-list-repo-menu");
    await page.getByRole("menuitemcheckbox", { name: /작업 폴더/ }).click();
    await topIs(page, "작업 폴더 + 예약", "gap:다른 저장소 예약 1개 숨김 (rc) · 차례는 그대로|reserved:둘째 예약|reserved:끌어 예약|reserved:상위 일");
    if ((await page.getByTestId("tasks-filter-reserved").textContent()) !== "예약3") throw new Error("저장소 안 예약 수");
    // 거른 채 Alt+↓: 보이는 다음 예약 뒤로, 숨은 rc 일은 맨 앞 그대로
    await row(page, "둘째 예약").locator(".task-pick").focus();
    await page.keyboard.press("Alt+ArrowDown");
    await topIs(page, "거른 채 아래로", "gap:다른 저장소 예약 1개 숨김 (rc) · 차례는 그대로|reserved:끌어 예약|reserved:둘째 예약|reserved:상위 일");
    if (hostOrder() !== "rc 일|끌어 예약|둘째 예약|상위 일") throw new Error(`host 차례 ${hostOrder()}`);
    await shot(page, "one-list-repo-reserved");
    // 새로고침해도 저장소 거르기는 이 기기에 남는다(상태 칩은 전체로)
    await page.reload();
    await domIs(page, "저장소 기억", () => document.querySelector('[data-testid="tasks-repo"]')?.dataset.repo === "작업 폴더");
    await topIs(page, "기억한 거르기", "gap:다른 저장소 예약 1개 숨김 (rc) · 차례는 그대로|reserved:끌어 예약|reserved:둘째 예약|reserved:상위 일|under:하위 일");
    await page.getByTestId("tasks-repo").click();
    await page.getByRole("menuitemcheckbox", { name: /^전체/ }).click();
    await topIs(page, "전체", "reserved:rc 일|reserved:끌어 예약|reserved:둘째 예약|reserved:상위 일|under:하위 일");
  });

  await step("휴대폰: 「순서」의 ▲▼(예약은 예약 차례, 일반은 파일 차례)와 ⋯ 시트의 예약 풀기", async () => {
    const wk = await webkit.launch();
    try {
      for (const [name, engine] of [["webkit", wk], ["chromium", browser]]) {
        const { page: p } = await pairDevice(engine, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
        try {
          await topIs(p, "휴대폰 맨 위", "reserved:rc 일|reserved:끌어 예약|reserved:둘째 예약|reserved:상위 일|under:하위 일");
          await shot(p, `one-list-phone-${name}`);
          if (name === "webkit") continue;
          await p.getByTestId("tasks-order").click();
          if ((await p.locator("[data-testid^='task-menu-'], [data-testid^='task-qmenu-']").count()) !== 0) throw new Error("순서 모드에 ⋯");
          await shot(p, "one-list-phone-order");
          // 예약 ▼: rc 일을 한 칸 아래
          await row(p, "rc 일").locator("[data-testid^='task-step-down-']").click();
          await topIs(p, "▼", "reserved:끌어 예약|reserved:rc 일|reserved:둘째 예약|reserved:상위 일|under:하위 일");
          if (hostOrder() !== "끌어 예약|rc 일|둘째 예약|상위 일") throw new Error(`host 차례 ${hostOrder()}`);
          // 일반 ▲: 나중 일을 첫 예약 위로(옛 Later 머리를 넘어 Backlog 끝으로)
          await row(p, "나중 일").locator("[data-testid^='task-step-up-']").click();
          await domIs(p, "일반 ▲", () => [...document.querySelectorAll('[data-testid="tasks-rest"] .task-row')].map((e) => e.dataset.title).join("|") === "나중 일|첫 예약");
          if (!fs.readFileSync(`${root}/projects/demo/TASKS.md`, "utf8").includes("## Backlog\n")) throw new Error("옛 목록 머리가 사라졌다");
          await p.getByTestId("tasks-order").click();
          // ⋯ 시트: 예약 풀기
          await row(p, "둘째 예약").locator("[data-testid^='task-qmenu-']").click();
          const sheet = p.locator(".row-sheet");
          await sheet.waitFor();
          await shot(p, "one-list-phone-sheet");
          await sheet.getByRole("button", { name: /예약 풀기/ }).click();
          await topIs(p, "풀기", "reserved:끌어 예약|reserved:rc 일|reserved:상위 일|under:하위 일");
          if (hostOrder() !== "끌어 예약|rc 일|상위 일") throw new Error(`host 차례 ${hostOrder()}`);
        } finally {
          await p.context().close();
        }
      }
    } finally {
      await wk.close();
    }
  });
}
