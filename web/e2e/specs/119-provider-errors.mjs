// native JSONL → agent → host → 웹. 실제 공급자·로그인·민감 로그는 쓰지 않는다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, aaa, step, until, waitOutput, inList, webkit, pairPage } from "../lib/harness.mjs";

export const labEnv = { CODEX_HOME: `${root}/codex` };

export default async function ({ page }) {
  await step("native 공급자 오류 시험 준비", async () => {
    fs.mkdirSync(`${root}/native-bin`, { recursive: true });
    for (const name of ["claude", "codex"]) {
      // 실제 PID·기록 연결 fixture를 재사용하되 메타데이터 flush에 필요한 최소 한 줄만 만든다.
      const fixture = fs.readFileSync(new URL("../lib/fake-shell-harness.py", import.meta.url), "utf8").replace("messages=360", "messages=1");
      fs.writeFileSync(`${root}/native-bin/${name}`, fixture, { mode: 0o755 });
    }
  });
  let wk, phone;
  await step("native 오류 휴대폰 WebKit 준비", async () => {
    wk = await webkit.launch();
    phone = (await pairPage(wk, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true })).page;
  });
  try {
  for (const [index, harness] of ["claude", "codex"].entries()) {
    const id = `01900000-0000-7000-8000-${String(index + 1).padStart(12, "0")}`;
    const name = `native-${harness}`;
    const path = harness === "claude" ? `${root}/claude/projects/shell/${id}.jsonl`
      : `${root}/codex/sessions/2026/10/23/rollout-shell-${id}_01900000-0000-7000-9000-000000000001.jsonl`;
    let pane;
    const state = () => aaa("pane", "list").panes.find((p) => p.id === pane)?.agent;
    const append = (...entries) => fs.appendFileSync(path, entries.map((v) => JSON.stringify({ timestamp: new Date().toISOString(), ...v })).join("\n") + "\n");
    let turn = 0;
    const start = () => harness === "claude" ? { type: "user", sessionId: id, message: { content: `작업 ${++turn}` } }
      : { type: "event_msg", payload: { type: "task_started", turn_id: `turn-${++turn}` } };
    const end = (code) => harness === "claude" ? { type: "assistant", sessionId: id, isApiErrorMessage: true, error: code, message: { content: [{ type: "text", text: "로그 원문은 웹에 보내지 않는다" }] } }
      : { type: "event_msg", payload: { type: "task_complete", turn_id: `turn-${turn}`, error: { codex_error_info: code, message: "로그 원문은 웹에 보내지 않는다" } } };

    await step(`${harness} native 오류 fixture 준비`, async () => {
      pane = aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh").tabs.at(-1).root.pane;
      aaa("pane", "run", pane, `${root}/native-bin/${harness} ${root} ${id}`);
      await waitOutput(pane, "foreground-harness-ready");
      aaa("pane", "run", pane, "open");
      await until("PID와 native 기록 연결", () => state()?.harness_session === id);
      await inList(page, `pane-${name}`);
      await inList(phone, `pane-${name}`);
      if (harness === "codex") append({ type: "session_meta", payload: { id } });
    });

    const cases = [
      ["overloaded", "server_overloaded", "overloaded", "공급자 과부하"],
      ["authentication_failed", "unauthorized", "authentication", "로그인 필요"],
      ["billing_error", "usage_limit_exceeded", "allowance", "구독·결제 한도"],
      ["model_not_found", "model_not_found", "model", "모델 오류"],
      ["future_error", "future_error", "unknown", "공급자 오류 · 원인 미확인"],
    ];
    for (const [claude, codex, kind, label] of cases) {
      await step(`${harness} native 원인 ${kind}`, async () => {
        append(start(), end(harness === "claude" ? claude : codex));
        await page.getByTestId("provider-error").filter({ hasText: label }).waitFor();
        const mobile = phone.getByTestId("provider-error");
        await mobile.filter({ hasText: label }).waitFor();
        assert.equal(await mobile.getAttribute("data-kind"), kind);
        const bounds = await mobile.evaluate((el) => ({ width: el.clientWidth, content: el.scrollWidth, right: el.getBoundingClientRect().right, viewport: innerWidth }));
        assert.ok(bounds.content <= bounds.width && bounds.right <= bounds.viewport, JSON.stringify(bounds));
        const banner = page.getByTestId("provider-error");
        assert.equal(await banner.getAttribute("data-kind"), kind);
        const text = await banner.innerText();
        assert.match(text, /완료 .+ · 실행 /);
        assert.ok(!text.includes("로그 원문"));
        assert.equal(state().provider_turn.error.kind, kind);
      });
    }
    await step(`${harness} 새 native turn이 지난 오류를 걷는다`, async () => {
      append(start());
      await page.getByTestId("provider-error").waitFor({ state: "detached" });
      await phone.getByTestId("provider-error").waitFor({ state: "detached" });
      assert.equal(state().provider_turn.error, null);
    });
    await step(`${harness} 다른 세션과 도구 오류로 현재 turn을 덮지 않는다`, async () => {
      const other = "01900000-0000-7000-8000-000000000099";
      if (harness === "claude") {
        append({ ...end("overloaded"), sessionId: other }, { type: "user", sessionId: id, message: { content: [{ type: "tool_result", is_error: true, content: "overloaded" }] } }, { type: "system", sessionId: id, subtype: "turn_duration" });
      } else {
        append({ type: "event_msg", payload: { type: "task_complete", turn_id: "old-turn", error: { codex_error_info: "server_overloaded" } } }, { type: "event_msg", payload: { type: "task_complete", turn_id: `turn-${turn}` } });
      }
      // 정상 완료 관측을 울타리로 삼는다. 부재를 기다리는 고정 sleep은 쓰지 않는다.
      await until("현재 native turn 정상 완료", () => state()?.provider_turn?.finished_ms != null);
      assert.equal(state().provider_turn.error, null);
      assert.equal(await page.getByTestId("provider-error").count(), 0);
      assert.equal(await phone.getByTestId("provider-error").count(), 0);
      aaa("pane", "run", pane, "quit");
      await waitOutput(pane, "foreground-harness-exited");
    });
  }
  } finally {
    await phone.context().close();
    await wk.close();
  }
}
