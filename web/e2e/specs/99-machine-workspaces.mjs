// 실제 host 권한·내구 설정과 CLI/큐/웹 자동 선택을 함께 검사한다. 고정 sleep 없이 HTTP·DOM·CLI 완료를 기다린다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { aaa, step, browser, webkit, pairPage, inList, domIs, root, shot } from "../lib/harness.mjs";

const api = (page, body) => page.evaluate(async body => {
  const res = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  return res.json();
}, body);
const open = async page => {
  await inList(page, page.getByRole("button", { name: "labA 기기 정보", exact: true, includeHidden: true }));
  await page.getByTestId("machine-info-panel").waitFor();
  await page.getByRole("checkbox", { name: "작업 폴더 자동 배정 허용" }).waitFor();
};
const saved = p => domIs(p, "workspace 저장 완료", () => document.querySelector('[aria-label="workspace 자동 배정"]')?.getAttribute("aria-busy") === "false");

export default async function () {
  await step("workspace 허용: 새 기본 꺼짐·Owner 변경·Reader 조회/거부·양 엔진 데스크톱/휴대폰·다른 기기 갱신", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    aaa("project", "set", "demo", "repos.add", `${root}/app@labA`);
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, name] of [[browser, "chromium"], [wk, "webkit"]]) {
        for (const width of [1280, 390]) {
          // 준비되지 않은 새 workspace는 꺼짐이다. 경로는 기존 설정에서만 읽는다.
          const { page: p } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 });
          try {
            await open(p);
            const dir = p.getByRole("checkbox", { name: "작업 폴더 자동 배정 허용" });
            const repo = p.getByRole("checkbox", { name: "app 자동 배정 허용" });
            assert.equal(await dir.isChecked(), false);
            assert.equal(await repo.isChecked(), false);
            await repo.click(); await saved(p);
            await dir.click(); await saved(p);
            const v = aaa("machine", "workspaces", "labA");
            assert(v.workspaces.every(row => row.enabled));
            assert.deepEqual(v.workspaces.find(row => row.workspace === "app").paths, [`${root}/app`]);
            const reader = (await pairPage(engine, aaa("web", "pair", "--reader").code, { width, height: 844 })).page;
            try {
              await open(reader);
              const readDir = reader.getByRole("checkbox", { name: "작업 폴더 자동 배정 허용" });
              assert.equal(await readDir.isDisabled(), true);
              assert.equal(await readDir.isChecked(), true);
              const deny = { op: "workspace_access_set", project: "demo", machine: "labA", workspace: "작업 폴더", enabled: false, revision: v.revision };
              assert.equal((await api(reader, deny)).err.code, "forbidden");
              await dir.click(); await saved(p);
              await domIs(reader, "다른 기기에서 꺼짐 반영", () => document.querySelector('input[aria-label="작업 폴더 자동 배정 허용"]')?.checked === false);
              assert.equal((await api(p, deny)).err.code, "conflict");
              await shot(p, `workspace-owner-${name}-${width}`);
              await shot(reader, `workspace-reader-${name}-${width}`);
              await repo.click(); await saved(p);
            } finally { await reader.context().close(); }
          } finally { await p.context().close(); }
        }
      }
    } finally { await wk.close(); }
  });

  await step("workspace 배정: CLI 자동 시작 거부·명시 머신 경고·큐 대기 이유·웹 자동 허용 반영·고정 선택 경고", async () => {
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, "## Backlog\n- [ ] 자동 일 (fake) [작업 폴더]\n  자동 배정 시험\n");
    const task = aaa("task", "list").tasks[0];
    assert.throws(() => aaa("thread", "start", "--from-task", task.short_id, "--link-task"), /자동 배정이 허용된/);
    const p = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 844 }, { hash: "#tasks" })).page;
    try {
      await p.locator('.task-row[data-title="자동 일"] .task-pick').click();
      await p.getByTestId("task-handoff-start").click();
      await p.getByTestId("task-start-problem").waitFor();
      assert.match(await p.getByTestId("task-start-problem").textContent(), /자동 배정이 허용된 머신이 없습니다/);
      assert.equal(await p.getByTestId("task-start-machine").inputValue(), "");
      await p.getByTestId("task-start-machine").selectOption("labA");
      await p.getByText("이 머신의 자동 배정 허용은 꺼져 있습니다. 고정 머신 지정을 따라 시작합니다.").waitFor();
      await p.getByTestId("task-start").getByRole("button", { name: "취소", exact: true }).click();
      aaa("task", "reserve", task.short_id);
      aaa("ticker", "tick");
      const q = aaa("task", "queue").entries[0];
      assert.equal(q.state, "queued");
      assert.match(q.reason, /자동 배정이 허용된/);
      // 예약 취소 후 허용된 자동 선택이 실제 thread로 이어진다.
      aaa("task", "unreserve", q.task_id);
      aaa("machine", "workspaces", "labA", "--allow", "작업 폴더");
      const thread = aaa("thread", "start", "--from-task", task.short_id, "--link-task");
      assert.equal(thread.machine, "labA");
      aaa("thread", "resolve", thread.id, "--skip-copy");
    } finally { await p.context().close(); }
  });

  await step("머신 독점 예약·비중: 웹 걸기·풀기, 지정 시작 대기, Reader 잠김", async () => {
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, "## Backlog\n- [ ] 측정 일 (fake) [작업 폴더]\n  예약 주인\n- [ ] 다른 일 (fake@labA) [작업 폴더]\n  예약 대기 시험\n");
    const tasks = aaa("task", "list").tasks;
    const holder = tasks.find(t => t.title === "측정 일"), other = tasks.find(t => t.title === "다른 일");
    const machine = () => aaa("machine", "list").machines.find(m => m.name === "labA");
    const p = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    try {
      await open(p);
      await p.getByLabel("예약 주인").fill(holder.short_id);
      await p.getByLabel("예약 까닭").fill("빌드 측정");
      await p.getByTestId("machine-reserve-form").getByRole("button", { name: "걸기" }).click();
      await p.getByTestId("machine-reservation").waitFor();
      assert.equal(machine().reservation.holder, holder.short_id);
      assert.throws(() => aaa("thread", "start", "--from-task", other.short_id, "--link-task"), /labA 독점 예약/);
      const reader = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 })).page;
      try {
        await open(reader);
        assert.equal(await reader.getByTestId("machine-reservation").getByRole("button", { name: "풀기" }).isDisabled(), true);
        await shot(p, "machine-reservation-owner-390");
      } finally { await reader.context().close(); }
      await p.getByTestId("machine-reservation").getByRole("button", { name: "풀기" }).click();
      await p.getByTestId("machine-reserve-form").waitFor();
      assert.equal(machine().reservation, undefined);
      await p.getByLabel("labA 비중").fill("0");
      await p.getByTestId("machine-weight").getByRole("button", { name: "저장" }).click();
      await domIs(p, "비중 0 반영", () => document.querySelector('[data-testid="machine-weight"] strong')?.textContent === "0 (자동 배정 안 함)");
      assert.equal(machine().weight, 0);
      await p.getByTestId("machine-weight").getByRole("button", { name: "CPU 수로" }).click();
      await domIs(p, "CPU 수로 되돌림", () => document.querySelector('[data-testid="machine-weight"] strong')?.textContent?.endsWith("(CPU 수)") === true);
      assert.equal(machine().weight, undefined);
    } finally { await p.context().close(); }
  });
}
