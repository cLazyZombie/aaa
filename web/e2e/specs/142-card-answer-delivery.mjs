// 실제 host·agent·PTY에서 사용량 대기 중 쓴 바이트와 재개 후 제출을 함께 본다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { aaa, browser, domIs, pairPage, root, step, until, waitOutput, webkit } from "../lib/harness.mjs";

const fixture = `${root}/limit-claude.cjs`;
const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "limit-test"]')
  + `\n[profiles.limit-test]\nagent = "claude"\ncommand = [${JSON.stringify(process.execPath)}, "${fixture}"]\n`);
const task = `${root}/limit-task.md`;
fs.writeFileSync(task, "카드 답 시험\n");
fs.writeFileSync(fixture, String.raw`
const fs = require("node:fs");
const dir = __dirname + "/" + process.env.AAA_THREAD;
fs.mkdirSync(dir);
const mode = dir + "/mode";
fs.writeFileSync(mode, "prompt");
const rule = "────────────────";
let draft = "";
let seen = false;
function draw(...lines) { process.stdout.write("\x1b[2J\x1b[H" + lines.join("\r\n") + "\r\n"); }
function showMode() {
  if (fs.readFileSync(mode, "utf8") === "limit") {
    draw("You've hit your limit · resets 12:30pm", "❯ Wait here, then continue automatically at 12:30pm",
      "  Stop", "Enter to select · Esc to cancel");
  } else draw(rule, "❯ ", rule, "? for shortcuts");
}
fs.watch(mode, showMode);
process.stdin.setRawMode(true);
process.stdin.on("data", bytes => {
  const text = bytes.toString("utf8");
  if (fs.readFileSync(mode, "utf8") === "limit") fs.appendFileSync(dir + "/blocked", bytes);
  for (const chunk of text.split(/(\r)/)) {
    if (!chunk) continue;
    if (chunk === "\r") {
      if (seen) fs.appendFileSync(dir + "/answers", JSON.stringify(draft) + "\n");
      seen = true;
      draft = "";
      draw("✻ Working… (1s · esc to interrupt)");
    } else {
      draft += chunk;
      draw(rule, "❯ 입력 확인 중", rule, "? for shortcuts");
    }
  }
});
showMode();
`);

async function answerInPane(engine, width) {
  const t = aaa("thread", "start", "demo", "--title", "한도 대기", "--machine", "labA",
    "--profile", "limit-test", "--kind", "dir", "--task-file", task);
  const dir = `${root}/${t.id}`;
  await until("brief 전달", () => aaa("thread", "show", t.id).brief === "delivered");
  const report = "[결정 필요] 진행\n\n## Questions\n### 1. 자리를 유지할까\n- A 유지\n- B 바꿈\n";
  fs.writeFileSync(`${t.work_dir}/report.md`, report);
  fs.writeFileSync(`${dir}/mode`, "limit");
  await waitOutput(t.pane, "Wait here, then continue automatically");
  const hash = createHash("sha256").update(report).digest("hex");
  const question = await until("질문 수집", () => {
    const view = aaa("thread", "show", t.id);
    return view.report_hash === hash && view.questions?.items[0];
  });
  const answer = `보고의 질문에 대한 사용자 답(1A)\n1A. ${question.id} 자리를 유지할까 — 유지`;
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { hash: `#question=${t.id}` });
  const notice = () => aaa("inbox", "list", "demo", "--all", "--peek").items
    .find(i => i.subject === t.id && ["question answer waiting", "question answered"].includes(i.event));
  try {
    const card = page.getByTestId(`thread-ask-${t.id}`);
    await card.getByTestId("thread-ask-pick-1A").click();
    await card.getByTestId("thread-ask-send").click();
    const done = page.getByTestId("ask-done-card");
    await domIs(page, "카드 전달 대기", () => {
      const card = document.querySelector('[data-testid="ask-done-card"][data-delivery="waiting"]');
      return card?.textContent.includes("전달 대기 1A");
    });
    const waiting = await until("inbox 전달 대기", () => {
      const item = notice();
      return item?.event === "question answer waiting" && item;
    });
    assert.match(waiting.summary, /카드 답 전달 대기/);
    assert.equal(waiting.body, answer);
    assert.equal(fs.existsSync(`${dir}/blocked`), false);
    assert.equal(fs.existsSync(`${dir}/answers`), false);
    fs.writeFileSync(`${dir}/mode`, "prompt");
    const accepted = await until("같은 inbox 항목이 수락으로 전환", () => {
      const item = notice();
      return item?.id === waiting.id && item.event === "question answered" && item;
    });
    assert.match(accepted.summary, /카드 답 전달됨/);
    aaa("ticker", "tick");
    assert.deepEqual(fs.readFileSync(`${dir}/answers`, "utf8").trimEnd().split("\n").map(JSON.parse), [answer]);
    await domIs(page, "재개 이후 카드 답함", () => {
      const card = document.querySelector('[data-testid="ask-done-card"]:not([data-delivery])');
      return card?.textContent.includes("답함 1A");
    });
    assert.equal(await done.count(), 1);
    assert.deepEqual(errors, []);
  } finally {
    await page.context().close();
  }
}

export default async function () {
  await step("한도 대기 카드 답을 재개 후 한 번만 받는다 — Chromium", async () => {
    if (!aaa("project", "list").projects.some(p => p.name === "demo")) aaa("project", "create", "demo");
    await answerInPane(browser, 1280);
  });
  await step("한도 대기 카드 답을 재개 후 한 번만 받는다 — WebKit", async () => {
    const wk = await webkit.launch();
    try { await answerInPane(wk, 390); } finally { await wk.close(); }
  });
}
