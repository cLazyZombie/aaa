// 필요: 아이디어 진행 모델(t-1581, 사용자 11-07 1A·2A). 웹 대화 머리 칩은 host 머신이 맡을 수 있는 profile만 고르고, 고른 뒤에는
// 지금 thread를 두었다가 다음 말에 앞 thread를 알림 없이 끝내고 고른 profile의 새 thread가 노트를 읽고 잇는다. thread 세션 안
// CLI는 모델을 바꾸지 못한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { AA, env, root, base, aaa, until, step, waitOutput, shot } from "../lib/harness.mjs";

const config = `${root}/c/host.toml`;
// 둘째 모델: 같은 가짜 agent를 다른 profile 이름으로 띄운다. 허용 목록에 없는 profile도 하나 둔다(고르기 목록에 나오면 안 된다)
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "fake-b"]')
  + ["fake-b", "fake-off"].map((p) => `\n[profiles.${p}]\nagent = "custom"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`).join(""));

const call = (page, request) => page.evaluate(async request => {
  const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(request) });
  const result = await r.json();
  if (!result.ok) throw new Error(JSON.stringify(result));
  return result.ok;
}, request);

export default async function ({ page }) {
  let note, first;
  const read = async () => (note = (await call(page, { op: "idea_read", idea: note.idea.short_id })).note);
  const chip = () => page.getByTestId("idea-model");

  await step("첫 말은 프로젝트 기본 모델이 맡는다", async () => {
    aaa("project", "create", "demo", "--thread-profile", "fake");
    note = (await call(page, { op: "idea_create", title: "알림 묶기", text: "# 알림 묶기\n- 5분 창\n" })).note;
    assert.equal(note.idea.profile, "fake", "고르지 않으면 프로젝트 기본");
    await page.goto(`${base}/#idea=${note.idea.short_id}`);
    await until("칩이 기본 모델", async () => await chip().inputValue() === "fake");
    await until("host 머신 profile만", async () =>
      JSON.stringify(await chip().locator("option").allInnerTexts()) === JSON.stringify(["fake", "fake-b"]));
    await call(page, { op: "idea_say", idea: note.idea.short_id, text: "준비" });
    await until("전용 thread 준비", async () => {
      await read();
      if (!note.idea.thread) return false;
      first = aaa("thread", "show", note.idea.thread);
      return first.pane;
    });
    await waitOutput(first.pane, "idea-ready");
    assert.equal(first.profile, "fake");
  }, { always: true });

  await step("칩으로 고르면 지금 thread는 두고 다음 말을 기다린다", async () => {
    await chip().selectOption("fake-b");
    await page.getByTestId("idea-model-from").filter({ hasText: "fake →" }).waitFor();
    await read();
    assert.equal(note.idea.profile, "fake-b");
    assert.equal(note.idea.thread, first.id, "고르기만으로는 thread를 바꾸지 않는다");
    assert.equal(aaa("thread", "show", first.id).status, "open");
    const shown = execFileSync(AA, ["idea", "show", note.idea.short_id], { env, encoding: "utf8" }).split("\n")[0];
    assert.match(shown, /· 모델 fake-b · 알림 묶기$/);
    await shot(page, "idea-model-chip");
  });

  await step("다음 말은 고른 모델의 새 thread가 노트를 읽고 받는다", async () => {
    await call(page, { op: "idea_say", idea: note.idea.short_id, text: "3분으로 줄여" });
    let next;
    await until("새 thread", async () => {
      await read();
      if (!note.idea.thread || note.idea.thread === first.id) return false;
      next = aaa("thread", "show", note.idea.thread);
      return next.pane;
    });
    assert.equal(next.profile, "fake-b");
    await waitOutput(next.pane, "idea-ready");
    await until("앞 thread 끝남", () => aaa("thread", "show", first.id).status === "resolved");
    const brief = fs.readFileSync(`${next.work_dir}/brief.md`, "utf8");
    assert.match(brief, /진행 모델을 바꿔\(fake에서\) 새 대화로 시작했다/);
    await until("보낸 말 전달", async () => (await read()).pending.length === 0);
    await waitOutput(next.pane, "3분으로 줄여");
    const items = aaa("inbox", "list", "demo", "--all", "--peek").items.filter((i) => i.subject === first.id);
    assert.deepEqual(items.map((i) => i.event), [], "아이디어 thread 끝냄은 알림함에 내지 않는다");
    await until("칩의 앞 모델 사라짐", async () => await page.getByTestId("idea-model-from").count() === 0);
    assert.equal(await chip().inputValue(), "fake-b");
  });

  await step("고를 수 없는 profile과 thread 세션의 바꾸기는 거절한다", async () => {
    await assert.rejects(call(page, { op: "idea_model", idea: note.idea.short_id, profile: "fake-off" }), /허용되지 않았다/);
    const thread = aaa("thread", "show", note.idea.thread);
    const own = () => execFileSync(AA, ["idea", "model", note.idea.short_id, "fake"], { env: { ...env, AAA_SESSION_ID: thread.pane }, encoding: "utf8", stdio: "pipe" });
    assert.throws(own, (e) => /웹이나 aaa 밖 CLI|보기 권한/.test(String(e.stdout) + String(e.stderr)));
    assert.equal((await read()).idea.profile, "fake-b");
  });
}
