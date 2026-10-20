// 웹 E2E 대화의 도구 호출 묶음(plan §8.303, t3code MessagesTimeline `work-toggle`): 이어진 도구 줄이 「명령 2개 실행, 파일 3개 읽음」
// 한 줄로 접히고, 누르면 그 줄들이 펼쳐진다. 도구 하나는 그대로 한 줄이다. 끝이 아닌 곳에서 펼치고 접어도 그 머리가 1px 안에서
// 제자리고, 끝을 보던 사람은 펼친 뒤에도 끝에 남는다. 데스크톱 Chromium 1280·패드 WebKit 820·휴대폰 WebKit 390.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 40-chat-tool-groups(새 실험실). 스크린샷 폴더를 주면 기기마다 접힘·펼침을 찍는다.

import fs from "node:fs";
import { aaa, until, step, browser, webkit, pairPage, inList, shot, root, AA } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const LABEL = "tools1";
const FIRST = ["Bash: cargo build", "Bash: cargo test", "Read: src/a.rs", "Read: src/b.rs", "Read: src/c.rs"];
const SECOND = ["Grep: fn main", "Edit: src/a.rs", "Edit: src/a.rs", "Edit: src/b.rs", "WebSearch: rust 2024 edition", "mcp__docs__read: guide.md"];

function chatPane() {
  const dir = `${root}/claude/projects/-lab-${LABEL}`;
  fs.mkdirSync(dir, { recursive: true });
  const tr = `${dir}/s1.jsonl`;
  const L = (o) => JSON.stringify(o) + "\n";
  const ask = (text) => L({ type: "user", message: { role: "user", content: text } });
  const say = (text) => L({ type: "assistant", message: { content: [{ type: "text", text }] } });
  const use = (name, input) => L({ type: "assistant", message: { content: [{ type: "tool_use", name, input }] } });
  const filler = (from, to) => Array.from({ length: to - from }, (_, n) => ask(`질문 ${from + n}`) + say(`답 ${from + n}\n\n${"긴 줄 ".repeat(30)}`)).join("");
  const body =
    filler(0, 12) +
    ask("빌드를 확인해 줘") + say("살펴보겠습니다.") +
    use("Bash", { command: "cargo build" }) + use("Bash", { command: "cargo test" }) +
    use("Read", { file_path: "src/a.rs" }) + use("Read", { file_path: "src/b.rs" }) + use("Read", { file_path: "src/c.rs" }) +
    say("빌드는 통과합니다.") +
    use("Write", { file_path: "docs/note.md", content: "x" }) +
    say("메모를 남겼습니다.") +
    ask("테스트를 많이 돌려 줘") + Array.from({ length: 25 }, (_, n) => use("Bash", { command: `cargo test case_${n}` })).join("") + say("다 돌렸습니다.") +
    filler(12, 24) +
    ask("검색도 해 줘") +
    use("Grep", { pattern: "fn main" }) + use("Edit", { file_path: "src/a.rs" }) + use("Edit", { file_path: "src/a.rs" }) +
    use("Edit", { file_path: "src/b.rs" }) + use("WebSearch", { query: "rust 2024 edition" }) + use("mcp__docs__read", { path: "guide.md" });
  fs.writeFileSync(tr, body);
  aaa("tab", "create", "--machine", "labA", "--label", LABEL, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b40", source: "startup", transcript_path: tr }));
}

const frames = (p) => p.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

/** 대화를 열고 끝과 첫 묶음 자리를 찍는다(전후 비교 캡처) */
async function open(p, name) {
  await inList(p, `pane-${LABEL}`);
  if ((await p.getByTestId("chat").count()) === 0) {
    await until("대화 단추", async () => (await p.getByTestId("chat-toggle").count()) === 1);
    await p.getByTestId("chat-toggle").click();
  }
  const chat = p.getByTestId("chat");
  await chat.locator('[data-role="user"]').filter({ hasText: "검색도 해 줘" }).waitFor();
  await until("기록 끝 도구 줄", async () => (await chat.getByText("mcp__docs__read: guide.md").count()) + (await chat.locator(".chat-tool-group").count()) >= 1);
  await frames(p);
  await shot(p, `${name}-tools-end`);
  const first = chat.locator(".chat-row").filter({ has: p.locator('[data-role="assistant"]', { hasText: "살펴보겠습니다." }) });
  await first.evaluate((el) => el.scrollIntoView({ block: "start" }));
  await frames(p);
  await shot(p, `${name}-tools-middle`);
}

async function check(p, name) {
  const chat = p.getByTestId("chat");
  // 이어진 도구 줄은 묶음 한 줄씩, 도구 하나(Write)는 그대로 한 줄이다
  const groups = chat.locator(".chat-tool-group");
  const heads = groups.locator(".chat-tool-group-head");
  const labels = await heads.evaluateAll((els) => els.map((e) => e.textContent.trim()));
  const want = ["명령 2개 실행, 파일 3개 읽음", "명령 25개 실행", "코드 1번 검색, 파일 2개 수정, 그 밖의 동작 2개"];
  if (JSON.stringify(labels) !== JSON.stringify(want)) throw new Error(`${name}: 묶음 요약 ${JSON.stringify(labels)} ≠ ${JSON.stringify(want)}`);
  const visibleTools = () => chat.locator('[data-role="tool"]').evaluateAll((els) => els.map((e) => e.textContent.trim()));
  if (JSON.stringify(await visibleTools()) !== JSON.stringify(["Write: docs/note.md"])) throw new Error(`${name}: 접힌 뒤 보이는 도구 줄 ${JSON.stringify(await visibleTools())}`);
  if ((await heads.evaluateAll((els) => els.map((e) => e.getAttribute("aria-expanded")))).join() !== "false,false,false") throw new Error(`${name}: 처음부터 펼친 묶음`);

  // 끝이 아닌 곳: 펼치고 접어도 머리가 제자리(t3code disclosure anchoring)
  const head = heads.nth(0);
  await head.evaluate((el) => el.scrollIntoView({ block: "center" }));
  await frames(p);
  if (await chat.evaluate((el) => el.scrollHeight - el.scrollTop - el.clientHeight <= 40)) throw new Error(`${name}: 첫 묶음이 끝 근처라 제자리 검사가 안 된다`);
  const y = () => head.evaluate((el) => el.getBoundingClientRect().top);
  const before = await y();
  await head.click();
  await until("첫 묶음 펼침", async () => (await head.getAttribute("aria-expanded")) === "true" && (await groups.nth(0).locator('[data-role="tool"]').count()) === FIRST.length);
  await frames(p);
  const inside = await groups.nth(0).locator('[data-role="tool"]').evaluateAll((els) => els.map((e) => e.textContent.trim()));
  if (JSON.stringify(inside) !== JSON.stringify(FIRST)) throw new Error(`${name}: 펼친 줄 ${JSON.stringify(inside)}`);
  if (Math.abs((await y()) - before) > 1) throw new Error(`${name}: 펼친 뒤 머리 ${await y()} ≠ ${before}`);
  await shot(p, `${name}-tools-expanded`);
  await head.click();
  await until("첫 묶음 접힘", async () => (await head.getAttribute("aria-expanded")) === "false" && (await groups.nth(0).locator('[data-role="tool"]').count()) === 0);
  await frames(p);
  if (Math.abs((await y()) - before) > 1) throw new Error(`${name}: 접은 뒤 머리 ${await y()} ≠ ${before}`);

  // 긴 묶음은 높이 min(18rem, 50dvh) 안에서 굴리고 맨 위부터 보인다(참고 ExpandedWorkGroupEntries)
  const long = heads.nth(1);
  await long.evaluate((el) => el.scrollIntoView({ block: "start" }));
  await frames(p);
  await long.click();
  await until("긴 묶음 펼침", async () => (await groups.nth(1).locator('[data-role="tool"]').count()) === 25);
  await frames(p);
  const box = await groups.nth(1).locator(".chat-tool-group-list").evaluate((el) => ({ top: el.scrollTop, scroll: el.scrollHeight, client: el.clientHeight, cap: Math.min(18 * parseFloat(getComputedStyle(document.documentElement).fontSize), innerHeight / 2) }));
  if (box.top !== 0 || box.scroll <= box.client || box.client > box.cap + 1) throw new Error(`${name}: 긴 묶음 목록 ${JSON.stringify(box)} (맨 위, 굴림, 높이 상한)`);
  await shot(p, `${name}-tools-long-expanded`);
  await long.click();
  await until("긴 묶음 접힘", async () => (await groups.nth(1).locator('[data-role="tool"]').count()) === 0);

  // 끝을 보던 사람: 마지막 묶음을 펼쳐도 끝에 남는다
  await chat.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  await until("끝", () => chat.evaluate((el) => el.scrollHeight - el.scrollTop - el.clientHeight <= 1));
  await heads.nth(2).click();
  await until("끝 묶음 펼침", async () => (await groups.nth(2).locator('[data-role="tool"]').count()) === SECOND.length);
  await until("펼친 뒤 끝에 남음", () => chat.evaluate((el) => el.scrollHeight - el.scrollTop - el.clientHeight <= 1));
  await frames(p);
  await shot(p, `${name}-tools-end-expanded`);
  await heads.nth(2).click();
  await until("끝 묶음 접힘", async () => (await groups.nth(2).locator('[data-role="tool"]').count()) === 0);
}

export default async function () {
  await step("대화 도구 호출 묶음: 이어진 도구 줄은 요약 한 줄, 누르면 펼침, 펼치고 접어도 머리 제자리·끝 유지(1280·820·390)", async () => {
    chatPane();
    const wk = await webkit.launch();
    try {
      const pages = [
        ["desktop", (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page],
        ["ipad", (await pairDevice(wk, aaa("web", "pair").code, "ipad")).page],
        ["phone", (await pairDevice(wk, aaa("web", "pair").code, "iphone")).page],
      ];
      for (const [name, p] of pages) await open(p, name);
      for (const [name, p] of pages) await check(p, name);
    } finally {
      await wk.close();
    }
  });
}
