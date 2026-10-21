// 실제 host의 승인판·차례·실제 기동·완료 보관을 데스크톱/휴대폰과 다른 기기에서 관측한다.
import fs from "node:fs";
import { root, aaa, until, step, browser, domIs, shot, inList } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";
const row=(p,title)=>p.locator(`.task-row[data-title="${title}"]`);
const pick=(p,title)=>row(p,title).locator(".task-pick").click();
const queued=()=>aaa("task","queue").entries;
export default async function ({ page }) {
  let busy,one,two,phone;
  await step("예약 내용 확인과 등록: 실제 한도가 찬 동안 기동하지 않고 다른 기기에도 같은 승인판을 보인다",async()=>{
    aaa("project","create","demo");aaa("project","set","demo","thread_profile","fake");aaa("project","set","demo","max_parallel_threads","1");
    fs.writeFileSync(`${root}/queue-busy.md`,"사람을 기다림");
    busy=aaa("thread","start","demo","--title","자리 보유","--machine","labA","--profile","fake","--kind","dir","--task-file",`${root}/queue-busy.md`).id;
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`,"## Backlog\n- [ ] 웹 예약 (fake@labA) [작업 폴더]\n  독립 작업을 끝내고 결과를 쓴다\n- [ ] 다음 예약 (fake@labA) [작업 폴더]\n  독립 작업을 끝내고 결과를 쓴다\n");aaa("ticker","tick");
    await page.getByTestId("activity-tasks").click();
    await pick(page,"웹 예약");await page.getByTestId("task-reserve-preview").click();
    await page.getByTestId("task-reserve-confirm").waitFor();
    if(!(await page.getByTestId("task-reserve-confirm").textContent()).includes("독립 작업을 끝내고 결과를 쓴다"))throw new Error("예약할 내용을 보여 주지 않았다");
    await page.getByTestId("task-reserve-go").click();
    await domIs(page,"예약 저장",()=>document.querySelector('[data-testid="task-queue-controls"]')?.dataset.state==="queued");
    one=queued().find(q=>q.task.title==="웹 예약").task_id;
    await page.getByTestId("task-detail-close").click();
    await pick(page,"다음 예약");await page.getByTestId("task-reserve-preview").click();await page.getByTestId("task-reserve-go").click();
    await domIs(page,"둘째 예약 저장",()=>document.querySelector('[data-testid="task-queue-controls"]')?.dataset.state==="queued");
    two=queued().find(q=>q.task.title==="다음 예약").task_id;
    aaa("ticker","tick");if(aaa("thread","list","demo").threads.length!==1)throw new Error("한도가 찬 동안 새 기동을 보냈다");
    const device=await pairDevice(browser,aaa("web","pair").code,"iphone",{hash:"#tasks"});phone=device.page;
    await pick(phone,"다음 예약");await phone.getByTestId("task-queue-status").waitFor();
    if(!(await phone.getByTestId("task-queue-status").textContent()).includes("예약 2"))throw new Error("다른 기기의 순번이 다르다");
    await shot(phone,"queue-phone-confirmed");
  });
  await step("휴대폰 예약 순서와 취소: 맨 앞으로 이동한 차례가 새로고침·다른 기기에도 유지된다",async()=>{
    await phone.getByRole("button",{name:"예약 맨 앞으로"}).click();
    await domIs(phone,"순번 저장",()=>document.querySelector('[data-testid="task-queue-status"]')?.textContent.includes("예약 1"));
    if(queued().filter(q=>q.state==="queued")[0].task_id!==two)throw new Error("host 예약 순서가 바뀌지 않았다");
    await phone.reload();await pick(phone,"다음 예약");await phone.getByTestId("task-queue-status").waitFor();
    await phone.getByRole("button",{name:"예약 취소",exact:true}).click();
    await domIs(phone,"취소 저장",()=>document.querySelector('[data-testid="task-queue-controls"]')?.dataset.state==="cancelled");
    if(queued().find(q=>q.task_id===two).state!=="cancelled")throw new Error("취소가 host에 남지 않았다");
    await page.getByTestId("task-detail-close").click();await pick(page,"웹 예약");
    await shot(page,"queue-desktop-wait");
  });
  await step("병합 대기·병합 중: 보고 사건으로 예약을 시작하고 할 일·thread 앞에 같은 표지를 보인다", async () => {
    const report = async (id, tag, stage) => {
      const t = aaa("thread", "show", "demo", id);
      fs.writeFileSync(`${t.work_dir}/report.md.tmp`, `${tag} 전체 확인과 착지\n\n## Next\n- 착지 창 대기\n`);
      fs.renameSync(`${t.work_dir}/report.md.tmp`, `${t.work_dir}/report.md`);
      await until("host의 보고 단계 회수", () => aaa("thread", "show", "demo", id).stage === stage);
    };
    await report(busy, "[병합 준비됨]", "merge_ready");
    const tid = await until("병합 대기 제외 뒤 예약 기동", () => queued().find(q => q.task_id === one)?.thread);
    await until("기동 thread의 첫 보고", () => aaa("thread", "show", "demo", tid).report_hash);
    await report(tid, "[착지]", "landing");
    if (queued().find(q => q.task_id === one).stage !== "landing") throw new Error("예약과 thread의 단계가 다르다");
    await page.getByTestId("task-detail-close").click();
    await phone.getByTestId("task-detail-close").click();
    for (const [p, name] of [[page, "desktop"], [phone, "phone"]]) {
      const badge = row(p, "웹 예약").locator('[data-stage="landing"]');
      await badge.waitFor();
      if ((await badge.textContent()) !== "병합 중") throw new Error("착지 표지 이름");
      const waiting = p.getByTestId(`tasks-unlinked-${busy}`).locator('[data-stage="merge_ready"]');
      await waiting.waitFor();
      if ((await waiting.textContent()) !== "병합 대기") throw new Error("병합 표지 이름");
      const colors = await p.evaluate(() => {
        const a = document.querySelector('[data-stage="merge_ready"]');
        const b = document.querySelector('[data-stage="landing"]');
        return [getComputedStyle(a).color, getComputedStyle(b).color];
      });
      if (colors[0] === colors[1]) throw new Error("두 단계의 색이 같다");
      await shot(p, `stages-${name}-tasks`);
    }
    await inList(page, `thread-demo-${busy}`);
    await page.getByTestId(`thread-demo-${busy}`).locator('[data-stage="merge_ready"]').waitFor();
    await page.getByTestId(`thread-demo-${tid}`).locator('[data-stage="landing"]').waitFor();
    await page.evaluate(() => location.hash = "#threads");
    await page.getByTestId("all-threads").waitFor();
    await page.getByTestId(`project-thread-${busy}`).locator('[data-stage="merge_ready"]').waitFor();
    await page.getByTestId(`project-thread-${tid}`).locator('[data-stage="landing"]').waitFor();
    await shot(page, "stages-desktop-threads");
    await page.getByTestId("activity-tasks").click();
    await pick(page, "웹 예약");
    await page.getByTestId("task-queue-status").locator('[data-stage="landing"]').waitFor();
    // 마지막 보관 단계는 원래 열린 상세에서 시작한다. phone도 취소한 둘째 task의 상세로 돌린다.
    await pick(phone, "다음 예약");
    // 착지는 완료 보관의 근거가 아니다. 실제 완료 보고를 회수한 뒤 원래 resolve/보관 keeper를 잇는다.
    await report(tid, "[완료]", undefined);
    await row(page, "웹 예약").locator('[data-stage="landing"]').waitFor({ state: "detached" });
  });
  await step("resolve 사건 뒤 실제 기동과 완료 보관: 새로고침 뒤 같은 ID로 복원하며 다시 시작하지 않는다",async()=>{
    aaa("thread","resolve","demo",busy);
    const tid=await until("예약의 thread 기동",()=>queued().find(q=>q.task_id===one)?.thread);
    await until("예약의 보고 수신",()=>aaa("thread","show","demo",tid).report_hash);
    const task=aaa("task","list").tasks.find(t=>t.id===one);
    if(task?.thread!==tid)throw new Error("실제 기동과 task 연결이 다르다");
    aaa("thread","resolve","demo",tid);aaa("ticker","tick");
    await row(page,"웹 예약").waitFor({state:"detached"});
    // 활성 task가 사라지면 손대지 않은 상세도 닫힌다. 닫힌 상세의 단추를 누르지 않는다.
    await page.getByTestId("task-detail").waitFor({state:"detached"});
    await phone.getByTestId("task-detail-close").click();await phone.getByTestId("task-archive-open").click();
    const stored=phone.getByTestId(`archive-${one}`);await stored.waitFor();await shot(phone,"queue-phone-archive");
    await stored.getByRole("button",{name:"활성 목록으로 복원"}).click();await row(phone,"웹 예약").waitFor();
    await page.reload();await row(page,"웹 예약").waitFor();
    const restored=aaa("task","list").tasks.find(t=>t.id===one);
    if(!restored||restored.thread!==null||queued().some(q=>q.task_id===one&&["queued","starting"].includes(q.state)))throw new Error("복원이 새 실행을 승인했다");
    await phone.close();
  });
}
