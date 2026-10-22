// 종료 사건은 실제 PTY·host·agent를 쓴다. 시작 요청의 전달만 보류해 중복 클릭 경계를 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, aaa, step, browser, webkit, pairPage, base, domIs, hashIs, waitOutput, inTime, shot, openSettings } from "../lib/harness.mjs";

export const labEnv = { AAA_TEST_WEB_LOCALHOST: "1" };

export default async function () {
  await step("종료된 coordinator 정리와 본문 상단 다시 띄우기(t-0816, 답 1B): Ctrl+C·정상 종료 뒤 트리에서 사라지고 기록은 남으며, 기본 profile 시작·시작 중 중복 방지·실패 안내·보기 권한·passkey 공존을 Chromium·WebKit 넓은 창·휴대폰에서 확인", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "coordinator_profile", "fake");
    fs.writeFileSync(`${root}/fake.sh`, "#!/bin/sh\ntrap 'exit 130' INT\nprintf 'coordinator-ready\\n'\nwhile IFS= read -r line; do\n  case \"$line\" in exit) exit 0;; esac\ndone\n");
    const panes = () => aaa("pane", "list").panes;
    const safari = await webkit.launch({ headless: true });
    try {
      for (const [engine, name] of [[browser, "chromium"], [safari, "webkit"]]) {
        for (const width of [1280, 390]) {
          const dev = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { at: base.replace("127.0.0.1", "localhost"), serviceWorkers: "block" });
          const p = dev.page;
          let release = () => {};
          try {
            const notice = p.getByTestId("coordinator-notice");
            const start = notice.getByRole("button", { name: "coordinator 띄우기" });
            await notice.waitFor();
            await p.getByTestId("passkey-notice").waitFor();
            await p.locator(".view-content .term-head").waitFor();
            const layout = await notice.evaluate(el => {
              const n = el.getBoundingClientRect(), b = el.querySelector("button").getBoundingClientRect();
              const passkey = document.querySelector('[data-testid="passkey-notice"]').getBoundingClientRect();
              // .view-content는 display:contents라 경계가 없다. 실제 본문 머리의 경계를 잰다.
              return { aboveContent: n.bottom <= document.querySelector(".view-content .term-head").getBoundingClientRect().top, belowPasskey: n.top >= passkey.bottom, fits: n.right <= innerWidth && b.right <= innerWidth, drawer: document.querySelector("#workspace-drawer.is-open") !== null };
            });
            assert.deepEqual(layout, { aboveContent: true, belowPasskey: true, fits: true, drawer: false });
            await shot(p, `91-${name}-${width}-notice`);
            const settings = await openSettings(p, "폰 및 기기");
            assert.equal(await notice.count(), 0, "설정 화면에는 coordinator 시작 안내를 겹치지 않는다");
            await settings.getByRole("button", { name: "설정 닫기" }).click();
            await notice.waitFor();

            // 실제 host 거부를 안내줄에 표시하고 다시 누를 수 있어야 한다.
            let mode = "deny", calls = 0, arrived;
            const held = new Promise(resolve => { arrived = resolve; });
            const resume = new Promise(resolve => { release = resolve; });
            await p.route("**/api/v1/call", async route => {
              const req = route.request().postDataJSON();
              if (req?.op !== "project_open") return route.continue();
              calls++;
              if (mode === "deny") {
                mode = "gate";
                return route.continue({ postData: JSON.stringify({ ...req, profile: "unavailable" }) });
              }
              if (mode === "gate") { mode = "pass"; arrived(); await resume; }
              await route.continue();
            });
            await start.click();
            await notice.getByRole("alert").waitFor();
            assert.match(await notice.getByRole("alert").textContent(), /unavailable/);
            assert.equal(await start.isEnabled(), true);
            assert.equal(panes().some(x => x.coordinator_project && x.state === "running"), false);

            const response = p.waitForResponse(r => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "project_open");
            await start.click();
            await inTime(held, 20_000, "시작 요청 도착");
            const busy = notice.getByRole("button", { name: "띄우는 중" });
            assert.equal(await busy.isDisabled(), true);
            assert.equal(await busy.getAttribute("aria-busy"), "true");
            await busy.evaluate(el => { el.click(); el.click(); });
            assert.equal(calls, 2);
            release();
            const opened = (await (await response).json()).ok;
            assert.equal(opened.started, true);
            assert.equal(opened.profile, "fake");
            await hashIs(p, `#pane=${encodeURIComponent(opened.pane.id)}`);
            await waitOutput(opened.pane.id, "coordinator-ready");
            await notice.waitFor({ state: "detached" });
            assert.equal(panes().filter(x => x.coordinator_project && x.state === "running").length, 1);

            // Ctrl+C로 끝내도 기록을 forget하지 않는다. 트리·기타 어디에도 종료 줄이 없어야 한다.
            aaa("pane", "send-keys", opened.pane.id, "ctrl+c");
            await notice.waitFor();
            assert.equal(panes().find(x => x.id === opened.pane.id)?.state, "exited");
            await domIs(p, "종료된 coordinator 줄 없음", id => !document.querySelector(`.pane-item[data-pane="${id}"]`) && !document.querySelector('[data-origin="coordinator"]'), opened.pane.id);
            await shot(p, `91-${name}-${width}-exited`);
            await p.reload();
            await notice.waitFor();
            assert.equal(await p.getByTestId("pane-demo coordinator").count(), 0);

            // 재시작 뒤 정상 종료도 같은 경계로 정리된다. 옛 pane은 그대로 남는다.
            const againResponse = p.waitForResponse(r => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "project_open");
            await start.click();
            const again = (await (await againResponse).json()).ok;
            assert.equal(again.started, true);
            assert.notEqual(again.pane.id, opened.pane.id);
            await waitOutput(again.pane.id, "coordinator-ready");
            await notice.waitFor({ state: "detached" });
            aaa("pane", "run", again.pane.id, "exit");
            await notice.waitFor();
            const saved = panes();
            assert.equal(saved.find(x => x.id === again.pane.id)?.state, "exited");
            assert.equal(saved.find(x => x.id === opened.pane.id)?.state, "exited");
            assert.equal(await p.getByTestId("pane-demo coordinator").count(), 0);
            assert.equal(await p.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
            assert.deepEqual(dev.errors, []);
          } finally {
            release();
            await p.unrouteAll({ behavior: "wait" });
            await p.context().close();
          }
        }
        const reader = await pairPage(engine, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
        try {
          // 휴대폰 막대는 닫힌 drawer 안에 있다. 표시 여부 대신 실제 연결·목록 수신을 기다린다.
          await domIs(reader.page, "보기 기기 연결과 초기 목록", () => document.querySelector('[data-testid="activity-brand"]')?.getAttribute("data-up") === "on" && document.querySelector('[data-testid="pane-t1"]'));
          assert.equal(await reader.page.getByTestId("coordinator-notice").count(), 0);
          assert.equal(await reader.page.getByTestId("activity-coordinator").count(), 0);
        } finally { await reader.page.context().close(); }
      }
    } finally { await safari.close(); }
  });
}
