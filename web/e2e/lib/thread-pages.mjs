// E2E12의 실제 CLI·IPC·HTTP·두 브라우저를 같은 대량 원장으로 검증한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { root, AA, env, aaa, domIs, browser, webkit, pairPage } from "./harness.mjs";
import { seedHost } from "./bulk-fixture.mjs";

export async function threadPages() {
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
        next: [], error: null, automation: null, automation_run: null };
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

  const safari = await webkit.launch({ headless: true });
  try {
    for (const engine of [browser, safari]) {
      const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width: 1280, height: 800 });
      const replies = [];
      try {
        await page.evaluate(() => { location.hash = "threads"; });
        // pairPage는 머신 목록까지만 기다린다. 초기 문서의 목록 읽기가 끝난 뒤 reload 응답만 관측한다.
        await domIs(page, "초기 목록의 마지막 thread", () => !!document.querySelector('[data-testid="all-threads"] [data-testid="project-thread-t-12499"]'));
        page.on("response", response => {
          if (response.url().endsWith("/api/v1/call") && response.request().postDataJSON()?.op === "threads") {
            replies.push(response.body().then(bytes => {
              assert.ok(bytes.length < 1024 * 1024);
              const body = JSON.parse(bytes).ok;
              assert.equal(body.kind, "threads");
              assert.ok(body.threads.length <= 200);
              return body;
            }));
          }
        });
        await page.reload();
        await domIs(page, "마지막 페이지의 끝난 thread", () => !!document.querySelector('[data-testid="all-threads"] [data-testid="project-thread-t-12499"]'));
        const pages = await Promise.all(replies);
        assert.ok(pages.length > 1);
        assert.equal(pages.flatMap(p => p.threads).length, all.length);
        assert.equal(pages.at(-1).next_after, undefined);
        const received = pages.flatMap(p => p.threads).map(t => `${t.project}/${t.id}`);
        assert.equal(new Set(received).size, all.length);
        assert.deepEqual(received, all.map(t => `${t.project}/${t.id}`));
        assert.deepEqual(errors, []);
      } finally { await page.context().close(); }
    }
  } finally { await safari.close(); }
}
