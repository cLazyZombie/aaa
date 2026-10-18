// 보고서 전체 목록은 CLI로 올린 host 사본·등록 폴더와 같은 정본을 본다. 기존 실험실의 자식 정리만 쓴다.
import fs from "node:fs";
import { root, aaa, base, browser, step, pairPage, inList, domIs, shot } from "../lib/harness.mjs";

export default async function ({ page }) {
  await step("보고서 전체 목록: publish·실시간·import, 데스크톱·휴대폰의 열기·복사·폐기·새 링크", async () => {
    aaa("project", "create", "demo");
    aaa("project", "create", "other");
    const input = `${root}/reports`;
    fs.mkdirSync(input);
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>첫 보고서</h1></html>");
    const link = aaa("library", "publish", input, "--project", "demo", "--name", "page");
    const live = aaa("library", "publish", input, "--project", "other", "--name", "design", "--live");
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
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>최신 보고서</h1></html>");
    aaa("library", "publish", input, "--project", "demo", "--name", "page");
    if (!(await (await fetch(new URL(live.base, base))).text()).includes("최신 보고서")) throw new Error("등록 폴더 변경을 반영하지 않았다");
    await inList(page, "reports-open");
    const panel = page.getByTestId("reports-panel");
    await panel.waitFor();
    await domIs(page, "보고서 전체 목록 준비", () => document.querySelector('[data-testid="reports-panel"]')?.dataset.status === "ready");
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
    // 폐기한 링크는 401이어도 pairing한 앱의 「앱에서 열기」는 계속 열려야 한다.
    await row.getByRole("button", { name: "링크 폐기" }).click();
    await domIs(page, "보고서 목록 폐기 반영", () => document.querySelector('[data-testid="report-demo-page"] [data-testid="library-link"]')?.dataset.state === "revoked");
    if ((await fetch(new URL(link.base, base))).status !== 401) throw new Error("폐기 뒤 옛 링크가 열린다");
    const opened = page.waitForEvent("popup");
    await row.getByRole("button", { name: "앱에서 열기" }).click();
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
      await inList(reader.page, "reports-open");
      await reader.page.getByTestId("report-other-design").waitFor();
      if (await reader.page.getByTestId("library-link").count() !== 0) throw new Error("보기 기기에 공유 링크 조작을 보였다");
      await reader.page.getByTestId("report-other-design").getByRole("button", { name: "앱에서 열기" }).waitFor();
    } finally { await reader.page.context().close(); }
    const prefixed=aaa("library","import",batch,"--project","demo","--prefix","archive-");
    if (!prefixed.reports.some((r) => r.name === "archive-t-0001") || prefixed.skipped.length !== 0) throw new Error("접두사를 붙인 이전 보고서 import");
  });
}
