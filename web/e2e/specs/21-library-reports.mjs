import { listIds } from "../lib/list-ids.mjs";
// 보고서 전체 목록은 CLI로 올린 host 사본·등록 폴더와 같은 정본을 본다. 기존 실험실의 자식 정리만 쓴다.
import fs from "node:fs";
import { root, aaa, base, browser, webkit, step, pairPage, inList, domIs, until, shot } from "../lib/harness.mjs";

export default async function ({ page }) {
  await step("보고서 전체 목록: publish·실시간·import, 활동 막대 「보고서」가 연 본문 화면(plan §8.240), 데스크톱·휴대폰의 열기·새 창·복사·폐기·새 링크", async () => {
    // host는 프로젝트 하나다(plan §8.285): 보고서는 모두 demo에. 프로젝트 인자 없는 새 꼴과 옛 꼴(--project demo)을 섞어 쓴다
    aaa("project", "create", "demo");
    const input = `${root}/reports`;
    fs.mkdirSync(input);
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>첫 보고서</h1></html>");
    // 사용자 보고서(--to-user, plan §8.256)는 「내 보고서」 탭의 카드다. import한 것은 「작업 보고」 탭이다
    const link = aaa("library", "publish", input, "--project", "demo", "--name", "page", "--to-user");
    const live = aaa("library", "publish", input, "--name", "design", "--live", "--to-user");
    const batch = `${root}/batch`;
    fs.mkdirSync(`${batch}/extra`, { recursive: true });
    fs.mkdirSync(`${batch}/page`);
    fs.mkdirSync(`${batch}/t-0001`);
    fs.writeFileSync(`${batch}/t-0001/index.html`,"이전 thread 보고서");
    fs.writeFileSync(`${batch}/extra/index.html`, "추가 보고서");
    fs.writeFileSync(`${batch}/page/index.html`, "덮이면 안 된다");
    const imported = aaa("library", "import", batch, "--project", "demo");
    if (imported.reports.length !== 1 || !imported.skipped.some((s) => s.name === "page") || !imported.skipped.some((s) => s.name === "t-0001")) throw new Error("CLI import의 건너뜀 목록");
    const list = aaa("library", "list").reports;
    if (list.length !== 3 || !list.some((r) => r.project === "demo" && r.name === "design" && r.live)) throw new Error("전체 보고서 CLI 목록");
    if (aaa("library", "prune", "--older-than", "30").removed.length !== 0) throw new Error("최근 보고서를 정리했다");
    const doctor = aaa("doctor").library;
    if (doctor?.reports !== 3 || doctor?.live !== 1 || doctor?.size !== 3 * Buffer.byteLength(fs.readFileSync(`${input}/index.html`)) + 2 * Buffer.byteLength("추가 보고서")) throw new Error("doctor의 host 사본 크기·실시간 수");
    if (doctor.max_days !== 30 || doctor.max_size !== 20 * 1024 ** 3 || doctor.protected_size !== Buffer.byteLength(fs.readFileSync(`${input}/index.html`))) throw new Error("doctor의 보관 한도·보호 용량");
    const preview = aaa("library", "prune", "--dry-run");
    if (preview.kind !== "library_prune_preview" || preview.candidates.length !== 0 || preview.max_days !== 30 || preview.max_size !== 20 * 1024 ** 3) throw new Error("자동 규칙의 CLI 미리 보기");
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>최신 보고서</h1></html>");
    aaa("library", "publish", input, "--project", "demo", "--name", "page", "--to-user");
    if (!(await (await fetch(new URL(live.base, base))).text()).includes("최신 보고서")) throw new Error("등록 폴더 변경을 반영하지 않았다");
    await inList(page, "activity-reports");
    const panel = page.getByTestId("reports-panel");
    await panel.waitFor();
    await domIs(page, "보고서 전체 목록 준비", () => document.querySelector('[data-testid="reports-panel"]')?.dataset.status === "ready");
    const usageText = await panel.getByTestId("reports-usage").innerText();
    if (!usageText.includes(" / 20 GB, 기한 30일")) throw new Error(`웹 보관 한도 표시: ${usageText}`);
    const row = page.getByTestId("report-demo-page");
    if (!(await row.innerText()).includes(`${Buffer.byteLength(fs.readFileSync(`${input}/index.html`))} B`) || !(await row.innerText()).includes("만료")) throw new Error("이름·크기·만료가 없다");
    if (!(await page.getByTestId("report-demo-design").innerText()).includes("실시간")) throw new Error("실시간 표시");
    await page.evaluate(() => {
      window.__reportCopies = [];
      Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text) => window.__reportCopies.push(text) } });
    });
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 800 });
      const copiesBefore=await page.evaluate(() => window.__reportCopies.length);
      await row.getByRole("button", { name: "링크 복사" }).click();
      await domIs(page, "이번 보고서 링크 복사 기록", (n) => window.__reportCopies.length === n + 1, copiesBefore);
      if (await page.evaluate(() => window.__reportCopies.at(-1)) !== new URL(link.base, base).href) throw new Error("보고서 링크 복사 값");
      const bounds = await panel.evaluate((p) => {
        const box=p.getBoundingClientRect();
        return { right:box.right, width:innerWidth, overflow:p.scrollWidth>p.clientWidth+1 };
      });
      if (bounds.right > bounds.width + 1 || bounds.overflow) throw new Error(`보고서 목록 폭 넘침 ${JSON.stringify(bounds)}`);
      await shot(page, `reports-${width}`);
    }
    // 폐기한 링크는 401이어도 pairing한 앱의 「열기」(본문)·「새 창」은 계속 열려야 한다.
    await row.getByRole("button", { name: "링크 폐기" }).click();
    await domIs(page, "보고서 목록 폐기 반영", () => document.querySelector('[data-testid="report-demo-page"] [data-testid="library-link"]')?.dataset.state === "revoked");
    if ((await fetch(new URL(link.base, base))).status !== 401) throw new Error("폐기 뒤 옛 링크가 열린다");
    await row.getByRole("button", { name: "열기", exact: true }).click();
    await page.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "최신 보고서" }).waitFor();
    if ((await page.evaluate(() => location.hash)) !== "#report=demo%2Fpage") throw new Error(`연 보고서 주소: ${await page.evaluate(() => location.hash)}`);
    await page.getByTestId("report-back").click();
    await panel.waitFor();
    const opened = page.waitForEvent("popup");
    await row.getByRole("button", { name: "새 창", exact: true }).click();
    const report = await opened;
    try { await report.getByRole("heading", { name: "최신 보고서" }).waitFor(); }
    finally { await report.close(); }
    await row.getByRole("button", { name: "새 링크 만들기" }).click();
    await row.getByRole("link", { name: "보고서 공유 링크" }).waitFor();
    const newUrl = await row.getByRole("link", { name: "보고서 공유 링크" }).getAttribute("href");
    if (newUrl === new URL(link.base, base).href) throw new Error("폐기한 링크 id를 재사용했다");
    await page.getByTestId("report-demo-design").waitFor();
    aaa("library", "rotate");
    if ((await fetch(newUrl)).status !== 401 || (await fetch(new URL(live.base, base))).status !== 401) throw new Error("rotate가 옛 링크를 끝내지 않았다");
    // 같은 기기 cookie로 API가 계속 된다.
    const me = await page.evaluate(async () => (await fetch("/api/v1/me")).status);
    if (me !== 200) throw new Error("rotate가 pairing을 바꿨다");
    const readerCode = aaa("web", "pair", "--reader").code;
    const reader = await pairPage(browser, readerCode, { width: 390, height: 844 });
    try {
      await inList(reader.page, "activity-reports");
      await reader.page.getByTestId("report-demo-design").waitFor();
      if (await reader.page.getByTestId("library-link").count() !== 0) throw new Error("보기 기기에 공유 링크 조작을 보였다");
      await reader.page.getByTestId("report-demo-design").getByRole("button", { name: "새 창", exact: true }).waitFor();
      if (await reader.page.getByTestId("report-delete").count() !== 0) throw new Error("보기 기기에 지우기를 보였다");
      await reader.page.getByTestId("reports-tab-work").click();
      await reader.page.getByTestId("report-demo-extra").waitFor();
      if (await reader.page.getByTestId("reports-delete").count() !== 0 || await reader.page.locator(".report-pick").count() !== 0) throw new Error("보기 기기에 작업 보고 고르기·지우기를 보였다");
    } finally { await reader.page.context().close(); }
    const prefixed=aaa("library","import",batch,"--project","demo","--prefix","archive-");
    if (!prefixed.reports.some((r) => r.name === "archive-t-0001") || prefixed.skipped.length !== 0) throw new Error("접두사를 붙인 이전 보고서 import");
  });
  await step("사용자 보고서(plan §8.256): index.html이 없으면 거부, 막대·탭 안 봄 배지와 알림 post, 열면 봄, 다시 보내면 다시 안 봄, 카드 지우기는 확인 한 번", async () => {
    const unseen = () => aaa("library", "list").reports.filter((r) => r.unseen).length;
    const badges = () => page.evaluate(() => [document.querySelector('[data-testid="activity-badge-reports"]')?.textContent ?? null, document.querySelector('[data-testid="reports-unseen"]')?.textContent ?? null]);
    const badgesAre = (n) => until(`안 봄 배지 ${n}`, async () => { const b = await badges(); return b[0] === (n ? String(n) : null) && b[1] === b[0]; });
    const plain = `${root}/plain`;
    fs.mkdirSync(plain);
    fs.writeFileSync(`${plain}/notes.md`, "웹 보고서가 아니다");
    let refused = "";
    try { aaa("library", "publish", plain, "--project", "demo", "--name", "plain", "--to-user"); } catch (e) { refused = e.message; }
    if (!refused.includes("index.html") || aaa("library", "list").reports.some((r) => r.name === "plain")) throw new Error(`index.html 없는 사용자 보고서: ${refused}`);
    await page.setViewportSize({ width: 1280, height: 800 });
    // 프로젝트 고르기는 없다(host는 프로젝트 하나, plan §8.291)
    if (await page.getByTestId("reports-panel").getByRole("combobox", { name: "보고서 프로젝트" }).count()) throw new Error("보고서 화면에 프로젝트 고르기가 남았다");
    await page.getByTestId("reports-tab-mine").click();
    // 앞 단계가 demo/page는 열었고(봄) demo/design은 보기 기기가 카드만 기다렸다(안 봄): 지금 1이다
    const before = unseen();
    await badgesAre(before);
    const weekly = `${root}/weekly`;
    fs.mkdirSync(weekly);
    fs.writeFileSync(`${weekly}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>주간 보고</h1></html>");
    const weeklySent = aaa("library", "publish", weekly, "--project", "demo", "--name", "weekly", "--to-user", "--title", "주간 보고");
    // 다시 읽기 단추를 누르지 않는다: host `reports` 사건이 막대·탭 배지와 목록을 고친다
    await badgesAre(before + 1);
    const card = page.getByTestId("report-demo-weekly");
    await domIs(page, "새 카드 안 봄", () => document.querySelector('[data-testid="report-demo-weekly"]')?.dataset.unseen === "true");
    if (!(await card.locator(".report-heading strong").innerText()).includes("주간 보고")) throw new Error(`제목: ${await card.innerText()}`);
    const label = await page.getByTestId("activity-reports").getAttribute("aria-label");
    if (label !== `보고서, 안 본 보고서 ${before + 1}`) throw new Error(`막대 단추 이름: ${label}`);
    const posts = aaa("post", "list", "demo").posts.filter((p) => p.topic === "report");
    if (!posts.some((p) => p.title === `${weeklySent.report_id} 새 보고서: 주간 보고` && p.to === "user" && p.refs.includes(`library/demo/${weeklySent.report_id}`))) throw new Error(`알림 post: ${JSON.stringify(posts)}`);
    await listIds(page, '[data-testid="report-demo-weekly"]', [weeklySent.report_id], "ids-report-desktop");
    await shot(page, "reports-mine-unseen");
    await card.getByRole("button", { name: "열기", exact: true }).click();
    await page.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "주간 보고" }).waitFor();
    await page.getByTestId("report-back").click();
    await badgesAre(before);
    await domIs(page, "연 카드는 봄", () => document.querySelector('[data-testid="report-demo-weekly"]')?.dataset.unseen === "false");
    aaa("library", "publish", weekly, "--project", "demo", "--name", "weekly", "--to-user", "--title", "주간 보고");
    await badgesAre(before + 1);
    await domIs(page, "다시 보내면 안 봄", () => document.querySelector('[data-testid="report-demo-weekly"]')?.dataset.unseen === "true");
    // 카드 지우기: 취소하면 남고, 확인 한 번이면 host 사본·링크·목록에서 지운다
    const shared = aaa("library", "link", "demo", "weekly");
    if (!fs.existsSync(`${root}/projects/demo/library/weekly/index.html`)) throw new Error("host 사본 자리가 다르다");
    await card.getByTestId("report-delete").click();
    const dialog = page.getByRole("alertdialog");
    await dialog.getByRole("button", { name: "취소" }).click();
    await dialog.waitFor({ state: "detached" });
    if (!aaa("library", "list").reports.some((r) => r.name === "weekly")) throw new Error("취소했는데 지웠다");
    await card.getByTestId("report-delete").click();
    if (!(await dialog.innerText()).includes("demo/weekly")) throw new Error(`확인 창: ${await dialog.innerText()}`);
    await shot(page, "reports-delete-confirm");
    await dialog.getByRole("button", { name: "영구히 지우기" }).click();
    await card.waitFor({ state: "detached" });
    await badgesAre(before);
    if (aaa("library", "list").reports.some((r) => r.name === "weekly")) throw new Error("지운 보고서가 목록에 있다");
    if ((await fetch(new URL(shared.base, base))).status === 200) throw new Error("지운 보고서의 공유 링크가 열린다");
    if (fs.existsSync(`${root}/projects/demo/library/weekly`)) throw new Error("host 사본이 남았다");
  });

  await step("보고서 링크 열람: 제목·공유 링크를 누르거나 인증 없는 Chromium·WebKit 탭에서 주소를 처음 열면 안 봄 배지와 마지막 열람이 바뀐다", async () => {
    const input = `${root}/link-report`;
    fs.mkdirSync(input);
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>링크로 읽기</h1></html>");
    const before = aaa("library", "list").reports.filter((r) => r.unseen).length;
    const publish = () => aaa("library", "publish", input, "--project", "demo", "--name", "link-report", "--to-user", "--title", "링크로 읽기");
    const badgesAre = (n) => domIs(page, `링크 열람 배지 ${n}`, (n) => {
      const want = n ? String(n) : null;
      return (document.querySelector('[data-testid="activity-badge-reports"]')?.textContent ?? null) === want
        && (document.querySelector('[data-testid="reports-unseen"]')?.textContent ?? null) === want;
    }, n);
    await page.getByTestId("reports-tab-mine").click();
    const card = page.getByTestId("report-demo-link-report");
    publish();
    await badgesAre(before + 1);
    await card.getByRole("link", { name: "링크로 읽기", exact: true }).click();
    await page.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "링크로 읽기" }).waitFor();
    await page.getByTestId("report-back").click();
    await badgesAre(before);
    publish();
    await badgesAre(before + 1);
    const popup = page.waitForEvent("popup");
    await card.getByRole("link", { name: "보고서 공유 링크" }).click();
    const report = await popup;
    try {
      await report.getByRole("heading", { name: "링크로 읽기" }).waitFor();
      await badgesAre(before);
    } finally { await report.close(); }
    const safari = await webkit.launch();
    try {
      for (const engine of [browser, safari]) {
        const link = publish();
        await badgesAre(before + 1);
        // cookie도 앱 JS도 없는 새 기기. 최초 문서 GET을 host가 기록해야 한다.
        const outside = await engine.newContext();
        try {
          const tab = await outside.newPage();
          await tab.goto(new URL(link.base, base).href);
          await tab.getByRole("heading", { name: "링크로 읽기" }).waitFor();
          await badgesAre(before);
          const saved = aaa("library", "list").reports.find((r) => r.name === "link-report");
          if (saved.unseen || !saved.link.last_opened_at_ms) throw new Error(`외부 탭의 열람 누락: ${JSON.stringify(saved)}`);
        } finally { await outside.close(); }
      }
    } finally { await safari.close(); }
  });

  await step("작업 보고(plan §8.256): 진행 중 thread의 사본은 고를 수 없고 host도 거부, resolve하면 내 보고서에 자동으로 올린다, 끝난 것 모두 고르기·확인 한 번으로 모두 지운다", async () => {
    // 진행 중 보고의 작업 탭 보호를 확인한 뒤 resolve 자동 표시를 확인한다.
    fs.writeFileSync(`${root}/report-fixture.md`, fs.readFileSync(`${root}/report-fixture.md`, "utf8").replace("[완료] 맡긴 일과 확인 끝", "[진행 중] 작업 확인 중"));
    fs.writeFileSync(`${root}/task-reports.md`, "웹 보기");
    const t = aaa("thread", "start", "demo", "--title", "보고 정리", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-reports.md`);
    await until("thread 사본", () => aaa("library", "list", "--project", "demo").reports.some((r) => r.name === t.id));
    await page.getByTestId("reports-tab-work").click();
    const row = page.getByTestId(`report-demo-${t.id}`);
    await domIs(page, "진행 중 사본", (id) => document.querySelector(`[data-testid="report-demo-${id}"]`)?.dataset.busy === "true", t.id);
    if (!(await row.locator(".report-pick").isDisabled()) || !(await row.innerText()).includes("진행 중")) throw new Error("진행 중 사본을 고를 수 있다");
    let refused = "";
    try { aaa("library", "delete", "--project", "demo", t.id, "extra"); } catch (e) { refused = e.message; }
    if (!refused.includes("끝나지 않은") || !aaa("library", "list").reports.some((r) => r.name === "extra")) throw new Error(`진행 중 사본 지우기: ${refused}`);
    await page.getByTestId("reports-pick-done").click();
    await domIs(page, "끝난 것만 고름", (id) => {
      const rows = [...document.querySelectorAll('[data-testid^="report-"][data-busy]')];
      return rows.length > 1 && rows.every((r) => r.querySelector(".report-pick").checked === (r.dataset.busy === "false")) && !document.querySelector(`[data-testid="report-demo-${id}"] .report-pick`).checked;
    }, t.id);
    await shot(page, "reports-work-picked");
    // 휴대폰 폭에서도 줄(고르기·이름·진행 중·크기·시각·단추 셋)이 패널을 넘지 않는다
    await page.setViewportSize({ width: 390, height: 844 });
    const narrow = await page.getByTestId("reports-panel").evaluate((p) => ({ right: p.getBoundingClientRect().right, width: innerWidth, overflow: p.scrollWidth > p.clientWidth + 1 }));
    if (narrow.right > narrow.width + 1 || narrow.overflow) throw new Error(`작업 보고 폭 넘침 ${JSON.stringify(narrow)}`);
    await shot(page, "reports-work-390");
    await page.setViewportSize({ width: 1280, height: 800 });
    aaa("thread", "resolve", "demo", t.id, "--skip-copy");
    await page.getByTestId("reports-tab-mine").click();
    const completed = page.getByTestId(`report-demo-${t.id}`);
    await completed.waitFor();
    if ((await completed.getAttribute("data-unseen")) !== "true") throw new Error("resolve한 마지막 보고가 내 보고서의 안 봄으로 올라오지 않았다");
    if (!fs.existsSync(`${root}/projects/demo/library/${t.id}`) || !fs.existsSync(`${root}/projects/demo/threads/${t.id}.files.json`)) throw new Error("thread 사본 자리가 다르다");
    await completed.getByTestId("report-delete").click();
    await page.getByRole("alertdialog").getByRole("button", { name: "영구히 지우기" }).click();
    await page.getByRole("alertdialog").waitFor({ state: "detached" });
    await page.getByTestId("reports-tab-work").click();
    const picked = await page.locator(".report-pick:checked").count();
    const names = aaa("library", "list").reports.filter((r) => !r.to_user).map((r) => `${r.project}/${r.name}`);
    if (picked !== names.length) throw new Error(`고른 수 ${picked}, 작업 보고 ${names.length}`);
    await page.getByTestId("reports-delete").click();
    const dialog = page.getByRole("alertdialog");
    if (!(await dialog.getByRole("heading").innerText()).includes(`${picked}개`)) throw new Error(`확인 창: ${await dialog.innerText()}`);
    await dialog.getByRole("button", { name: "영구히 지우기" }).click();
    await dialog.waitFor({ state: "detached" });
    await domIs(page, "작업 보고 비움", () => document.querySelectorAll('[data-testid^="report-"][data-busy]').length === 0);
    const left = aaa("library", "list").reports;
    if (left.some((r) => !r.to_user)) throw new Error(`작업 보고가 남았다: ${JSON.stringify(left.map((r) => r.name))}`);
    if (fs.existsSync(`${root}/projects/demo/library/${t.id}`) || fs.existsSync(`${root}/projects/demo/threads/${t.id}.files.json`)) throw new Error("thread 사본이 남았다");
    if (!left.some((r) => r.name === "page")) throw new Error("내 보고서까지 지웠다");
  });
  await step("보고서 보관함: 특정 판 고정·별도 용량·한 번에 넣기와 빼기·Reader 제한·명시 삭제", async () => {
    const source = `${root}/archive-source`;
    fs.mkdirSync(source);
    fs.writeFileSync(`${source}/index.html`, "<!doctype html><meta charset=utf-8><h1>보관한 첫 판</h1>");
    const first = aaa("library", "publish", source, "--name", "archive-demo", "--to-user").report_id;
    await page.getByTestId("reports-tab-mine").click();
    const row = page.getByTestId("report-demo-archive-demo");
    await row.waitFor();
    await row.getByTestId("report-archive").click();
    await until("첫 판 보관", () => aaa("library", "list").reports.some((r) => r.name === first && r.archived));
    fs.writeFileSync(`${source}/index.html`, "<!doctype html><meta charset=utf-8><h1>나중의 둘째 판</h1>");
    const second = aaa("library", "publish", source, "--name", "archive-demo", "--to-user").report_id;
    await until("보관 판은 하나", () => aaa("library", "list").reports.filter((r) => r.archived).length === 1);
    await row.getByRole("combobox", { name: "demo/archive-demo 보관할 판" }).waitFor();
    const safari = await webkit.launch({ headless: true });
    try {
      for (const engine of [browser, safari]) {
        for (const width of [1280, 390]) {
          const dev = await pairPage(engine, aaa("web", "pair").code, { width, height: width === 390 ? 844 : 800 });
          try {
            await dev.page.setViewportSize({ width, height: width === 390 ? 844 : 800 });
            await inList(dev.page, "activity-reports");
            await dev.page.getByTestId("reports-tab-archive").click();
            const saved = dev.page.getByTestId(`report-demo-${first}`);
            await saved.waitFor();
            if (await dev.page.getByTestId(`report-demo-${second}`).count()) throw new Error("후속 판을 자동 보관했다");
            const usage = await dev.page.getByTestId("reports-usage").innerText();
            if (!usage.includes("보관함") || !usage.includes("상한 제외")) throw new Error(`보관함 별도 용량: ${usage}`);
            const v = aaa("library", "show", first);
            if (!(await (await fetch(new URL(v.base, base))).text()).includes("보관한 첫 판")) throw new Error("보관 판이 최신 본문으로 바뀌었다");
            const bounds = await dev.page.getByTestId("reports-panel").evaluate((p) => ({ width: innerWidth, right: p.getBoundingClientRect().right, overflow: p.scrollWidth > p.clientWidth + 1 }));
            if (bounds.right > bounds.width + 1 || bounds.overflow) throw new Error(`보관함 폭 넘침 ${JSON.stringify(bounds)}`);
            await shot(dev.page, `reports-archive-${engine === browser ? "chromium" : "webkit"}-${width}`);
          } finally { await dev.page.context().close(); }
        }
      }
      const reader = await pairPage(safari, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
      try {
        await inList(reader.page, "activity-reports");
        await reader.page.getByTestId("reports-tab-archive").click();
        await reader.page.getByTestId(`report-demo-${first}`).waitFor();
        if (await reader.page.getByTestId("report-archive").count() || await reader.page.getByTestId("report-delete").count()) throw new Error("Reader에게 보관 변경을 보였다");
      } finally { await reader.page.context().close(); }
    } finally { await safari.close(); }
    await page.getByTestId("reports-tab-archive").click();
    await page.getByTestId(`report-demo-${first}`).getByTestId("report-archive").click();
    await until("보관함에서 빼기", () => !aaa("library", "list").reports.some((r) => r.archived));
    await page.getByTestId("reports-tab-mine").click();
    await row.getByRole("combobox").selectOption(first);
    await row.getByTestId("report-archive").click();
    await page.getByTestId("reports-tab-archive").click();
    const saved = page.getByTestId(`report-demo-${first}`);
    await saved.waitFor();
    await saved.getByTestId("report-delete").click();
    await page.getByRole("alertdialog").getByRole("button", { name: "영구히 지우기" }).click();
    await page.getByRole("alertdialog").waitFor({ state: "detached" });
    await until("보관 보고 명시 삭제", () => !aaa("library", "list").reports.some((r) => ["archive-demo", first, second].includes(r.name)));
  });

  await step("완료 보고 기본 동작: 자동 내 보고서·안 봄·사본 추가 없음·재완료와 resolve 멱등", async () => {
    fs.writeFileSync(`${root}/report-fixture.md`, "[완료] 자동으로 올릴 결과\n\n## Report\n완료 본문 하나\n");
    fs.writeFileSync(`${root}/automatic-task.md`, "완료 자동 표시");
    const before = new Set(fs.readdirSync(`${root}/projects/demo/library`));
    const t = aaa("thread", "start", "demo", "--title", "자동 완료 결과", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/automatic-task.md`);
    await page.getByTestId("reports-tab-mine").click();
    const card = page.getByTestId(`report-demo-${t.id}`);
    await domIs(page, "완료 보고 자동 안 봄", (id) => document.querySelector(`[data-testid="report-demo-${id}"]`)?.dataset.unseen === "true", t.id);
    const row = aaa("library", "list").reports.find((r) => r.name === t.id);
    const added = fs.readdirSync(`${root}/projects/demo/library`).filter((n) => !before.has(n));
    if (added.length !== 2 || !added.includes(t.id) || !added.includes(row.report_id)) throw new Error(`자동 올리기가 별도 사본을 만들었다: ${JSON.stringify(added)}`);
    if (!await card.getByTestId("report-delete").isDisabled()) throw new Error("진행 중 thread를 자동 완료 카드에서 지울 수 있다");
    await card.getByTestId("report-open").click();
    await page.getByTestId("report-back").click();
    await until("완료 보고 봄", () => !aaa("library", "list").reports.find((r) => r.name === t.id).unseen);
    const previous = aaa("thread", "show", "demo", t.id).report_hash;
    const work = aaa("thread", "show", "demo", t.id).work_dir;
    fs.writeFileSync(`${work}/report.md`, "[완료] 갱신한 결과\n\n## Report\n완료 본문 둘\n");
    await until("새 완료 판", () => aaa("thread", "show", "demo", t.id).report_hash !== previous);
    await domIs(page, "새 완료 보고 다시 안 봄", (id) => document.querySelector(`[data-testid="report-demo-${id}"]`)?.dataset.unseen === "true", t.id);
    await card.getByTestId("report-open").click();
    await page.getByTestId("report-back").click();
    aaa("thread", "resolve", "demo", t.id, "--skip-copy");
    if (aaa("library", "list").reports.find((r) => r.name === t.id).unseen) throw new Error("같은 본문의 resolve가 다시 안 봄으로 바꿨다");
  });

}
