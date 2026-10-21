// A2의 화면 owner: 실제 자동화 thread의 기억 사본과 보고/Idle 사건에 따른 끝남 표시.
import fs from "node:fs";
import { root, aaa, start, stop, step, until, domIs, drawerSettled, shot } from "../lib/harness.mjs";
export const labEnv = { AAA_TEST_ROUTINE_SECONDS: "1" };
const project = "auto-memory";
const rules = () => aaa("routine", "list").routines;
const threads = () => aaa("thread", "list").threads;
const call = (p, body) => p.evaluate(async (req) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(req) })).json(), body);
async function refresh(p) { await p.evaluate(() => window.dispatchEvent(new Event("focus"))); }
async function openList(p) {
  await drawerSettled(p);
  const toggle = p.locator(".view [data-testid='drawer-toggle']");
  if ((await toggle.isVisible()) && (await toggle.getAttribute("aria-expanded")) !== "true") { await toggle.click(); await drawerSettled(p); }
}
export default async function ({ page }) {
  let thread;
  await step("기억 사본을 보이는 실제 자동화 thread 준비", async () => {
    // 이 spec의 격리된 host만 다시 읽어 fake profile을 Claude 화면 감지 fixture로 바꾼다.
    await stop("host");
    fs.writeFileSync(`${root}/c/host.toml`, fs.readFileSync(`${root}/c/host.toml`, "utf8").replace('[profiles.fake]\nagent = "custom"', '[profiles.fake]\nagent = "claude"'));
    fs.writeFileSync(`${root}/fake.sh`, `#!/bin/sh
work=".aaa/$AAA_PROJECT-$AAA_THREAD"
mkfifo "$work/turn"
printf '\\033[2J\\033[H────────────────────────────────────────\\n❯ \\n────────────────────────────────────────\\n'
while IFS= read -r line; do
  printf '\\033[2J\\033[H✻ Working… (1s · esc to interrupt)\\n────────────────────────────────────────\\n❯ \\n────────────────────────────────────────\\n'
  read proceed < "$work/turn"
  find "$work/inbox" -name '*.json' -type f -exec basename '{}' \\; > "$work/processed.txt"
  case "$line" in
    *brief.md*) printf '[진행 중] 첫 입력을 살핀다\\n\\n## Report\\n준비\\n' > "$work/report.md" ;;
    complete) printf '판단: 서로 다른 실패를 묶는다\\n다음에 볼 것: 새 실패\\n교훈: 입력 뒤에 보고한다\\n' > "$work/memory/memory.md"; printf '[완료] 입력 분류 끝\\n\\n## Report\\n사본\\n' > "$work/report.md"; read idle < "$work/turn" ;;
  esac
  printf '\\033[2J\\033[H────────────────────────────────────────\\n❯ \\n────────────────────────────────────────\\n'
done
`);
    start("host", "host");
    await until("격리 host와 agent 재연결", () => aaa("machine", "list").machines.some((m) => m.connection === "connected"));
    aaa("project", "create", project);
    fs.appendFileSync(`${root}/projects/${project}/project.toml`, '\nstart_threads = "auto"\n');
    fs.mkdirSync(`${root}/projects/${project}/automations`, { recursive: true });
    fs.writeFileSync(`${root}/projects/${project}/automations/check.toml`, 'title="실패 분류"\n[on.thread]\nevents=["report_ready"]\n[do.start]\ntitle="자동 실패 분류"\ntext="입력을 분류한다"\nprofiles=["fake"]\nmachines=["labA"]\nreuse="open"\nmemory=true\nclose_idle="30m"\n');
    const rule = await until("규칙 범위 준비", () => { aaa("ticker", "tick"); return rules().find((r) => r.name === "check" && r.start_approval_sha256); });
    const result = await call(page, { op: "automation_approve", project, name: "check", approval_sha256: rule.start_approval_sha256 });
    if (result.err) throw new Error(JSON.stringify(result));
    fs.writeFileSync(`${root}/source-task.md`, "새 입력을 만드는 보고\n");
    const human = aaa("thread", "start", project, "--title", "시작 신호", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/source-task.md`);
    const source = await until("원인 thread 작업 화면", () => threads().find((t) => t.id === human.id && t.agent?.activity === "working"));
    fs.writeFileSync(`${source.work_dir}/turn`, "go\n");
    thread = await until("첫 작업 화면", () => threads().find((t) => t.automation === "check" && t.agent?.activity === "working"));
    fs.writeFileSync(`${thread.work_dir}/turn`, "go\n");
    await until("첫 진행 보고", () => threads().find((t) => t.id === thread.id && t.report_hash && t.agent?.activity === "idle"));
    const inputs = fs.readdirSync(`${thread.work_dir}/inbox`).filter((n) => n.endsWith(".json"));
    if (!inputs.length || !inputs.every((n) => /^run-[a-f0-9]{60}\.json$/.test(n))) throw new Error(`실행 입력 이름: ${inputs}`);
    const processed = fs.readFileSync(`${thread.work_dir}/processed.txt`, "utf8").trim().split("\n");
    for (const name of inputs) if (!processed.includes(name)) throw new Error(`처리 영수증에 실행 이름이 없다: ${name}`);
    await page.getByTestId("activity-automations").click();
    await refresh(page);
    await page.getByTestId(`automation-open-${project}-${thread.id}`).waitFor();
    if (await page.getByTestId("automation-closing").count()) throw new Error("진행 중 보고에 끝남 시각");
  });
  await step("보고만 왔을 때는 끝남 표시가 없고 Idle 뒤 생긴다", async () => {
    await page.evaluate(() => {
      window.a2CompletionNotices = [];
      new MutationObserver(() => { for (const d of document.querySelectorAll('.droplet[data-kind="done"]')) { if (d.querySelector('.droplet-title')?.textContent === "자동 실패 분류") window.a2CompletionNotices.push(d.textContent); } }).observe(document.body, { childList: true, subtree: true });
    });
    aaa("thread", "message", project, thread.id, "--text", "complete");
    fs.writeFileSync(`${thread.work_dir}/turn`, "go\n");
    await until("기억 host 사본", () => rules().find((r) => r.name === "check" && r.memory?.text.includes("입력 뒤에 보고")));
    await refresh(page);
    await page.getByTestId(`automation-${project}-check`).getByRole("button", { name: "실패 분류 자세히" }).click();
    const memory = page.getByTestId("automation-memory");
    await memory.waitFor();
    // focus는 조회를 시작할 뿐이다. 앞 판에도 있는 상세 상자가 아니라 새 본문이 그려진 사건을 기다린다.
    await domIs(page, "새 기억 본문", () => document.querySelector('[data-testid="automation-memory"]')?.textContent.includes("입력 뒤에 보고"));
    if (!(await memory.textContent()).includes("입력 뒤에 보고")) throw new Error("기억 본문 없음");
    if (!(await memory.textContent()).includes(thread.id)) throw new Error("마지막 갱신 thread 없음");
    await page.keyboard.press("Escape");
    await page.getByTestId("automation-detail").waitFor({ state: "detached" });
    if (await page.getByTestId("automation-closing").count()) throw new Error("아직 작업 중인데 끝남 표시");
    fs.writeFileSync(`${thread.work_dir}/turn`, "idle\n");
    await until("Idle 뒤 끝남 예정", () => { aaa("ticker", "tick"); return rules().find((r) => r.closing?.some((c) => c.thread === thread.id)); });
    await refresh(page);
    await domIs(page, "끝남 예정 글", () => document.querySelector('[data-testid="automation-closing"]')?.textContent === "30분 뒤 끝남");
    const notices = await page.evaluate(() => window.a2CompletionNotices);
    if (notices.length) throw new Error(`자동화 완료는 앱 안 알림을 내지 않는다: ${notices[0]}`);
  });
  await step("데스크톱·휴대폰 기억과 남은 시간, 넘침 없이 실제 상세 열기", async () => {
    for (const [device, size] of [["desktop", { width: 1280, height: 800 }], ["phone", { width: 390, height: 844 }]]) {
      await page.setViewportSize(size);
      await openList(page);
      await shot(page, `a2-panel-${device}`);
      await page.getByTestId(`automation-${project}-check`).getByRole("button", { name: "실패 분류 자세히" }).click();
      await page.getByTestId("automation-memory").waitFor();
      await domIs(page, "가로 넘침 없음", () => document.documentElement.scrollWidth <= innerWidth);
      await shot(page, `a2-memory-${device}`);
      await page.keyboard.press("Escape");
      await page.getByTestId("automation-detail").waitFor({ state: "detached" });
    }
  });
};
