// 완료 보관과 예약 기록이 어긋난 구판 DB를 실제 host에 열고 두 기기의 같은 목록을 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, step, domIs, browser, pairPage, stop, start, until, shot } from "../lib/harness.mjs";

const row = (page, title) => page.locator(`.task-row[data-title="${title}"]`);
export async function legacyReservations() {
  aaa("project", "create", "demo");
  fs.writeFileSync(`${root}/projects/demo/TASKS.md`, "## Backlog\n- [ ] aaa CI 웹 E54 완료 (fake) [작업 폴더]\n  완료 보고와 착지를 마친 할 일\n- [ ] 연결을 잃은 예약 (fake) [작업 폴더]\n  완료·삭제 근거가 없어 확인할 할 일\n- [ ] 다음 할 일 (me) [작업 폴더]\n  남아 있는 일반 할 일\n");
  const tasks = aaa("task", "list").tasks;
  await stop("host");
  const db = new DatabaseSync(`${root}/s/host/host.db`);
  const put = db.prepare("INSERT INTO task_queue(project,task_id,data) VALUES('demo',?,?)");
  for (const [i, task] of tasks.slice(0, 2).entries()) {
    const q = { task_id: task.id, task, selection: { owner: task.owner, profile: "fake", machine: null, actor: "device:user", at_ms: 1 }, approved_block: task.block_sha256, profile: "fake", model: "fake", effort: "", profile_revision: "old", machine: null, position: i + 1, state: "check", reason: "task가 없어졌다", launch_id: null, thread: null, actor: "device:user", created_ms: 1, updated_ms: 1 };
    put.run(task.id, JSON.stringify(q));
  }
  const task = tasks[0];
  const archived = { task_id: task.id, task, body: `- [ ] ${task.title} (fake) [작업 폴더] <!-- aaa-task:${task.id} -->\n  완료 보고와 착지를 마친 할 일\n`, reason: "보고 [완료]", report: null, completed_ms: 1, restored: false, committed: true, removed_sha256: "", history: [] };
  db.prepare("INSERT INTO task_archive(project,task_id,data) VALUES('demo',?,?)").run(task.id, JSON.stringify(archived));
  db.close();
  fs.writeFileSync(`${root}/projects/demo/TASKS.md`, `## Backlog\n${tasks[2].raw}\n  남아 있는 일반 할 일\n`);
  start("host", "host");
  await until("구판 예약 fixture의 host 준비", () => aaa("task", "list").tasks.length === 1);
  aaa("ticker", "tick");
  return { completed: task.id, missing: tasks[1].id };
}

export default async function ({ page }) {
  let ids, phone;
  await step("구판 완료 예약 준비: 완료 근거와 실제 연결 끊김을 함께 복원한다", async () => {
    ids = await legacyReservations();
    await page.getByTestId("activity-tasks").click();
    await row(page, "다음 할 일").waitFor();
    ({ page: phone } = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { hash: "#tasks", touch: true }));
  }, { always: true });
  await step("완료 예약 제거: 두 기기·새로고침 뒤에도 완료 보관만 남고 실제 연결 끊김은 예약 칩에 센다", async () => {
    for (const [p, device] of [[page, "desktop"], [phone, "phone"]]) {
      await domIs(p, "구판 완료 예약 제거", () => document.querySelectorAll('.task-row[data-kind="reserved"]').length === 1);
      assert.equal(await row(p, "aaa CI 웹 E54 완료").count(), 0);
      await row(p, "연결을 잃은 예약").getByText("연결된 할 일 없음", { exact: true }).waitFor();
      assert.equal(await p.getByTestId("tasks-filter-reserved").locator(".tasks-count").textContent(), "1");
      assert.equal(await p.getByTestId("tasks-filter-all").locator(".tasks-count").textContent(), "2");
      assert.equal(await p.getByTestId("tasks-tab-tasks").locator(".tasks-count").textContent(), "2");
      assert.equal(await p.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, "경고 문구가 화면 폭을 넘지 않는다");
      await shot(p, `task-queue-after-${device}`);
      await p.reload();
      await row(p, "다음 할 일").waitFor();
      assert.equal(await row(p, "aaa CI 웹 E54 완료").count(), 0);
      await p.getByTestId("tasks-filter-reserved").click();
      await row(p, "연결을 잃은 예약").waitFor();
      assert.equal(await row(p, "다음 할 일").count(), 0);
      await p.getByTestId("tasks-tab-completed").click();
      await p.getByTestId(`archive-${ids.completed}`).waitFor();
      await shot(p, `task-queue-archive-${device}`);
    }
    const queue = aaa("task", "queue").entries;
    assert.equal(queue.find(q => q.task_id === ids.completed).state, "finished");
    assert.equal(queue.find(q => q.task_id === ids.missing).state, "check");
  });
  await step("완료 복원: 다른 기기에서도 일반 할 일로 돌아오고 옛 예약은 되살아나지 않는다", async () => {
    aaa("task", "archive", "--restore", ids.completed);
    for (const p of [page, phone]) {
      await p.getByTestId("tasks-tab-tasks").click();
      await p.getByTestId("tasks-filter-all").click();
      const restored = row(p, "aaa CI 웹 E54 완료");
      await restored.waitFor();
      assert.equal(await restored.getAttribute("data-kind"), "task");
    }
    assert.equal(aaa("task", "queue").entries.find(q => q.task_id === ids.completed).state, "finished");
    await phone.close();
  });
}
