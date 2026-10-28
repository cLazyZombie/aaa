// OS 예약 키 전달은 자동화하지 않는다. Mac headless에서 실제 host·agent와
// standalone 미디어 조건으로 화면 이동·설정·입력·PTY 전송 경계를 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { aaa, AA, root, step, browser, webkit, pairPage, openSettings, until, sentCount, sentFrames } from "../lib/harness.mjs";
import { selectInputAndWait } from "../lib/input-selection.mjs";

export default async function () {
  let pane, coordinator;
  await step("단축키 대화 fixture 준비", async () => {
    const dir = `${root}/claude/projects/-shortcut`;
    fs.mkdirSync(dir, { recursive: true });
    const transcript = `${dir}/chat.jsonl`;
    fs.writeFileSync(transcript, JSON.stringify({ type: "user", message: { content: "단축키 시험" } }) + "\n");
    const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "1ef4c7b0-2f5c-490a-bd21-48b0b27d1a15", source: "startup", transcript_path: transcript });
    aaa("tab", "create", "--machine", "labA", "--label", "shortcut-chat", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA, hook);
    pane = await until("대화 hook 감지", () => aaa("pane", "list").panes.find(p => p.name === "shortcut-chat" && p.agent?.harness === "claude")?.id);
    aaa("project", "create", "demo");
    coordinator = aaa("project", "open", "demo", "--profile", "fake").pane.id;
  }, { always: true });

  for (const engine of ["chromium", "webkit"]) {
    const wk = engine === "webkit" ? await webkit.launch({ headless: true }) : null;
    try {
      for (const standalone of [false, true]) {
        let ctl;
        const mode = `${engine} ${standalone ? "PWA" : "탭"}`;
        await step(`${mode} 단축키 화면 준비`, async () => {
          ctl = await pairPage(wk ?? browser, aaa("web", "pair").code, { width: 1280, height: 900 }, {
            platform: "MacIntel", hash: `#pane=${pane}`,
            init: () => {
              localStorage.setItem("aaa:default-view", "chat");
              window.shortcutStandalone = false;
              const media = window.matchMedia.bind(window);
              window.matchMedia = query => {
                const result = media(query);
                if (query === "(display-mode: standalone)") Object.defineProperty(result, "matches", { get: () => window.shortcutStandalone });
                return result;
              };
            },
          });
          await ctl.page.evaluate(value => { window.shortcutStandalone = value; }, standalone);
          await ctl.page.locator('.term:not([hidden]) .composer-text').waitFor();
        }, { always: true });
        const p = ctl.page;
        const box = () => p.locator('.term:not([hidden]) .composer-text');
        const home = async () => {
          await p.evaluate(id => { location.hash = `pane=${id}`; }, pane);
          await p.locator(`.term[data-pane="${pane}"]:not([hidden])`).waitFor();
        };
        const hash = async expected => p.waitForFunction(value => location.hash === value, expected);
        try {
          await step(`${mode} Option O·Q·T·D: 입력 초안 보존·반복·특수 문자·터미널 전송 차단`, async () => {
            await box().fill("남길 초안 한글 English");
            await box().click();
            const from = await sentCount(p);
            // macOS Option의 특수 문자와 dead key도 물리 code로 읽는다.
            await box().evaluate(el => el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "œ", code: "KeyQ", altKey: true })));
            await hash("#asks");
            await p.keyboard.press("Alt+KeyT"); await hash("#threads");
            await p.keyboard.press("Alt+KeyD"); await hash("#tasks");
            await p.keyboard.down("Alt"); await p.keyboard.down("KeyD"); await p.keyboard.down("KeyD");
            await p.keyboard.up("KeyD"); await p.keyboard.up("Alt");
            assert.equal(new URL(p.url()).hash, "#tasks", "반복해도 같은 화면");
            await p.keyboard.press("Alt+KeyO"); await hash(`#pane=${coordinator}`);
            await home();
            assert.equal(await box().inputValue(), "남길 초안 한글 English");
            // 조합 중과 229 사건은 이동하지 않는다.
            for (const ime of [{ isComposing: true }, { keyCode: 229 }]) {
              const accepted = await box().evaluate((el, ime) => el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Dead", code: "KeyT", altKey: true, ...ime })), ime);
              assert.equal(accepted, true);
              assert.equal(new URL(p.url()).hash, `#pane=${pane}`);
            }
            // 터미널 자체의 키 처리도 앱 키를 PTY로 보내지 않는다.
            await p.keyboard.press("Meta+Shift+KeyJ");
            const terminal = p.locator('.term:not([hidden]) .xterm-helper-textarea');
            await terminal.focus();
            await p.keyboard.press("Alt+KeyT"); await hash("#threads");
            assert.deepEqual((await sentFrames(p, from)).filter(f => ["input", "bin", "submit"].includes(f.t)), []);
            await home(); await p.keyboard.press("Meta+Shift+KeyJ"); await box().waitFor();
          });

          await step(`${mode} 설정: 새 기본값·충돌·끄기·기록·초기화`, async () => {
            const settings = await openSettings(p, "단축키");
            for (const [name, key] of [["coordinator 대화", "O"], ["답할 것", "Q"], ["모든 thread", "T"], ["할 일", "D"]]) {
              assert.equal(await settings.getByRole("combobox", { name, exact: true }).locator('option[value="default"]').textContent(), `⌥+${key}`);
            }
            await settings.getByRole("combobox", { name: "모든 thread", exact: true }).selectOption("off");
            await settings.getByRole("button", { name: "할 일의 단축키 기록", exact: true }).click();
            await p.keyboard.press("Alt+KeyQ");
            await settings.getByRole("status").filter({ hasText: "이미 지정됨" }).waitFor();
            await p.keyboard.press("Alt+KeyY");
            await settings.getByRole("button", { name: "그래도 저장", exact: true }).click();
            await settings.getByRole("button", { name: "설정 닫기", exact: true }).click();
            await box().click();
            const accepted = await box().evaluate(el => el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "†", code: "KeyT", altKey: true })));
            assert.equal(accepted, true, "끈 조합은 입력칸 몫");
            await p.keyboard.press("Alt+KeyY"); await hash("#tasks");
            const again = await openSettings(p, "단축키");
            await again.getByRole("button", { name: "단축키 초기화", exact: true }).click();
            await again.getByRole("button", { name: "설정 닫기", exact: true }).click();
            await p.keyboard.press("Alt+KeyT"); await hash("#threads");
            await home();
          });

          await step(`${mode} Ctrl W: PWA 경계·선택·커서·IME·되돌리기`, async () => {
            // 불러온 초안을 준비한다. WebKit의 fill은 삽입과 뒤 삭제를 같은 undo
            // 묶음으로 만들므로 이 시험의 삭제 전 상태를 편집 명령으로 넣지 않는다.
            await box().evaluate(el => {
              Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set.call(el, "앞말 hello 세계   뒤");
              el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertReplacementText" }));
            });
            await box().evaluate(selectInputAndWait, [15]);
            const from = await sentCount(p);
            const invoke = extra => box().evaluate((el, extra) => el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "w", code: "KeyW", ctrlKey: true, ...extra })), extra);
            assert.equal(await invoke({ isComposing: true }), true);
            assert.equal(await invoke({ keyCode: 229 }), true);
            for (const modifier of ["altKey", "metaKey", "shiftKey"]) assert.equal(await invoke({ [modifier]: true }), true);
            assert.equal(await box().inputValue(), "앞말 hello 세계   뒤");
            const accepted = await invoke({});
            assert.equal(accepted, !standalone, "일반 탭은 Ctrl W를 가로채지 않음");
            if (standalone) {
              assert.equal(await box().inputValue(), "앞말 hello 세계   ");
              await box().press("Control+KeyW");
              assert.equal(await box().inputValue(), "앞말 hello ");
              await box().press("Meta+KeyZ");
              assert.ok((await box().inputValue()).includes("세계"), "단어 삭제 되돌리기");
              await box().fill("hello 세계 next");
              await box().evaluate(selectInputAndWait, [2, 8]);
              await box().press("Control+KeyW");
              assert.equal(await box().inputValue(), "he next", "선택만 삭제");
              assert.equal(await box().evaluate(el => el.selectionStart), 2);
              await box().evaluate(selectInputAndWait, [0]);
              await box().press("Control+KeyW");
              assert.equal(await box().inputValue(), "he next");
              await p.keyboard.press("Alt+KeyT"); await home();
              assert.equal(await box().inputValue(), "he next", "초안 저장");
            } else assert.equal(await box().inputValue(), "앞말 hello 세계   뒤");
            assert.deepEqual((await sentFrames(p, from)).filter(f => ["input", "bin", "submit"].includes(f.t)), []);
          });
          assert.deepEqual(ctl.errors, []);
        } finally { await p.context().close(); }
      }
    } finally { await wk?.close(); }
  }
}
