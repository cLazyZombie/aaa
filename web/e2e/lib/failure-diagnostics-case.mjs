import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { failureDiagnostics, writeFailure } from "./failure-diagnostics.mjs";

export async function failureDiagnosticsCase(browser, type, parent = os.tmpdir()) {
  fs.mkdirSync(parent, { recursive: true });
  const directory = fs.mkdtempSync(path.join(parent, "aaa-failure-diag-"));
  let context, passed = false;
  try {
    context = await browser.newContext();
    const diagnostics = await failureDiagnostics(context, directory, type);
    const page = await context.newPage();
    diagnostics.watch(page);
    await context.route("**/*", route => route.fulfill({ contentType: "text/html; charset=utf-8", body: '<meta charset="utf-8"><button id="present">현재 화면</button>' }));
    await page.goto("http://127.0.0.1/fixture");
    await page.evaluate(() => { history.pushState({ "aaa:sheet": "fixture" }, "", "#sheet"); });
    let original;
    try {
      // 실패 진단의 입력이다. 없는 locator의 실제 call log를 생성하며 성공으로 넘기지 않는다.
      await page.getByRole("button", { name: "없는 버튼" }).waitFor({ timeout: 100 });
      assert.fail("없는 locator가 준비됐다");
    } catch (error) { original = error; }
    if (type === "webkit") page.screenshot = async () => { throw new Error("화면 저장 거부 fixture"); };
    await context.close(); // step catch보다 안쪽 finally가 먼저 닫는 경우
    await diagnostics.finish(); // 중복 호출도 끝난 첫 저장을 기다린다.
    writeFailure(directory, "failure", new Error("바깥 오류", { cause: original }));
    const error = fs.readFileSync(path.join(directory, "failure.txt"), "utf8");
    assert.match(error, /없는 버튼/);
    assert.match(error, /Call log:/);
    assert.match(error, /failure-diagnostics-case.mjs/);
    assert.match(fs.readFileSync(path.join(directory, `${type}-1-0.html`), "utf8"), /현재 화면/);
    const state = JSON.parse(fs.readFileSync(path.join(directory, `${type}-1-0.json`), "utf8"));
    assert.equal(state.state["aaa:sheet"], "fixture");
    assert.ok(state.events.some(e => e.event === "history.pushState"));
    if (type === "chromium") assert.ok(fs.statSync(path.join(directory, `${type}-1-0.png`)).size > 0);
    else assert.match(fs.readFileSync(path.join(directory, `${type}-events.json`), "utf8"), /화면 저장 거부 fixture/);
    const trace = execFileSync("unzip", ["-p", path.join(directory, `${type}-trace.zip`), "trace.trace"], { encoding: "utf8" });
    assert.match(trace, /없는 버튼/);
    assert.match(trace, /frame-snapshot/);
    passed = true;
  } finally {
    await context?.close();
    if (passed) fs.rmSync(directory, { recursive: true, force: true });
    else console.log(`진단 회귀 자료: ${directory}`);
  }
}
