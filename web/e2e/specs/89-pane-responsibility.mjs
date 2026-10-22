// 실제 host·agent·thread의 화면. 책임 경로 관측값만 fixture로 바꿔 macOS 밖에서도 세 상태를 검증한다.
// 실제 커널 관측·worker exec·기존 pane 생존은 Rust restart 회귀가 맡는다. 자동 보류·강제 종료는 없다.
import fs from "node:fs";
import { root, aaa, step, until, domIs, shot, pairPage, browser, webkit } from "../lib/harness.mjs";

export default async function ({ page }) {
  await step("책임 경고: 목록·보고·320px·WebKit·Reader와 정리 후 새 pane 안내", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/responsibility-task.md`, "# 책임 경로 확인\n사람을 기다림\n");
    const id = aaa("thread", "start", "demo", "--title", "책임 경로 확인", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/responsibility-task.md`).id;
    await until("thread 준비", () => aaa("thread", "show", "demo", id).brief === "delivered");
    const original = aaa("thread", "show", "demo", id);
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, width, reader, state] of [
        [browser, 1280, false, "detached"], [browser, 320, false, "detached"],
        [wk, 390, true, "detached"], [browser, 1280, false, "unknown"], [browser, 1280, false, "launcher"],
      ]) {
        const value = { state, pid: state === "unknown" ? null : 42,
          path: state === "unknown" ? null : `/opt/example/${"긴경로/".repeat(16)}${state === "launcher" ? "aaa-launcher.app/Contents/MacOS/aaa-launcher" : "aaa"}` };
        const dev = await pairPage(engine, aaa("web", "pair", ...(reader ? ["--reader"] : [])).code,
          { width, height: 844 }, { touch: width < 768, serviceWorkers: "block", hash: `#thread=demo%2F${id}`,
            setup: async (context) => {
              await context.route("**/api/v1/call", async (route) => {
                const response = await route.fetch();
                const body = await response.json();
                const visit = (v) => {
                  if (!v || typeof v !== "object") return;
                  if ((v.project === "demo" && v.id === id) || v.id === original.pane) v.responsibility = value;
                  for (const child of Object.values(v)) if (child && typeof child === "object") visit(child);
                };
                visit(body);
                await route.fulfill({ response, json: body });
              });
            },
          });
        const p = dev.page;
        try {
          const view = p.getByTestId("thread-view");
          await view.waitFor();
          const banner = p.getByTestId("responsibility-banner");
          const rowBadge = p.getByTestId(`thread-demo-${id}`).getByTestId("responsibility-badge");
          if (state === "launcher") await rowBadge.waitFor({ state: "detached" });
          else {
            await rowBadge.waitFor({ state: "attached" });
            const label = await rowBadge.textContent();
            if (label !== (state === "detached" ? "책임 연결 끊김" : "책임 확인 못함")) throw new Error(`목록 경고: ${label}`);
          }
          if (state === "launcher") {
            await banner.waitFor({ state: "detached" });
          } else {
            await banner.waitFor();
            if (state === "detached") {
              await banner.getByText("개발자 도구 책임 연결 끊김", { exact: true }).waitFor();
              const text = await banner.innerText();
              if (!text.includes("Handoff") || !text.includes("같은 머신") || !text.includes("커밋·push")) throw new Error("저장·재개 안내 없음");
              if (reader) {
                if (await banner.getByTestId("responsibility-prepare").count()) throw new Error("Reader에게 보류 조작이 보인다");
              } else {
                await banner.getByTestId("responsibility-prepare").click();
                const dialog = p.getByTestId("park-dialog");
                await dialog.getByTestId("park-ask").waitFor();
                if (!(await dialog.innerText()).includes("Handoff")) throw new Error("기존 정리 경로가 아니다");
                await p.keyboard.press("Escape");
                await dialog.waitFor({ state: "detached" });
              }
            } else {
              await banner.getByText("개발자 도구 책임 확인 못함", { exact: true }).waitFor();
              if (await banner.getByTestId("responsibility-prepare").count()) throw new Error("조회 실패를 끊김으로 다룬다");
            }
            if (state === "detached") {
              await shot(p, `responsibility-collapsed-${width}-${reader ? "reader" : "controller"}`);
              await banner.locator("summary").click();
              await banner.getByText(/관측 경로:/).waitFor();
            }
            await domIs(p, "책임 경고 가로 넘침 없음", () => {
              const el = document.querySelector('[data-testid="responsibility-banner"]');
              const r = el?.getBoundingClientRect();
              return !!el && r.left >= 0 && r.right <= innerWidth + 1 && el.scrollWidth <= el.clientWidth + 1;
            });
            await shot(p, `responsibility-${state}-${width}-${reader ? "reader" : "controller"}`);
          }
          if (width === 1280 && state === "detached" && !reader) {
            await p.getByTestId("open-terminal").click();
            await p.getByTestId("terminal").waitFor();
            await p.locator(".term-head:visible").getByTestId("responsibility-badge").waitFor();
            await shot(p, "responsibility-terminal");
          }
          if (dev.errors.length) throw new Error(dev.errors.join("\n"));
          const current = aaa("thread", "show", "demo", id);
          if (current.status !== "open" || current.pane !== original.pane || current.park) throw new Error("안내를 보기만 했는데 pane이나 보류 상태가 바뀌었다");
        } finally {
          await p.context().unrouteAll({ behavior: "wait" });
          await p.context().close();
        }
      }
    } finally { await wk.close(); }
  });
}
