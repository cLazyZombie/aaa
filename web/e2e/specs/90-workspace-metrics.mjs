// 승인된 workspace A: 실제 보고 감시→agent 기존 산출물 수집→host 원장→웹 조회.
// fixture는 기존 runner의 종료 결과만 만들며 지표 API·집계·UI를 대체하지 않는다.
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { aaa, root, step, domIs, until, pairPage, browser, webkit, shot, inList, openSettings } from "../lib/harness.mjs";
const put = (path, text) => { fs.mkdirSync(path.slice(0, path.lastIndexOf("/")), { recursive: true }); fs.writeFileSync(path, text); };
const api = (page, query = {}) => page.evaluate(async (query) => {
  const response = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "workspace_dashboard", query }) });
  const body = await response.json();
  if (!body.ok) throw new Error(JSON.stringify(body));
  return body.ok;
}, query);
async function theme(page, value) {
  const settings = await openSettings(page, "모양");
  await settings.getByRole("group", { name: "테마", exact: true }).getByRole("button", { name: value === "light" ? "밝게" : "어둡게", exact: true }).click();
  await settings.locator("#settings-palette").selectOption("charcoal");
  await settings.getByRole("button", { name: "설정 닫기" }).click();
  await domIs(page, "테마 적용", (value) => document.documentElement.dataset.theme === value && !new URL(location.href).searchParams.has("settings"), value);
}
export default async function ({ page }) {
  let thread, cwd, work, workspace, link;
  await step("workspace A: 등록 저장소와 미수집, 실제 보고 회수로 검증 시간·개수·E2E TOP 5·커밋 줄 수를 보존한다", async () => {
    const repo = `${root}/app`;
    fs.mkdirSync(repo);
    const git = (...args) => execFileSync("git", ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.com", ...args], { stdio: "pipe" });
    git("init", "-q", "-b", "main"); put(`${repo}/src/product.ts`, "// comment\nexport const value = 1;\n\n"); git("add", "."); git("commit", "-qm", "fixture");
    aaa("project", "create", "demo"); aaa("project", "set", "demo", "repos.add", `${repo}@labA`);
    await theme(page, "light");
    await page.getByTestId("activity-workspace").click();
    await page.getByTestId("workspace-repo").waitFor();
    await domIs(page, "수집 전", () => document.querySelector('[data-testid="workspace-view"]')?.textContent.includes("기존 실행 결과 수집 전"));
    const before = await api(page); workspace = before.workspace_id;
    if (before.runs.length || before.code_lines.length) throw new Error("수집 전 가짜 수치");
    put(`${root}/metrics-task.md`, "workspace 결과를 확인한다\n");
    thread = aaa("thread", "start", "demo", "--title", "workspace 결과", "--machine", "labA", "--repo", repo, "--profile", "fake", "--task-file", `${root}/metrics-task.md`);
    await until("초기 보고 회수", () => aaa("thread", "show", "demo", thread.id).report_hash);
    const record = aaa("thread", "show", "demo", thread.id); cwd = record.cwd; work = record.work_dir;
    const dir = `${cwd}/target/ci-local/run-fixture`;
    put(`${dir}/rust/summary.json`, JSON.stringify({ jobs: [{ name: "unit", log: "unit.log", seconds: 300 }] }));
    put(`${dir}/rust/unit.log`, "test result: ok. 12 passed; 0 failed; 2 ignored; 0 measured; 3 filtered out; finished in 300s\n");
    put(`${dir}/e2e.log`, "## 요약: 파일 2개 × 1번, worker 2개, 모두 80.0초\n## 작업: 3개(독립 단계 분할 포함)\n##   01-first 통과 2/2 90.5초 (독립 작업 합계)\n##   02-last 통과 1/1 2.0초\n");
    put(`${dir}/workspace-summary.tsv`, "v1|100|164|succeeded\nbuild|ok|00:00|00:01|12|unused\ntest|ok|00:01|00:02|300|unused\ne2e|ok|00:01|00:02|80|unused\n");
    fs.appendFileSync(`${work}/report.md`, "\n새 검증 결과를 보존했다.\n");
    // 수집 완료 사건이 열린 화면을 갱신해야 한다. 다시 읽기나 고정 sleep은 없다.
    await domIs(page, "실제 수집·웹 갱신", () => document.querySelector(".ws-metrics")?.textContent.includes("2분 44초"));
    const got = await api(page, { workspace_id: workspace });
    if (got.runs.length !== 1 || got.runs[0].tests.passed !== 12 || got.runs[0].e2e_files !== 2 || got.runs[0].e2e_jobs !== 3) throw new Error(JSON.stringify(got));
    if (got.runs[0].slow_tests.items.length || got.runs[0].slow_e2e.items[0].duration_ms !== 90500 || got.code_lines[0]?.total_lines !== 2) throw new Error("집계 값·묶음 시간 구분·dirty 코드 제외");
    await shot(page, "workspace-desktop-light");
  });
  await step("workspace: 기준 실행·URL 복원·키보드 상세·필터 결과 없음·로그 삭제 뒤 host 원장 보존", async () => {
    await page.getByLabel("시간 집계").selectOption("day");
    await page.getByLabel("시간대").selectOption("utc");
    await page.getByLabel("코드 줄 수 축").selectOption("commit");
    await page.getByRole("button", { name: "30일", exact: true }).click();
    await page.getByRole("tab", { name: "실행 기록", exact: true }).click();
    link = page.url(); await page.reload(); await page.getByTestId("workspace-view").waitFor();
    await page.locator(".ws-table-scroll tbody button").first().click();
    await page.getByRole("dialog").waitFor();
    if (!(await page.getByRole("dialog").textContent()).includes("단계 시간의 합계는 전체 경과가 아닙니다")) throw new Error("병렬 경과 설명");
    await page.keyboard.press("Escape"); await page.getByRole("dialog").waitFor({ state: "hidden" });
    await page.getByRole("tab", { name: "개요", exact: true }).click();
    await page.getByRole("group", { name: /경과 초 추이/ }).getByRole("button").first().focus(); await page.keyboard.press("Enter"); await page.getByRole("dialog").waitFor(); await page.getByLabel("상세 닫기").click();
    await page.getByLabel("profile", { exact: true }).selectOption("unknown");
    await domIs(page, "미기록 profile", () => document.querySelector(".ws-metrics")?.textContent.includes("12"));
    const empty = await api(page, { workspace_id: workspace, machine: "missing" }); if (empty.runs.length) throw new Error("빈 필터");
    // 원본 로그가 없어져도 조회가 파일을 다시 열어서는 안 된다.
    fs.unlinkSync(`${cwd}/target/ci-local/run-fixture/rust/unit.log`); fs.unlinkSync(`${cwd}/target/ci-local/run-fixture/e2e.log`);
    const saved = await api(page, { workspace_id: workspace }); if (saved.runs[0]?.tests.passed !== 12) throw new Error("host 보존 요약이 사라졌다");
    await page.getByTestId("activity-workspace").click();
    if (!(await page.locator(".app").getAttribute("class")).includes("sidebar-collapsed")) throw new Error("같은 활동 단추로 접기");
    await page.getByTestId("activity-workspace").click();
    await shot(page, "workspace-filtered");
  });
  await step("workspace: Chromium·WebKit 휴대폰·밝음·어두움 Reader와 drawer·URL, 가로 넘침·조회 오류", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      for (const [engine, launch] of [["chromium", browser], ["webkit", safari]]) for (const width of [390, 320]) for (const scheme of ["light", "dark"]) {
        await theme(page, scheme);
        const device = await pairPage(launch, aaa("web", "pair", "--reader").code, { width, height: 844 }, { hash: new URL(link).hash, colorScheme: scheme, touch: true });
        const p = device.page;
        try {
          await p.getByTestId("workspace-view").waitFor();
          await p.locator(".ws-table-scroll tbody tr").first().waitFor();
          await inList(p, p.getByTestId("activity-workspace"));
          await p.getByTestId("drawer-toggle").click();
          await p.getByTestId("workspace-repo").click();
          await p.getByRole("tab", { name: "개요", exact: true }).click();
          await p.locator(".ws-metrics").waitFor();
          const overflow = await p.evaluate(() => ({ document: document.documentElement.scrollWidth > innerWidth, view: document.querySelector(".ws-content").scrollWidth > document.querySelector(".ws-content").clientWidth + 1, theme: document.documentElement.dataset.theme }));
          if (overflow.document || overflow.view || overflow.theme !== scheme) throw new Error(JSON.stringify(overflow));
          if (device.errors.length) throw new Error(device.errors.join(";"));
          await shot(p, `workspace-${engine}-${width}-${scheme}`);
        } finally { await p.context().close(); }
      }
    } finally { await safari.close(); }
    await page.route("**/api/v1/call", async (route) => route.request().postDataJSON()?.op === "workspace_dashboard" ? route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ err: { code: "unavailable", message: "지표 조회 오류 fixture" } }) }) : route.fallback());
    await page.getByRole("button", { name: "다시 읽기", exact: true }).click(); await page.getByRole("alert").filter({ hasText: "지표 조회 오류 fixture" }).first().waitFor();
    if (!(await page.locator(".ws-metrics").textContent()).includes("2분 44초")) throw new Error("오류 때 마지막 성공 요약 보존");
  });
}
