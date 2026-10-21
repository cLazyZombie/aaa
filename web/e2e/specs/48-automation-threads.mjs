// 자동화가 띄운 thread 표시(plan t-0475 A1): 실제 host가 자동화 규칙(`do.start`)으로 thread를 띄우고, 웹 「작업」 목록은 그 줄을 사람이 띄운
// thread와 같은 자리에 섞어 왼쪽 호박색 막대와 「자동화 thread」 표지로 보인다. 표지는 자동화 패널의 그 규칙 상세를, 자동화 패널의
// 「열림」 칩과 기록의 thread 번호는 그 thread를 연다. 「모든 thread」는 「자동화만」으로 거른다. 시간은 기다리지 않는다: thread 기록을 기다린다.
import fs from "node:fs";
import { root, aaa, step, until, domIs, drawerSettled, inList, openSettings, shot } from "../lib/harness.mjs";
export const labEnv = { AAA_TEST_ROUTINE_SECONDS: "1" };

const project = "auto-bar";
const dir = () => `${root}/projects/${project}`;
const call = (p, req) => p.evaluate(async (body) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })).json(), req);
const threads = () => aaa("thread", "list").threads.filter((t) => t.project === project);
const SIZES = { desktop: { width: 1280, height: 800 }, pad: { width: 820, height: 1180 }, phone: { width: 390, height: 844 } };

/** 좁은 화면은 목록이 서랍 안이다: 닫혀 있으면 열고 움직임이 끝나기를 기다린다 */
async function openList(p) {
  await drawerSettled(p);
  const toggle = p.locator(".view [data-testid='drawer-toggle']");
  if ((await toggle.isVisible()) && (await toggle.getAttribute("aria-expanded")) !== "true") {
    await toggle.click();
    await p.locator("#workspace-drawer.is-open").waitFor();
    await drawerSettled(p);
  }
}

/** 막대(줄 select의 ::before)와 그 줄 표지(agent 표지 그림) 사이: 막대가 줄 안이고 표지를 덮지 않는다(12-threads-push coordinator 막대와 같은 방법) */
const barClear = (p, testid) => p.evaluate((id) => {
  const select = document.querySelector(`[data-panel="work"] [data-testid="${id}"]`);
  const bar = getComputedStyle(select, "::before");
  const left = select.getBoundingClientRect().left + parseFloat(bar.left);
  const icon = select.querySelector(".sidebar-mark > *").getBoundingClientRect();
  return { inRow: left - select.closest(".workspace-header").getBoundingClientRect().left, gap: icon.left - (left + parseFloat(bar.width)) };
}, testid);

/** 그 줄 막대의 계산 배경과, 같은 사이드바에서 `var(<name>)`이 푸는 색 */
const barColor = (p, testid, name) => p.evaluate(([id, v]) => {
  const select = document.querySelector(`[data-panel="work"] [data-testid="${id}"]`);
  const probe = document.createElement("span");
  probe.style.background = `var(${v})`;
  select.closest(".sidebar-shell").append(probe);
  const want = getComputedStyle(probe).backgroundColor;
  probe.remove();
  const bar = getComputedStyle(select, "::before");
  return { content: bar.content, bar: bar.backgroundColor, width: bar.width, want };
}, [testid, name]);

/**
 * 화면 여섯 장(데스크톱·패드·휴대폰 × 밝음·어두움). 오래 열어 둔 첫 화면(t1)에서 크기와 테마(설정 「모양」)만 바꿔 찍는다: 새로 페어링한
 * 화면은 첫 고르기 뒤에도 고른 줄이 바뀔 수 있어 찍는 줄이 흔들린다. 휴대폰은 목록 서랍을 연다. 끝나면 데스크톱·어두움으로 돌린다
 */
async function capture(p, name) {
  for (const [scheme, word] of [["light", "밝게"], ["dark", "어둡게"]]) {
    await p.setViewportSize(SIZES.desktop);
    const dialog = await openSettings(p, "모양");
    await dialog.getByRole("group", { name: "테마" }).getByRole("button", { name: word }).click();
    await domIs(p, "테마", (want) => document.documentElement.getAttribute("data-theme") === want, scheme);
    await p.keyboard.press("Escape");
    await dialog.waitFor({ state: "detached" });
    for (const [device, viewport] of Object.entries(SIZES)) {
      await p.setViewportSize(viewport);
      await openList(p);
      await dropletsGone(p);
      await shot(p, `${name}-${device}-${scheme}`);
      await closeList(p);
    }
  }
  await p.setViewportSize(SIZES.desktop);
}

/** 보고 소식 물방울(앱 안 알림)이 목록을 가리지 않게 닫는다. 쌓인 것은 하나씩 내려오므로 없어질 때까지 */
async function dropletsGone(p) {
  // 서랍이 위를 덮은 휴대폰에서도 닫기 단추를 누른다(DOM click). 나가는 움직임이 끝나 물방울이 사라질 때까지 본다
  await until("물방울 닫힘", () => p.evaluate(() => {
    const drop = document.querySelector(".droplet");
    if (!drop) return true;
    if (drop.dataset.phase !== "out") drop.querySelector(".droplet-close")?.click();
    return false;
  }));
}

async function closeList(p) {
  const toggle = p.locator(".view [data-testid='drawer-toggle']");
  if ((await toggle.isVisible()) && (await toggle.getAttribute("aria-expanded")) === "true") {
    await toggle.click();
    await drawerSettled(p);
  }
}

export default async function ({ page }) {
  let auto;
  let human;
  await step("자동화 thread 만들기와 화면(데스크톱·패드·휴대폰, 밝음·어두움): 실제 host가 규칙으로 띄운 thread가 사람이 띄운 thread와 같은 목록에 선다", async () => {
    aaa("project", "create", project);
    // 이 실험실의 host.toml에는 [safety]가 없어 프로젝트의 옛 자리 값을 쓴다(사람만 고치는 safety는 건드리지 않는다)
    fs.appendFileSync(`${dir()}/project.toml`, '\nstart_threads = "auto"\n');
    aaa("coordinator", "open", "--profile", "fake");
    fs.writeFileSync(`${root}/human.md`, "사람이 맡긴 일\n");
    human = aaa("thread", "start", project, "--title", "사람이 맡긴 일", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/human.md`).id;
    fs.mkdirSync(`${dir()}/automations`, { recursive: true });
    fs.writeFileSync(`${dir()}/automations/ci-main.toml`, 'title = "CI 실패 분류"\n[on.schedule]\nevery = "1s"\n[do.start]\ntitle = "CI 분류"\ntext = "실패를 분류한다"\nprofiles = ["fake"]\nmachines = ["labA"]\n[limits]\nmax_per_day = 1\n');
    const rule = await until("자동 시작 범위 승인 대기", () => {
      aaa("ticker", "tick");
      return aaa("routine", "list").routines.find((r) => r.name === "ci-main" && r.start_approval_sha256);
    });
    const approved = await call(page, { op: "automation_approve", project, name: "ci-main", approval_sha256: rule.start_approval_sha256 });
    if (approved.err) throw new Error(`승인 ${JSON.stringify(approved)}`);
    auto = await until("규칙이 띄운 thread의 보고", () => {
      aaa("ticker", "tick");
      return threads().find((t) => t.automation === "ci-main" && t.report_hash);
    });
    await until("사람 thread 보고", () => threads().find((t) => t.id === human && t.report_hash));
    if (!auto.automation_trigger?.includes("every 1s")) throw new Error(`일정 출처 ${JSON.stringify(auto)}`);
    if (auto.automation_title !== "CI 실패 분류") throw new Error(`규칙 제목 ${JSON.stringify(auto)}`);
    if (threads().find((t) => t.id === human).automation !== undefined) throw new Error("사람 thread에 자동화 표지");
    // 할 일 화면의 「task 없는 열린 thread」에는 자동화 thread가 없다(출처가 있어 task가 필요 없다, plan t-0475 §5.3)
    const board = aaa("task", "list");
    if (board.unlinked_threads.some((t) => t.id === auto.id) || !board.unlinked_threads.some((t) => t.id === human)) throw new Error(`task 없는 열린 thread ${JSON.stringify(board.unlinked_threads)}`);

    await inList(page, `thread-${project}-${auto.id}`);
    await inList(page, `thread-${project}-${human}`);
    await capture(page, "a1");
  });

  await step("막대와 표지: 자동화 thread 줄은 data-origin=automation과 호박색 막대(계산 배경 = --automation), 사람 thread 줄은 막대 없음, coordinator 줄은 그대로 보라. 데스크톱·휴대폰 폭에서 막대가 agent 표지를 덮지 않는다", async () => {
    const autoRow = `thread-${project}-${auto.id}`;
    const humanRow = `thread-${project}-${human}`;
    await inList(page, autoRow);
    const origins = await page.evaluate(([a, h]) => [a, h].map((id) => document.querySelector(`[data-panel="work"] [data-testid="${id}"]`).closest("li").dataset.origin ?? null), [autoRow, humanRow]);
    if (origins[0] !== "automation" || origins[1] !== null) throw new Error(`data-origin ${JSON.stringify(origins)}`);
    for (const scheme of ["dark", "light"]) {
      if (scheme === "light") {
        const dialog = await openSettings(page, "모양");
        await dialog.getByRole("group", { name: "테마" }).getByRole("button", { name: "밝게" }).click();
        await domIs(page, "밝은 테마", () => document.documentElement.getAttribute("data-theme") === "light");
        await page.keyboard.press("Escape");
        await dialog.waitFor({ state: "detached" });
      }
      const a = await barColor(page, autoRow, "--automation");
      // 밝은 테마의 호박색은 진하게(#9a6700), 어두운 테마는 밝게(#e3b341)
      const want = scheme === "light" ? "rgb(154, 103, 0)" : "rgb(227, 179, 65)";
      if (a.bar !== a.want || a.bar !== want || a.width !== "2px") throw new Error(`${scheme} 자동화 막대 ${JSON.stringify(a)}`);
      const h = await barColor(page, humanRow, "--automation");
      if (h.content !== "none") throw new Error(`${scheme} 사람 thread 줄에 막대 ${JSON.stringify(h)}`);
      const c = await barColor(page, "pane-auto-bar coordinator", "--coordinator");
      if (c.bar !== c.want || c.bar === a.bar) throw new Error(`${scheme} coordinator 막대 ${JSON.stringify(c)}`);
    }
    const dialog = await openSettings(page, "모양");
    await dialog.getByRole("group", { name: "테마" }).getByRole("button", { name: "어둡게" }).click();
    await page.keyboard.press("Escape");
    await dialog.waitFor({ state: "detached" });
    const tag = page.getByTestId(`thread-origin-${project}-${auto.id}`);
    const words = async () => (await tag.textContent()).replace(/\s+/g, " ");
    if ((await words()) !== "자동화 thread") throw new Error(`표지 글 ${await words()}`);
    if (await page.getByTestId(`thread-origin-${project}-${human}`).count()) throw new Error("사람 thread에 표지");
    for (const [device, viewport] of [["데스크톱", SIZES.desktop], ["휴대폰", SIZES.phone]]) {
      await page.setViewportSize(viewport);
      await openList(page);
      const m = await barClear(page, autoRow);
      if (m.inRow < 0 || m.gap < 3) throw new Error(`${device} 자동화 막대와 표지: ${JSON.stringify(m)}`);
      // 휴대폰 폭에서도 「자동화 thread」 이름을 유지한다
      const shown = await tag.evaluate((el) => el.innerText.replace(/\s+/g, " ").trim());
      if (shown !== "자동화 thread") throw new Error(`${device} 표지 ${shown}`);
      await closeList(page);
    }
    await page.setViewportSize(SIZES.desktop);
  });

  await step("표지를 누르면 줄(pane)을 열지 않고 자동화 패널의 그 규칙 상세가 열린다. 상세 기록의 thread 번호와 규칙 줄 「열림」 칩은 그 thread를 연다. ⋯ 메뉴에도 「자동화 규칙」", async () => {
    const autoRow = `thread-${project}-${auto.id}`;
    await inList(page, autoRow);
    const before = await page.evaluate(() => location.hash);
    await page.getByTestId(`thread-origin-${project}-${auto.id}`).click();
    const sheet = page.getByTestId("automation-detail");
    await sheet.waitFor();
    if (!(await sheet.textContent()).includes("CI 실패 분류")) throw new Error("다른 규칙 상세");
    if ((await page.evaluate(() => location.hash)) !== before) throw new Error("표지가 줄(pane)도 열었다");
    if ((await page.getByTestId("activity-automations").getAttribute("aria-selected")) !== "true") throw new Error("자동화 패널이 보이지 않는다");
    // 기록의 thread 번호 → 창을 닫고 그 thread(도는 pane)를 연다
    await sheet.getByTestId(`automation-thread-${auto.id}`).first().click();
    await sheet.waitFor({ state: "detached" });
    await domIs(page, "규칙 thread pane", (pane) => location.hash === `#pane=${pane}`, auto.pane);
    // 규칙 줄의 「열림」 칩
    const t1 = aaa("pane", "list").panes.find((x) => x.name === "t1" && !x.role).id;
    await page.getByTestId("activity-work").click();
    await page.getByTestId("pane-t1").click();
    await domIs(page, "t1", (pane) => location.hash === `#pane=${pane}`, t1);
    await page.getByTestId("activity-automations").click();
    const chip = page.getByTestId(`automation-open-${project}-${auto.id}`);
    await chip.waitFor();
    if ((await chip.textContent()) !== `열림 ${auto.id}`) throw new Error(`칩 글 ${await chip.textContent()}`);
    await chip.click();
    await domIs(page, "칩이 연 thread pane", (pane) => location.hash === `#pane=${pane}`, auto.pane);
    await shot(page, "a1-automation-panel");
    // ⋯ 메뉴(키보드 길)
    await page.getByTestId("activity-work").click();
    await page.locator(`[data-panel="work"] li[data-thread="${project}/${auto.id}"] > .workspace-header`).click({ button: "right" });
    const menu = page.locator(".row-menu, .row-sheet");
    await menu.waitFor();
    await menu.getByRole("menuitem", { name: "자동화 규칙" }).click();
    await page.getByTestId("automation-detail").waitFor();
    await page.keyboard.press("Escape");
    await page.getByTestId("automation-detail").waitFor({ state: "detached" });
    await page.getByTestId("activity-work").click();
    // 휴대폰 폭: 목록 서랍 안의 표지를 손가락으로 누르면 서랍 안에서 자동화 패널과 규칙 상세가 열린다(줄 pane은 열지 않는다)
    await page.setViewportSize(SIZES.phone);
    await openList(page);
    const at = await page.evaluate(() => location.hash);
    await page.getByTestId(`thread-origin-${project}-${auto.id}`).click();
    await page.getByTestId("automation-detail").waitFor();
    if ((await page.evaluate(() => location.hash)) !== at) throw new Error("휴대폰 표지가 줄(pane)도 열었다");
    await shot(page, "a1-phone-rule-detail");
    await page.keyboard.press("Escape");
    await page.getByTestId("automation-detail").waitFor({ state: "detached" });
    await page.getByTestId("activity-work").click();
    await closeList(page);
    await page.setViewportSize(SIZES.desktop);
  });

  await step("「모든 thread」의 「자동화만」: 자동화가 띄운 thread만 남고 줄 사실에 「자동화 thread」가 붙는다", async () => {
    await page.evaluate(() => (location.hash = "#threads"));
    const all = page.getByTestId("all-threads");
    await all.waitFor();
    await all.getByTestId(`project-thread-${human}`).waitFor();
    const toggle = all.getByTestId("threads-automation-only");
    await toggle.click();
    await domIs(page, "자동화만", (h) => !document.querySelector(`[data-testid="project-thread-${h}"]`), human);
    if ((await toggle.getAttribute("aria-pressed")) !== "true") throw new Error("토글 상태");
    if (!(await all.getByTestId(`project-thread-${auto.id}`).textContent()).includes("자동화 thread")) throw new Error("줄 사실에 자동화 표지가 없다");
    await shot(page, "a1-all-threads-automation-only");
    await all.getByTestId(`project-thread-${auto.id}`).click();
    const origin = page.getByTestId("thread-automation-origin");
    await origin.waitFor();
    const detail = await origin.textContent();
    if (!detail.includes("CI 실패 분류 (ci-main)") || !detail.includes(auto.automation_trigger) || !detail.includes("프로젝트 실행 한도와 예약 자리에서 제외")) throw new Error(`thread 출처 상세 ${detail}`);
    await shot(page, "a1-thread-origin-detail");
    await page.evaluate(() => (location.hash = "#threads"));
    await all.waitFor();
    if ((await toggle.getAttribute("aria-pressed")) === "true") await toggle.click();
    await all.getByTestId(`project-thread-${human}`).waitFor();
  });
}
