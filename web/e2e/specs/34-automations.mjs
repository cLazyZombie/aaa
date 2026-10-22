import { listIds } from "../lib/list-ids.mjs";
// 자동화 보기의 주 경계: 실제 일정 판정 → DB 기록 → CLI/웹, 파일 enabled 편집, Reader, 휴대폰.
// 목록은 사이드바 카드(이름·표지·조건 한 줄)이고, 카드를 누르면 본문에 그 규칙 설정(켜기·기록·승인)이 열린다(t-0628).
import fs from "node:fs";
import { root, aaa, step, browser, pairPage, until, domIs, drawerSettled, shot, usageRoutes } from "../lib/harness.mjs";
export const labEnv = { AAA_TEST_ROUTINE_SECONDS: "1", AAA_TEST_USAGE_REFRESH_MS: "0" };
let original, ruleId;
const file = () => `${root}/projects/auto-demo/routines/check.md`;
export default async function ({ page }) {
  await step("자동화: 활동 막대에 규칙과 임무 조건을 함께 보고 host가 기록한 건너뜀 까닭·최근 기록을 읽는다", async () => {
    aaa("project", "create", "auto-demo");
    original = '+++\n# 보존할 주석\nschedule = "every 1s"\nenabled = true\n+++\n자료를 확인한다.\n';
    fs.writeFileSync(file(), original);
    fs.mkdirSync(`${root}/projects/auto-demo/missions`, { recursive: true });
    fs.writeFileSync(`${root}/projects/auto-demo/missions/watch.md`, '+++\nprofile="fake"\nevents=["git_pushed"]\nrepos=["app"]\nwithdrawal="none"\n+++\n자료를 분류한다.\n');
    await until("coordinator 없는 실제 차례", () => {
      aaa("ticker", "tick");
      return aaa("routine", "runs", "auto-demo", "check").runs.some((r) => r.detail === "coordinator 없음");
    });
    await page.getByTestId("activity-automations").click();
    const row = page.getByTestId("automation-auto-demo-check");
    await row.waitFor();
    ruleId = aaa("routine", "list", "auto-demo").routines.find((r) => r.name === "check").short_id;
    if (!/^a-\d{4,}$/.test(ruleId)) throw new Error(`자동화 번호 ${ruleId}`);
    await listIds(page, '[data-testid="automation-auto-demo-check"]', [ruleId], "ids-automation-desktop");
    // 카드는 두 줄(이름·표지 / 조건 한 줄)이다: 지난 결과·파일·건너뜀 까닭은 본문 설정에 있다
    if ((await row.locator(".automation-card-meta").textContent()) !== "1초마다") throw new Error(`카드 조건 ${await row.textContent()}`);
    if ((await row.textContent()).includes("coordinator 없음")) throw new Error("카드에 지난 결과가 남았다");
    const legacy=page.getByTestId("automation-auto-demo-missions/watch.md");
    await legacy.waitFor();
    await legacy.click();
    const detail = page.getByTestId("automation-detail");
    await domIs(page, "옛 파일 변환 안내", () => document.querySelector('[data-testid="automation-file-error"]')?.textContent.includes("없어짐"));
    await row.click();
    await domIs(page, "건너뜀 기록", () => document.querySelector('[data-testid="automation-detail"] [aria-label="마지막 건너뜀"]')?.textContent.includes("coordinator 없음"));
    await detail.locator(".automation-runs li").first().waitFor();
    if (!(await detail.locator(".automation-runs").textContent()).includes("coordinator 없음")) throw new Error("최근 기록이 없다");
    if (await page.locator(".modal-scrim").count()) throw new Error("설정이 떠 있는 창으로 열렸다");
    await shot(page, "automations-desktop");
  });
  await step("자동화: 토글은 enabled만 바꾸고 주석·일정·본문을 지킨다. 임무 파일 토글도 다른 칸을 바꾸지 않는다", async () => {
    const button = page.getByTestId("automation-detail").getByRole("switch", { name: "auto-demo/check 켜기", exact: true });
    await button.click();
    await domIs(page, "규칙 꺼짐", () => document.querySelector('[data-testid="automation-detail"] [role="switch"]')?.getAttribute("aria-checked") === "false" && document.querySelector('[data-testid="automation-auto-demo-check"]')?.dataset.status === "off");
    await page.getByTestId("automation-detail").getByTestId("automation-close").click();
    await page.getByTestId("automation-detail").waitFor({ state: "hidden" });
    if (fs.readFileSync(file(), "utf8") !== original.replace("enabled = true", "enabled = false")) throw new Error("enabled 외 파일 변경");
  });
  await step("자동화: Reader는 기록을 읽되 토글할 수 없고 휴대폰 drawer 안 카드 → 설정 화면과 토글이 화면 폭 안에 있다", async () => {
    const dev = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, { touch: true });
    const p = dev.page;
    try {
      await p.locator(".view [data-testid='drawer-toggle']").click();
      await p.locator("#workspace-drawer.is-open").waitFor();
      await drawerSettled(p);
      await p.getByTestId("activity-automations").click();
      const row = p.getByTestId("automation-auto-demo-check");
      await row.waitFor();
      const panelFits = await p.evaluate(() => { const panel = document.querySelector(".automations"); return panel.scrollWidth <= panel.clientWidth + 1; });
      if (!panelFits) throw new Error("휴대폰 카드 목록이 옆으로 넘친다");
      await listIds(p, '[data-testid="automation-auto-demo-check"]', [ruleId], "ids-automation-phone");
      await p.setViewportSize({ width: 320, height: 844 });
      await listIds(p, '[data-testid="automation-auto-demo-check"]', [ruleId], "ids-automation-320");
      await shot(p, "automations-phone-list");
      // 카드를 누르면 drawer가 닫히고 본문이 그 설정 화면이다
      await row.click();
      await p.locator("#workspace-drawer.is-open").waitFor({ state: "detached" });
      const toggle = p.getByTestId("automation-detail").getByRole("switch");
      await toggle.waitFor();
      if (!(await toggle.isDisabled())) throw new Error("Reader 토글 활성");
      const forbidden = await p.evaluate(async () => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "routine_toggle", project: "auto-demo", name: "check", on: true }) })).json());
      if (forbidden.err?.code !== "forbidden") throw new Error(`Reader 쓰기 ${JSON.stringify(forbidden)}`);
      await p.getByTestId("automation-detail").locator(".automation-runs li").first().waitFor();
      const geometry = await p.evaluate(() => {
        const body = document.querySelector('[data-testid="automation-detail"] .automation-view-body');
        const button = document.querySelector('[data-testid="automation-detail"] [role="switch"]').getBoundingClientRect();
        return { overflow: body.scrollWidth > body.clientWidth + 1 || document.documentElement.scrollWidth > innerWidth, left: button.left, right: button.right, width: innerWidth };
      });
      if (geometry.overflow || geometry.left < 0 || geometry.right > geometry.width) throw new Error(`휴대폰 폭 ${JSON.stringify(geometry)}`);
      await shot(p, "automations-phone-reader");
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await p.context().close(); }
  });
  await step("자동화 TOML 변환: 원본 보존·임무 조건 분리·새 파일 토글·Reader 변환 금지", async () => {
    const before=fs.readFileSync(file(),"utf8");
    const converted=aaa("automation","convert");
    if (!converted.files.includes("routines/.converted/check.md") || !converted.files.includes("automations/watch.brief.md")) throw new Error(`변환 결과 ${JSON.stringify(converted)}`);
    if (fs.readFileSync(`${root}/projects/auto-demo/routines/.converted/check.md`,"utf8")!==before) throw new Error("변환 원본 변경");
    if (aaa("automation","convert").files.length!==0) throw new Error("변환 재실행이 무음이 아니다");
    const implicit = await page.evaluate(async () => (await fetch("/api/v1/call", {method:"POST", headers:{"content-type":"application/json"}, body:JSON.stringify({op:"automation_convert"})})).json());
    if (implicit.ok?.kind !== "automation_converted" || implicit.ok.files.length !== 0) throw new Error(`프로젝트 생략 변환 ${JSON.stringify(implicit)}`);
    await page.getByRole("button",{name:"다시 읽기",exact:true}).click();
    const row=page.getByTestId("automation-auto-demo-check");
    await page.getByTestId("automation-auto-demo-watch").waitFor();
    if (await page.getByTestId("automation-auto-demo-mission:watch").count()) throw new Error("임무 조건이 새 규칙과 중복으로 남았다");
    if (aaa("routine", "list", "auto-demo").routines.find((r) => r.name === "check").short_id !== ruleId) throw new Error("변환 뒤 번호 변경");
    await listIds(page, '[data-testid="automation-auto-demo-check"]', [ruleId], "ids-automation-converted");
    await row.click();
    await domIs(page,"새 파일 출처",()=>document.querySelector('[data-testid="automation-detail"]')?.textContent.includes("automations/check.toml"));
    const path=`${root}/projects/auto-demo/automations/check.toml`;
    const original=fs.readFileSync(path,"utf8");
    await page.getByTestId("automation-detail").getByRole("switch").click();
    await domIs(page,"TOML 켜짐",()=>document.querySelector('[data-testid="automation-detail"] [role="switch"]')?.getAttribute("aria-checked")==="true");
    if (fs.readFileSync(path,"utf8")!==original.replace("enabled = false","enabled = true")) throw new Error("TOML enabled 외 변경");
    const dev=await pairPage(browser,aaa("web","pair","--reader").code,{width:1280,height:800});
    try {
      const forbidden=await dev.page.evaluate(async()=>(await fetch("/api/v1/call",{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify({op:"automation_convert",project:"auto-demo"})})).json());
      if(forbidden.err?.code!=="forbidden") throw new Error(`Reader 변환 ${JSON.stringify(forbidden)}`);
    } finally {await dev.page.context().close();}
    await shot(page,"automations-toml-desktop");
  });

  await step("자동 시작 승인: Controller는 정규화 범위를 확인하고 Reader는 승인할 수 없다", async () => {
    const machine=aaa("machine","list").machines.find(m=>m.connection==="connected").name;
    const path=`${root}/projects/auto-demo/automations/start.toml`;
    fs.writeFileSync(path,`enabled=false\n[on.schedule]\nevery="10m"\n[do.start]\ntitle="주기 점검"\ntext="자료 확인"\nprofiles=["fake"]\nmachines=[${JSON.stringify(machine)}]\nwait=["timing"]\n`);
    await page.getByRole("button",{name:"다시 읽기",exact:true}).click();
    const row=page.getByTestId("automation-auto-demo-start");
    // 카드는 「승인 필요」 표지만, 승인은 본문 설정의 색 상자에서 한다
    await domIs(page,"승인 필요 카드",()=>document.querySelector('[data-testid="automation-auto-demo-start"]')?.dataset.status==="approve");
    if(await row.getByRole("button",{name:"승인 검토",exact:true}).count()) throw new Error("카드에 승인 단추가 있다");
    const current=aaa("routine","list").routines.find(r=>r.name==="start");
    const dev=await pairPage(browser,aaa("web","pair","--reader").code,{width:1280,height:800});
    try {
      await dev.page.getByTestId("activity-automations").click();
      const reader=dev.page.getByTestId("automation-auto-demo-start");
      await reader.click();
      await dev.page.getByTestId("automation-start-approval").waitFor();
      if(await dev.page.getByTestId("automation-detail").getByRole("button",{name:"승인 검토",exact:true}).count()) throw new Error("Reader 승인 단추가 있다");
      const result=await dev.page.evaluate(async sha=>(await fetch("/api/v1/call",{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify({op:"automation_approve",project:"auto-demo",name:"start",approval_sha256:sha})})).json(),current.start_approval_sha256);
      if(result.err?.code!=="forbidden") throw new Error(`Reader 승인 ${JSON.stringify(result)}`);
    } finally {await dev.page.context().close();}
    await row.click();
    await page.getByTestId("automation-start-approval").getByRole("button",{name:"승인 검토",exact:true}).click();
    const dialog=page.getByRole("alertdialog");
    await dialog.waitFor();
    if(!(await dialog.textContent()).includes(machine)||!(await dialog.textContent()).includes("claude") && !(await dialog.textContent()).includes("fake")) throw new Error("대상 범위가 보이지 않는다");
    await shot(page,"automations-start-scope");
    await dialog.getByRole("button",{name:"승인",exact:true}).click();
    await dialog.waitFor({state:"hidden"});
    await domIs(page,"자동 시작 범위 승인 반영",()=>!document.querySelector('[data-testid="automation-start-approval"]') && document.querySelector('[data-testid="automation-auto-demo-start"]')?.dataset.status!=="approve");
    if(aaa("routine","list").routines.find(r=>r.name==="start").start_approved!==true) throw new Error("승인이 내구 저장되지 않았다");
  });

  await step("사용량 확인 불가: 실제 구독 조회 실패가 규칙 대기 이유와 inbox에 나타난다", async () => {
    usageRoutes["/api/oauth/usage"] = () => [503, {}, {}];
    usageRoutes["/backend-api/wham/usage"] = () => [503, {}, {}];
    await page.evaluate(async () => (await fetch("/api/v1/usage?refresh=1", { cache: "no-store" })).json());
    const layout = aaa("tab", "create", "--machine", "labA", "--cwd", `${root}/projects/auto-demo`, "--label", "usage-coordinator", "--", "/bin/sh");
    const coordinator = layout.tabs.at(-1).root.pane;
    await until("일정 입력을 받을 coordinator pane", () => aaa("pane", "list").panes.some((p) => p.id === coordinator && p.cwd && fs.realpathSync(p.cwd) === fs.realpathSync(`${root}/projects/auto-demo`)));
    fs.mkdirSync(`${root}/projects/auto-demo/automations`, { recursive: true });
    fs.writeFileSync(`${root}/projects/auto-demo/automations/usage.toml`, '[on.schedule]\nevery="1s"\n[do.inbox]\ntext="문턱 뒤 처리"\n[limits]\nusage_session_below=5\n');
    await until("host 사용량 확인 불가 판정", () => {
      aaa("ticker", "tick");
      return aaa("routine", "list").routines.some((r) => r.name === "usage" && r.last_result?.status === "waiting" && r.last_result.detail.startsWith("사용량"));
    });
    const waiting = aaa("routine", "list").routines.find((r) => r.name === "usage");
    if (waiting.last_result?.detail !== "사용량 확인 불가") throw new Error(`확인 불가 이유 ${JSON.stringify(waiting.last_result)}`);
    await page.getByRole("button", { name: "다시 읽기", exact: true }).click();
    await page.getByTestId("automation-auto-demo-usage").click();
    await domIs(page, "확인 불가 대기 표시", () => document.querySelector('[data-testid="automation-summary"]')?.textContent.includes("대기 · 사용량 확인 불가"));
    const inbox = aaa("inbox", "list", "--peek").items.filter((i) => i.subject === "usage");
    if (inbox.length !== 1 || inbox[0].event !== "usage unavailable" || !inbox[0].summary.includes("사용량 확인 불가")) throw new Error(`확인 불가 inbox ${JSON.stringify(inbox)}`);
    await page.getByTestId("automation-auto-demo-usage").scrollIntoViewIfNeeded();
    await shot(page, "automations-usage-unavailable");
  });

}
