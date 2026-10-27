// E2E12의 실제 CLI·IPC·HTTP·두 브라우저를 같은 대량 원장으로 검증한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { root, AA, env, aaa, domIs, browser, webkit, pairPage, inTime, specCleanup } from "./harness.mjs";
import { traceThreadRequests, threadPageTrace, threadTraceDocument } from "./thread-page-trace.mjs";
import { seedHost } from "./bulk-fixture.mjs";

export async function prepareThreadPages() {
  // 전체 stdout은 1 MiB보다 크다. Node execFileSync 기본 버퍼 대신 파일에 받는다.
  let call = 0;
  const list = async (...args) => {
    const path = `${root}/thread-list-${++call}.json`;
    const fd = fs.openSync(path, "w");
    try {
      const child = spawn(AA, ["--json", "thread", "list", ...args], { env, stdio: ["ignore", fd, "pipe"] });
      let stderr = "";
      child.stderr.on("data", bytes => { stderr += bytes; });
      const [code, signal] = await once(child, "exit");
      assert.equal(code, 0, `thread list ${args.join(" ")}: ${code}/${signal}: ${stderr}`);
    } finally { fs.closeSync(fd); }
    return JSON.parse(fs.readFileSync(path, "utf8"));
  };
  const before = (await list()).threads.length;
  await seedHost(db => {
    const original = JSON.parse(db.prepare("SELECT data FROM threads WHERE project='demo' ORDER BY id LIMIT 1").get().data);
    const put = db.prepare("INSERT INTO threads(project,id,status,data,updated_at) VALUES('demo',?,'resolved',?,1)");
    for (let i = 10000; i < 12500; i++) {
      const id = `t-${i}`;
      const record = { ...original, id, title: `규모 회귀 ${i}`, status: "resolved", role: "worker",
        pane: null, work_dir: null, cwd: null, launch: null, resolve_pending: null, park: null,
        questions: [], question_warning: null, report_hash: null, report_copy_hash: null, acked_hash: null,
        next: [], error: null, automation: null, automation_run: null,
        // 보고/Next 없는 독립 기록에 원본 thread의 후속 원장을 복제하지 않는다.
        task_clean: { ...original.task_clean, followups: [], followup_revisions: [] } };
      put.run(id, JSON.stringify(record));
    }
  });
  const all = (await list()).threads;
  assert.equal(all.length, before + 2500);
  assert.ok(Buffer.byteLength(JSON.stringify(all)) > 1024 * 1024, "한 frame보다 큰 목록이어야 한다");
  assert.equal(new Set(all.map(t => `${t.project}/${t.id}`)).size, all.length);
  assert.equal((await list("demo")).threads.length, all.length);
  assert.deepEqual((await list("--open")).threads.map(t => t.id), all.filter(t => t.status !== "resolved").map(t => t.id));
  assert.ok((await list("--role", "worker")).threads.every(t => t.role === "worker"));
  const first = await list("--status", "resolved", "--limit", "3");
  const next = await list("--status", "resolved", "--limit", "3", "--after", first.next_after);
  assert.equal(first.threads.length, 3);
  assert.equal(next.threads.length, 3);
  assert.deepEqual([...first.threads, ...next.threads].map(t => t.id), all.filter(t => t.status === "resolved").slice(0, 6).map(t => t.id));

  return all;
}

export async function threadPages(all, name) {
  const owned = name === "webkit" ? specCleanup.own(() => webkit.launch({ headless: true }), value => value.close()) : null;
  const engine = owned ? await owned.value : browser;
  try {
    const trace = threadPageTrace(`${root}/diagnostics`, engine.browserType().name());
    let armed = false, held = false, socket, server, release, arrive, restart, releaseNew;
    const holding = new Promise(resolve => { release = resolve; });
    const arrived = new Promise(resolve => { arrive = resolve; });
    const restarted = new Promise(resolve => { restart = resolve; });
    const newHolding = new Promise(resolve => { releaseNew = resolve; });
    let context, primary, routeError, cleaning = false, phase = "pairing";
    try {
      const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width: 1280, height: 800 }, {
        init: traceThreadRequests, onPage: trace.onPage,
        // WebKit에서도 실제 HTTP 전달을 가로채도록 이 목록 전용 context는 SW를 거치지 않는다.
        serviceWorkers: "block",
        wsRoute: ["**/api/v1/events", ws => { socket = ws; server = ws.connectToServer(); }],
        setup: async ctx => {
          // pairing 자체가 실패해도 이 context의 route부터 마친 뒤 닫는다.
          context = ctx;
          await context.route("**/api/v1/call", async route => {
            try {
              const request = route.request().postDataJSON();
              if (request?.op !== "threads") return await route.fallback();
              if (held && !request.after) {
                trace.record({ event: "new-first-held", after: request.after ?? null });
                restart();
                await newHolding;
              }
              const response = await route.fetch();
              const body = (await response.json()).ok;
              const oldFinal = armed && !held && body?.kind === "threads" && !body.next_after;
              if (oldFinal) {
                held = true;
                trace.record({ event: "old-final-held" });
                arrive();
                await holding;
              }
              await route.fulfill({ response });
              if (held && !oldFinal && body?.kind === "threads" && !body.next_after) {
                trace.record({ event: "new-final-delivered" });
              }
            } catch (error) {
              // route 해제 중의 늦은 fulfill 실패가 원래 단계 오류와 실험실 정리를 가리지 않는다.
              trace.record({ event: "route-failure", phase, cleaning, error: { message: error.message.split("\n")[0], stack: error.stack?.split("\n").filter(line => /^\s+at /.test(line)).join("\n") } });
              if (!cleaning) routeError ??= error;
            }
          });
        },
      });
      phase = "initial-list";
      await page.evaluate(() => { location.hash = "threads"; });
      await domIs(page, "초기 목록의 마지막 thread", () => !!document.querySelector('[data-testid="all-threads"] [data-testid="project-thread-t-12499"]'));
      await inTime(trace.drain(), 20000, "초기 문서의 완료 응답 본문 회수");
      armed = true;
      phase = "reload-last-response";
      await page.reload();
      await inTime(arrived, 20000, "reload 목록의 마지막 실제 응답");
      // host 응답은 바꾸지 않는다. 마지막 응답 전달 전에 실제 events 연결을 한 번 바꿔 두 읽기를 겹친다.
      await socket.close({ code: 1012, reason: "E2E12 재연결 경계" });
      await server.close();
      phase = "reconnected-list";
      await inTime(restarted, 20000, "새 events 연결의 첫 목록 요청");
      trace.record({ event: "old-final-released" });
      release();
      await domIs(page, "마지막 페이지의 끝난 thread", () => !!document.querySelector('[data-testid="all-threads"] [data-testid="project-thread-t-12499"]'));
      // 새 연결의 첫 응답이 아직 없더라도 옛 연결의 완전한 목록은 보여야 한다.
      trace.record({ event: "old-complete-visible-before-new-response" });
      trace.record({ event: "new-first-released" });
      const doc = await page.evaluate(threadTraceDocument);
      releaseNew();
      await inTime(trace.completed(doc, 2), 20000, "재연결 HTTP 두 완전 목록의 본문 수집");
      const expected = all.map(t => `${t.project}/${t.id}`);
      phase = "trace-and-dom";
      const reads = await inTime(trace.reads(expected), 20000, "모든 관측 요청의 본문·취소 회수");
      // 초기 문서는 따로 검사하며 reload 문서의 두 독립 읽기는 각각 전체 목록이어야 한다.
      assert.ok(reads.filter(r => r.doc === doc && r.next === null).length >= 2);
      const rendered = await page.locator('[data-testid="all-threads"] [data-testid^="project-thread-"]').evaluateAll(rows => rows.map(row => row.dataset.testid));
      assert.equal(rendered.length, all.length);
      assert.equal(new Set(rendered).size, all.length);
      assert.deepEqual(rendered.sort(), all.map(t => `project-thread-${t.id}`).sort());
      assert.deepEqual(errors, []);
    } catch (error) {
      primary = routeError ?? error;
      // 추가 진단과 정리보다 먼저 남긴다. 닫는 중 생긴 오류가 최초 단계를 가리지 않는다.
      trace.record({ event: "failure", phase, original: { message: error.message, stack: error.stack } });
      throw primary;
    } finally {
      cleaning = true;
      release();
      releaseNew();
      let cleanupError;
      if (context) for (const [part, close] of [
        ["routes", () => context.unrouteAll({ behavior: "wait" })],
        ["context", () => context.close()],
      ]) {
        try { await close(); trace.record({ event: "cleanup", part }); }
        catch (error) {
          trace.record({ event: "cleanup-failure", part, error: String(error) });
          cleanupError ??= error;
        }
      }
      if (!primary && routeError) throw routeError;
      if (!primary && cleanupError) throw cleanupError;
    }
  } finally { await owned?.close(); }
}
