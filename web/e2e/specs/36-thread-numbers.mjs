// thread 번호(사용자 요청 10-20 「보고서에 항상 스레드 번호 적어 … 특수스레드도 번호 … 왼쪽에」): 보고서 목록·열람 머리와
// 「특수」 패널 줄 왼쪽에 근원 thread 번호를 보인다. 데스크톱·휴대폰 폭에서 번호가 줄 안에 다 보이고 다른 표지와 겹치지 않는다.
import fs from "node:fs";
import { root, aaa, step, showPanel, drawerSettled, domIs, until, shot } from "../lib/harness.mjs";

const SIZES = [["desktop", 1280, 800], ["phone", 390, 844]];

/** 크기마다 새 탭(이전 크기의 움직임을 섞지 않는다). drawer면 좁은 화면에서 목록 drawer를 연다 */
async function sized(page, width, height, drawer, body) {
  const pg = await page.context().newPage();
  try {
    await pg.setViewportSize({ width, height });
    await pg.goto(page.url());
    if (drawer) {
      await pg.getByTestId("activity-special").waitFor({ state: "attached" });
      const toggle = pg.locator(".view [data-testid='drawer-toggle']");
      if (await toggle.isVisible() && await toggle.getAttribute("aria-expanded") !== "true") await toggle.click();
      await drawerSettled(pg);
    }
    await body(pg);
  } finally { await pg.close(); }
}

/**
 * 줄(`row`) 안 번호(`mark`)의 글과 문제: 크기 0, 잘림(넘침을 숨기는 가장 가까운 조상·화면 밖), 다른 것에 가림(가운데 점),
 * `others` 표지와 겹침, 줄의 가로 넘침. 번호가 없으면 text가 null이다.
 */
function numberIn(row, mark, others) {
  return row.evaluate((el, [mark, others]) => {
    const no = el.querySelector(mark);
    if (!no) return { text: null, problems: [] };
    const r = no.getBoundingClientRect();
    let clip = no.parentElement;
    while (clip !== el && getComputedStyle(clip).overflowX === "visible") clip = clip.parentElement;
    const c = clip.getBoundingClientRect();
    const area = (a, b) => Math.max(0, Math.min(a.right, b.right) - Math.max(a.left, b.left)) * Math.max(0, Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top));
    const hit = document.elementFromPoint((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    const problems = [];
    if (r.width < 1 || r.height < 1) problems.push("크기 0");
    if (r.left < c.left - 0.5 || r.right > c.right + 0.5 || r.left < 0 || r.right > innerWidth) problems.push(`잘림 ${JSON.stringify([r.left, r.right, c.left, c.right])}`);
    if (!no.contains(hit)) problems.push(`가림 ${hit?.className}`);
    for (const o of others.flatMap((s) => [...el.querySelectorAll(s)])) {
      if (!o.contains(no) && !no.contains(o) && area(o.getBoundingClientRect(), r) > 0.5) problems.push(`겹침 ${o.className || o.tagName}`);
    }
    if (el.scrollWidth > el.clientWidth + 1) problems.push("줄 넘침");
    return { text: no.textContent, problems };
  }, [mark, others]);
}

export default async function ({ page }) {
  let thread;
  await step("보고서 thread 번호: thread library에서 올린 사용자 보고서·--thread·접두사 붙인 import는 번호를, thread 사본 줄은 번호 한 번만, 근원 없는 보고서는 번호 없이", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task-numbers.md`, "번호 보기");
    thread = aaa("thread", "start", "demo", "--title", "번호 보고", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-numbers.md`);
    await until("thread 사본", () => aaa("library", "list", "--project", "demo").reports.some((r) => r.name === thread.id));
    const page1 = (dir, text) => {
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(`${dir}/index.html`, `<!doctype html><html lang=ko><meta charset=utf-8><h1>${text}</h1></html>`);
      return dir;
    };
    // coordinator가 thread library를 옮겨 둔 꼴(library/t-NNNN)에서 올린 사용자 보고서는 경로에서, --thread는 그대로
    aaa("library", "publish", page1(`${root}/coord/library/${thread.id}`, "주간 보고"), "--name", "weekly", "--to-user", "--title", "주간 보고");
    aaa("library", "publish", page1(`${root}/coord/summary`, "요약"), "--name", "summary", "--to-user", "--title", "요약", "--thread", thread.id);
    aaa("library", "publish", page1(`${root}/coord/notes`, "메모"), "--name", "notes", "--to-user", "--title", "근원 없는 메모");
    page1(`${root}/batch/${thread.id}`, "옛 사본");
    aaa("library", "import", `${root}/batch`, "--project", "demo", "--prefix", "old-");
    const origin = Object.fromEntries(aaa("library", "list").reports.map((r) => [r.name, r.from_thread ?? null]));
    const want = { weekly: thread.id, summary: thread.id, notes: null, [thread.id]: thread.id, [`old-${thread.id}`]: thread.id };
    if (JSON.stringify(Object.keys(want).map((n) => origin[n])) !== JSON.stringify(Object.values(want))) throw new Error(`CLI 목록의 근원 thread: ${JSON.stringify(origin)}`);
    await page.getByTestId("activity-reports").click();
    await domIs(page, "보고서 목록 준비", () => document.querySelector('[data-testid="reports-panel"]')?.dataset.status === "ready");
    const bad = [];
    const check = async (pg, where, row, want, others) => {
      const got = await numberIn(row, '[data-testid="report-thread"]', others);
      if (got.text !== want || got.problems.length) bad.push({ where, want, ...got });
    };
    for (const [name, width, height] of SIZES) {
      await sized(page, width, height, false, async (pg) => {
        await pg.getByTestId("report-demo-weekly").waitFor();
        // 내 보고서 카드: 안 봄 점·이름·크기와 겹치지 않는다
        const card = [".report-new-dot", ".report-heading a", ".report-heading > span"];
        for (const [n, w] of [["weekly", thread.id], ["summary", thread.id], ["notes", null]]) await check(pg, `${name} 내 보고서 ${n}`, pg.getByTestId(`report-demo-${n}`).locator(".report-heading"), w, card);
        await shot(pg, `${name}-reports-mine`);
        await pg.getByTestId("reports-tab-work").click();
        await pg.getByTestId(`report-demo-${thread.id}`).waitFor();
        // 작업 보고 줄: 고르기·이름·진행 중·크기·단추와 겹치지 않는다
        const row = [".report-pick", ".report-row-name", ".report-tag.is-busy", ".report-row-meta", ".report-row-actions"];
        // thread 사본 줄은 이름이 곧 번호라 표지를 빼고 번호를 한 번만 보인다. 접두사 붙인 import는 표지를 단다
        for (const [n, w] of [[thread.id, null], [`old-${thread.id}`, thread.id]]) {
          const main = pg.getByTestId(`report-demo-${n}`).locator(".report-row-main");
          await check(pg, `${name} 작업 보고 ${n}`, main, w, row);
          const shown = (await main.innerText()).split(thread.id).length - 1;
          if (n === thread.id && shown !== 1) bad.push({ where: `${name} 작업 보고 사본 줄`, want: "번호 한 번", shown });
        }
        await shot(pg, `${name}-reports-work`);
        await pg.getByTestId("reports-tab-mine").click();
        await pg.getByTestId("report-demo-weekly").getByTestId("report-open").click();
        await domIs(pg, "보고서 열람", () => document.querySelector('[data-testid="report-detail"]')?.dataset.status === "ready");
        await check(pg, `${name} 열람 머리`, pg.getByTestId("report-detail").locator(".term-head"), thread.id, [".term-title strong", ".term-title .muted", "[data-testid='report-back']", "[data-testid='report-window']"]);
        await shot(pg, `${name}-report-detail`);
      });
    }
    if (bad.length) throw new Error(`보고서 thread 번호: ${JSON.stringify(bad)}`);
  });

  await step("특수 줄 thread 번호: 시작한 임무와 사후 평가자는 일반 thread 줄처럼 줄 왼쪽에 번호, 시작 전 임무는 번호 없이", async () => {
    fs.mkdirSync(`${root}/projects/demo/missions`, { recursive: true });
    fs.writeFileSync(`${root}/projects/demo/missions/ci-watch.md`, '+++\nprofile="fake"\nmachine="labA"\ntitle="rc 검증·병합 시간 최적화 감시"\nwithdrawal="none"\n+++\n새 입력을 자료로 읽고 감시한다.\n');
    fs.writeFileSync(`${root}/projects/demo/missions/idle-watch.md`, '+++\nprofile="fake"\nmachine="labA"\ntitle="아직 시작하지 않은 감시"\nwithdrawal="none"\n+++\n시작 전이다.\n');
    aaa("ticker", "tick");
    await page.setViewportSize({ width: 1280, height: 800 });
    await showPanel(page, "special");
    const item = page.getByTestId("mission-demo-ci-watch");
    await item.waitFor();
    await item.locator(".workspace-header").click({ button: "right" });
    await page.getByRole("menuitem", { name: "승인 검토" }).click();
    await page.getByRole("alertdialog").getByRole("button", { name: "승인", exact: true }).click();
    await until("임무 승인", () => aaa("mission", "show", "demo", "ci-watch").mission.approved === true);
    const mission = aaa("mission", "start", "demo", "ci-watch").mission.thread.id;
    const evaluator = aaa("evaluator", "assign", "demo", "--profile", "fake", "--machine", "labA").evaluators[0].thread.id;
    // 임무·평가자 번호는 일반 thread와 같은 차례에서 온다
    if (new Set([thread.id, mission, evaluator]).size !== 3 || ![mission, evaluator].every((id) => /^t-\d{4}$/.test(id))) throw new Error(`특수 thread 번호: ${thread.id} ${mission} ${evaluator}`);
    await page.getByTestId("evaluator-demo").waitFor();
    await domIs(page, "임무 줄 pane", () => !document.querySelector('[data-testid="mission-demo-ci-watch"] .special-dot')?.classList.contains("is-nopane"));
    // 번호 모양은 일반 thread 줄과 같다(같은 조각): 글꼴·크기·굵기·색
    const look = (row) => row.evaluate((r) => {
      const n = r.querySelector('[data-testid="thread-no"]');
      const s = n && getComputedStyle(n);
      return s ? [s.fontFamily, s.fontSize, s.fontWeight, s.color].join("|") : "번호 없음";
    });
    const special = await look(page.getByTestId("mission-demo-ci-watch"));
    await showPanel(page, "work");
    const plain = await look(page.locator(`[data-thread="demo/${thread.id}"]`));
    if (special !== plain) throw new Error(`특수 줄 번호 모양 ${special} ≠ 일반 줄 ${plain}`);
    const bad = [];
    const marks = [".sidebar-mark", ".special-dot", ".special-state", ".thread-machine", ".row-menu-toggle"];
    for (const [name, width, height] of SIZES) {
      await sized(page, width, height, true, async (pg) => {
        await showPanel(pg, "special");
        await pg.getByTestId("evaluator-demo").waitFor();
        for (const [testid, want] of [["mission-demo-ci-watch", mission], ["mission-demo-idle-watch", null], ["evaluator-demo", evaluator]]) {
          const row = pg.getByTestId(testid).locator(".workspace-header");
          const got = await numberIn(row, '[data-testid="thread-no"]', marks);
          // 번호는 줄 왼쪽: 이름 글의 맨 앞이다
          const first = await row.locator(".workspace-name").evaluate((n) => n.textContent);
          if (got.text !== want || got.problems.length || (want && !first.startsWith(`${want} · `))) bad.push({ where: `${name} ${testid}`, want, first, ...got });
        }
        await shot(pg, `${name}-special`);
      });
    }
    if (bad.length) throw new Error(`특수 줄 thread 번호: ${JSON.stringify(bad)}`);
  });
}
