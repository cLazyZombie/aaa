// 실제 전용 thread 저장 → host 미디어 사본·읽음·푸시 → 두 브라우저의 아이디어 화면.
import assert from "node:assert/strict";
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { AA, env, root, base, aaa, until, step, waitOutput, showPanel, inList, webkit, pairPage, shot,
  pushDevice, setPushKey, nextPush, pushes, setInUse, pushInUse } from "../lib/harness.mjs";

export const labEnv = { AAA_TEST_NOTIFY_DEBOUNCE_MS: "100" };

const call = (page, request) => page.evaluate(async request => {
  const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(request) });
  const result = await r.json();
  if (!result.ok) throw new Error(JSON.stringify(result));
  return result.ok;
}, request);

export default async function ({ page }) {
  let note, thread, text, device;
  const save = (base, body) => {
    fs.writeFileSync(`${root}/idea-save.md`, body);
    return JSON.parse(execFileSync(AA, ["--json", "idea", "save", note.idea.short_id, "--base", String(base), "--file", `${root}/idea-save.md`],
      { env: { ...env, AAA_SESSION_ID: thread.pane }, encoding: "utf8" })).note;
  };
  await step("아이디어 전용 thread와 푸시 구독 준비", async () => {
    aaa("project", "create", "demo", "--thread-profile", "fake");
    note = (await call(page, { op: "idea_create", title: "그림 아이디어", text: "" })).note;
    await call(page, { op: "idea_say", idea: note.idea.short_id, text: "준비" });
    await until("전용 thread 준비", async () => {
      note = (await call(page, { op: "idea_read", idea: note.idea.short_id })).note;
      if (!note.idea.thread) return false;
      thread = aaa("thread", "show", note.idea.thread);
      return thread.pane && thread.work_dir;
    });
    await waitOutput(thread.pane, "idea-ready");
    fs.mkdirSync(`${thread.work_dir}/library`, { recursive: true });
    fs.copyFileSync(`${root}/pic.png`, `${thread.work_dir}/library/pic.png`);
    const info = await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok);
    setPushKey(info.key);
    device = pushDevice("push/idea");
    const status = await page.evaluate(async body => (await fetch("/api/v1/push/subscribe", {
      method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body),
    })).status, device.json);
    assert.equal(status, 200);
  });

  await step("thread 저장은 그림·링크를 보존하고 아이디어 숫자·푸시를 만든다", async () => {
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => await pushInUse(page) === 0);
    const from = pushes.length;
    note = save(1, "# 그림 아이디어\n[그림 링크](library/pic.png)\n\n![시안](library/pic.png)\n");
    assert.equal(note.rev, 2);
    assert.equal(note.idea.unseen, true);
    assert.doesNotMatch(note.text, /library\//);
    assert.match(note.text, /\[그림 링크\]\([0-9a-f]{16}\.png\)/);
    text = note.text;
    const toast = page.locator('.droplet[data-topic="ideas"][data-phase="in"]');
    await toast.locator(".droplet-card").waitFor();
    assert.equal(await toast.locator(".droplet-title").innerText(), "아이디어 갱신");
    assert.equal(await toast.locator(".droplet-detail").innerText(), `${note.idea.short_id} · 그림 아이디어`);
    assert.equal(await toast.locator(".droplet-mark svg").innerHTML(), await page.getByTestId("activity-ideas").locator("svg").innerHTML());
    await toast.evaluate(async el => { await Promise.all(el.getAnimations({ subtree: true }).map(a => a.finished)); });
    await shot(page, "120-idea-notice");
    const badge = page.getByTestId("activity-badge-ideas");
    await badge.waitFor();
    assert.equal(await badge.innerText(), "1");
    const { msg } = await nextPush(device, from, "아이디어 갱신");
    assert.equal(msg.url, `/#idea=${note.idea.short_id}`);
    assert.match(msg.title, /그림 아이디어/);
    await page.reload();
    await badge.waitFor();
    assert.equal(await badge.innerText(), "1", "새로 열어도 host의 새 글이 남는다");
  });

  await step("기존 library 노트도 그림·파란 링크로 열리고 실제 본 판만 확인한다", async () => {
    // 구판 데이터: 아직 해시 주소로 옮기지 않은 노트를 재현한다. 판과 시각은 그대로 둔다.
    const folder = `${root}/projects/demo/ideas/${note.idea.id}`;
    const old = "# 그림 아이디어\n[그림 링크](library/pic.png)\n\n![시안](library/pic.png)\n";
    for (const path of ["note.md", "revs/0002.md"]) fs.writeFileSync(`${folder}/${path}`, old);
    await showPanel(page, "ideas");
    await page.getByTestId("idea-row").filter({ hasText: note.idea.short_id }).click();
    const body = page.getByTestId("idea-note");
    await body.locator("img[alt=시안]").waitFor();
    await body.locator("img").evaluate(img => img.decode());
    const link = body.getByRole("link", { name: "그림 링크", exact: true });
    assert.equal(await link.getAttribute("href"), await body.locator("img").getAttribute("src"));
    assert.equal(await body.getAttribute("data-rev"), "2");
    await page.getByTestId("activity-badge-ideas").waitFor({ state: "detached" });
    assert.equal((await call(page, { op: "idea_read", idea: note.idea.short_id })).note.idea.unseen, false);
    assert.equal(fs.readFileSync(`${folder}/note.md`, "utf8"), text);
    fs.unlinkSync(`${thread.work_dir}/library/pic.png`);
    await page.reload();
    await page.getByTestId("idea-note").locator("img").waitFor();
    await page.getByTestId("idea-note").locator("img").evaluate(img => img.decode());
  });

  await step("넓은 창의 노트·대화를 드래그·키보드로 조절한다", async () => {
    const grip = page.getByRole("separator", { name: "노트와 대화 크기" });
    const pane = page.getByTestId("idea-note-pane");
    const before = await pane.evaluate(el => el.getBoundingClientRect().width);
    const box = await grip.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + 80);
    await page.mouse.down();
    await page.mouse.move(box.x - 100, box.y + 80);
    await page.mouse.up();
    assert.ok(await pane.evaluate(el => el.getBoundingClientRect().width) < before - 80);
    await grip.focus();
    await page.keyboard.press("Home");
    assert.equal(await grip.getAttribute("aria-valuenow"), "25");
    await page.keyboard.press("End");
    assert.equal(await grip.getAttribute("aria-valuenow"), "75");
    await grip.dblclick();
    assert.equal(await grip.getAttribute("aria-valuenow"), "60");
    await inList(page, "pane-t1");
    await page.getByTestId("pane-t1").click();
  });

  await step("원본이 사라진 기존 그림은 원인과 노트를 함께 남긴다", async () => {
    const folder = `${root}/projects/demo/ideas/${note.idea.id}`;
    const old = "# 그림 아이디어\n읽을 글\n\n![없는 그림](library/missing.png)\n";
    try {
      for (const path of ["note.md", "revs/0002.md"]) fs.writeFileSync(`${folder}/${path}`, old);
      await page.goto(`${base}/#idea=${note.idea.short_id}`);
      await page.getByTestId("idea-note").getByText("읽을 글", { exact: true }).waitFor();
      const woke = await call(page, { op: "idea_wake", idea: note.idea.short_id });
      assert.match(woke.note.media_error, /library\/missing\.png/, "깨우기 응답도 원본 경로와 이행 오류를 보존한다");
      await page.getByRole("alert").filter({ hasText: "그림을 옮기지 못했다" }).waitFor();
      assert.equal(fs.readFileSync(`${folder}/note.md`, "utf8"), old, "실패한 이행은 원문을 바꾸지 않는다");
    } finally {
      for (const path of ["note.md", "revs/0002.md"]) fs.writeFileSync(`${folder}/${path}`, text);
    }
    await inList(page, "pane-t1");
    await page.getByTestId("pane-t1").click();
  });

  await step("WebKit 휴대폰 대화에서는 새 판을 남기고 노트를 열면 확인한다", async () => {
    const wk = await webkit.launch();
    const phone = (await pairPage(wk, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true, hash: `#idea=${note.idea.short_id}` })).page;
    try {
      await phone.getByTestId("idea-note").locator("img").waitFor();
      await phone.getByTestId("idea-note").locator("img").evaluate(img => img.decode());
      assert.equal(await phone.getByTestId("idea-resizer").count(), 0);
      await phone.getByTestId("idea-tab-chat").click();
      note = save(2, `${text}\n- 셋째 판\n`);
      await phone.getByTestId("activity-badge-ideas").waitFor({ state: "attached" });
      assert.equal((await call(phone, { op: "idea_read", idea: note.idea.short_id })).note.idea.unseen, true);
      await phone.getByTestId("idea-tab-note").click();
      await phone.getByTestId("idea-note").getByText("셋째 판", { exact: true }).waitFor();
      await phone.getByTestId("activity-badge-ideas").waitFor({ state: "detached" });
      assert.equal((await call(phone, { op: "idea_read", idea: note.idea.short_id })).note.idea.unseen, false);
    } finally { await phone.context().close(); await wk.close(); }
  });
}
