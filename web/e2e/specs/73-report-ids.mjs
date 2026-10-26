// 실제 publish·thread 새 보고 판의 r-ID, 옛 링크·파일, CLI·목록·웹의 같은 참조를 검사한다.
import fs from "node:fs";
import { createHash } from "node:crypto";
import { root, aaa, until, step, domIs, shot, pairPage, browser, webkit } from "../lib/harness.mjs";
const call = (page, req) => page.evaluate(async (req) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(req) })).json(), req);
const open = async (page, id) => {
  await page.evaluate((id) => { location.hash = `report=${id}`; }, id);
  await domIs(page, "특정 보고 판 준비", (id) => document.querySelector('[data-testid="report-detail"]')?.dataset.status === "ready" && document.querySelector('[data-testid="report-id"]')?.textContent === id, id);
};
export default async function ({ page }) {
  let first, second, thread, threadInitial, threadFirst, threadSecond;
  await step("같은 이름 웹 보고서를 재발행하면 새 r 번호, 기존 이름은 최신 판·r 번호는 원래 그림까지 보존", async () => {
    aaa("project", "create", "demo");
    const input = `${root}/version-report`; fs.mkdirSync(input);
    const publish = (text) => {
      fs.writeFileSync(`${input}/index.html`, `<!doctype html><meta charset=utf-8><h1>${text}</h1><img src=preview.svg>`);
      fs.writeFileSync(`${input}/preview.svg`, `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><text y="40">${text}</text></svg>`);
      return aaa("library", "publish", input, "--name", "version-report", "--to-user").report_id;
    };
    first = publish("첫 시안"); second = publish("둘째 시안");
    if (first !== "r-0001" || second !== "r-0002") throw new Error(`보고 번호 ${first}, ${second}`);
    const list = aaa("library", "list").reports;
    if (list.length !== 1 || list[0].report_id !== second || JSON.stringify(list[0].versions) !== JSON.stringify([second, first])) throw new Error(JSON.stringify(list));
    if (!aaa("library", "show", first).files.some((f) => f.path === "preview.svg")) throw new Error("r 번호 CLI 열기");
    if (aaa("library", "link", first).report_id !== first) throw new Error("r 번호 공유 링크");
    await open(page, first);
    await page.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "첫 시안" }).waitFor();
    const lib = (await call(page, { op: "library_open", project: "demo", name: first })).ok;
    const svg = await page.evaluate(async (base) => (await fetch(base + "preview.svg")).text(), lib.base);
    if (!svg.includes("첫 시안") || svg.includes("둘째 시안")) throw new Error("이전 판 그림이 최신 그림으로 바뀌었다");
    await page.getByLabel("보고 판", { exact: true }).selectOption(second);
    await page.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "둘째 시안" }).waitFor();
    await page.evaluate(() => { location.hash = "report=version-report"; });
    await domIs(page, "기존 이름은 최신 판", (id) => document.querySelector('[data-testid="report-id"]')?.textContent === id, second);
  });
  await step("새 thread 보고마다 r 판과 inbox 참조: old r는 옛 원문·그림, t 번호는 최신", async () => {
    fs.writeFileSync(`${root}/task-version.md`, "보고 판 보존 시험\n");
    thread = aaa("thread", "start", "demo", "--title", "보고 판", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-version.md`);
    threadInitial = await until("첫 회수", () => aaa("thread", "show", thread.id).report_id);
    fs.unlinkSync(`${thread.work_dir}/library/index.html`); // 이 판은 HTML 대신 보고 Markdown을 표시한다.
    const write = (text) => {
      fs.writeFileSync(`${thread.work_dir}/library/revision.svg`, `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><text y="40">${text}</text></svg>`);
      const report = `[착지] ${text}\n\n## Report\n${text}\n\n![시안](library/revision.svg)\n`;
      fs.writeFileSync(`${thread.work_dir}/report.md`, report);
      return createHash("sha256").update(report).digest("hex");
    };
    const firstHash = write("thread 첫 판");
    threadFirst = await until("첫 판 새 번호", () => { const t = aaa("thread", "show", thread.id); return t.report_hash === firstHash && t.report_id; });
    const secondHash = write("thread 둘째 판");
    threadSecond = await until("둘째 판 새 번호", () => { const t = aaa("thread", "show", thread.id); return t.report_hash === secondHash && t.report_id !== threadFirst && t.report_id; });
    const old = aaa("thread", "show", threadFirst);
    if (old.id !== thread.id || old.report_id !== threadFirst || !aaa("thread", "library", threadFirst).report.includes("thread 첫 판")) throw new Error("thread 명령의 r 참조");
    await until("inbox 특정 보고 번호", () => aaa("inbox", "list", "--peek").items.some((i) => i.event === "new report" && i.summary.includes(threadSecond)));
    await open(page, threadFirst);
    await page.getByTestId("report-detail").getByText("thread 첫 판", { exact: true }).waitFor();
    const img = page.getByTestId("report-detail").locator('img[alt="시안"]');
    await img.evaluate((img) => img.decode());
    const original = await img.getAttribute("src");
    if (!(await page.evaluate(async (src) => (await fetch(src)).text(), original)).includes("thread 첫 판")) throw new Error("thread 옛 그림 보존");
    await open(page, threadSecond);
    await page.getByTestId("report-detail").getByText("thread 둘째 판", { exact: true }).waitFor();
  });
  // 실제 thread 회수의 누락이 HTML/Markdown 보존 판에서도 화면에 드러나야 한다.
  const safari = await webkit.launch({ headless: true });
  try {
    for (const [engine, name] of [[browser, "Chromium"], [safari, "WebKit"]]) {
      for (const width of [1280, 390]) {
        await step(`보고 사본 누락 안내: ${name} ${width}px HTML·Markdown 판`, async () => {
          const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 });
          try {
            for (const id of [threadInitial, threadFirst]) {
              await open(paired.page, id);
              const notice = paired.page.getByTestId("library-copy-missing");
              await notice.locator("summary").click();
              await notice.getByText("big.bin", { exact: true }).waitFor();
              if (id === threadInitial) await paired.page.getByTestId("report-frame").waitFor();
              else await paired.page.getByTestId("report-detail").getByText("thread 첫 판", { exact: true }).waitFor();
              await domIs(paired.page, "누락 안내 가로 넘침 없음", () => document.documentElement.scrollWidth <= innerWidth);
            }
            if (paired.errors.length) throw new Error(paired.errors.join("; "));
          } finally { await paired.page.context().close(); }
        });
      }
    }
  } finally { await safari.close(); }
  await step("데스크톱·휴대폰 r 번호와 판 선택, Reader 열람과 쓰기 거부", async () => {
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 800 });
      for (const theme of ["light", "dark"]) {
        await page.evaluate((theme) => { localStorage.setItem("aaa:appearance", JSON.stringify({ theme, palette: "amber" })); }, theme);
        await page.reload(); await open(page, first);
        await page.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "첫 시안" }).waitFor();
        await domIs(page, "보고 머리 가로 넘침 없음", () => document.documentElement.scrollWidth <= innerWidth);
        await shot(page, `73-report-${width}-${theme}`);
      }
    }
    const reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, { hash: `#report=${first}` });
    await reader.page.getByTestId("report-id").waitFor();
    if ((await call(reader.page, { op: "library_delete", reports: [{ project: "demo", name: first }] })).err?.code !== "forbidden") throw new Error("Reader가 판을 지웠다");
    if (reader.errors.length) throw new Error(reader.errors.join("; "));
    await reader.page.context().close();
    aaa("thread", "resolve", thread.id);
  });
}
