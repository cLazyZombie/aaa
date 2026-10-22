// 웹 관리 W5c(plan §8.357): 설정 「안전」(실효값·출처, 넷만 바꾸고 yolo 켜기는 한 번 더 확인, 그사이 바뀐 host.toml은 쓰지 않음),
// 「profile」(custom은 고치지 못함, 더하기 → 미리보기의 실행 인자 → 적용, 허용 목록), 「머신」의 작업 폴더(절대 경로만, 점검 안내 →
// 통과한 경로만 전환, 다음 시작 설정만 바뀌고 host.toml은 그대로)와 Codex hook 설치·제거(다른 hook은 그대로), 보기 전용 기기, 휴대폰 390.
// Codex 설정은 실험실 폴더(CODEX_HOME)다: 실제 ~/.codex는 건드리지 않는다.
import fs from "node:fs";
import { isDeepStrictEqual } from "node:util";
import { root, aaa, step, browser, pairPage, shot, openSettings, inTime } from "../lib/harness.mjs";

const codex = `${root}/codex`;
fs.mkdirSync(codex, { recursive: true });
// 사람이 먼저 둔 다른 hook: aaa 설치·제거가 건드리지 않아야 한다
const other = { hooks: { Stop: [{ hooks: [{ type: "command", command: "echo other-hook" }] }] } };
fs.writeFileSync(`${codex}/hooks.json`, JSON.stringify(other));
export const labEnv = { CODEX_HOME: codex };
const hostToml = `${root}/c/host.toml`;
const read = (f) => fs.readFileSync(f, "utf8");
const tab = async (dialog, name) => {
  await dialog.getByRole("tab", { name }).click();
  await dialog.getByRole("tabpanel", { name }).waitFor();
};
const row = (scope, key) => scope.getByTestId("safety-rows").locator(`[data-key="${key}"]`);

export default async function ({ page }) {
  let dialog;
  await step("안전: 실효값·출처, 웹은 넷만 바꾼다(인자·명령 목록은 host 터미널), start_threads auto는 host 설정에 닿는다", async () => {
    aaa("project", "create", "demo");
    dialog = await openSettings(page, "안전");
    const sp = dialog.getByTestId("settings-safety");
    await row(sp, "yolo").getByTestId("safety-source").filter({ hasText: "기본값" }).waitFor();
    for (const key of ["thread_agent_args", "coordinator_agent_args", "routine_commands", "thread_profiles"]) {
      if (await row(sp, key).locator("select").count()) throw new Error(`${key}를 웹에서 바꾸는 칸이 있다`);
      await row(sp, key).getByText("host 터미널에서").waitFor();
    }
    await row(sp, "start_threads").locator("select").selectOption("auto");
    await row(sp, "start_threads").getByTestId("safety-value").filter({ hasText: "auto" }).waitFor();
    await sp.getByTestId("safety-notes").getByText("이제부터 띄우는 thread").waitFor();
    const shown = aaa("safety", "show", "demo").rows.find((r) => r.key === "start_threads");
    if (shown.value !== "auto" || shown.source !== "프로젝트") throw new Error(`host safety: ${JSON.stringify(shown)}`);
  });

  await step("yolo 켜기는 한 번 더 확인하고, 그사이 사람이 고친 host.toml에는 쓰지 않는다", async () => {
    const sp = dialog.getByTestId("settings-safety");
    await row(sp, "yolo").locator("select").selectOption("on");
    const ask = page.getByRole("alertdialog", { name: "demo의 yolo를 켤까요?" });
    await ask.getByText("묻지 않고 명령을 실행하고").waitFor();
    await shot(page, "61-yolo-confirm-desktop");
    await ask.getByRole("button", { name: "yolo 켜기" }).click();
    await ask.waitFor({ state: "detached" });
    await row(sp, "yolo").getByTestId("safety-value").filter({ hasText: "on" }).waitFor();
    if (!/\[safety\.demo\][^[]*yolo = true/.test(read(hostToml))) throw new Error(`host.toml에 yolo가 없다:\n${read(hostToml)}`);
    // 사람이 터미널에서 host.toml을 고친 뒤 화면이 본 판으로 바꾸면 거부된다(파일 그대로)
    fs.appendFileSync(hostToml, "\n# 사람이 고침\n");
    const edited = read(hostToml);
    // 충돌 뒤 재조회 답만 보류한다. 새 rev를 읽기 전에 입력을 다시 열면 옛 판으로 다음 저장을 보낸다.
    let release;
    let reached;
    let finished;
    let reloadError;
    const gate = new Promise((done) => { release = done; });
    const refreshing = new Promise((done) => { reached = done; });
    const handled = new Promise((done) => { finished = done; });
    let held = false;
    const holdReload = async (route) => {
      if (held || route.request().postDataJSON()?.op !== "safety_show") return route.fallback();
      held = true;
      try {
        const response = await route.fetch();
        reached();
        await gate;
        await route.fulfill({ response });
      } catch (e) {
        reloadError = e;
        reached();
      } finally {
        finished();
      }
    };
    await page.route("**/api/v1/call", holdReload);
    try {
      await row(sp, "trust_screens").locator("select").selectOption("user");
      await sp.getByRole("alert").getByText("그사이 host 설정이 바뀌어").waitFor();
      await inTime(refreshing, 20_000, "충돌 뒤 새 판 재조회");
      if (reloadError) throw reloadError;
      if (read(hostToml) !== edited) throw new Error("바뀐 판에 썼다");
      if (!(await row(sp, "trust_screens").locator("select").isDisabled())) throw new Error("새 판을 읽기 전에 다음 저장을 열었다");
      release();
      // 새로 읽은 판에는 쓴다(선택 가능 사건까지 Playwright가 기다린다).
      await row(sp, "trust_screens").locator("select").selectOption("user");
      await row(sp, "trust_screens").getByTestId("safety-value").filter({ hasText: "user" }).waitFor();
      if (!read(hostToml).includes("# 사람이 고침")) throw new Error("사람이 고친 줄이 사라졌다");
    } finally {
      release();
      if (held) await inTime(handled, 20_000, "보류한 재조회 응답 완료");
      await page.unroute("**/api/v1/call", holdReload);
      if (reloadError) throw reloadError;
    }
    await shot(page, "61-safety-desktop");
  });

  await step("profile: custom은 고치지 못하고, 더하기는 미리보기의 실행 인자를 본 뒤 적용한다", async () => {
    await tab(dialog, "profile");
    const pp = dialog.getByTestId("settings-profiles");
    const fake = pp.getByTestId("profile-list").locator('[data-name="fake"]');
    await fake.getByText("host 터미널에서").waitFor();
    if (await fake.getByRole("button").count()) throw new Error("custom profile에 고치기 단추");
    if ((await fake.textContent()).includes("fake.sh")) throw new Error("custom 실행 명령이 화면에 나왔다");
    const before = read(hostToml);
    await pp.getByTestId("profile-add").click();
    const add = page.getByRole("dialog", { name: "profile 더하기" });
    await add.getByTestId("profile-name").fill("e2e-opus");
    if (await add.getByTestId("profile-harness").locator('option[value="custom"]').count()) throw new Error("harness에 custom이 있다");
    await add.getByTestId("profile-model").fill("opus");
    await add.getByTestId("profile-args").fill("--verbose\n--add-dir\n/tmp/a b");
    await add.getByTestId("profile-preview").click();
    const plan = page.getByRole("dialog", { name: "적용할까요?" });
    await plan.getByTestId("plan-after").filter({ hasText: "claude --model opus --verbose --add-dir /tmp/a b" }).waitFor();
    if (read(hostToml) !== before) throw new Error("미리보기가 host.toml에 썼다");
    await shot(page, "61-profile-plan-desktop");
    await plan.getByTestId("profile-apply").click();
    await plan.waitFor({ state: "detached" });
    await pp.getByTestId("profile-done").getByText("profile `e2e-opus`를 더했다").waitFor();
    await pp.getByTestId("profile-list").locator('[data-name="e2e-opus"]').waitFor();
    const r = aaa("profile", "resolve", "e2e-opus");
    if (JSON.stringify(r.args) !== JSON.stringify(["--model", "opus", "--verbose", "--add-dir", "/tmp/a b"])) throw new Error(`host profile: ${JSON.stringify(r)}`);
  });

  await step("허용 목록: 이 프로젝트 thread는 고른 profile만(미리보기 → 적용)", async () => {
    const pp = dialog.getByTestId("settings-profiles");
    const allow = pp.getByTestId("allow-thread");
    // 실험실은 thread를 fake로 좁혀 둔다(모든 프로젝트 기본): 이 프로젝트는 e2e-opus만
    await allow.getByText("지금 fake").waitFor();
    await allow.getByRole("checkbox", { name: "모든 profile" }).setChecked(false);
    await allow.getByRole("checkbox", { name: "e2e-opus" }).check();
    await allow.getByRole("checkbox", { name: "fake" }).uncheck();
    await allow.getByRole("button", { name: "미리보기…" }).click();
    const plan = page.getByRole("dialog", { name: "적용할까요?" });
    await plan.getByText("thread는 `demo`에서 e2e-opus를 쓸 수 있다").waitFor();
    await plan.getByTestId("profile-apply").click();
    await plan.waitFor({ state: "detached" });
    await allow.getByText("지금 e2e-opus").waitFor();
    const shown = aaa("safety", "show", "demo").rows.find((r) => r.key === "thread_profiles");
    if (shown.value !== "e2e-opus") throw new Error(`host 허용 목록: ${JSON.stringify(shown)}`);
    await shot(page, "61-profiles-desktop");
  });

  await step("작업 폴더: 절대 경로만, 점검(검사 파일 안내) 통과한 경로만 전환, 다음 시작 설정만 바뀐다", async () => {
    await tab(dialog, "머신");
    const wr = dialog.getByTestId("work-root");
    await wr
      .getByTestId("work-root-next")
      .filter({ hasText: `${root}/work` })
      .waitFor();
    const hostBefore = read(hostToml);
    await wr.getByTestId("work-root-path").fill("work2");
    if (!(await wr.getByTestId("work-root-check").isDisabled())) throw new Error("상대 경로로 점검이 열렸다");
    const next = `${root}/work2`;
    await wr.getByTestId("work-root-path").fill(next);
    if (!(await wr.getByTestId("work-root-set").isDisabled())) throw new Error("점검 전에 전환이 열렸다");
    await wr.getByTestId("work-root-check").click();
    const ask = page.getByRole("alertdialog", { name: "작업 폴더를 점검할까요?" });
    await ask.getByText("검사 파일을 만듭니다").waitFor();
    await ask.getByRole("button", { name: "점검" }).click();
    await ask.waitFor({ state: "detached" });
    await wr.getByTestId("work-root-checked").waitFor();
    if (!fs.existsSync(next)) throw new Error("점검이 없는 폴더를 만들지 않았다");
    // 점검한 뒤 경로를 바꾸면 전환이 닫힌다
    await wr.getByTestId("work-root-path").fill(`${root}/work3`);
    if (!(await wr.getByTestId("work-root-set").isDisabled())) throw new Error("점검하지 않은 경로로 전환이 열렸다");
    await wr.getByTestId("work-root-path").fill(next);
    await wr.getByTestId("work-root-set").click();
    const set = page.getByRole("alertdialog", { name: "작업 폴더를 바꿀까요?" });
    await set.getByRole("button", { name: "전환" }).click();
    await set.waitFor({ state: "detached" });
    await wr.getByTestId("work-root-note").getByText("다음 agent 시작부터").waitFor();
    await wr.getByTestId("work-root-next").filter({ hasText: next }).waitFor();
    await wr.getByTestId("work-root-restart").waitFor();
    if (!read(`${root}/c/agent.local.toml`).includes(next)) throw new Error("agent 다음 시작 설정에 없다");
    if (read(hostToml) !== hostBefore) throw new Error("작업 폴더 전환이 host.toml을 바꿨다");
    await shot(page, "61-work-root-desktop");
  });

  await step("Codex hook: 설치 → 설치됨(다른 hook 그대로) → 제거 → 없음", async () => {
    const hk = dialog.getByTestId("hook");
    await hk.getByTestId("hook-codex").filter({ hasText: "없음" }).waitFor();
    await hk.getByTestId("hook-toggle").click();
    let ask = page.getByRole("alertdialog", { name: "labA에 Codex hook을 설치할까요?" });
    await ask.getByRole("button", { name: "설치" }).click();
    await ask.waitFor({ state: "detached" });
    await hk.getByTestId("hook-codex").filter({ hasText: "설치됨" }).waitFor();
    const hooks = JSON.parse(read(`${codex}/hooks.json`));
    if (!JSON.stringify(hooks.hooks.SessionStart).includes("hook --agent codex")) throw new Error(`설치된 hook이 없다: ${JSON.stringify(hooks)}`);
    if (!isDeepStrictEqual(hooks.hooks.Stop, other.hooks.Stop)) throw new Error("다른 hook이 바뀌었다");
    await shot(page, "61-hook-desktop");
    await hk.getByTestId("hook-toggle").click();
    ask = page.getByRole("alertdialog", { name: "labA의 Codex hook을 제거할까요?" });
    await ask.getByRole("button", { name: "제거" }).click();
    await ask.waitFor({ state: "detached" });
    await hk.getByTestId("hook-codex").filter({ hasText: "없음" }).waitFor();
    const after = JSON.parse(read(`${codex}/hooks.json`));
    if (JSON.stringify(after).includes("hook --agent codex")) throw new Error("제거 뒤 aaa hook이 남았다");
    if (!isDeepStrictEqual(after.hooks.Stop, other.hooks.Stop)) throw new Error("제거가 다른 hook을 바꿨다");
  });

  await step("보기 전용 기기: 안전·profile·작업 폴더를 보지 않는다", async () => {
    const { page: rd } = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 });
    const d = await openSettings(rd, "안전");
    await d.getByText("보기 전용 기기에는 safety 설정을 보이지 않는다").waitFor();
    await tab(d, "머신");
    if (await d.getByTestId("work-root").count()) throw new Error("보기 전용 기기에 작업 폴더가 보인다");
    await rd.close();
  });

  await step("휴대폰 390: profile·안전 쪽이 폭 안에", async () => {
    const { page: ph } = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    let d = await openSettings(ph, "profile");
    await d.getByTestId("profile-list").locator('[data-name="e2e-opus"]').waitFor();
    const wide = await ph.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    if (wide) throw new Error("profile 쪽이 가로로 넘친다");
    await shot(ph, "61-profiles-phone");
    await ph.getByRole("button", { name: "설정으로 돌아가기" }).click();
    await d.getByRole("tab", { name: "안전" }).click();
    d = ph.getByRole("region", { name: "설정" });
    await d.getByTestId("settings-safety").getByTestId("safety-rows").waitFor();
    await shot(ph, "61-safety-phone");
    await ph.close();
  });
}
