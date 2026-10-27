import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, inList, domIs, browser, pairPage, shot, openSettings, inTime, drawerSettled } from "./harness.mjs";
import { clickDialogButton } from "./menu.mjs";
import { seedHost } from "./bulk-fixture.mjs";
import { pendingDeletionCleared } from "./bulk-delete-ready.mjs";

export const count = 500;
const png = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=", "base64");
const dir = () => `${root}/projects/demo`;
export function seedAsks(db, amount = count, prefix = "bulk") {
  const put = db.prepare("INSERT INTO asks(project,id,kind,subject,done_ms,data) VALUES('demo',?,'question',?,?,?)");
  db.exec("SAVEPOINT asks_fixture");
  try {
    for (let i = 0; i < amount; i++) {
      const id = `${prefix}-${i}`;
      const body = `보존한 원문 ${i}\n\n${"본문과 선택 근거. ".repeat(200)}\n![그림](library/pixel.png)`;
      const record = { id, kind: "question", subject: `t-${String(i + 1).padStart(4, "0")}`, title: `대량 답 ${i}`,
        asked_ms: Date.now(), done_ms: Date.now(), by: "시험 기기", summary: "1A", items: [{ text: "고르기", body, choices: [{ label: "선택", recommended: true }] }],
        body, files_base: `/api/v1/projects/demo/asks/${id}/` };
      const folder = `${dir()}/asks/${id}`;
      fs.mkdirSync(`${folder}/library`, { recursive: true });
      fs.writeFileSync(`${folder}/report.md`, body);
      fs.writeFileSync(`${folder}/library/pixel.png`, png);
      put.run(id, record.subject, record.done_ms, JSON.stringify(record));
    }
    db.exec("RELEASE asks_fixture");
  } catch (e) { db.exec("ROLLBACK TO asks_fixture; RELEASE asks_fixture"); throw e; }
}

// 서버 응답을 만든 뒤 전달만 보류한다. 실제 변경·파일 제거는 host가 수행한다.
export function responseGate() {
  let selected = null;
  const calls = [];
  const setup = ctx => ctx.route("**/api/v1/call", async route => {
    const req = route.request().postDataJSON();
    if (!selected || !selected.match(req)) return route.fallback();
    calls.push(req);
    const gate = selected;
    if (typeof gate.fail === "function" ? gate.fail(req, calls) : gate.fail) {
      gate.arrive();
      return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ err: { code: "unavailable", message: "실험실 일괄 저장 실패" } }) });
    }
    try {
      const started = performance.now();
      const response = await route.fetch();
      const elapsed = performance.now() - started;
      if (req.op === "library_delete") {
        const latest = fs.readFileSync(`${root}/host.log`, "utf8").split("\n").filter(l => l.includes("보고서 영구 삭제 계측")).at(-1);
        if (!latest) throw new Error("보고서 삭제의 DB·파일 계측이 없다");
        console.log("bulk-delete-profile", latest);
      }
      console.log("bulk-response", JSON.stringify({ op: req.op, ms: Math.round(elapsed), count: req.reports?.length ?? req.ids?.length }));
      if (req.op === "library_delete" && req.reports.length === 500) {
        assert.ok(elapsed < 10_000, `보고서 500개 삭제 ${Math.round(elapsed)}ms: 20초 상한의 절반 미만이어야 한다`);
      }
      gate.arrive();
      await gate.wait;
      await route.fulfill({ response });
    } catch (e) {
      gate.reject(new Error(`일괄 응답 전달 실패: ${req.op} (${e.name}): ${e.message}`, { cause: e }));
    }
  });
  const arm = (match, fail = false) => {
    const arrived = Promise.withResolvers(), release = Promise.withResolvers();
    selected = { match: typeof match === "string" ? req => req.op === match : match, fail, arrive: arrived.resolve, reject: arrived.reject, wait: release.promise };
    calls.length = 0;
    arrived.promise.catch(() => {});
    return { arrived: inTime(arrived.promise, 20_000, "host 일괄 응답"), release: release.resolve };
  };
  return { setup, arm, calls, clear: () => { selected = null; } };
}

export async function responsive(page, dialog = page.getByRole("alertdialog")) {
  await page.getByTestId("action-progress").waitFor();
  if (await dialog.count()) {
    assert.equal(await dialog.getByRole("button", { name: "취소", exact: true }).isDisabled(), true);
    await page.keyboard.press("Escape");
    assert.equal(await dialog.count(), 1, "진행 중 Escape가 창을 없애지 않는다");
  }
  // 고정 sleep 없이 frame과 실제 키 사건을 처리하는지 확인한다.
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => resolve())));
  await page.evaluate(() => {
    window.bulkKeyHandled = false;
    window.addEventListener("keydown", () => { window.bulkKeyHandled = true; }, { once: true });
  });
  await page.keyboard.press("Shift");
  await domIs(page, "일괄 처리 중 키 사건", () => window.bulkKeyHandled);
}

// 확인창 뒤 생긴 답도 host가 기록한다. 실행 중 host DB에 별도 writer를 열지 않는다.
async function answeredAfterDialog(page, label, width) {
  const call = async request => {
    const reply = await page.evaluate(async body => (await fetch("/api/v1/call", {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
    })).json(), request);
    assert.equal(reply.err, undefined, JSON.stringify(reply));
    return reply.ok;
  };
  const title = `늦은 답 ${label} ${width}`;
  const body = "## Questions\n### 1. 늦게 생긴 답\n확인창 뒤에 생성한 원문이다.\n![그림](library/later-pixel.png)\n- A 확인 (추천)\n";
  fs.mkdirSync(`${dir()}/library`, { recursive: true });
  fs.writeFileSync(`${dir()}/library/later-pixel.png`, png);
  const posted = await call({ op: "post", project: "demo", input: {
    post_id: crypto.randomBytes(16).toString("hex"), to: "user", post_type: "request",
    topic: "user-action", title, body, needs_user: true, refs: [],
  }});
  assert.equal(posted.kind, "post");
  assert.equal(posted.result.state, "accepted");
  const asks = await call({ op: "asks", project: "demo" });
  assert.equal(asks.kind, "asks");
  const request = asks.requests.find(r => r.title === title);
  assert.ok(request?.questions, "새 요청에 실제 질문이 있어야 한다");
  const answered = await call({ op: "ask_answer", project: "demo", item: request.id,
    body_hash: request.questions.report_hash, picks: [0], text: null });
  assert.equal(answered.kind, "asks");
  const late = answered.done.find(r => r.subject === request.id);
  assert.ok(late?.files_base, "host가 답과 원문 사본을 보존해야 한다");
  // API가 사본을 확정했으므로 시험용 원본은 남기지 않는다. 뒤의 보고서 폴더 빈 값 단언도 유지한다.
  fs.unlinkSync(`${dir()}/library/later-pixel.png`);
  return { id: late.id, body };
}

export async function askScenario(engine, label, width) {
  console.log(`bulk ${label} ${width}`);
  await seedHost(db => seedAsks(db));
  const db = new DatabaseSync(`${root}/s/host/host.db`, { readOnly: true });
  const gate = responseGate();
  const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: gate.setup });
  const page = paired.page;
  try {
    await page.getByTestId("terminal").waitFor();
    await inList(page, "activity-asks");
    await domIs(page, "500개 답한 기록", () => document.querySelector('[data-testid="asks-done-count"]')?.textContent === "500");
    assert.equal(await page.locator('.ask-row.is-done').count(), 50, "대량 기록은 첫 50개만 그린다");
    await inList(page, "asks-forget-all");
    const dialog = page.getByRole("alertdialog");
    if (width < 769) await page.evaluate(() => {
      window.bulkCancelBack = new Promise(resolve => addEventListener("popstate", resolve, { once: true }));
    });
    await clickDialogButton(dialog.getByRole("button", { name: "취소", exact: true }));
    if (width < 769) {
      await inTime(page.evaluate(() => window.bulkCancelBack.then(() => true)), 20_000, "취소한 확인 창의 내부 복귀");
      await drawerSettled(page);
      assert.equal(await page.getByTestId("asks-forget-all").isVisible(), true, "확인 창 취소가 답 목록을 닫지 않는다");
    }
    assert.equal(db.prepare("SELECT count(*) AS n FROM asks").get().n, count);
    await inList(page, "asks-forget-all");
    gate.arm("ask_forget", true);
    await clickDialogButton(dialog.getByRole("button", { name: "모두 지우기", exact: true }));
    await dialog.getByRole("alert").filter({ hasText: "실험실 일괄 저장 실패" }).waitFor();
    assert.equal(await dialog.getByRole("button", { name: "취소", exact: true }).isEnabled(), true);
    assert.equal(db.prepare("SELECT count(*) AS n FROM asks").get().n, count);
    // 창을 연 뒤 생긴 답은 실제 host API로 만들며 삭제 대상에는 넣지 않는다.
    const late = await answeredAfterDialog(page, label, width);
    assert.equal(db.prepare("SELECT count(*) AS n FROM asks").get().n, count + 1);
    const held = gate.arm("ask_forget");
    await clickDialogButton(dialog.getByRole("button", { name: "모두 지우기", exact: true }));
    try {
      await held.arrived;
      await responsive(page, dialog);
      await dialog.getByText(/답한 것 500개와/).waitFor();
      await shot(page, `bulk-asks-${label}-${width}`);
      assert.equal(gate.calls.length, 1);
      assert.equal(gate.calls[0].ids.length, count);
      assert.equal(gate.calls[0].all, false);
      assert.equal(db.prepare("SELECT count(*) AS n FROM asks").get().n, 1);
      assert.deepEqual(fs.readdirSync(`${dir()}/asks`), [late.id]);
      assert.equal(fs.readFileSync(`${dir()}/asks/${late.id}/report.md`, "utf8"), late.body);
      assert.deepEqual(fs.readFileSync(`${dir()}/asks/${late.id}/library/later-pixel.png`), png);
    } finally { held.release(); }
    await dialog.waitFor({ state: "hidden" });
    await domIs(page, "늦게 온 한 답만 남음", () => document.querySelector('[data-testid="asks-done-count"]')?.textContent === "1");
    gate.clear();
    await inList(page, "asks-forget-all");
    await clickDialogButton(dialog.getByRole("button", { name: "모두 지우기", exact: true }));
    await dialog.waitFor({ state: "hidden" });
    await domIs(page, "답 삭제 완료 목록", () => document.querySelector('[data-testid="asks-done-count"]')?.textContent === "0");
    assert.equal(fs.readdirSync(`${dir()}/asks`).length, 0);
    assert.deepEqual(paired.errors, []);
  } catch (e) { console.error(label, width, paired.errors, await page.locator("body").innerText()); throw e; }
  finally { await page.context().close(); db.close(); }
}

export function seedReports(db, amount, prefix, old = false, versions = false, { manifest = true } = {}) {
  const put = db.prepare("INSERT INTO library_reports(project,name,updated,to_user,title,sent_at) VALUES('demo',?,?,0,?,1)");
  const version = db.prepare("INSERT INTO report_ids(project,seq,id,source,revision,ready) VALUES('demo',?,?,?,'fixture',1)");
  const first = db.prepare("SELECT coalesce(max(seq),10000)+1 AS n FROM report_ids").get().n;
  // seedHost의 준비 transaction 안에서도 같은 원자적 fixture를 쓴다.
  db.exec("SAVEPOINT report_fixture");
  try {
    for (let i = 0; i < amount; i++) {
      const name = `${prefix}-${i}`;
      const names = versions ? [name, `r-${first + i}`] : [name];
      for (const n of names) {
        fs.mkdirSync(`${dir()}/library/${n}`, { recursive: true });
        fs.writeFileSync(`${dir()}/library/${n}/index.html`, `<h1>${name}</h1>`);
        if (manifest) fs.writeFileSync(`${dir()}/threads/${n}.files.json`, "[]");
        put.run(n, old ? 1 : Date.now(), name);
      }
      if (versions) version.run(first + i, names[1], name);
    }
    db.exec("RELEASE report_fixture");
  } catch (e) { db.exec("ROLLBACK TO report_fixture; RELEASE report_fixture"); throw e; }
}

export async function reportScenario(engine, label, width) {
  console.log(`bulk ${label} ${width}`);
  const total = 601; // host 한도 500개를 넘는다.
  await seedHost(db => seedReports(db, total, "batch", false, true, { manifest: false }));
  const db = new DatabaseSync(`${root}/s/host/host.db`, { readOnly: true });
  // 마이그레이션이 별도 사본을 만들지 않게 manifest는 host 재연결 뒤 둔다.
  for (const { name } of db.prepare("SELECT name FROM library_reports WHERE project='demo'").all())
    fs.writeFileSync(`${dir()}/threads/${name}.files.json`, "[]");
  const gate = responseGate();
  const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: gate.setup });
  const page = paired.page;
  try {
    await page.getByTestId("terminal").waitFor();
    await inList(page, "activity-reports");
    await inList(page, "reports-tab-work");
    await domIs(page, "작업 보고 601개", () => document.querySelector('[data-testid="reports-tab-work"]')?.textContent.includes("601"));
    assert.equal(await page.getByTestId("reports-mark-read").isDisabled(), true, "작업 보고는 안 봄 대상이 아니다");
    for (const tab of ["mine", "archive"]) {
      if (tab === "mine") {
        await seedHost(db => db.exec("UPDATE library_reports SET to_user=1, seen_at=NULL"));
        await page.getByTestId("reports-panel").getByRole("button", { name: "새로고침", exact: true }).click();
      }
      if (tab === "archive") {
        await seedHost(db => db.exec("UPDATE library_reports SET to_user=CASE WHEN name LIKE 'r-%' THEN 1 ELSE 0 END, seen_at=NULL"));
        await page.getByTestId("reports-panel").getByRole("button", { name: "새로고침", exact: true }).click();
        await inList(page, "reports-tab-work");
        await inList(page, "reports-pick-done");
        await domIs(page, "보관 대상 601개", () => document.querySelector('[data-testid="reports-picked"]')?.textContent.includes("601개 선택"));
        const held = gate.arm("library_archive");
        await page.locator('.reports-work-tools').getByRole("button", { name: "보관함에 넣기", exact: true }).click();
        try { await held.arrived; await responsive(page); await shot(page, `bulk-archive-${label}-${width}`); }
        finally { held.release(); }
        await domIs(page, "보관 진행 종료", () => !document.querySelector('[data-testid="action-progress"]'));
        assert.deepEqual(gate.calls.map(c => c.reports.length), [500, 101]);
        assert.equal(db.prepare("SELECT count(*) AS n FROM library_archives").get().n, total);
      }
      gate.clear();
      await inList(page, `reports-tab-${tab}`);
      await domIs(page, "읽음 대상 준비", () => !document.querySelector('[data-testid="reports-mark-read"]')?.disabled);
      await inList(page, "reports-mark-read");
      const dialog = page.getByRole("alertdialog");
      await dialog.getByText(/601개를 읽음/).waitFor();
      const held = gate.arm("library_mark_read");
      await clickDialogButton(dialog.getByRole("button", { name: "모두 읽음", exact: true }));
      try { await held.arrived; await responsive(page, dialog); }
      finally { held.release(); }
      await dialog.waitFor({ state: "hidden" });
      await domIs(page, "모두 읽음 목록 반영", () => document.querySelector('[data-testid="reports-mark-read"]')?.disabled);
      assert.equal(gate.calls.length, 1);
      assert.equal(gate.calls[0].reports.length, total);
      assert.equal(db.prepare("SELECT count(*) AS n FROM library_reports WHERE name LIKE 'r-%' AND seen_at IS NOT NULL").get().n, total);
    }
    // 보관 판이 있어도 작업 보고 원본에서 전체 영구 삭제할 수 있다.
    gate.clear();
    await inList(page, "reports-tab-work");
    await inList(page, "reports-pick-done");
    await inList(page, "reports-delete");
    const dialog = page.getByRole("alertdialog");
    const held = gate.arm("library_delete", (_req, calls) => calls.length === 2);
    await clickDialogButton(dialog.getByRole("button", { name: "영구히 지우기", exact: true }));
    try { await held.arrived; await responsive(page, dialog); await shot(page, `bulk-delete-${label}-${width}`); }
    finally { held.release(); }
    await dialog.getByRole("alert").getByText(/601개 중 500개 완료 후 중단: 실험실 일괄 저장 실패/).waitFor();
    assert.equal(gate.calls.length, 2, "실패 뒤 자동 재전송 없음");
    assert.equal(db.prepare("SELECT count(*) AS n FROM library_reports").get().n, 202);
    const rest = gate.arm("library_delete");
    await clickDialogButton(dialog.getByRole("button", { name: "영구히 지우기", exact: true }));
    try { await rest.arrived; assert.equal(gate.calls[0].reports.length, 101, "성공한 묶음은 다시 보내지 않는다"); }
    finally { rest.release(); }
    await dialog.waitFor({ state: "hidden" });
    await domIs(page, "삭제 목록 반영", () => document.querySelector('[data-testid="reports-tab-work"]')?.textContent.includes("0"));
    assert.deepEqual(gate.calls.map(c => c.reports.length), [101]);
    assert.equal(db.prepare("SELECT count(*) AS n FROM library_reports").get().n, 0);
    assert.equal(fs.readdirSync(`${dir()}/library`).length, 0);
    const hostId = db.prepare("SELECT value FROM meta WHERE key='host_id'").get().value;
    const pending = `${dir()}/.library-delete-${Buffer.from(hostId).toString("hex")}`;
    await pendingDeletionCleared(pending, `${root}/host.log`, inTime);
    console.log("bulk-delete-cleanup", JSON.stringify({ label, width, pending: fs.readdirSync(pending).length }));
    assert.deepEqual(paired.errors, []);
  } catch (e) { console.error(label, width, paired.errors, await page.locator("body").innerText()); throw e; }
  finally { await page.context().close(); db.close(); }
}

export async function maintenanceScenario(engine, label, width) {
  console.log(`bulk ${label} ${width}`);
  await seedHost(db => {
    // 기동의 구형 보고 마이그레이션이 별도 r 사본을 만들지 않게 빈 manifest는 재연결 뒤 둔다.
    seedReports(db, count, "prune", true, false, { manifest: false });
    const putLink = db.prepare("INSERT INTO library_links(id,project,thread,created,expires) VALUES(?,'demo',?,?,?)");
    for (let i = 0; i < count; i++) putLink.run(`bulk-link-${i}`, `prune-${i}`, Date.now(), Date.now() + 86400000);
  });
  for (let i = 0; i < count; i++) fs.writeFileSync(`${dir()}/threads/prune-${i}.files.json`, "[]");
  const db = new DatabaseSync(`${root}/s/host/host.db`, { readOnly: true });
  const gate = responseGate();
  const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: gate.setup });
  const page = paired.page;
  try {
    await page.getByTestId("terminal").waitFor();
    const settings = await openSettings(page, "이 aaa");
    await settings.getByTestId("link-list-load").click();
    await domIs(page, "살아 있는 공유 링크 500개", () => document.querySelectorAll('[data-testid="link-list-rows"] [data-state="active"]').length === 500);
    await settings.getByTestId("key-rotate-open").click();
    const dialog = page.getByRole("alertdialog");
    await dialog.getByText(/살아 있는 공유 링크 500개/).waitFor();
    const rotated = gate.arm("library_rotate");
    await clickDialogButton(dialog.getByRole("button", { name: "키 교체", exact: true }));
    try { await rotated.arrived; await responsive(page, dialog); }
    finally { rotated.release(); }
    await dialog.waitFor({ state: "hidden" });
    assert.equal(db.prepare("SELECT count(*) AS n FROM library_links WHERE revoked=0").get().n, 0);
    await domIs(page, "키 교체 뒤 열린 링크 목록 갱신", () => document.querySelectorAll('[data-testid="link-list-rows"] [data-state="revoked"]').length === 500);
    gate.clear();
    await settings.getByTestId("prune-preview").click();
    await settings.getByTestId("prune-run").waitFor();
    assert.match(await settings.getByTestId("prune-run").innerText(), /500개/);
    await settings.getByTestId("prune-run").click();
    const pruned = gate.arm("library_prune_apply");
    await clickDialogButton(dialog.getByRole("button", { name: "500개 지우기", exact: true }));
    try { await pruned.arrived; await responsive(page, dialog); await shot(page, `bulk-prune-${label}-${width}`); }
    finally { pruned.release(); }
    await dialog.waitFor({ state: "hidden" });
    await settings.getByTestId("clean-note").getByText("500개를 지웠다", { exact: true }).waitFor();
    assert.equal(fs.readdirSync(`${dir()}/library`).length, 0);
    await settings.getByTestId("prune-preview").click();
    await settings.getByText("지울 사본이 없다", { exact: true }).waitFor();
    await seedHost(db => db.exec("DELETE FROM library_links"));
    assert.deepEqual(paired.errors, []);
  } catch (e) { console.error(label, width, paired.errors, await page.locator("body").innerText()); throw e; }
  finally { await page.context().close(); db.close(); }
}
