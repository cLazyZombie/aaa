// 실제 host·agent·PTY와 최근 화면 전환. standalone 판별만 모의하며 OS 단축키 선점 검증과 구분한다.
import assert from "node:assert/strict";
import { root, aaa, until, step, browser, webkit, pairPage, inList, screenText, sentCount, sentFrames, sentText, openSettings, shot } from "../lib/harness.mjs";

export default async function () {
  await step("PWA 최근 화면: MRU 고정·양방향·해제 확정·취소·닫힌 pane·앱 화면·터미널 무전송·일반 탭 유지", async () => {
    aaa("project", "create", "recent");
    const wk = await webkit.launch();
    try {
      for (const engine of [browser, wk]) {
        const name = engine.browserType().name();
        const a = `recent-a-${name}`, b = `recent-b-${name}`;
        aaa("tab", "create", "--machine", "labA", "--label", a, "--", process.execPath, `${root}/bar.js`);
        aaa("tab", "create", "--machine", "labA", "--label", b, "--", process.execPath, `${root}/bar.js`);
        const { page: p, errors } = await pairPage(engine, aaa("web", "pair").code, { width: 1280, height: 850 }, {
          init: () => Object.defineProperty(navigator, "standalone", { value: true, configurable: true }),
        });
        const hash = () => p.evaluate(() => location.hash);
        const visit = async (route) => {
          await p.evaluate((route) => { location.hash = route; }, route);
          const split = route.indexOf("=");
          const key = split < 0 ? `${route}:` : `${route.slice(0, split)}:${decodeURIComponent(route.slice(split + 1))}`;
          await until(`최근 방문 ${route}`, async () => (await hash()) === `#${route}` && await p.evaluate((key) => JSON.parse(localStorage.getItem("aaa:recent-screens") ?? "[]")[0] === key, key));
        };
        const popup = p.getByRole("dialog", { name: "최근에 연 화면" });
        const selected = () => popup.locator('[role="option"][aria-selected="true"]').getAttribute("data-recent-key");
        await inList(p, `pane-${a}`);
        await until("첫 최근 셸 준비", async () => (await screenText(p)).includes("bar>"));
        const aHash = await hash();
        await visit("tasks");
        await p.locator(".tasks-view:visible").waitFor();
        await visit("automation=recent%2F");
        await visit("reports");
        await inList(p, `pane-${b}`);
        await until("둘째 최근 셸 준비", async () => (await screenText(p)).includes("bar>"));
        const bHash = await hash();
        await openSettings(p, "단축키");
        // sidebar 도우미는 설정을 닫은 뒤 본문을 잠깐 다시 보여 준다. 이 중간 방문을 만들지 않고 실제 chord로 복귀한다.
        await p.keyboard.down("Control");
        await p.keyboard.press("Tab");
        await popup.waitFor();
        assert.equal(await selected(), bHash.replace("#pane=", "pane:"));
        assert.equal(await p.getByRole("region", { name: "설정" }).isVisible(), true, "목록 선택 중 설정 본문 유지");
        await p.keyboard.up("Control");
        await popup.waitFor({ state: "detached" });
        await until("설정에서 최근 셸로 전환", async () => !(await p.getByRole("region", { name: "설정" }).count()) && (await screenText(p)).includes("bar>"));
        await until("확정 뒤 새 터미널 focus", () => p.locator(".xterm-helper-textarea").evaluate((el) => el === document.activeElement));
        const inputAt = await sentCount(p);
        await p.keyboard.type("z");
        await until("전환 뒤 실제 터미널 입력", async () => (await screenText(p)).includes('GOT:1:"z"'));
        assert.equal(sentText(await sentFrames(p, inputAt)), "z", "확정 뒤 입력은 새 pane에 한 번만");
        await p.locator(".xterm-helper-textarea").focus();
        const from = await sentCount(p);
        await p.keyboard.down("Control");
        await p.keyboard.press("Tab");
        await popup.waitFor();
        assert.equal(await selected(), "settings:");
        assert.equal(await hash(), bHash, "선택 중 본문과 URL은 그대로");
        const entries = await popup.locator('[role="option"]').allTextContents();
        assert.ok(entries.some((s) => s.includes("할 일")));
        assert.ok(entries.some((s) => s.includes("보고서")));
        assert.ok(entries.some((s) => s.includes("새 자동화")));
        await p.keyboard.press("Tab");
        assert.equal(await selected(), "reports:");
        await p.keyboard.press("Tab");
        assert.equal(await selected(), "automation:recent/");
        await p.keyboard.press("Shift+Tab");
        assert.equal(await selected(), "reports:");
        assert.equal(sentText(await sentFrames(p, from)), "", "터미널에 전환 키 bytes를 보내지 않음");
        await shot(p, `recent-${name}-held`);
        await p.keyboard.up("Control");
        await popup.waitFor({ state: "detached" });
        await until("Ctrl 해제 뒤 보고서 전환", async () => await hash() === "#reports");
        assert.equal(await p.evaluate(() => JSON.parse(localStorage.getItem("aaa:recent-screens"))[0]), "reports:");

        await visit(bHash.slice(1));
        await p.locator(".xterm-helper-textarea").focus();
        await p.keyboard.down("Control");
        await p.keyboard.press("Tab");
        await popup.waitFor();
        await p.keyboard.press("Escape");
        await popup.waitFor({ state: "detached" });
        await p.keyboard.up("Control");
        assert.equal(await hash(), bHash);
        assert.equal(await p.locator(".xterm-helper-textarea").evaluate((el) => el === document.activeElement), true, "취소하면 원래 터미널 focus");
        await p.keyboard.down("Control");
        await p.keyboard.press("Tab");
        await popup.waitFor();
        await p.evaluate(() => window.dispatchEvent(new Event("blur")));
        await popup.waitFor({ state: "detached" });
        await p.keyboard.up("Control");
        assert.equal(await hash(), bHash, "창 포커스 상실은 확정하지 않음");

        // 짧은 chord의 down/up이 같은 JS task 안에 와도 ref가 이어 받는다.
        await p.evaluate(() => {
          for (const [type, key, ctrlKey] of [["keydown", "Control", true], ["keydown", "Tab", true], ["keyup", "Tab", true], ["keyup", "Control", false]])
            window.dispatchEvent(new KeyboardEvent(type, { key, ctrlKey, bubbles: true, cancelable: true }));
        });
        await until("같은 task 안 빠른 전환", async () => await hash() === "#reports");
        assert.equal(await popup.count(), 0);

        await visit(aHash.slice(1));
        await until("닫기 전 첫 셸 복귀", async () => (await screenText(p)).includes("bar>"));
        await visit(bHash.slice(1));
        await until("닫기 전 둘째 셸 복귀", async () => (await screenText(p)).includes("bar>"));
        await p.keyboard.down("Control");
        await p.keyboard.press("Tab");
        await popup.waitFor();
        assert.equal(await selected(), aHash.replace("#pane=", "pane:"));
        aaa("pane", "kill", a);
        await until("닫힌 선택을 목록에서 제거", async () => !(await popup.locator('[role="option"]').allTextContents()).some((s) => s.includes(a)));
        await p.keyboard.up("Control");
        await popup.waitFor({ state: "detached" });
        assert.equal(await hash(), bHash, "닫힌 선택은 열지 않음");
        await p.reload();
        await p.getByTestId("machine-labA").waitFor({ state: "attached" });
        await until("새로고침 후 셸 복귀", async () => (await screenText(p)).includes("bar>"));
        await p.keyboard.down("Control");
        await p.keyboard.press("Tab");
        await popup.waitFor();
        assert.ok((await popup.locator('[role="option"]').allTextContents()).some((s) => s.includes("설정")), "기기 MRU가 새로고침에도 남음");
        assert.ok(!(await popup.locator('[role="option"]').allTextContents()).some((s) => s.includes(a)));
        await p.keyboard.press("Escape");
        await p.keyboard.up("Control");
        assert.deepEqual(errors, []);
        await p.context().close();
        aaa("pane", "kill", b);

        const plain = (await pairPage(engine, aaa("web", "pair").code, { width: 1280, height: 850 })).page;
        await plain.evaluate(() => { location.hash = "tasks"; });
        await plain.locator(".tasks-view:visible").waitFor();
        await plain.locator("main.view").focus();
        await plain.evaluate(() => { window.__recentKeySeen = null; window.addEventListener("keydown", (e) => { if (e.key === "Tab" && e.ctrlKey) window.__recentKeySeen = e.defaultPrevented; }); });
        await plain.keyboard.press("Control+Tab");
        assert.equal(await plain.evaluate(() => window.__recentKeySeen), false, "일반 탭 Ctrl+Tab 기본 동작 유지");
        assert.equal(await plain.locator(".recent-scrim").count(), 0);
        await plain.context().close();
      }
    } finally { await wk.close(); }
  });
}
