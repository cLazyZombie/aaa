// 자동화 보기의 주 경계: 실제 일정 판정 → DB 기록 → CLI/웹, 파일 enabled 편집, Reader, 휴대폰.
import fs from "node:fs";
import { root, aaa, step, browser, pairPage, until, domIs, drawerSettled, shot } from "../lib/harness.mjs";
export const labEnv = { AAA_TEST_ROUTINE_SECONDS: "1" };
let original;
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
    await domIs(page, "건너뜀 기록", () => document.querySelector('[data-testid="automation-auto-demo-check"]')?.textContent.includes("마지막 건너뜀: coordinator 없음"));
    const mission = page.getByTestId("automation-auto-demo-mission:watch");
    await mission.waitFor();
    if (!(await mission.textContent()).includes("임무 승인 필요")) throw new Error("승인 전 임무 조건의 승인 필요 표시가 없다");
    if (!(await mission.textContent()).includes("임무 파일에서") || !(await mission.textContent()).includes("git_pushed")) throw new Error("임무 조건 출처가 없다");
    await row.locator("summary").click();
    await row.locator("ol li").first().waitFor();
    if (!(await row.locator("ol").textContent()).includes("건너뜀: coordinator 없음")) throw new Error("최근 기록이 없다");
    await shot(page, "automations-desktop");
  });
  await step("자동화: 토글은 enabled만 바꾸고 주석·일정·본문을 지킨다. 임무 파일 토글도 다른 칸을 바꾸지 않는다", async () => {
    const button = page.getByRole("switch", { name: "auto-demo/check 켜기", exact: true });
    await button.click();
    await domIs(page, "규칙 꺼짐", () => document.querySelector('[data-testid="automation-auto-demo-check"] [role="switch"]')?.getAttribute("aria-checked") === "false");
    if (fs.readFileSync(file(), "utf8") !== original.replace("enabled = true", "enabled = false")) throw new Error("enabled 외 파일 변경");
    const missionPath = `${root}/projects/auto-demo/missions/watch.md`;
    const before = fs.readFileSync(missionPath, "utf8");
    await page.getByRole("switch", { name: "auto-demo/mission:watch 켜기", exact: true }).click();
    await domIs(page, "임무 조건 꺼짐", () => document.querySelector('[data-testid="automation-auto-demo-mission:watch"] [role="switch"]')?.getAttribute("aria-checked") === "false");
    const after = fs.readFileSync(missionPath, "utf8");
    if (after.replace('enabled = false\n', '') !== before) throw new Error("임무 enabled 외 파일 변경");
  });
  await step("자동화: Reader는 기록을 읽되 토글할 수 없고 휴대폰 drawer 안 목록·토글이 화면 폭 안에 있다", async () => {
    const dev = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, { touch: true });
    const p = dev.page;
    try {
      await p.locator(".view [data-testid='drawer-toggle']").click();
      await p.locator("#workspace-drawer.is-open").waitFor();
      await drawerSettled(p);
      await p.getByTestId("activity-automations").click();
      const row = p.getByTestId("automation-auto-demo-check");
      await row.waitFor();
      const toggle = row.getByRole("switch");
      if (!(await toggle.isDisabled())) throw new Error("Reader 토글 활성");
      const forbidden = await p.evaluate(async () => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "routine_toggle", project: "auto-demo", name: "check", on: true }) })).json());
      if (forbidden.err?.code !== "forbidden") throw new Error(`Reader 쓰기 ${JSON.stringify(forbidden)}`);
      await row.locator("summary").click();
      await row.locator("ol li").first().waitFor();
      const geometry = await p.evaluate(() => {
        const panel = document.querySelector('.automations');
        const button = document.querySelector('[data-testid="automation-auto-demo-check"] [role="switch"]').getBoundingClientRect();
        return { overflow: panel.scrollWidth > panel.clientWidth + 1, left: button.left, right: button.right, width: innerWidth };
      });
      if (geometry.overflow || geometry.left < 0 || geometry.right > geometry.width) throw new Error(`휴대폰 폭 ${JSON.stringify(geometry)}`);
      await shot(p, "automations-phone-reader");
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await p.context().close(); }
  });
  await step("자동화 TOML 변환: 원본 보존·임무 조건 분리·새 파일 토글·Reader 변환 금지", async () => {
    const before=fs.readFileSync(file(),"utf8");
    const converted=aaa("automation","convert");
    if (!converted.files.includes("routines/.converted/check.md") || !converted.files.includes("missions/watch.toml")) throw new Error(`변환 결과 ${JSON.stringify(converted)}`);
    if (fs.readFileSync(`${root}/projects/auto-demo/routines/.converted/check.md`,"utf8")!==before) throw new Error("변환 원본 변경");
    if (aaa("automation","convert").files.length!==0) throw new Error("변환 재실행이 무음이 아니다");
    const implicit = await page.evaluate(async () => (await fetch("/api/v1/call", {method:"POST", headers:{"content-type":"application/json"}, body:JSON.stringify({op:"automation_convert"})})).json());
    if (implicit.ok?.kind !== "automation_converted" || implicit.ok.files.length !== 0) throw new Error(`프로젝트 생략 변환 ${JSON.stringify(implicit)}`);
    await page.getByRole("button",{name:"다시 읽기",exact:true}).click();
    const row=page.getByTestId("automation-auto-demo-check");
    await domIs(page,"새 파일 출처",()=>document.querySelector('[data-testid="automation-auto-demo-check"]')?.textContent.includes("automations/check.toml"));
    await page.getByTestId("automation-auto-demo-watch-events").waitFor();
    if (await page.getByTestId("automation-auto-demo-mission:watch").count()) throw new Error("임무 조건이 새 규칙과 중복으로 남았다");
    const path=`${root}/projects/auto-demo/automations/check.toml`;
    const original=fs.readFileSync(path,"utf8");
    await row.getByRole("switch").click();
    await domIs(page,"TOML 켜짐",()=>document.querySelector('[data-testid="automation-auto-demo-check"] [role="switch"]')?.getAttribute("aria-checked")==="true");
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
    await row.getByRole("button",{name:"승인 검토",exact:true}).waitFor();
    const current=aaa("routine","list").routines.find(r=>r.name==="start");
    const dev=await pairPage(browser,aaa("web","pair","--reader").code,{width:1280,height:800});
    try {
      await dev.page.getByTestId("activity-automations").click();
      const reader=dev.page.getByTestId("automation-auto-demo-start");
      await reader.waitFor();
      if(await reader.getByRole("button",{name:"승인 검토",exact:true}).count()) throw new Error("Reader 승인 단추가 있다");
      const result=await dev.page.evaluate(async sha=>(await fetch("/api/v1/call",{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify({op:"automation_approve",project:"auto-demo",name:"start",approval_sha256:sha})})).json(),current.start_approval_sha256);
      if(result.err?.code!=="forbidden") throw new Error(`Reader 승인 ${JSON.stringify(result)}`);
    } finally {await dev.page.context().close();}
    await row.getByRole("button",{name:"승인 검토",exact:true}).click();
    const dialog=page.getByRole("alertdialog");
    await dialog.waitFor();
    if(!(await dialog.textContent()).includes(machine)||!(await dialog.textContent()).includes("claude") && !(await dialog.textContent()).includes("fake")) throw new Error("대상 범위가 보이지 않는다");
    await shot(page,"automations-start-scope");
    await dialog.getByRole("button",{name:"승인",exact:true}).click();
    await dialog.waitFor({state:"hidden"});
    await domIs(page,"자동 시작 범위 승인 반영",()=>!document.querySelector('[data-testid="automation-auto-demo-start"]')?.textContent.includes("자동 시작 승인 필요"));
    if(aaa("routine","list").routines.find(r=>r.name==="start").start_approved!==true) throw new Error("승인이 내구 저장되지 않았다");
  });

}
