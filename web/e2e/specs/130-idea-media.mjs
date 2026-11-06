// 아이디어 그림(t-1575): 웹 「그림」 단추·aaa idea attach로 붙이고 그림 메뉴로 고친 뒤, 그 아이디어를 할 일로 넘겨
// 웹 할 일 화면의 첨부와 실제 thread 작업 폴더(attachments/·task·brief)에서 그림이 깨지지 않는지 본다.
import assert from "node:assert/strict";
import fs from "node:fs";
import zlib from "node:zlib";
import { aaa, base, root, step, until, shot, webkit, pairPage } from "../lib/harness.mjs";

/** 단색 PNG(w×h): 화면에서 그림 자리·설명·메뉴가 실제 크기로 보이게 */
function png(w, h, [r, g, b]) {
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (buf) => { let c = 0xffffffff; for (const x of buf) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0); body.copy(out, 4); out.writeUInt32BE(crc(body), body.length + 4);
    return out;
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(w, 0); head.writeUInt32BE(h, 4); head[8] = 8; head[9] = 2;
  const row = Buffer.concat([Buffer.from([0]), Buffer.from(Array.from({ length: w }, () => [r, g, b]).flat())]);
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", head),
    chunk("IDAT", zlib.deflateSync(Buffer.concat(Array(h).fill(row)))), chunk("IEND", Buffer.alloc(0))]);
}

const call = (page, request) => page.evaluate(async request => {
  const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(request) });
  const result = await r.json();
  if (!result.ok) throw new Error(JSON.stringify(result));
  return result.ok;
}, request);

export default async function ({ page }) {
  let note;
  const project = `${root}/projects/demo`;
  const front = `${root}/idea-front.png`, night = `${root}/idea-night.png`, extra = `${root}/idea-extra.png`;
  fs.writeFileSync(front, png(320, 180, [214, 120, 40]));
  fs.writeFileSync(night, png(320, 180, [40, 70, 160]));
  fs.writeFileSync(extra, png(160, 90, [90, 90, 90]));
  fs.writeFileSync(`${root}/idea-note.txt`, "글");
  const read = async () => (note = (await call(page, { op: "idea_read", idea: note.idea.short_id })).note);
  const images = () => page.getByTestId("idea-note").locator("img");

  await step("그림 아이디어 준비", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    note = (await call(page, { op: "idea_create", title: "간판", text: "# 간판\n\n## 시안\n- 첫 생각\n\n## 결정\n- 밝게\n" })).note;
    await page.goto(`${base}/#idea=${note.idea.short_id}`);
    await page.getByTestId("idea-note").getByText("첫 생각", { exact: true }).waitFor();
  }, { always: true });

  await step("그림 단추로 고른 그림은 노트 끝에 새 판으로 붙고 못 받는 파일은 까닭을 보인다", async () => {
    await page.getByTestId("idea-media-input").setInputFiles(front);
    await images().first().waitFor();
    await images().first().evaluate(img => img.decode());
    await read();
    assert.equal(note.rev, 2, "고르면 바로 저장");
    assert.match(note.text, /- 밝게\n\n!\[그림\]\([0-9a-f]{16}\.png\)\n$/, "노트 끝, 빈 줄 뒤");
    assert.deepEqual([note.media[0].source, note.media[0].by, note.media[0].file], ["web", "사람", "idea-front.png"]);
    await page.getByTestId("idea-media-input").setInputFiles(`${root}/idea-note.txt`);
    const failed = page.getByTestId("idea-upload").filter({ hasText: "idea-note.txt" });
    await failed.getByText("그림(PNG·JPEG·GIF·WebP)·영상(MP4·WebM)만 올린다").waitFor();
    assert.equal((await read()).rev, 2, "못 받는 파일은 판을 만들지 않는다");
    await failed.getByRole("button", { name: "닫기" }).click();
  });

  await step("aaa idea attach는 고른 절 끝에 설명과 함께 붙인다", async () => {
    const r = aaa("idea", "attach", note.idea.short_id, night, "--alt", "밤 시안", "--section", "시안");
    assert.equal(r.note.rev, 3);
    assert.match(r.note.text, new RegExp(`- 첫 생각\\n\\n!\\[밤 시안\\]\\(${r.names[0]}\\)\\n\\n## 결정`));
    assert.equal(r.note.media.find(m => m.name === r.names[0]).source, "file");
    await page.getByTestId("markdown-caption").filter({ hasText: "밤 시안" }).waitFor();
    assert.equal(await images().count(), 2);
  });

  await step("그림 메뉴로 설명·순서·지우기를 새 판으로 고친다", async () => {
    const menuOf = (i) => page.locator(".idea-media-note").nth(i);
    // 둘째 그림(노트 끝의 웹 그림): 설명
    await until("그림 단추 둘", async () => await page.getByTestId("idea-media-menu-open").count() === 2);
    await menuOf(1).getByTestId("idea-media-menu-open").click();
    await page.getByTestId("idea-media-source").filter({ hasText: "웹 · 사람 · idea-front.png" }).waitFor();
    await page.getByTestId("idea-media-alt").click();
    await page.getByTestId("idea-media-alt-input").fill("가게 앞");
    await page.getByTestId("idea-media-alt-save").click();
    await page.getByTestId("markdown-caption").filter({ hasText: "가게 앞" }).waitFor();
    // 「밤 시안」을 한 칸 아래로: 「## 결정」 머리 아래로 간다
    await menuOf(0).getByTestId("idea-media-menu-open").click();
    await page.getByTestId("idea-media-down").click();
    await until("순서 바뀜", async () => /## 결정\n\n!\[밤 시안\]\(\w+\.png\)\n- 밝게/.test((await read()).text));
    // 셋째 그림을 붙였다가 메뉴로 지운다(확인 한 번 더)
    aaa("idea", "attach", note.idea.short_id, extra, "--alt", "지울 것");
    await page.getByTestId("markdown-caption").filter({ hasText: "지울 것" }).waitFor();
    await until("그림 단추 셋", async () => await page.getByTestId("idea-media-menu-open").count() === 3);
    await menuOf(2).getByTestId("idea-media-menu-open").click();
    await page.getByTestId("idea-media-remove").click();
    await page.getByTestId("idea-media-remove-confirm").click();
    await page.getByTestId("markdown-caption").filter({ hasText: "지울 것" }).waitFor({ state: "detached" });
    await read();
    assert.equal(note.media.length, 2);
    assert.doesNotMatch(note.text, /지울 것/);
    assert.deepEqual(note.history.slice(0, 4).map(h => h.by), ["user", "user", "user", "user"], "고칠 때마다 판");
    await shot(page, "130-idea-media");
  });

  await step("WebKit 휴대폰에서 그림 단추로 붙이고 그림 메뉴를 연다", async () => {
    const wk = await webkit.launch();
    const phone = (await pairPage(wk, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true, hash: `#idea=${note.idea.short_id}` })).page;
    try {
      const before = (await read()).rev;
      await phone.getByTestId("idea-media-add").waitFor();
      await phone.getByTestId("idea-media-input").setInputFiles(extra);
      await until("휴대폰 붙이기 새 판", async () => (await read()).rev === before + 1 && note.media.length === 3);
      const last = phone.locator(".idea-media-note").nth(2);
      await last.getByTestId("idea-media-menu-open").click();
      await phone.getByTestId("idea-media-menu").waitFor();
      await phone.getByTestId("idea-media-menu").scrollIntoViewIfNeeded();
      await shot(phone, "130-idea-phone");
      await phone.getByTestId("idea-media-remove").click();
      await phone.getByTestId("idea-media-remove-confirm").click();
      await until("휴대폰 지우기", async () => (await read()).media.length === 2);
    } finally { await phone.context().close(); await wk.close(); }
  });

  await step("그림 든 아이디어를 넘기면 할 일 화면과 thread 작업 폴더에서 그림이 보인다", async () => {
    const done = (await call(page, { op: "idea_handoff", idea: note.idea.short_id, base_rev: note.rev,
      tasks: [{ title: "간판 만들기", repo: null, notes: "정리본대로 만든다." }] })).note;
    const [short] = done.idea.tasks;
    const task = aaa("task", "list").tasks.find(t => t.short_id === short);
    const names = note.media.map(m => m.name);
    const memoImages = task.notes.filter(l => l.startsWith("!["));
    assert.deepEqual(memoImages.sort(), names.map(n => `![${n === note.media.find(m => m.source === "web").name ? "가게 앞" : "밤 시안"}](uploads/${n})`).sort());
    for (const n of names) assert.deepEqual(fs.readFileSync(`${project}/uploads/${n}`), fs.readFileSync(`${project}/ideas/${note.idea.id}/${n}`));
    // 웹 할 일 상세: 첨부 썸네일 둘이 실제로 그려진다
    await page.goto(`${base}/#task=${short}`);
    const items = page.getByTestId("task-attach-item");
    await until("할 일 첨부 둘", async () => await items.count() === 2);
    for (let i = 0; i < 2; i++) await items.nth(i).locator("img").evaluate(img => img.decode());
    await shot(page, "130-idea-task");
    // 그 할 일로 thread를 띄우면 그림·노트가 attachments/로 가고 task·brief·노트의 그림 링크가 그 폴더를 가리킨다
    const started = aaa("thread", "start", "--from-task", "간판 만들기", "--link-task", "--machine", "labA", "--profile", "fake", "--kind", "dir");
    const thread = aaa("thread", "show", started.id);
    for (const n of names) {
      assert.deepEqual(fs.readFileSync(`${thread.work_dir}/attachments/${n}`), fs.readFileSync(`${project}/ideas/${note.idea.id}/${n}`), `${n} 사본`);
    }
    const text = fs.readFileSync(`${project}/threads/${started.id}.task.md`, "utf8");
    for (const n of names) assert.ok(text.includes(`](attachments/${n})`), `task의 ${n}`);
    const noteLink = /\[아이디어 n-\d+\]\(attachments\/([0-9a-f]{16}\.md)\)/.exec(text);
    assert.ok(noteLink, text);
    const copied = fs.readFileSync(`${thread.work_dir}/attachments/${noteLink[1]}`, "utf8");
    for (const n of names) assert.ok(copied.includes(`](${n})`), "노트의 그림은 같은 폴더 이름");
    await until("thread 보고", () => aaa("thread", "show", started.id).report_hash);
    const brief = fs.readFileSync(`${thread.work_dir}/brief.md`, "utf8");
    for (const n of names) assert.ok(brief.includes(`](attachments/${n})`), `brief의 ${n}`);
    aaa("thread", "resolve", started.id);
  });
}
