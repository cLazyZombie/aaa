// 대화 보관 시험의 격리 fixture. 운영 기록·인증을 사용하지 않는다.
import fs from "node:fs";
import { root, AA, aaa, waitOutput, until, inTime } from "./harness.mjs";
const quote = s => `'${s.replaceAll("'", "'\\''")}'`;
export const line = n => JSON.stringify({ type: n % 2 ? "assistant" : "user", message: { content: [{ type: "text", text: `기록 ${n} · ${"긴 대화 줄 ".repeat(30)}` }] } }) + "\n";
export const frames = page => page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))));
export const shown = page => page.locator('.term:not([hidden]) [data-testid="chat"]');
export const go = (page, hash) => page.evaluate(hash => { location.hash = hash; }, hash);
// 변경 알림으로 시작한 실제 최신 응답만 보류한다. 본문 복원과 조회 lane의 완료를 분리하는 회귀용이다.
export async function holdLatest(page, pane, transcript) {
  let release, arrive, fail, armed = true;
  let delivery = Promise.resolve();
  const ready = new Promise((resolve, reject) => { arrive = resolve; fail = reject; });
  const released = new Promise(resolve => { release = resolve; });
  const handler = async route => {
    const request = route.request().postDataJSON();
    if (!armed || request?.op !== "pane_chat" || request.pane !== pane || request.before) return route.continue();
    armed = false;
    delivery = (async () => {
      try {
        const response = await route.fetch();
        arrive();
        await released;
        try { await route.fulfill({ response }); } finally { await response.dispose(); }
      } catch (error) { fail(error); throw error; }
    })();
    await delivery;
  };
  await page.context().route("**/api/v1/call", handler);
  const dispose = async () => {
    release();
    await page.context().unroute("**/api/v1/call", handler);
    await delivery;
  };
  // 빈 줄은 기록 내용을 바꾸지 않고 실제 파일 감시 알림만 만든다.
  fs.appendFileSync(transcript, "\n");
  try { await inTime(ready, 20_000, "보류할 최신 응답 도착"); }
  catch (error) { await dispose(); throw error; }
  return { release, dispose };
}
export async function prepare() {
  const dir = `${root}/claude/projects/-lab-retention`;
  fs.mkdirSync(dir, { recursive: true });
  const transcript = `${dir}/s.jsonl`;
  fs.writeFileSync(transcript, Array.from({ length: 2400 }, (_, n) => line(n)).join(""));
  const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "6e5d4c3b-2a19-4f08-8e7d-6c5b4a392817", source: "startup", transcript_path: transcript });
  aaa("project", "create", "demo");
  const coordinator = aaa("project", "open", "demo", "--profile", "fake").pane.id;
  await waitOutput(coordinator, "fake>");
  fs.writeFileSync(`${dir}/hook.sh`, `${quote(AA)} hook --agent claude ${quote(hook)}\nprintf 'retention-ready>\\n'\n`);
  aaa("pane", "run", coordinator, `post-probe ${dir}/hook.sh`);
  await waitOutput(coordinator, "retention-ready>");
  aaa("tab", "create", "--machine", "labA", "--label", "retention-thread", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA, hook);
  const thread = await until("대화 pane hook", () => aaa("pane", "list").panes.find(p => p.name === "retention-thread" && p.agent?.harness === "claude"));
  return { coordinator, thread: thread.id, transcript, dir, hook, keys: aaa("pane", "list").panes.map(p => `${p.machine_id}:${p.id}`) };
}
