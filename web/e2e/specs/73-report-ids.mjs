// 실제 publish·thread 새 보고 판의 r-ID, 옛 링크·파일, CLI·목록·웹의 같은 참조를 검사한다.
import fs from "node:fs";
import { createHash } from "node:crypto";
import { root, aaa, until, step, domIs, shot, pairPage, browser, webkit } from "../lib/harness.mjs";
const call = (page, req) => page.evaluate(async (req) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(req) })).json(), req);
const open = async (page, id) => {
  await page.evaluate((id) => { location.hash = `report=${id}`; }, id);
  await domIs(page, "특정 보고 판 준비", (id) => document.querySelector('[data-testid="report-detail"]')?.dataset.status === "ready" && document.querySelector('[data-testid="report-id"]')?.textContent === id, id);
};

// 실제 파일 GET만 사건으로 보류하고 HEAD 200→401, 새 cap의 실제 decode를 확인한다.
async function checkMediaDiagnosis(engine, width, id) {
  const owned = engine === "webkit" ? await webkit.launch() : null;
  // WebKit의 SW 경유 요청은 page.route에 오지 않는다. 응답을 제어하는 이 fixture만 SW를 막는다.
  const paired = await pairPage(owned ?? browser, aaa("web", "pair").code, { width, height: 844 }, { hash: "#reports", serviceWorkers: "block" });
  const p = paired.page;
  let release;
  const gate = new Promise(r => { release = r; });
  let blocked = true, failed = false;
  const heads = [], opens = [], replies = [];
  try {
    await p.route("**/api/v1/call", async route => {
      const body = route.request().postDataJSON();
      if (body?.op !== "library_open") return route.continue();
      opens.push(body.name);
      if (opens.length === 2) return route.fulfill({ status: 502, json: { err: { code: "unavailable", message: "시험용 보고서 읽기 실패" } } });
      const response = await route.fetch(); const json = await response.json();
      if (opens.length === 1) json.ok.expires_at_ms = Date.now() - 1;
      replies.push({ name: body.name, expired: json.ok.expires_at_ms < Date.now(), deadline: json.ok.expires_at_ms });
      await route.fulfill({ response, json });
    });
    await p.route("**/v/**/revision.svg", async route => {
      if (route.request().method() === "HEAD") {
        heads.push({ cookie: !!route.request().headers().cookie });
        return failed ? route.fulfill({ status: 401, body: "expired" }) : route.continue();
      }
      if (blocked) { await gate; await route.fulfill({ status: 401, body: "expired" }); }
      else await route.continue();
    });
    await open(p, id);
    await p.getByTestId("report-expired").waitFor();
    const slot = p.locator(".image-slot.is-loading");
    await slot.getByText("이미지 불러오는 중", { exact: true }).waitFor();
    const old = await slot.locator("img").getAttribute("src");
    await slot.getByRole("button", { name: "상태 확인" }).click();
    await slot.getByText(/HTTP 200/).waitFor();
    if (engine === "chromium" && width === 1280 || engine === "webkit" && width === 390) await shot(p, `73-media-wait-${engine}-${width}`);
    failed = true; release();
    const broken = p.locator(".image-slot.is-failed");
    await broken.getByText("이미지를 표시할 수 없음", { exact: true }).waitFor();
    await broken.getByRole("button", { name: "상태 확인" }).click();
    await broken.getByText(/HTTP 401/).waitFor();
    if (engine === "chromium" && width === 1280 || engine === "webkit" && width === 390) await shot(p, `73-media-error-${engine}-${width}`);
    if (!(await broken.innerText()).includes("보기 권한이 끝났습니다") || (await broken.innerText()).includes("/v/")) throw Error("권한 원인 또는 비밀 주소 표시");
    if (heads.length !== 2 || heads.some(h => h.cookie)) throw Error("진단 HEAD 횟수·쿠키 계약");
    blocked = false;
    await p.getByTestId("report-reopen").click();
    await p.getByRole("alert").filter({ hasText: "시험용 보고서 읽기 실패" }).waitFor();
    if (await p.locator('.files a[href$="/revision.svg"]').getAttribute("href") !== old) throw Error("실패한 다시 열기가 이전 판을 버렸다");
    await p.getByTestId("report-expired").waitFor();
    // 첫 실패를 자동 재요청하지 않는다. 두 번째 사용자 누름에서만 새 권한을 받는다.
    if (opens.length !== 2) throw Error("다시 열기가 자동 재시도했다");
    await p.getByTestId("report-reopen").click();
    await p.getByTestId("report-expired").waitFor({ state: "detached" });
    await domIs(p, "새 권한의 현재 판 이미지 디코드", old => {
      const img = document.querySelector('img[alt="시안"]');
      return img?.complete && img.naturalWidth > 0 && img.getAttribute("src") !== old;
    }, old);
    if (opens.length !== 3 || opens.slice(1).some(name => name !== id)) throw Error("다시 열기가 선택한 r 판을 바꿨다");
    if ((await p.getByTestId("report-id").textContent()) !== id) throw Error("보고 판 변경");
    if (engine === "chromium" && width === 1280 || engine === "webkit" && width === 390) await shot(p, `73-media-reopened-${engine}-${width}`);
    await domIs(p, "진단·다시 열기 가로 넘침 없음", () => document.documentElement.scrollWidth <= innerWidth);
    for (const checkWidth of width === 390 ? [390, 320] : [width]) {
    await p.setViewportSize({ width: checkWidth, height: 844 });
    await domIs(p, "보고 번호와 동작 단추 겹침 없음", () => {
      const id = document.querySelector('[data-testid="report-id"]');
      const next = document.querySelector('[aria-label="보고 판"]') ?? document.querySelector('[data-testid="report-reopen"]');
      const a = id.getBoundingClientRect(), b = next.getBoundingClientRect();
      return a.right <= b.left || a.bottom <= b.top || a.left >= b.right || a.top >= b.bottom;
    });
    }
    if (paired.errors.length) throw Error(paired.errors.join("; "));
  } catch (error) {
    const state = await p.evaluate(() => ({ now: Date.now(), hash: location.hash, expired: document.querySelector('[data-testid="report-expired"]')?.textContent, status: document.querySelector('[data-testid="report-detail"]')?.dataset.status }));
    throw new Error(`${error.message} ‖ 진단 요청/시각: ${JSON.stringify({ opens, replies, state })}`, { cause: error });
  } finally {
    release(); await p.unrouteAll({ behavior: "wait" }); await p.context().close(); if (owned) await owned.close();
  }
}
export default async function ({ page }) {
  let first, second, thread, threadFirst, threadSecond;
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
    await until("첫 회수", () => aaa("thread", "show", thread.id).report_id);
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
  await step("Chromium 1280px 이미지 대기·HTTP 진단·만료·현재 판 다시 열기", () => checkMediaDiagnosis("chromium", 1280, threadFirst));
  await step("Chromium 390px 이미지 대기·HTTP 진단·만료·현재 판 다시 열기", () => checkMediaDiagnosis("chromium", 390, threadFirst));
  await step("WebKit 1280px 이미지 대기·HTTP 진단·만료·현재 판 다시 열기", () => checkMediaDiagnosis("webkit", 1280, threadFirst));
  await step("WebKit 390px 이미지 대기·HTTP 진단·만료·현재 판 다시 열기", () => checkMediaDiagnosis("webkit", 390, threadFirst));
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
