// A4 이전 판의 세션 기록을 실제 agent가 복원한다. 운영 상태에는 닿지 않는다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { aaa, domIs, root, shot, showPanel, start, step, stop, until, waitOutput } from "../lib/harness.mjs";

export default async function ({ page }) {
  await step("A4 뒤 옛 평가자·임무 종료 기록 18개는 기타에서 숨기고, 실제 실행 중 pane·사용자 종료 기록·직접 읽기는 보존한다", async () => {
    const sessions = `${root}/s/agent/sessions`;
    const user = aaa("pane", "list").panes.find((p) => p.name === "t1");
    const userMeta = fs.readFileSync(`${sessions}/${user.id}/meta.json`);
    aaa("tab", "create", "--machine", "labA", "--label", "mission-user", "--", "/bin/sh", "-c", "printf 'retained-history\\n'; exit 1");
    const ended = await until("사용자 프로그램 종료", () => aaa("pane", "list").panes.find((p) => p.name === "mission-user" && p.state === "exited"));
    const template = `${sessions}/${ended.id}`;
    await stop("agent");
    const original = JSON.parse(fs.readFileSync(`${template}/spec.json`, "utf8"));
    const saved = new Map();
    const legacy = [];
    for (let i = 0; i < 19; i++) {
      const id = "0".repeat(24) + i.toString(16).padStart(2, "0").toUpperCase();
      const dir = `${sessions}/${id}`;
      const record = structuredClone(original);
      record.id = id;
      record.spec.name = i === 18 ? "legacy-live" : `${i % 2 ? "mission" : "evaluator"}-${i}`;
      record.spec.role = "evaluator"; // 옛 임무도 같은 역할을 썼다.
      record.spec.argv = ["/bin/sh"];
      fs.mkdirSync(dir);
      fs.writeFileSync(`${dir}/spec.json`, JSON.stringify(record));
      for (const name of ["exit.json", "final.json", "final.replay"]) fs.copyFileSync(`${template}/${name}`, `${dir}/${name}`);
      if (i < 18) {
        legacy.push(id);
        for (const name of fs.readdirSync(dir)) saved.set(`${dir}/${name}`, fs.readFileSync(`${dir}/${name}`));
      }
    }
    start("agent2", "agent", "--host", "local", "--name", "labA");
    await until("옛 세션 기록 복원", () => aaa("pane", "list").panes.filter((p) => p.role === "evaluator").length === 19);
    const live = aaa("pane", "resume", "0".repeat(24) + "12");
    assert.equal(live.role, "evaluator");
    assert.equal(live.state, "running");
    aaa("pane", "run", live.id, "echo live-$((40+2))");
    await waitOutput(live.id, "live-42");

    await page.reload();
    await page.getByTestId("pane-legacy-live").waitFor();
    await page.getByTestId("pane-mission-user").waitFor();
    await page.getByTestId("pane-t1").waitFor();
    // t-0540·W4의 새 막대에서도 같은 작업 목록을 쓴다. 다른 패널을 오가도 옛 기록이 되살아나지 않는다.
    assert.deepEqual(await page.getByRole("tablist", { name: "사이드바 패널" }).getByRole("tab").evaluateAll((tabs) => tabs.map((t) => t.dataset.testid)), ["activity-asks", "activity-work", "activity-machines", "activity-automations", "activity-inbox"]);
    for (const panel of ["asks", "inbox", "work"]) await showPanel(page, panel);
    // 보존할 줄을 그린 뒤, 같은 목록의 숨김 결과를 검사한다(고정 sleep 없음).
    const visible = await page.locator('[data-panel="work"] [data-pane]').evaluateAll((rows) => rows.map((r) => r.dataset.pane).sort());
    assert.deepEqual(visible, [user.id, ended.id, live.id].sort());
    assert.equal(aaa("pane", "list").panes.filter((p) => legacy.includes(p.id) && p.state === "exited").length, 18);
    for (const [file, bytes] of saved) assert.deepEqual(fs.readFileSync(file), bytes, `종료 기록 보존: ${file}`);
    assert.deepEqual(fs.readFileSync(`${sessions}/${user.id}/meta.json`), userMeta);
    assert.ok(aaa("pane", "read", legacy[0]).text.includes("retained-history"));
    // 숨겨진 pane도 주소로 직접 열 수 있다. 목록에서만 제외한다.
    await page.evaluate((id) => { location.hash = `pane=${encodeURIComponent(id)}`; }, legacy[0]);
    await domIs(page, "숨긴 pane의 기록 직접 보기", () => document.querySelector(".xterm-rows")?.textContent.includes("retained-history"));
    await shot(page, "legacy-special-hidden-history-kept");
  });
}
