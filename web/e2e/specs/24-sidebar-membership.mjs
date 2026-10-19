// HTTP 재연결 목록보다 뒤에 온 thread 사건은 먼저 도착해도 보존한다. 실제 adopt·목록 UI의 경계다.
import fs from "node:fs";
import {
  root, aaa, step, browser, pairPage, claudeSh, claudeIdle, waitOutput, inTime, domIs, shot,
} from "../lib/harness.mjs";

export default async function () {
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
}
