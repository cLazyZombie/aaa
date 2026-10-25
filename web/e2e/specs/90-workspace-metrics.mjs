// 승인된 workspace A: 실제 보고 감시→agent 기존 산출물 수집→host 원장→웹 조회.
// fixture는 기존 runner의 종료 결과만 만들며 지표 API·집계·UI를 대체하지 않는다.
import fs from "node:fs";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import readline from "node:readline";
import { terminateSessionHosts } from "../lib/session-cleanup.mjs";
import { aaa, AA, env, procs, start, stop, inTime, root, step, domIs, until, pairPage, browser, webkit, shot, inList, openSettings } from "../lib/harness.mjs";
// 두 agent가 같은 host에 연결되는 격리 TCP 실험실이다.
const hostToml = `${root}/c/host.toml`;
fs.writeFileSync(hostToml, fs.readFileSync(hostToml,"utf8").replace('agent_listen = ""', 'agent_listen = "127.0.0.1:0"'));
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
  let thread, cwd, work, workspace, link, repo, secondThread;
  try {
  await step("workspace A: 등록 저장소와 미수집, 실제 보고 회수로 검증 시간·개수·E2E TOP 5·커밋 줄 수를 보존한다", async () => {
    repo = `${root}/app`;
    fs.mkdirSync(repo);
    const git = (...args) => execFileSync("git", ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.com", ...args], { stdio: "pipe", env: { ...process.env, GIT_COMMITTER_DATE: "2001-01-01T00:00:00Z" } });
    git("init", "-q", "-b", "main"); put(`${repo}/src/product.ts`, "// comment\nexport const value = 1;\n\n"); git("add", "."); git("commit", "-qm", "fixture");
    aaa("project", "create", "demo"); aaa("project", "set", "demo", "repos.add", `${repo}@labA`);
    await theme(page, "light");
    await page.getByTestId("activity-workspace").click();
    await page.getByTestId("workspace-view").waitFor();
    const before = await api(page); workspace = before.workspace_id;
    if (before.runs.length || before.code_lines.length) throw new Error("수집 전 가짜 수치");
    put(`${root}/metrics-task.md`, "workspace 결과를 확인한다\n");
    thread = aaa("thread", "start", "demo", "--title", "workspace 결과", "--machine", "labA", "--repo", repo, "--profile", "fake", "--task-file", `${root}/metrics-task.md`);
    await until("초기 보고 회수", () => aaa("thread", "show", "demo", thread.id).report_hash);
    const record = aaa("thread", "show", "demo", thread.id); cwd = record.cwd; work = record.work_dir;
    // thread 산출물은 수집하지 않고 등록한 main checkout만 읽는다.
    put(`${cwd}/target/ci-local/thread/workspace-summary.tsv`, "v1|1000000000|999|succeeded\n");
    git("remote", "add", "origin", "https://example.com/team/app.git");
    git("update-ref", "refs/remotes/origin/main", "HEAD");
    const dir = `${repo}/target/ci-local/run-fixture`;
    put(`${dir}/rust/summary.json`, JSON.stringify({ jobs: [{ name: "unit", log: "unit.log", seconds: 300 }] }));
    put(`${dir}/rust/unit.log`, "test result: ok. 12 passed; 0 failed; 2 ignored; 0 measured; 3 filtered out; finished in 300s\n");
    put(`${dir}/e2e.log`, "## 요약: 파일 2개 × 1번, worker 2개, 모두 80.0초\n## 작업: 3개(독립 단계 분할 포함)\n##   01-first 통과 2/2 90.5초 (독립 작업 합계)\n##   02-last 통과 1/1 2.0초\n");
    put(`${dir}/workspace-summary.tsv`, "v1|1000000000|164|succeeded\nbuild|ok|00:00|00:01|12|unused\ntest|ok|00:01|00:02|300|unused\ne2e|ok|00:01|00:02|80|unused\n");
    fs.appendFileSync(`${work}/report.md`, "\n새 검증 결과를 보존했다.\n");
    // 수집 완료 사건이 열린 화면을 갱신해야 한다. 다시 읽기나 고정 sleep은 없다.
    await domIs(page, "실제 수집·웹 갱신", () => document.querySelector(".ws-metrics")?.textContent.includes("2분 44초"));
    const got = await api(page); workspace = got.workspace_id;
    if (got.runs.length !== 1 || got.runs[0].tests.passed !== 12 || got.runs[0].e2e_files !== 2 || got.runs[0].e2e_jobs !== 3) throw new Error(JSON.stringify(got));
    if (got.runs[0].slow_tests.items.length || got.runs[0].slow_e2e.items[0].duration_ms !== 90500 || got.code_lines[0]?.total_lines !== 2) throw new Error("집계 값·묶음 시간 구분·dirty 코드 제외");
    await shot(page, "workspace-desktop-light");
  });
  await step("같은 origin의 두 머신을 workspace 하나로 묶고 선·점·필터와 개수의 독립성을 확인한다", async () => {
    const nodeB = { AAA_CONFIG_DIR: `${root}/b/c`, AAA_STATE_DIR: `${root}/b/s` };
    fs.mkdirSync(`${root}/b/c`, { recursive:true });
    fs.mkdirSync(`${root}/b/work`, { recursive:true });
    fs.writeFileSync(`${root}/b/c/agent.toml`, `schema = 1\n[agent]\nwork_root = "${root}/b/work"\n`);
    const addr = aaa("status").agent_listen;
    aaa("machine", "enroll", "--open", "5m");
    const proc = spawn(AA, ["agent", "enroll", addr, "--name", "labB"], { env: { ...env, ...nodeB }, stdio: ["ignore", "pipe", "pipe"] });
    procs.set("metrics-enroll", proc);
    let enrollError = ""; proc.stderr.on("data", data => { enrollError += data; });
    const exited = new Promise(resolve => proc.once("exit", resolve));
    const code = await inTime(new Promise((resolve, reject) => {
      readline.createInterface({ input: proc.stdout }).on("line", line => { const match = line.match(/--code (\d{6})/); if (match) resolve(match[1]); });
      proc.once("exit", code => reject(new Error(`등록 코드 전 종료 ${code}: ${enrollError}`)));
    }), 20000, "두 번째 머신 등록 코드");
    aaa("machine", "approve", "labB", "--code", code);
    if (await inTime(exited, 20000, "머신 등록 완료") !== 0) throw new Error("머신 등록 실패");
    start("metrics-agentB", nodeB, "agent", "--host", addr, "--name", "labB");
    await until("두 번째 머신 연결", () => aaa("machine", "list").machines.some(m => m.name === "labB" && m.connection === "connected"));
    const repoB = `${root}/other/app`;
    fs.mkdirSync(`${root}/other`);
    execFileSync("git", ["clone", "--quiet", repo, repoB]);
    execFileSync("git", ["-C", repoB, "remote", "remove", "origin"]);
    aaa("project", "set", "demo", "repos.add", `${repoB}@labB`);
    const second = secondThread = aaa("thread", "start", "demo", "--title", "다른 머신 main", "--machine", "labB", "--repo", repoB, "--profile", "fake", "--task-file", `${root}/metrics-task.md`);
    await until("두 번째 보고", () => aaa("thread", "show", "demo", second.id).report_hash);
    execFileSync("git", ["-C", repoB, "remote", "add", "origin", "git@example.com:team/app.git"]);
    execFileSync("git", ["-C", repoB, "update-ref", "refs/remotes/origin/main", "HEAD"]);
    const dir = `${repoB}/target/ci-local/main`;
    fs.cpSync(`${repo}/target/ci-local/run-fixture`, dir, { recursive:true });
    // clone 직후 Git reflog의 시계와 실행 시작 시계를 명시한다. 고정 대기 없이 fixture 시간만 쓴다.
    const now = Math.floor(Date.now()/1000);
    const reflog = `${repoB}/.git/logs/HEAD`;
    fs.writeFileSync(reflog, fs.readFileSync(reflog,"utf8").replace(/ \d{10} ([+-]\d{4})\t/g, ` ${now-1000} $1\t`));
    put(`${dir}/workspace-summary.tsv`, `v1|${now-100}|164|succeeded\nbuild|ok|00:00|00:01|24|unused\n`);
    const nextA = `${repo}/target/ci-local/main-next`;
    fs.cpSync(`${repo}/target/ci-local/run-fixture`, nextA, { recursive:true });
    put(`${nextA}/workspace-summary.tsv`, "v1|1000000000|180|succeeded\nbuild|ok|00:00|00:01|18|unused\n");
    fs.appendFileSync(`${work}/report.md`, "\n다음 main 결과\n");
    const nextB = `${repoB}/target/ci-local/main-next`;
    fs.cpSync(dir, nextB, { recursive:true });
    put(`${nextB}/workspace-summary.tsv`, `v1|${now-90}|164|succeeded\nbuild|ok|00:00|00:01|20|unused\n`);
    const rec = aaa("thread", "show", "demo", second.id);
    fs.appendFileSync(`${rec.work_dir}/report.md`, "\nmain 결과 준비\n");
    await domIs(page, "두 머신의 실행 네 개 회수", () => document.querySelectorAll('.ws-chart polyline[data-machine]').length === 2 && document.querySelectorAll('.ws-chart g[role=button][data-machine]').length === 4);
    const got = await api(page);
    if (got.workspaces.length !== 1 || got.workspaces[0].machines.length !== 2 || got.runs.length !== 4) throw new Error(`workspace 머신 묶음 실패: ${JSON.stringify(got)}`);
    if (await page.getByLabel("코드 줄 수 branch").count()) throw new Error("branch 선택이 남았다");
    await page.getByTestId("workspace-view").getByLabel("머신", { exact:true }).selectOption("labA");
    await domIs(page, "머신 하나 필터", () => document.querySelectorAll('.ws-chart polyline[data-machine]').length === 1 && document.querySelector('.ws-chart polyline')?.getAttribute('data-machine') === 'labA');
    const filtered = await api(page, { workspace_id:workspace, machine:"labA" });
    if (filtered.counts_run.run_id !== got.counts_run.run_id) throw new Error("workspace 개수가 머신 필터를 따라 바뀐다");
    await page.getByTestId("workspace-view").getByLabel("머신", { exact:true }).selectOption("");
    await domIs(page, "머신 전체 복원", () => document.querySelectorAll('.ws-chart polyline[data-machine]').length === 2);
    await shot(page, "workspace-machines-desktop");
    await page.getByLabel("시간 지표").selectOption("build");
    await page.getByRole("group", { name:/경과 초 추이/ }).scrollIntoViewIfNeeded();
    await shot(page, "workspace-machine-lines-desktop");
    await page.getByLabel("시간 지표").selectOption("verify");
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
    fs.unlinkSync(`${repo}/target/ci-local/run-fixture/rust/unit.log`); fs.unlinkSync(`${repo}/target/ci-local/run-fixture/e2e.log`);
    const saved = await api(page, { workspace_id: workspace }); if (saved.runs[0]?.tests.passed !== 12) throw new Error("host 보존 요약이 사라졌다");
    await page.getByTestId("activity-workspace").click();
    if (!(await page.locator(".app").getAttribute("class")).includes("sidebar-collapsed")) throw new Error("같은 활동 단추로 접기");
    await page.getByTestId("activity-workspace").click();
    await shot(page, "workspace-filtered");
  });
  await step("긴 workspace 목록 fixture를 실제 등록·원장에 추가한다", async () => {
    // host DB에는 host만 쓴다. 고유 origin을 가진 실제 checkout을 등록하고,
    // 보고 회수 → agent 신원 수집 → host 원장 저장이 끝난 뒤 목록을 검사한다.
    // 기존 thread 두 개와 합쳐 기본 동시 한도(10) 안에서 여섯 개씩 수집한다.
    for (let batch=0; batch<24; batch+=6) {
      const fixtures = [];
      for (let i=batch; i<batch+6; i++) {
        const name = `sample-${String(i).padStart(2,"0")}`;
        const path = `${root}/${name}`;
        execFileSync("git", ["clone", "--quiet", "--shared", repo, path]);
        execFileSync("git", ["-C", path, "remote", "set-url", "origin", `https://example.com/team/${name}.git`]);
        aaa("project", "set", "demo", "repos.add", `${path}@labA`);
        const fixture = aaa("thread", "start", "demo", "--title", name, "--machine", "labA", "--repo", path, "--profile", "fake", "--task-file", `${root}/metrics-task.md`);
        fixtures.push({ name, id: fixture.id });
      }
      await until(`workspace 묶음 ${batch/6+1}의 실제 수집`, async () => {
        const listed = (await api(page)).workspaces;
        return fixtures.every(f => listed.some(w => w.name === f.name && w.id));
      });
      // 다음 묶음이 동시 thread 한도를 차지하지 않게 끝낸다. dirty 산출물은 실험실과 함께 지운다.
      for (const fixture of fixtures) aaa("thread", "resolve", "demo", fixture.id, "--keep-worktree");
    }
    const listed = (await api(page)).workspaces;
    if (listed.length !== 25 || listed.filter(w => w.name.startsWith("sample-")).length !== 24) throw new Error(`긴 workspace 목록 수집 실패: ${JSON.stringify(listed)}`);
  });
  await step("workspace: Chromium·WebKit 휴대폰·밝음·어두움 Reader와 drawer·URL, 가로 넘침·조회 오류", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      for (const [engine, launch] of [["chromium", browser], ["webkit", safari]]) for (const width of [1280, 820, 390, 320]) for (const scheme of ["light", "dark"]) {
        await theme(page, scheme);
        const device = await pairPage(launch, aaa("web", "pair", "--reader").code, { width, height: width === 820 ? 1180 : 844 }, { hash: new URL(link).hash, colorScheme: scheme, touch: width < 1280 });
        const p = device.page;
        try {
          await p.getByTestId("workspace-view").waitFor();
          await p.locator(".ws-table-scroll tbody tr").first().waitFor();
          await inList(p, p.getByTestId("workspace-repo").first());
          await p.getByRole("tab", { name: "개요", exact: true }).click();
          await p.locator(".ws-metrics").waitFor();
          await p.getByTestId("workspace-view").getByLabel("머신", { exact:true }).selectOption("labB");
          await domIs(p, "선택 머신 선과 사각 점", () => document.querySelectorAll('.ws-chart polyline[data-machine]').length === 1 && document.querySelector('.ws-chart polyline')?.getAttribute('data-machine') === 'labB' && document.querySelector('.ws-chart g[role=button] rect'));
          await p.getByTestId("workspace-view").getByLabel("머신", { exact:true }).selectOption("");
          await domIs(p, "전체 머신 선 복원", () => document.querySelectorAll('.ws-chart polyline[data-machine]').length === 2);
          await domIs(p, "공유 테마 반영", value => document.documentElement.dataset.theme === value, scheme);
          const overflow = await p.evaluate(() => ({ document: document.documentElement.scrollWidth > innerWidth, view: document.querySelector(".ws-content").scrollWidth > document.querySelector(".ws-content").clientWidth + 1, theme: document.documentElement.dataset.theme }));
          if (overflow.document || overflow.view || overflow.theme !== scheme) throw new Error(JSON.stringify(overflow));
          if (device.errors.length) throw new Error(device.errors.join(";"));
          await shot(p, `workspace-${engine}-${width}-${scheme}`);
          for (const panel of ["workspace", "asks", "work", "tasks"]) {
            const activity = p.getByTestId(`activity-${panel}`);
            if (panel === "tasks" || await activity.getAttribute("aria-selected") !== "true") await inList(p, activity);
            const toggle = p.locator(".view [data-testid=drawer-toggle]:visible");
            if (width <= 768 && await toggle.isVisible() && await toggle.getAttribute("aria-expanded") !== "true") await toggle.click();
            if (panel === "workspace") {
              await domIs(p, "긴 목록만 스크롤", () => {
                const list = document.querySelector(".ws-list");
                if (list.scrollHeight <= list.clientHeight) return false;
                list.scrollTop = list.scrollHeight;
                return list.scrollTop > 0 && document.querySelector(".sidebar-shell").scrollHeight <= document.querySelector(".sidebar-shell").clientHeight + 1;
              });
            }
            await domIs(p, "사용량 바닥 고정", () => {
              const foot = document.querySelector(".list-foot").getBoundingClientRect();
              const shell = document.querySelector(".sidebar-shell").getBoundingClientRect();
              const gear = document.querySelector('[data-testid="settings-open"]').getBoundingClientRect();
              return Math.abs(foot.bottom - shell.bottom) < 2 && Math.abs((foot.top+foot.bottom)/2 - (gear.top+gear.bottom)/2) < 12;
            });
            if (scheme === "light") await shot(p, `footer-${panel}-${engine}-${width}`);
          }
        } finally { await p.context().close(); }
      }
    } finally { await safari.close(); }
    await page.route("**/api/v1/call", async (route) => route.request().postDataJSON()?.op === "workspace_dashboard" ? route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ err: { code: "unavailable", message: "지표 조회 오류 fixture" } }) }) : route.fallback());
    await page.getByRole("button", { name: "다시 읽기", exact: true }).click(); await page.getByRole("alert").filter({ hasText: "지표 조회 오류 fixture" }).first().waitFor();
    if (!(await page.locator(".ws-metrics").textContent()).includes("2분 44초")) throw new Error("오류 때 마지막 성공 요약 보존");
  });
  await step("별도 로그의 실제 ci-local 종료를 push 뒤 회수하고 착지 worktree 삭제 후에도 보존한다", async () => {
    await page.unroute("**/api/v1/call");
    const landing = `${root}/landing`;
    const git = (...args) => execFileSync("git", ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.com", ...args], { encoding:"utf8", stdio:"pipe" }).trim();
    git("worktree", "add", "-qb", "landing", landing, "main");
    const runGit = (...args) => execFileSync("git", ["-C", landing, "-c", "user.name=Test", "-c", "user.email=test@example.com", ...args], { encoding:"utf8", stdio:"pipe" }).trim();
    put(`${landing}/src/product.ts`, "// comment\nexport const value = 2;\nexport const added = true;\n");
    runGit("commit", "-qam", "landing source");
    const commit = runGit("rev-parse", "HEAD");
    fs.mkdirSync(`${landing}/scripts`);
    for (const name of ["ci-local.sh", "workspace_metrics.py"]) fs.copyFileSync(new URL(`../../../scripts/${name}`, import.meta.url), `${landing}/scripts/${name}`);
    for (const code of [0, 7]) {
      put(`${landing}/scripts/check-public.sh`, `exit ${code}\n`);
      const result = spawnSync("bash", ["scripts/ci-local.sh", "--only", "public", "--logs", `${root}/external-logs-${code}`], { cwd:landing, env, encoding:"utf8" });
      if (result.status !== (code === 0 ? 0 : 1) || result.stderr.includes("요약 보관 실패")) throw new Error(`실제 CI 종료 보관: ${result.status} ${result.stdout} ${result.stderr}`);
    }
    fs.appendFileSync(`${work}/report.md`, "\n미push 착지 결과 준비\n");
    await until("push 전 main 반영 대기", async () => (await api(page)).workspaces.find(w => w.id === workspace)?.collection_notes.some(n => n.includes("반영 대기 CI 실행 2개")));
    if ((await api(page)).runs.length !== 4) throw new Error("미push 실행을 main 기록에 넣었다");
    git("update-ref", "refs/remotes/origin/main", commit);
    git("worktree", "remove", "--force", landing);
    for (const code of [0, 7]) fs.rmSync(`${root}/external-logs-${code}`, { recursive:true });
    if (git("rev-parse", "HEAD") === commit) throw new Error("등록 checkout이 새 main으로 이동했다");
    await page.getByRole("tab", { name:"실행 기록", exact:true }).click();
    fs.appendFileSync(`${work}/report.md`, "\nmain push와 착지 폴더 정리 완료\n");
    await domIs(page, "열린 실행 표에 실제 CI 두 건 표시", () => document.querySelectorAll('.ws-table-scroll tbody tr').length === 6);
    const got = await api(page);
    const archived = got.runs.filter(r => r.commit === commit);
    if (archived.length !== 2 || !archived.some(r => r.outcome === "failed") || !archived.some(r => r.outcome === "succeeded") || archived.some(r => r.machine !== "labA" || !r.source.includes("main 반영 확인"))) throw new Error(JSON.stringify(got));
    if (got.code_lines.at(-1)?.commit !== commit || got.code_lines.at(-1)?.total_lines !== 3) throw new Error("등록 HEAD가 아닌 push된 main 코드 집계 실패");
    const collected = got.workspaces.find(w => w.id === workspace).last_collected_ms;
    fs.appendFileSync(`${work}/report.md`, "\n같은 실행 재회수\n");
    await until("중복 없이 원장 재회수", async () => {
      const value = await api(page);
      return value.workspaces.find(w => w.id === workspace).last_collected_ms > collected && value.runs.length === 6;
    });
    await shot(page, "workspace-ci-after-push");
  });
  await step("rc·엔진·pt의 동일 main 반복 검증을 다른 머신과 기존 저장소의 후속 보고에서 함께 회수한다", async () => {
    const fixtures = [];
    for (const [name, runner, machine] of [["rc","rc","labA"],["no2-engine","engine","labB"],["pt","pt","labA"]]) {
      const repo = `${root}/games/${name}`;
      fs.mkdirSync(`${repo}/scripts`, {recursive:true});
      fs.copyFileSync(new URL("../../../scripts/workspace_record.py",import.meta.url), `${repo}/scripts/workspace_record.py`);
      put(`${repo}/.gitignore`, "target/\n");
      const git = (...args) => execFileSync("git",["-C",repo,"-c","user.name=Test","-c","user.email=test@example.com",...args],{encoding:"utf8",stdio:"pipe"}).trim();
      git("init","-qb","main"); git("add","."); git("commit","-qm","verification fixture");
      git("remote","add","origin",`https://example.com/team/${name}.git`);
      git("update-ref","refs/remotes/origin/main","HEAD");
      const commit = git("rev-parse","HEAD");
      const checkout = `${root}/verify-${name}`;
      git("worktree","add","-qb","verification",checkout,"main");
      for (const [mode, code] of [["coverage","0"],["default","7"]]) {
        const py = (...args) => execFileSync("python3",["scripts/workspace_record.py",...args],{cwd:checkout,env,encoding:"utf8",stdio:"pipe"}).trim();
        const token = py("start",runner,mode,"full");
        const names = runner === "rc" ? ["default-tests","workspace-tests"] : runner === "engine" ? ["workspace-tests","automation-tests","2d-tests","3d-tests"] : [];
        for (const suite of names) put(`${checkout}/target/verification/${suite}/report.json`,JSON.stringify([{target:"game",exit:0,tests:[{name:"unit",event:"ok",exec_time:0.25}],suites:[{passed:1,failed:0,ignored:0}]}]));
        if (runner === "rc") put(`${checkout}/target/verification/summary.json`,JSON.stringify({gates:[{name:"runtime",status:"passed",seconds:2}]}));
        if (runner === "engine") put(`${checkout}/target/verification/gates/runtime.status`,"runtime\truntime tests\t0\t2\n");
        if (runner === "pt") put(`${checkout}/${token.replace(/\.json$/,".stages")}`,"build|ok|0|0|2\n");
        if (runner !== "pt") put(`${checkout}/target/${runner === "rc" ? "rc-e2e/report.json" : "no2-e2e/all-headless.json"}`,JSON.stringify({status:"passed",reports:[{scenario:"smoke",status:"passed",duration_ms:30}]}));
        py("finish",token,code);
      }
      git("worktree","remove","--force",checkout);
      aaa("project","set","demo","repos.add",`${repo}@${machine}`);
      fixtures.push({name,machine,commit});
    }
    fs.appendFileSync(`${work}/report.md`,"\n여러 저장소 검증 종료\n");
    fs.appendFileSync(`${aaa("thread","show","demo",secondThread.id).work_dir}/report.md`,"\n엔진 검증 종료\n");
    await until("모든 등록 저장소의 원격 신원 회수", async () => (await api(page)).workspaces.filter(w => fixtures.some(f => f.name === w.name)).length === 3);
    for (const fixture of fixtures) {
      const ws = (await api(page)).workspaces.find(w => w.name === fixture.name);
      const got = await api(page,{workspace_id:ws.id,days:30});
      if (got.runs.length !== 2 || got.runs.some(r => r.commit !== fixture.commit || r.machine !== fixture.machine) || !got.runs.some(r => r.outcome === "failed")) throw new Error(`game 원장 ${fixture.name}: ${JSON.stringify(got)}`);
      if (fixture.name !== "pt" && got.runs.some(r => r.tests?.passed !== (fixture.name === "rc" ? 2 : 4) || r.e2e_files !== 1)) throw new Error(`game 개수 ${JSON.stringify(got)}`);
      await page.getByTestId("workspace-repo").filter({has:page.locator("strong",{hasText:new RegExp(`^${fixture.name}$`)})}).click();
      await page.getByRole("tab",{name:"실행 기록",exact:true}).click();
      await domIs(page,"선택한 game 원장의 성공·실패 두 행",({name,machine}) => document.querySelector('.ws-heading h1')?.textContent.startsWith(name) && document.querySelectorAll('.ws-table-scroll tbody tr').length === 2 && [...document.querySelectorAll('.ws-table-scroll tbody tr')].every(r => r.children[1].textContent.startsWith(machine + " /")) && document.querySelector('.ws-table-scroll [data-outcome="failed"]'),fixture);
      await shot(page,`workspace-${fixture.name}-recorded`);
    }
  });
  } finally {
    await stop("metrics-agentB");
    await terminateSessionHosts(`${root}/b/s/agent/sessions`);
  }

}
