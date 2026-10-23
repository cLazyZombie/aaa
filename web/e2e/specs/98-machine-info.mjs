// 실제 agent의 표시 정보와 구판·끊김 화면, 네 브라우저 조합의 오른쪽 패널.
import assert from "node:assert/strict";
import { aaa, step, browser, webkit, pairPage, inList, openSettings, until, shot } from "../lib/harness.mjs";

export default async function () {
  await step("기기 정보: 실제 수집·판 표시·오른쪽 패널·휴대폰·Esc·전환·구판·끊김 (Chromium·WebKit 1280·390)", async () => {
    const info = await until("실제 agent 정보", () => aaa("machine", "list").machines.find(m => m.name === "labA")?.health?.info);
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, name] of [[browser, "chromium"], [wk, "webkit"]]) {
        for (const width of [1280, 390]) {
          const route = ws => {
            const server = ws.connectToServer();
            ws.onMessage(m => server.send(m));
            server.onMessage(m => {
              if (typeof m === "string") {
                const e = JSON.parse(m);
                if (e.ev === "state") {
                  const real = e.machines.find(m => m.name === "labA");
                  e.machines.push({ ...real, id: "info-old", name: "old-agent", local: false, connection: "disconnected", health: null, software: null });
                  e.machines.push({ ...real, id: "info-offline", name: "offline-agent", local: false, connection: "disconnected" });
                  m = JSON.stringify(e);
                }
              }
              ws.send(m);
            });
          };
          const p = (await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { wsRoute: [/\/api\/v1\/events/, route] })).page;
          try {
            const open = async machine => {
              await inList(p, p.getByRole("button", { name: `${machine} 기기 정보`, exact: true, includeHidden: true }));
              await p.getByRole("heading", { name: machine, exact: true }).waitFor();
            };
            const panel = p.getByTestId("machine-info-panel");
            await open("labA");
            const box = await panel.boundingBox();
            assert.equal(Math.round(box.x + box.width), width);
            assert.equal(Math.round(box.width), width === 390 ? 390 : 420);
            assert.equal(await panel.locator('[data-label="작업 폴더"] dd').textContent(), info.work_root);
            assert.equal(await panel.locator('[data-label="CPU 모델"] dd').textContent(), info.cpu_model ?? "모름");
            assert.equal(await panel.locator('[data-label="Claude Code"] dd').textContent(), info.claude ?? "모름");
            assert.equal(await panel.locator('[data-label="Codex"] dd').textContent(), info.codex ?? "모름");
            assert.notEqual(await panel.locator('[data-label="정보 수집"] dd').textContent(), "모름");
            await p.keyboard.press("Escape");
            await panel.waitFor({ state: "detached" });
            for (const palette of ["amber", "catppuccin", "github-dark"]) {
              const settings = await openSettings(p, "모양");
              await settings.locator("#settings-palette").selectOption(palette);
              await settings.getByRole("button", { name: "설정 닫기" }).click();
              await open("labA");
              assert.equal(await panel.evaluate(el => el.scrollWidth > el.clientWidth), false);
              await shot(p, `machine-info-${name}-${width}-${palette}`);
              await panel.getByRole("button", { name: "기기 정보 닫기" }).click();
            }
            await open("labA");
            if (width === 1280) {
              // 패널을 닫지 않고 왼쪽 목록의 다른 머신을 고른다.
              await p.getByRole("button", { name: "old-agent 기기 정보", exact: true }).click();
            } else {
              await panel.getByRole("button", { name: "기기 정보 닫기" }).click();
              await open("old-agent");
            }
            await panel.getByRole("heading", { name: "old-agent", exact: true }).waitFor();
            assert.equal(await panel.locator('[data-label="Codex"] dd').textContent(), "모름");
            await panel.getByText("연결이 끊겼습니다. 마지막으로 받은 정보입니다.").waitFor();
            await panel.getByRole("button", { name: "기기 정보 닫기" }).click();
            await open("offline-agent");
            assert.notEqual(await panel.locator('[data-label="정보 수집"] dd').textContent(), "모름");
            assert.notEqual(await panel.locator('[data-label="작업 폴더"] dd').textContent(), "모름");
            await panel.getByRole("button", { name: "기기 정보 닫기" }).click();
          } finally { await p.context().close(); }
        }
      }
    } finally { await wk.close(); }
  });
}
