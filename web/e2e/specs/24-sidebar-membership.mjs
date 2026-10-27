// HTTP 재연결 목록보다 뒤에 온 thread 사건은 먼저 도착해도 보존한다. 실제 adopt·목록 UI의 경계다.
import fs from "node:fs";
import assert from "node:assert/strict";
import {
  root, aaa, step, browser, webkit, pairPage, claudeSh, claudeIdle, waitOutput, inTime, domIs, shot,
} from "../lib/harness.mjs";

// host의 실제 완전 목록만 보류한다. 머신/pane state는 먼저 화면까지 도착한다.
async function heldList(which, viewport, check) {
  const captured = Promise.withResolvers();
  const release = Promise.withResolvers();
  const finished = Promise.withResolvers();
  let held = false;
  const device = await pairPage(which, aaa("web", "pair").code, viewport, {
    serviceWorkers: "block",
    route: ["**/api/v1/call", async route => {
      if (route.request().postDataJSON()?.op !== "threads" || held) return route.continue();
      held = true;
      try {
        const response = await route.fetch();
        captured.resolve(await response.json());
        const fail = await release.promise;
        if (fail) await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ err: { code: "unavailable", message: "목록 조회 시험 오류" } }) });
        else await route.fulfill({ response });
      } catch (error) {
        captured.reject(error);
        throw error;
      } finally { finished.resolve(); }
    }],
  });
  try {
    const snapshot = await inTime(captured.promise, 20_000, "첫 thread 목록 보류");
    const pg = device.page;
    assert.equal(await pg.getByTestId("other-group").count(), 0, "소속 미확정을 기타로 그리지 않는다");
    assert.equal(await pg.getByTestId("work-list-loading").count(), 1, "pane state를 받아도 첫 thread 목록 전에는 불러오는 중이다");
    assert.equal(await pg.locator(".machine-list [data-testid^='pane-']").count(), 0, "미확정 pane을 고정 카드에도 그리지 않는다");
    await check(pg, snapshot.ok, fail => release.resolve(fail));
    if (device.errors.length) throw new Error(device.errors.join("; "));
  } finally {
    release.resolve();
    await inTime(finished.promise, 20_000, "보류한 목록 전달 완료");
    await device.page.context().close();
  }
}

export default async function () {
  await step("처음 받은 빈 thread 목록도 완료이며 그때부터 일반 pane을 기타에 표시한다", async () => {
    await heldList(browser, { width: 1280, height: 800 }, async (pg, snapshot, release) => {
      assert.equal(snapshot.threads.length, 0);
      release();
      await pg.getByTestId("pane-t1").waitFor();
      assert.equal(await pg.getByTestId("work-list-loading").count(), 0);
      assert.equal(await pg.getByTestId("other-group").count(), 1);
    });
  });

  await step("첫 thread 목록 실패는 빈 목록이나 무한 로딩 대신 조회 오류를 표시한다", async () => {
    await heldList(browser, { width: 1280, height: 800 }, async (pg, _snapshot, release) => {
      release(true);
      await pg.getByTestId("work-list-error").waitFor();
      assert.match(await pg.getByTestId("work-list-error").textContent(), /목록 조회 시험 오류/);
      assert.equal(await pg.getByTestId("work-list-loading").count(), 0);
      assert.equal(await pg.getByTestId("other-group").count(), 0);
      await pg.reload();
      await pg.getByTestId("pane-t1").waitFor();
      assert.equal(await pg.getByTestId("work-list-error").count(), 0);
    });
  });

  await step("adopt: 늦은 thread 목록이 프로젝트 소속을 지워 기타로 돌리지 않는다", async () => {
    aaa("project", "create", "adopted");
    const cwd = `${root}/work/existing`;
    fs.mkdirSync(cwd, { recursive: true });
    const pane = aaa("tab", "create", "--machine", "labA", "--label", "기존 Claude", "--cwd", cwd,
      "--", claudeSh, "-c", `stty -echo; ${claudeIdle}; echo adopt-ready; exec cat > /dev/null`).tabs.at(-1).root.pane;
    await waitOutput(pane, "adopt-ready");
    const captured = Promise.withResolvers();
    const release = Promise.withResolvers();
    let held = false;
    const device = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, {
      serviceWorkers: "block",
      route: ["**/api/v1/call", async (route) => {
        if (route.request().postDataJSON()?.op !== "threads" || held) return route.continue();
        held = true;
        const response = await route.fetch();
        captured.resolve(await response.json());
        await release.promise;
        await route.fulfill({ response });
      }],
    });
    try {
      const snapshot = await inTime(captured.promise, 20_000, "adopt 전 thread 목록");
      if (snapshot.ok?.threads?.length !== 0) throw new Error(`빈 목록이 아니다: ${JSON.stringify(snapshot)}`);
      const t = aaa("thread", "adopt", "adopted", "--pane", pane, "--title", "프로젝트에 데려온 작업");
      const pg = device.page;
      const row = pg.getByTestId(`thread-adopted-${t.id}`);
      await row.waitFor();
      assert.equal(await pg.getByTestId("work-list-loading").count(), 1, "thread 사건 하나가 첫 전체 목록 완료를 뜻하지 않는다");
      if (aaa("pane", "list").panes.find((p) => p.id === pane)?.role != null) throw new Error("adopt는 원래 pane role을 바꾸지 않는다");
      const before = Number(await pg.locator(".app").getAttribute("data-threads-sync"));
      release.resolve();
      await domIs(pg, "이전 thread 목록 처리 완료", (n) => Number(document.querySelector(".app")?.dataset.threadsSync) > n, before);
      await shot(pg, "adopt-membership");
      if (await row.count() !== 1) throw new Error("adopt 사건 뒤 도착한 옛 목록이 thread를 지워 pane이 기타로 돌아갔다");
      if (await pg.getByTestId("pane-기존 Claude").count() !== 0) throw new Error("adopt pane이 기타에도 중복된다");
      if (device.errors.length) throw new Error(device.errors.join("; "));
    } finally {
      release.resolve();
      await device.page.context().close();
    }
  });

  async function firstThreadList(which, viewport) {
    const t = aaa("thread", "list", "adopted").threads[0];
    await heldList(which, viewport, async (pg, snapshot, release) => {
      assert(snapshot.threads.some(row => row.project === t.project && row.id === t.id));
      release();
      await pg.getByTestId(`thread-adopted-${t.id}`).waitFor({ state: "attached" });
      await pg.getByTestId("pane-t1").waitFor({ state: "attached" });
      assert.equal(await pg.getByTestId("work-list-loading").count(), 0);
      assert.equal(await pg.getByTestId("pane-기존 Claude").count(), 0);
    });
  }
  await step("Chromium 첫 작업 목록: thread 목록보다 빠른 pane은 기타를 거치지 않고 작업 묶음에 나타난다", async () => {
    await firstThreadList(browser, { width: 1280, height: 800 });
  });
  await step("WebKit 휴대폰 첫 작업 목록: thread 목록보다 빠른 pane은 기타를 거치지 않고 작업 묶음에 나타난다", async () => {
    const wk = await webkit.launch({ headless: true });
    try { await firstThreadList(wk, { width: 390, height: 844 }); }
    finally { await wk.close(); }
  });
}
