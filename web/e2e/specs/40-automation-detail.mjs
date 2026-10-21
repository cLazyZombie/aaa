// 자동화 상세·옛 형식 변환·막힌 큐 풀기(관리 GUI W1, plan §8.303). 실제 일정 판정·DB 기록·파일 변환·thread 전송 실패를 host가 만들고
// 웹은 그것을 읽고 사람의 결정만 보낸다. 시간은 기다리지 않는다: routine 기록·큐 상태·pane 출력을 기다린다.
import fs from "node:fs";
import { root, aaa, step, browser, pairPage, until, domIs, drawerSettled, shot } from "../lib/harness.mjs";
export const labEnv = { AAA_TEST_ROUTINE_SECONDS: "1" };

const dir = () => `${root}/projects/auto-detail`;
const CHECK = '+++\n# 보존할 주석\nschedule = "every 1s"\nenabled = true\n+++\n자료를 확인한다.\n';
const WATCH = '+++\nprofile="fake"\nevents=["git_pushed"]\nrepos=["app"]\nwithdrawal="none"\n+++\n자료를 분류한다.\n';
const call = (p, req) => p.evaluate(async (body) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })).json(), req);
/** 본문 설정 화면이 화면 폭 안에 있고 옆으로 넘치지 않는다(떠 있는 창이 아니다, t-0628) */
const fits = (p, selector) => p.evaluate((sel) => {
  const view = document.querySelector(sel);
  const body = view?.querySelector(".automation-view-body");
  const box = view?.getBoundingClientRect();
  const close = view?.querySelector('[aria-label="닫기"]')?.getBoundingClientRect();
  return view && body && !document.querySelector(".modal-scrim") && box.left >= 0 && box.right <= innerWidth + 0.5 && body.scrollWidth <= body.clientWidth + 1 && close && close.height >= 32 ? true : JSON.stringify({ box, close, sw: body?.scrollWidth, cw: body?.clientWidth, w: innerWidth });
}, selector);
/** 본문 설정 화면을 닫는다(앞 화면으로) */
const closeDetail = async (p) => {
  const view = p.getByTestId("automation-detail");
  await view.getByTestId("automation-close").click();
  await view.waitFor({ state: "hidden" });
};

export default async function ({ page }) {
  await step("자동화 설정: 카드를 누르면 본문에 요약·설정(조건·동작·대상·큐)·승인 범위·마지막 건너뜀·기록·파일 원문이 열린다", async () => {
    aaa("project", "create", "auto-detail");
    fs.writeFileSync(`${dir()}/routines/check.md`, CHECK);
    fs.mkdirSync(`${dir()}/missions`, { recursive: true });
    fs.writeFileSync(`${dir()}/missions/watch.md`, WATCH);
    await until("coordinator 없는 실제 차례 둘", () => {
      aaa("ticker", "tick");
      return aaa("routine", "runs", "auto-detail", "check").runs.length >= 2;
    });
    await page.getByTestId("activity-automations").click();
    const row = page.getByTestId("automation-auto-detail-check");
    await row.waitFor();
    if (!(await row.textContent()).includes("이전 형식")) throw new Error("옛 routine 줄에 이전 형식 배지가 없다");
    await row.click();
    const sheet = page.getByTestId("automation-detail");
    await sheet.waitFor();
    const sections = await sheet.locator("section > h3").allTextContents();
    for (const want of ["설정", "승인 범위"]) if (!sections.includes(want)) throw new Error(`설정 절 ${want}: ${sections}`);
    if (!sections.some((s) => s.startsWith("기록 · 최근"))) throw new Error(`기록 절 ${sections}`);
    const facts = await sheet.getByLabel("설정").locator("dt").allTextContents();
    for (const want of ["조건", "동작", "대상", "대기 입력", "파일"]) if (!facts.includes(want)) throw new Error(`설정 칸 ${want}: ${facts}`);
    for (const want of ["지난", "다음", "오늘", "열린 thread"]) if (!(await sheet.getByLabel("요약").locator("dt").allTextContents()).includes(want)) throw new Error(`요약 칸 ${want}`);
    // 기록은 host의 50건 기록(routine_runs)이다: 목록의 20건 요약이 아니다
    const host = aaa("routine", "runs", "auto-detail", "check").runs;
    await until("기록 줄 수", async () => (await sheet.locator(".automation-runs li").count()) === host.length);
    if (!(await sheet.locator(".automation-runs").textContent()).includes("coordinator 없음")) throw new Error("기록 글");
    await until("파일 원문", async () => (await sheet.getByTestId("automation-source").locator("pre").first().textContent()) === CHECK);
    if (!(await sheet.textContent()).includes("마지막 건너뜀") || !(await sheet.getByLabel("마지막 건너뜀").textContent()).includes("coordinator 없음")) throw new Error("마지막 건너뜀");
    await shot(page, "automation-detail-desktop");
    await closeDetail(page);

    // 패드 폭(820): 640px 밖이라 가운데 창이며 화면 안에 있다
    const pad = await pairPage(browser, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true });
    try {
      await pad.page.getByTestId("activity-automations").click();
      await pad.page.getByTestId("automation-auto-detail-check").click();
      await pad.page.getByTestId("automation-detail").locator(".automation-runs li").first().waitFor();
      const fit = await fits(pad.page, '[data-testid="automation-detail"]');
      if (fit !== true) throw new Error(`패드 상세 폭 ${fit}`);
      await shot(pad.page, "automation-detail-pad");
      if (pad.errors.length) throw new Error(pad.errors.join("\n"));
    } finally { await pad.page.context().close(); }
  });

  await step("새 형식으로 바꾸기: 미리보기는 쓰지 않고 생기는 규칙·보존 위치를 보이며, 실행은 본 계획만 바꾼다. Reader는 미리보기만", async () => {
    const before = { check: fs.readFileSync(`${dir()}/routines/check.md`, "utf8"), watch: fs.readFileSync(`${dir()}/missions/watch.md`, "utf8") };
    const dev = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 });
    try {
      await dev.page.getByTestId("activity-automations").click();
      await dev.page.getByTestId("automation-legacy-auto-detail").getByRole("button", { name: "새 형식으로 바꾸기", exact: true }).click();
      const reader = dev.page.getByTestId("automation-convert");
      await reader.locator('[data-group="rules"]').waitFor();
      if (await reader.getByRole("button", { name: "변환", exact: true }).count()) throw new Error("Reader 변환 단추");
      const forbidden = await call(dev.page, { op: "automation_convert", project: "auto-detail", plan_sha256: "0".repeat(64) });
      if (forbidden.err?.code !== "forbidden") throw new Error(`Reader 변환 ${JSON.stringify(forbidden)}`);
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await dev.page.context().close(); }

    const legacy = page.getByTestId("automation-legacy-auto-detail");
    if (!(await legacy.textContent()).includes("3개")) throw new Error(`이전 형식 수 ${await legacy.textContent()}`);
    await legacy.getByRole("button", { name: "새 형식으로 바꾸기", exact: true }).click();
    const sheet = page.getByTestId("automation-convert");
    await sheet.locator('[data-group="rules"]').waitFor();
    const group = async (key) => (await sheet.locator(`[data-group="${key}"] li`).allTextContents());
    if (!(await group("rules")).includes("automations/watch.toml")) throw new Error(`생기는 규칙 ${await group("rules")}`);
    for (const f of ["routines/.converted/check.md", "routines/.converted/pr-followup.md", "missions/.converted/watch.md"]) if (!(await group("archived")).includes(f)) throw new Error(`보존 ${f}: ${await group("archived")}`);
    if (!(await group("rules")).includes("automations/check.toml")) throw new Error("새 규칙 파일");
    // 미리보기는 쓰지 않는다
    if (fs.readFileSync(`${dir()}/routines/check.md`, "utf8") !== before.check || fs.existsSync(`${dir()}/automations/check.toml`)) throw new Error("미리보기가 파일을 바꿨다");
    await shot(page, "automation-convert-preview");
    await sheet.getByRole("button", { name: "변환", exact: true }).click();
    await sheet.getByTestId("convert-archived").waitFor();
    if (!(await sheet.getByTestId("convert-archived").textContent()).includes("routines/.converted/")) throw new Error("보존 위치");
    await shot(page, "automation-convert-done");
    if (fs.readFileSync(`${dir()}/routines/.converted/check.md`, "utf8") !== before.check || fs.readFileSync(`${dir()}/missions/.converted/watch.md`, "utf8") !== before.watch) throw new Error("보존 사본이 원본과 다르다");
    if (!fs.existsSync(`${dir()}/automations/check.toml`) || fs.existsSync(`${dir()}/routines/check.md`)) throw new Error("변환 결과 파일");
    await sheet.getByRole("button", { name: "닫기", exact: true }).first().click();
    await sheet.waitFor({ state: "detached" });
    await domIs(page, "이전 형식 줄이 사라진다", () => !document.querySelector('[data-testid="automation-legacy-auto-detail"]') && !document.querySelector('[data-testid="automation-auto-detail-check"]')?.textContent.includes("이전 형식"));
  });

  // 변환한 watch의 시작 범위는 아직 승인 전이다.
  await domIs(page,"변환 규칙 승인 배지",()=>document.querySelector('[data-testid="activity-badge-automations"]')?.textContent==="1");
  let thread = "";
  await step("막힌 큐: 대상 pane이 끝나 보낸 결과를 모르는 do.message 묶음은 다시 보내지 않고, 사람이 확인하면 계속 보내기는 다음 입력을 보내고 버리기는 남은 입력을 닫는다", async () => {
    fs.writeFileSync(`${root}/detail-task.md`, "# 자동 글 받기\n\n사람을 기다림\n");
    thread = aaa("thread", "start", "auto-detail", "--title", "자동 글 받기", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/detail-task.md`).id;
    await until("brief 전달", () => aaa("thread", "show", "auto-detail", thread).brief === "delivered");
    const pane = aaa("thread", "show", "auto-detail", thread).pane;
    aaa("pane", "kill", pane);
    await until("pane 끝남", () => aaa("pane", "list").panes.some((p) => p.id === pane && p.state !== "running"));
    fs.writeFileSync(`${dir()}/automations/ping.toml`, `[on.schedule]\nevery="1s"\n[do.message]\nthread=${JSON.stringify(thread)}\ntext="자동 확인 글"\n`);
    await until("host가 묶음을 보내다 실패해 확인 필요로 기다림", () => {
      aaa("ticker", "tick");
      return aaa("routine", "runs", "auto-detail", "ping").runs.some((r) => r.status === "waiting" && r.detail === "입력 전달 모름 · 확인 필요");
    });
    const stuck = aaa("routine", "list").routines.find((r) => r.name === "ping").queue.inflight;
    if (!stuck?.unknown || stuck.thread !== thread) throw new Error(`막힌 묶음 ${JSON.stringify(stuck)}`);
    // 자동화 막대 배지는 열리지 않은 패널에서도 실제 막힌 큐를 센다.
    await page.reload();
    await domIs(page,"승인 규칙 하나와 막힌 규칙 하나의 배지",()=>document.querySelector('[data-testid="activity-badge-automations"]')?.textContent==="2");
    if (await page.getByTestId("activity-special").count()) throw new Error("특수 단추가 남았다");
    // 다시 판정해도 같은 묶음은 보내지 않는다(같은 묶음 그대로, 실패 기록이 늘지 않음)
    const failed = aaa("routine", "runs", "auto-detail", "ping").runs.filter((r) => r.status === "failed").length;
    aaa("ticker", "tick");
    aaa("ticker", "tick");
    const again = aaa("routine", "list").routines.find((r) => r.name === "ping").queue.inflight;
    if (again?.id !== stuck.id || aaa("routine", "runs", "auto-detail", "ping").runs.filter((r) => r.status === "failed").length !== failed) throw new Error("막힌 묶음을 다시 보냈다");

    await page.getByRole("button", { name: "다시 읽기", exact: true }).click();
    const row = page.getByTestId("automation-auto-detail-ping");
    await domIs(page, "막힘 줄", () => document.querySelector('[data-testid="automation-auto-detail-ping"]')?.dataset.status === "stuck");
    // Reader는 막힘을 보지만 풀 수 없다
    const dev = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, { touch: true });
    try {
      const p = dev.page;
      await p.locator(".view [data-testid='drawer-toggle']").click();
      await p.locator("#workspace-drawer.is-open").waitFor();
      await drawerSettled(p);
      await p.getByTestId("activity-automations").click();
      await p.getByTestId("automation-auto-detail-ping").click();
      const sheet = p.getByTestId("automation-detail");
      await sheet.getByTestId("automation-stuck").waitFor();
      if (await sheet.getByRole("button", { name: /남은 입력/ }).count()) throw new Error("Reader 풀기 단추");
      const fit = await fits(p, '[data-testid="automation-detail"]');
      if (fit !== true) throw new Error(`휴대폰 상세 폭 ${fit}`);
      await shot(p, "automation-stuck-phone-reader");
      const forbidden = await call(p, { op: "automation_unblock", project: "auto-detail", name: "ping", batch: stuck.id, discard: false });
      if (forbidden.err?.code !== "forbidden") throw new Error(`Reader 풀기 ${JSON.stringify(forbidden)}`);
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await dev.page.context().close(); }

    await row.click();
    const sheet = page.getByTestId("automation-detail");
    await sheet.getByTestId("automation-stuck").waitFor();
    if (!(await sheet.getByTestId("automation-stuck").textContent()).includes(thread)) throw new Error("막힌 묶음 대상 thread");
    await shot(page, "automation-stuck-desktop");
    // 계속 보내기: 그 묶음은 닫고(다시 보내지 않음) 다음 입력은 보낸다. pane이 아직 끝난 채라 다음 입력이 실제로 시도되어 실패한다
    const answered = Date.now();
    await sheet.getByRole("button", { name: "확인함 · 남은 입력 계속 보내기", exact: true }).click();
    const confirm = page.getByRole("alertdialog");
    await confirm.waitFor();
    await confirm.getByRole("button", { name: "계속 보내기", exact: true }).click();
    await confirm.waitFor({ state: "detached" });
    await sheet.getByTestId("automation-stuck").waitFor({ state: "detached" });
    const runs = () => aaa("routine", "runs", "auto-detail", "ping").runs;
    if (!runs().some((r) => r.status === "skipped" && r.detail === "확인함 · 다시 보내지 않음")) throw new Error(`푼 기록 ${JSON.stringify(runs().slice(0, 3))}`);
    await until("다음 입력을 새 묶음으로 보내려다 다시 막힘", () => {
      aaa("ticker", "tick");
      const next = aaa("routine", "list").routines.find((r) => r.name === "ping").queue.inflight;
      return next?.unknown && next.id !== stuck.id && runs().filter((r) => r.status === "failed" && r.at_ms >= answered).length === 1;
    });
    await closeDetail(page);

    // 버리기: 다시 막힌 묶음과 뒤 입력을 닫는다. 그 뒤 판정은 아무것도 보내지 않는다(규칙을 끈 뒤 tick 울타리)
    const second = aaa("routine", "list").routines.find((r) => r.name === "ping").queue.inflight;
    await page.getByRole("button", { name: "다시 읽기", exact: true }).click();
    await domIs(page, "다시 막힘 줄", () => document.querySelector('[data-testid="automation-auto-detail-ping"]')?.dataset.status === "stuck");
    await row.click();
    await sheet.getByTestId("automation-stuck").waitFor();
    aaa("routine", "toggle", "auto-detail", "ping", "--off");
    await sheet.getByRole("button", { name: "확인함 · 남은 입력 버리기", exact: true }).click();
    await confirm.waitFor();
    await confirm.getByRole("button", { name: "버리기", exact: true }).click();
    await confirm.waitFor({ state: "detached" });
    await sheet.getByTestId("automation-stuck").waitFor({ state: "detached" });
    aaa("ticker", "tick");
    const after = aaa("routine", "list").routines.find((r) => r.name === "ping").queue;
    if (after.inflight || after.pending !== 0) throw new Error(`버린 뒤 큐 ${JSON.stringify(after)}`);
    // 기록은 입력 id마다 하나: 두 묶음의 입력 모두 「다시 보내지 않음」으로 닫혔다
    const closed = runs().filter((r) => r.status === "skipped" && r.detail === "확인함 · 다시 보내지 않음").length;
    if (closed !== stuck.events + second.events) throw new Error(`닫힌 입력 기록 ${closed}: ${JSON.stringify(runs().slice(0, 5))}`);
    await closeDetail(page);
  });
}
