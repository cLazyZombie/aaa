// 앱 GitHub Dark 복원과 독립 터미널 GitHub Dark 보존, 실제 대화·질문 카드의 네 화면 캡처.
import assert from "node:assert/strict";
import fs from "node:fs";
import {
  root, AA, aaa, step, browser, webkit, pairPage, openSettings, domIs, waitOutput, until, shot, inList,
} from "../lib/harness.mjs";

const beforeCSS = fs.readFileSync(new URL("../fixtures/github-dark-before.css", import.meta.url), "utf8");
const beforeKeys = [...beforeCSS.matchAll(/(--[a-z0-9-]+):/g)].map((match) => match[1]);

export default async function () {
  await step("GitHub Dark 복원: 앱은 이전 색, 독립 터미널은 기존 색, 기존 기기 선택 보존, 대화·질문 카드(Chromium·WebKit 1280·390)", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/github-task.md`, "질문 카드 시험\nGitHub Dark 화면 확인");
    const thread = aaa("thread", "start", "demo", "--title", "GitHub Dark", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/github-task.md`);
    await waitOutput(thread.pane, "asked-cards");
    const question = await until("테마 질문 수집", () => aaa("thread", "show", "demo", thread.id).questions?.items[0]?.id);
    const transcript = `${root}/claude/projects/-github/s1.jsonl`;
    fs.mkdirSync(`${root}/claude/projects/-github`, { recursive: true });
    fs.writeFileSync(transcript, [
      { type: "user", message: { role: "user", content: "앱과 터미널에 GitHub Dark를 적용해 주세요." } },
      { type: "assistant", message: { role: "assistant", content: [{ type: "text", text: "배경은 `#24292e`, 글자는 `#C9D1D9`입니다.\n\n```rust\nfn main() { println!(\"GitHub Dark\"); }\n```\n\n- 터미널 색 유지\n- 화면 나머지는 Primer 색 사용" }] } },
    ].map((v) => JSON.stringify(v)).join("\n") + "\n");
    aaa("tab", "create", "--machine", "labA", "--label", "github-chat", "--", "/bin/sh", "-c",
      '"$0" hook --agent claude "$1"; printf "GITHUB_READY\\n"; exec cat', AA,
      JSON.stringify({ hook_event_name: "SessionStart", session_id: "github-chat", source: "startup", transcript_path: transcript }));
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, label] of [[browser, "chromium"], [wk, "webkit"]]) {
        for (const width of [1280, 390]) {
          const p = (await pairPage(engine, aaa("web", "pair").code, { width, height: 844 })).page;
          try {
            // 기존 저장 ID가 배포 뒤에도 복원되는 실제 기기 저장 경로.
            await p.evaluate(() => localStorage.setItem("aaa:terminal-color-theme", "wezterm-github-dark-user"));
            await p.reload();
            const settings = await openSettings(p, "모양");
            await settings.locator("#settings-palette").selectOption("github-dark");
            await domIs(p, "GitHub Dark 앱 적용", () => document.documentElement.dataset.palette === "github-dark");
            await settings.getByRole("button", { name: "설정 닫기" }).click();
            await inList(p, "pane-github-chat");
            // pane 보기 선택은 다른 기기에서도 복원된다. 앞 기기가 연 대화에서 터미널로 돌아온다.
            const toggle = p.getByTestId("chat-toggle");
            await toggle.waitFor();
            if (await toggle.getAttribute("aria-pressed") === "true") await toggle.click();
            await p.getByTestId("terminal").waitFor({ state: "visible" });
            await domIs(p, "테마 터미널 준비", () => document.querySelector(".xterm-rows")?.textContent.includes("GITHUB_READY"));
            const read = () => p.evaluate(() => {
              const css = getComputedStyle(document.querySelector(".term"));
              const keys = ["--term-bg", "--term-fg", "--term-cursor", "--term-selection", ...Array.from({ length: 16 }, (_, i) => `--ansi-${i}`)];
              return keys.map((key) => css.getPropertyValue(key).trim().replace(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/i, "#$1$1$2$2$3$3"));
            });
            const selected = await read();
            assert.equal(selected[0], "#24292e");
            assert.equal(selected[1], "#c9d1d9");
            assert.equal(selected[4], "#000000");
            await shot(p, `github-dark-${label}-${width}-independent-terminal`);
            const terminal = await openSettings(p, "터미널");
            const select = terminal.getByLabel("터미널 색 테마", { exact: true });
            assert.equal(await select.inputValue(), "wezterm-github-dark-user");
            assert.equal(await select.locator("option:checked").textContent(), "GitHub Dark");
            await select.selectOption("app");
            await terminal.getByRole("button", { name: "설정 닫기" }).click();
            await domIs(p, "앱 터미널 색 반영", () => !document.querySelector(".term").style.getPropertyValue("--term-bg"));
            const appColors = await read();
            assert.equal(appColors[0], "#0d1117");
            assert.equal(appColors[1], "#c9d1d9");
            assert.equal(appColors[2], "#58a6ff");
            assert.equal(appColors[4], "#8b949e");
            assert.notDeepEqual(appColors, selected, "독립 테마와 앱 팔레트의 원래 구분");
            const app = await p.evaluate(() => {
              const css = getComputedStyle(document.documentElement);
              return { bg: getComputedStyle(document.body).backgroundColor, danger: css.getPropertyValue("--danger-text").trim(), pwa: document.querySelector('meta[name="theme-color"]').content };
            });
            assert.deepEqual(app, { bg: "rgb(13, 17, 23)", danger: "#f85149", pwa: "#0d1117" });
            await shot(p, `github-dark-${label}-${width}-terminal`);
            await toggle.click();
            await p.getByTestId("chat").getByText("앱과 터미널에 GitHub Dark를 적용해 주세요.", { exact: true }).waitFor();
            await shot(p, `github-dark-${label}-${width}-chat`);
            const paletteSnapshot = () => p.evaluate((keys) => {
              const css = getComputedStyle(document.documentElement);
              const canvas = document.createElement("canvas"); canvas.width = canvas.height = 1;
              const ctx = canvas.getContext("2d", { willReadFrequently: true });
              ctx.globalCompositeOperation = "copy";
              return keys.map((key) => {
                ctx.fillStyle = css.getPropertyValue(key).trim();
                ctx.fillRect(0, 0, 1, 1);
                return [...ctx.getImageData(0, 0, 1, 1).data];
              });
            }, beforeKeys);
            const restored = await paletteSnapshot();
            const baseline = await p.addStyleTag({ content: beforeCSS });
            assert.deepEqual(await paletteSnapshot(), restored, "adf056a9 원본을 적용해도 앱 전체 색 token은 동일");
            await shot(p, `github-dark-${label}-${width}-before-reference`);
            await baseline.evaluate((style) => style.remove());
            await p.goto(new URL(`/#question=${question}`, p.url()).href);
            await p.getByTestId("asks-scroll").getByText("로그인 단추 자리", { exact: false }).first().waitFor();
            await shot(p, `github-dark-${label}-${width}-question`);
            await p.reload();
            await domIs(p, "질문 새로고침 팔레트 유지", () => document.documentElement.dataset.palette === "github-dark");
          } finally { await p.context().close(); }
        }
      }
    } finally { await wk.close(); }
  });
}
