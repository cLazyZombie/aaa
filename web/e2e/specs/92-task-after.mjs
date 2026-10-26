// 예약의 앞 할 일(plan §8.463, t-0829): 예약 확인에서 앞 할 일을 고르면 host가 그 할 일이 완료 보관될 때까지 자리 차례에 넣지 않고,
// 완료 체크가 보관되는 tick에 뒤 할 일을 실제로 기동한다. 순환은 CLI에서 거부한다. 휴대폰 폭의 확인창 넘침도 본다.
// 시간은 기다리지 않는다: host 예약 기록·thread 목록·화면 표지와 `ticker tick`으로 끝낸다.
import fs from "node:fs";
import { root, aaa, until, step, webkit, domIs, shot } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const MD = `# Tasks

## Backlog
- [ ] 시안 그리기 (fake@labA) [작업 폴더]
  시안을 그리고 승인받는다
- [ ] 시안대로 구현 (fake@labA) [작업 폴더]
  승인된 시안대로 만든다
- [ ] 후속 문서 (fake@labA) [작업 폴더]
  구현 뒤 문서를 쓴다
${Array.from({ length: 30 }, (_, i) => `  - 문서 항목 ${i + 1}: 설명이 길어 상세가 화면보다 길다\n`).join("")}`;
const row = (p, title) => p.locator(`.task-row[data-title="${title}"]`);
const pick = (p, title) => row(p, title).locator(".task-pick").click();
const queued = (title) => aaa("task", "queue").entries.find((q) => q.task.title === title);
const shortOf = (title) => aaa("task", "list").tasks.find((t) => t.title === title).short_id;

/** 상세의 「자리 나면 시작 예약」 → 앞 할 일을 고르고 예약 */
async function reserveAfter(p, title, after, shotName) {
  await pick(p, title);
  await p.getByTestId("task-reserve-preview").click();
  const confirm = p.getByTestId("task-reserve-confirm");
  await confirm.waitFor();
  await confirm.getByTestId("task-reserve-after").selectOption({ label: `${shortOf(after)} · ${after}` });
  // 예약 확인은 상세 아래 고정 줄 자리에서 열린다: 메모가 화면보다 길어도 굴리지 않고 「이 내용으로 예약」이 보이고 눌린다(t-1097)
  await domIs(p, "예약 단추가 화면 안", () => {
    const go = document.querySelector('[data-testid="task-reserve-go"]');
    const r = go?.getBoundingClientRect();
    if (!r || r.top < 0 || r.bottom > innerHeight) return false;
    return go.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2));
  });
  if (shotName) await shot(p, shotName);
  await confirm.getByTestId("task-reserve-go").click();
  await confirm.waitFor({ state: "detached" });
}

export default async function ({ page }) {
  let draft;

  await step("예약 확인에서 앞 할 일을 고르면 그 할 일이 완료 보관될 때까지 기동하지 않고 까닭을 보인다", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, MD);
    aaa("task", "list");
    draft = shortOf("시안 그리기");
    await page.getByTestId("activity-tasks").click();
    await reserveAfter(page, "시안대로 구현", "시안 그리기", "task-after-confirm");
    const q = queued("시안대로 구현");
    if (q?.after_short_id !== draft || q.state !== "queued") throw new Error(`앞 할 일 예약 ${JSON.stringify(q)}`);
    aaa("ticker", "tick");
    if (queued("시안대로 구현").reason !== `앞 할 일 ${draft} 완료 대기`) throw new Error(queued("시안대로 구현").reason);
    if (aaa("thread", "list", "demo").threads.length !== 0) throw new Error("앞 할 일이 끝나기 전에 기동했다");
    await domIs(page, "앞 할 일 표시", (d) => document.querySelector('[data-testid="task-queue-after"]')?.textContent.includes(`${d} 완료 뒤 시작`), draft);
    await domIs(page, "기다리는 까닭", (d) => {
      const t = document.querySelector('[data-testid="task-queue-why"]')?.textContent ?? "";
      return t.includes(`앞 할 일 ${d} 완료 대기`) && t.includes("앞 할 일이 완료 보관되면");
    }, draft);
    await domIs(page, "예약 상태 글", () => document.querySelector('[data-testid="task-queue-status"]')?.textContent.includes("예약됨 · 앞 할 일 완료 뒤 시작"));
    await shot(page, "task-after-waiting");
    // 예약 내용 갱신은 지금 앞 할 일을 그대로 고른 채 연다
    await page.getByTestId("task-reserve-preview").click();
    const kept = await page.getByTestId("task-reserve-after").inputValue();
    if (kept !== q.after) throw new Error(`갱신 확인의 앞 할 일 ${kept}`);
    await page.getByTestId("task-reserve-confirm").getByRole("button", { name: "취소", exact: true }).click();
    // 앞 할 일이 이 할 일을 기다리게 하면 서로 영영 시작하지 않는다
    let refused = "";
    try {
      aaa("task", "reserve", "시안 그리기", "--after", shortOf("시안대로 구현"));
    } catch (e) {
      refused = e.message;
    }
    if (!refused.includes("순환")) throw new Error(`순환 예약을 받았다: ${refused}`);
  });

  await step("앞 할 일을 완료 체크하면 같은 tick에 보관되고 뒤 할 일이 실제로 기동한다", async () => {
    aaa("task", "edit", "시안 그리기", "--done");
    aaa("ticker", "tick");
    if (!aaa("task", "archive").entries.some((a) => a.task.title === "시안 그리기")) throw new Error("앞 할 일이 보관되지 않았다");
    const q = await until("뒤 할 일 기동", () => {
      const q = queued("시안대로 구현");
      return q?.thread && ["starting", "running"].includes(q.state) ? q : null;
    });
    const threads = aaa("thread", "list", "demo").threads;
    if (threads.length !== 1 || threads[0].id !== q.thread) throw new Error(`기동한 thread ${JSON.stringify(threads.map((t) => t.id))}`);
    await domIs(page, "진행 표시", () => /시작 중|진행/.test(document.querySelector('[data-testid="task-queue-status"]')?.textContent ?? ""));
    await shot(page, "task-after-started");
  });

  await step("휴대폰 폭: 앞 할 일 고르기가 넘치지 않고, 진행 중인 할 일을 앞에 두면 기다린다", async () => {
    const wk = await webkit.launch();
    try {
      const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
      try {
        await reserveAfter(p, "후속 문서", "시안대로 구현", "task-after-phone-confirm");
        await domIs(p, "가로 넘침 없음", () => document.documentElement.scrollWidth <= window.innerWidth);
        aaa("ticker", "tick");
        const q = queued("후속 문서");
        if (q.reason !== `앞 할 일 ${shortOf("시안대로 구현")} 완료 대기` || q.thread) throw new Error(JSON.stringify(q));
        if (aaa("thread", "list", "demo").threads.length !== 1) throw new Error("진행 중인 앞 할 일 뒤에서 기동했다");
        await shot(p, "task-after-phone-waiting");
      } finally {
        await p.context().close();
      }
    } finally {
      await wk.close();
    }
  });
}
