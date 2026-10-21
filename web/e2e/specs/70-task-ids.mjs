// 실제 host의 ID 이주·웹 편집·CLI 선택자·재시작·복원. 식별 주석까지 포함한 bytes와 내부 ULID를 대조한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, aaa, step, shot, domIs, browser, until, start, stop } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";
import { taskContent } from "../lib/task-content.mjs";

const board = () => aaa("task", "list");
const file = () => `${root}/projects/demo/TASKS.md`;
const row = (page, title) => page.locator(`.task-row[data-title="${title}"]`);
const byId = (id) => board().tasks.find(t => t.short_id === id);

export default async function ({ page }) {
  let id, ulid, other, original;
  await step("할 일 ID 이주: 기존 ULID·메모·CRLF 보존과 웹·context의 같은 번호", async () => {
    aaa("project", "create", "demo");
    const source = "# Tasks\r\n\r\n## Backlog\r\n안내는 그대로\r\n- [ ] 기존 할 일 (me) <!-- aaa-task:01AAAAAAAAAAAAAAAAAAAAAAAA -->\r\n  첫 메모\r\n- [ ] 두 번째 할 일\r\n  다음 메모\r\n";
    fs.writeFileSync(file(), source);
    const b = board();
    id = b.tasks[0].short_id;
    ulid = b.tasks[0].id;
    other = b.tasks[1].short_id;
    assert.equal(id, "d-0001");
    assert.equal(other, "d-0002");
    assert.equal(ulid, "01AAAAAAAAAAAAAAAAAAAAAAAA");
    original = fs.readFileSync(file(), "utf8");
    assert.equal(taskContent(original), taskContent(source));
    assert.ok(original.includes("- [ ] 기존 할 일 (me) <!-- aaa-task:01AAAAAAAAAAAAAAAAAAAAAAAA -->\r\n"));
    assert.equal(board().text, original, "두 번째 읽기는 파일 bytes를 바꾸지 않는다");
    assert.equal(aaa("context", "--peek").tasks[0].short_id, id);
    await page.getByTestId("activity-tasks").click();
    await row(page, "기존 할 일").getByTestId("task-short-id").waitFor();
    assert.equal(await row(page, "기존 할 일").getByTestId("task-short-id").textContent(), id);
    await shot(page, "task-ids-desktop");
  });

  await step("같은 할 일 ID: 웹 제목 변경 뒤 CLI 이동·기록 조회·삭제·복원에서도 같은 ULID", async () => {
    await row(page, "기존 할 일").locator(".task-pick").click();
    const detail = page.getByTestId("task-detail");
    assert.equal(await detail.getByTestId("task-detail-id").textContent(), id);
    await detail.getByTestId("task-field-title").fill("이름을 바꾼 할 일");
    await detail.getByTestId("task-save").click();
    await row(page, "이름을 바꾼 할 일").waitFor();
    assert.equal(byId(id).id, ulid);
    const secondLine = original.split("\r\n").find(l => l.startsWith("- [ ] 두 번째"));
    assert.ok(fs.readFileSync(file(), "utf8").includes(`${secondLine}\r\n  다음 메모\r\n`), "다른 task bytes를 보존한다");
    aaa("task", "move", id, "--list", "다른 목록");
    assert.equal(byId(id).list, "다른 목록");
    assert.ok(aaa("task", "log", id).entries.some(e => e.action === "update"));
    aaa("task", "rm", id);
    assert.equal(byId(id), undefined);
    aaa("task", "add", "새 할 일");
    assert.equal(board().tasks.find(t => t.title === "새 할 일").short_id, "d-0003");
    aaa("task", "restore", id);
    assert.equal(byId(id).id, ulid);
    assert.equal(byId(id).title, "이름을 바꾼 할 일");
    // 제목 입력도 그대로 유효하다.
    aaa("task", "edit", "이름을 바꾼 할 일", "--note", "제목 선택도 지원");
    assert.deepEqual(byId(id).notes, ["제목 선택도 지원"]);
  });

  await step("할 일 ID 재연결과 휴대폰: host 재시작 뒤 같은 번호 검색·표시와 가로 넘침 없음", async () => {
    await stop("host");
    start("host", "host");
    await until("재시작한 host 준비", () => board().tasks.some(t => t.short_id === id));
    assert.equal(byId(id).id, ulid);
    const device = await pairDevice(browser, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
    try {
      for (const width of [320, 390, 768]) {
        await device.page.setViewportSize({ width, height: 844 });
        await device.page.getByTestId("tasks-search").fill(id);
        await domIs(device.page, "ID로 검색한 한 행", () => document.querySelectorAll(".task-row").length === 1);
        assert.equal(await row(device.page, "이름을 바꾼 할 일").getByTestId("task-short-id").textContent(), id);
        assert.ok(await device.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
        await shot(device.page, `task-ids-${width}`);
      }
    } finally { await device.page.close(); }
  });
}
