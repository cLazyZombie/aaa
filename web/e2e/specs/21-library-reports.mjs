// 보고서 전체 목록은 CLI로 올린 host 사본·등록 폴더와 같은 정본을 본다. 기존 실험실의 자식 정리만 쓴다.
import fs from "node:fs";
import { root, aaa, base, browser, step, pairPage, inList, domIs, until, shot } from "../lib/harness.mjs";

export default async function ({ page }) {
  await step("보고서 전체 목록: publish·실시간·import, 활동 막대 「보고서」가 연 본문 화면(plan §8.240), 데스크톱·휴대폰의 열기·새 창·복사·폐기·새 링크", async () => {
    aaa("project", "create", "demo");
    aaa("project", "create", "other");
    const input = `${root}/reports`;
    fs.mkdirSync(input);
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>첫 보고서</h1></html>");
    // 사용자 보고서(--to-user, plan §8.256)는 「내 보고서」 탭의 카드다. import한 것은 「작업 보고」 탭이다
    const link = aaa("library", "publish", input, "--project", "demo", "--name", "page", "--to-user");
    const live = aaa("library", "publish", input, "--project", "other", "--name", "design", "--live", "--to-user");
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
    if (list.length !== 3 || !list.some((r) => r.project === "other" && r.name === "design" && r.live)) throw new Error("전체 보고서 CLI 목록");
    if (aaa("library", "prune", "--older-than", "30").removed.length !== 0) throw new Error("최근 보고서를 정리했다");
    const doctor = aaa("doctor").library;
    if (doctor?.reports !== 3 || doctor?.live !== 1 || doctor?.size !== Buffer.byteLength(fs.readFileSync(`${input}/index.html`)) + Buffer.byteLength("추가 보고서")) throw new Error("doctor의 host 사본 크기·실시간 수");
    if (doctor.max_days !== 30 || doctor.max_size !== 20 * 1024 ** 3 || doctor.protected_size !== 0) throw new Error("doctor의 보관 한도·보호 용량");
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
    if (!(await page.getByTestId("report-other-design").innerText()).includes("실시간")) throw new Error("실시간 표시");
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
    await panel.getByRole("combobox", { name: "보고서 프로젝트" }).selectOption("other");
    await row.waitFor({ state: "detached" });
    await page.getByTestId("report-other-design").waitFor();
    aaa("library", "rotate");
    if ((await fetch(newUrl)).status !== 401 || (await fetch(new URL(live.base, base))).status !== 401) throw new Error("rotate가 옛 링크를 끝내지 않았다");
    // 같은 기기 cookie로 API가 계속 된다.
    const me = await page.evaluate(async () => (await fetch("/api/v1/me")).status);
    if (me !== 200) throw new Error("rotate가 pairing을 바꿨다");
    const readerCode = aaa("web", "pair", "--reader").code;
    const reader = await pairPage(browser, readerCode, { width: 390, height: 844 });
    try {
      await inList(reader.page, "activity-reports");
      await reader.page.getByTestId("report-other-design").waitFor();
      if (await reader.page.getByTestId("library-link").count() !== 0) throw new Error("보기 기기에 공유 링크 조작을 보였다");
      await reader.page.getByTestId("report-other-design").getByRole("button", { name: "새 창", exact: true }).waitFor();
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
    await page.getByTestId("reports-panel").getByRole("combobox", { name: "보고서 프로젝트" }).selectOption("");
    await page.getByTestId("reports-tab-mine").click();
    // 앞 단계가 demo/page는 열었고(봄) other/design은 보기 기기가 카드만 기다렸다(안 봄): 지금 1이다
    const before = unseen();
    await badgesAre(before);
    const weekly = `${root}/weekly`;
    fs.mkdirSync(weekly);
    fs.writeFileSync(`${weekly}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>주간 보고</h1></html>");
    aaa("library", "publish", weekly, "--project", "demo", "--name", "weekly", "--to-user", "--title", "주간 보고");
    // 다시 읽기 단추를 누르지 않는다: host `reports` 사건이 막대·탭 배지와 목록을 고친다
    await badgesAre(before + 1);
    const card = page.getByTestId("report-demo-weekly");
    await domIs(page, "새 카드 안 봄", () => document.querySelector('[data-testid="report-demo-weekly"]')?.dataset.unseen === "true");
    if (!(await card.locator(".report-heading strong").innerText()).includes("주간 보고")) throw new Error(`제목: ${await card.innerText()}`);
    const label = await page.getByTestId("activity-reports").getAttribute("aria-label");
    if (label !== `보고서, 안 본 보고서 ${before + 1}`) throw new Error(`막대 단추 이름: ${label}`);
    const posts = aaa("post", "list", "demo").posts.filter((p) => p.topic === "report");
    if (!posts.some((p) => p.title === "새 보고서: 주간 보고" && p.to === "user" && p.refs.includes("library/demo/weekly"))) throw new Error(`알림 post: ${JSON.stringify(posts)}`);
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

  await step("작업 보고(plan §8.256): 진행 중 thread의 사본은 고를 수 없고 host도 거부, resolve하면 풀린다, 끝난 것 모두 고르기·확인 한 번으로 모두 지운다", async () => {
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
    await domIs(page, "resolve하면 풀림", (id) => document.querySelector(`[data-testid="report-demo-${id}"]`)?.dataset.busy === "false", t.id);
    await row.locator(".report-pick").check();
    const picked = await page.locator(".report-pick:checked").count();
    const names = aaa("library", "list").reports.filter((r) => !r.to_user).map((r) => `${r.project}/${r.name}`);
    if (picked !== names.length) throw new Error(`고른 수 ${picked}, 작업 보고 ${names.length}`);
    if (!fs.existsSync(`${root}/projects/demo/library/${t.id}`) || !fs.existsSync(`${root}/projects/demo/threads/${t.id}.files.json`)) throw new Error("thread 사본 자리가 다르다");
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
}
