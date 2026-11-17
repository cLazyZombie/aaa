// 기획 트리 전송 실패(t-1969): 실제 agent 트리를 저장소별 HTTP로 읽는다. 한 응답을 붙잡고 다른 요청을 끊어도
// 정상 트리가 먼저 나타나며, 실패한 저장소의 오류·지난 트리 보존·사람이 누른 새로 읽기를 Chromium·WebKit으로 확인한다.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { aaa, root, step, browser, webkit, pairPage, inList, shot } from "../lib/harness.mjs";

const deferred = () => { let resolve; const promise = new Promise((r) => { resolve = r; }); return { promise, resolve }; };
const git = (cwd, ...args) => execFileSync("git", ["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: "pipe" });

export const labEnv = { AAA_E2E_FAILURE_DIAGNOSTICS: "1" };

export default async function () {
  const configs = new Map();
  await step("브라우저마다 빈 캐시로 읽을 저장소 준비", async () => {
    aaa("project", "create", "demo");
    for (const engine of ["chromium", "webkit"]) {
      const config = ["schema = 1"];
      for (const kind of ["good", "bad", "slow"]) {
        const name = `${kind}-${engine}`;
        const path = `${root}/${name}`;
        fs.mkdirSync(`${path}/docs`, { recursive: true });
        fs.writeFileSync(`${path}/docs/README.md`, `# ${name} 기획\n\n실제 agent가 읽은 글.\n`);
        git(path, "init", "-q"); git(path, "add", "."); git(path, "commit", "-qm", "기획");
        config.push(`[[workspaces]]\npath = "${path}"\nmachine = "labA"\ndesign = "docs"`);
      }
      configs.set(engine, `${config.join("\n\n")}\n`);
    }
  }, { always: true });

  for (const name of ["chromium", "webkit"]) {
    await step(`기획 HTTP 실패 격리·느린 저장소·지난 트리·수동 복구: ${name}`, async () => {
      fs.writeFileSync(`${root}/projects/demo/aaa.toml`, configs.get(name));
      const wk = name === "webkit" ? await webkit.launch({ headless: true }) : null;
      let p;
      const slow = deferred();
      let slowRequest;
      let failBad = true, holdSlow = true, badCalls = 0;
      try {
        const intercept = async (route) => {
          const req = route.request().postDataJSON();
          if (req?.op !== "design_tree" || req.cached) return route.continue();
          if (req.repo === `bad-${name}`) {
            badCalls++;
            if (failBad) return route.abort("failed");
          }
          if (req.repo === `slow-${name}` && holdSlow) await slow.promise;
          await route.continue();
        };
        ({ page: p } = await pairPage(wk ?? browser, aaa("web", "pair").code, { width: 1280, height: 800 }, {
          serviceWorkers: "block",
          route: ["**/api/v1/call", intercept],
          onPage: (page) => { slowRequest = page.waitForRequest((req) => req.url().endsWith("/api/v1/call") && req.postDataJSON()?.repo === `slow-${name}`); },
        }));
        // 앞 브라우저가 남긴 패널 선택과 무관하게 지금 트리를 보인다.
        const activity = p.getByTestId("activity-design");
        if (await activity.getAttribute("aria-selected") !== "true") await inList(p, "activity-design");
        const row = (repo) => p.locator(`[data-testid="design-repo"][data-repo="${repo}-${name}"]`);
        await slowRequest;
        await row("good").getByTestId("design-doc").waitFor();
        await row("bad").getByRole("alert").waitFor();
        assert.match(await row("bad").textContent(), /기획을 읽지 못했다/);
        assert.equal(await row("slow").getByTestId("design-doc").count(), 0, "느린 저장소는 아직 안 왔다");
        assert.equal(await p.locator('[data-testid="design-panel"] > .banner.err').count(), 0, "전체 오류가 아니다");
        assert.equal(badCalls, 1, "전송 실패를 자동 재시도하지 않는다");
        await row("good").getByTestId("design-doc").click();
        await p.getByTestId("design-body").getByRole("heading", { name: `good-${name} 기획`, exact: true }).waitFor();
        await shot(p, `design-load-partial-${name}`);
        holdSlow = false; slow.resolve();
        await row("slow").getByTestId("design-doc").waitFor();

        failBad = false;
        await p.locator('#sidebar-column [data-testid="design-reload"]').click();
        await row("bad").getByTestId("design-doc").waitFor();
        assert.equal(await row("bad").getByTestId("design-repo-error").count(), 0);
        failBad = true;
        await p.locator('#sidebar-column [data-testid="design-reload"]').click();
        await row("bad").getByTestId("design-repo-error").waitFor();
        assert.equal(await row("bad").getByTestId("design-doc").count(), 1, "지난 트리를 보존한다");
        assert.match(await row("bad").getByTestId("design-repo-error").locator("svg").getAttribute("aria-label"), /기획을 읽지 못했다/);
        assert.equal(await row("good").getByTestId("design-doc").count(), 1);
        assert.equal(badCalls, 3, "각 새로 읽기에 딱 한 번씩 요청했다");
        await shot(p, `design-load-cached-${name}`);
      } finally {
        slow.resolve();
        await p?.context().close();
        await wk?.close();
      }
    });
  }
}
