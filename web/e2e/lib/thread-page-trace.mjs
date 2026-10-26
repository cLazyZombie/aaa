// E2E12의 실제 HTTP를 문서·events 연결·커서로 잇는다. URL·쿠키·인증 값은 기록하지 않는다.
import assert from "node:assert/strict";
import fs from "node:fs";

export function traceThreadRequests() {
  const doc = String(performance.timeOrigin);
  let connection = 0, nextConnection = 0, request = 0;
  const log = row => console.debug("THREAD_TRACE " + JSON.stringify({ doc, connection, ...row }));
  const WS = window.WebSocket;
  window.WebSocket = class extends WS {
    constructor(...args) {
      super(...args);
      if (!String(args[0]).endsWith("/api/v1/events")) return;
      const link = ++nextConnection;
      this.addEventListener("open", () => { connection = link; log({ event: "ws-open", link }); });
      this.addEventListener("close", e => log({ event: "ws-close", link, code: e.code }));
    }
  };
  const fetch = window.fetch;
  window.fetch = function (input, init) {
    let body;
    try { body = JSON.parse(init?.body); } catch {}
    if (body?.op !== "threads") return fetch.call(this, input, init);
    const id = String(++request), headers = new Headers(init?.headers);
    headers.set("x-e2e-document", doc);
    headers.set("x-e2e-connection", String(connection));
    headers.set("x-e2e-request", id);
    return fetch.call(this, input, { ...init, headers });
  };
  for (const event of ["pageshow", "pagehide", "beforeunload"]) addEventListener(event, () => log({ event }));
  log({ event: "document" });
}

export function threadPageTrace(directory, engine) {
  fs.mkdirSync(directory, { recursive: true });
  const file = `${directory}/thread-pages-${engine}.jsonl`;
  const pending = [], requests = new WeakMap();
  let next = 0;
  const log = row => fs.appendFileSync(file, JSON.stringify({ at: Date.now(), engine, ...row }) + "\n");
  const onPage = page => {
    page.on("console", msg => {
      if (msg.text().startsWith("THREAD_TRACE ")) log(JSON.parse(msg.text().slice(13)));
    });
    page.on("request", request => {
      if (!request.url().endsWith("/api/v1/call") || request.postDataJSON()?.op !== "threads") return;
      const headers = request.headers();
      const record = { id: ++next, doc: headers["x-e2e-document"], connection: headers["x-e2e-connection"],
        fetch: headers["x-e2e-request"], after: request.postDataJSON().after ?? null };
      requests.set(request, record);
      log({ ...record, event: "request" });
    });
    page.on("response", response => {
      const record = requests.get(response.request());
      if (!record) return;
      const body = response.body().then(bytes => {
        const result = { ...record, status: response.status(), bytes: bytes.length, body: JSON.parse(bytes).ok };
        log({ ...record, event: "response", status: result.status, bytes: result.bytes,
          count: result.body?.threads?.length, next: result.body?.next_after ?? null,
          ids: result.body?.threads?.map(t => `${t.project}/${t.id}`) });
        return result;
      }).catch(error => {
        log({ ...record, event: "body-error", error: String(error) });
        return { ...record, error: String(error) };
      });
      pending.push(body);
    });
    page.on("requestfailed", request => {
      const record = requests.get(request);
      if (record) log({ ...record, event: "request-failed", error: request.failure() });
    });
  };
  const reads = async expected => {
    const records = (await Promise.all(pending)).sort((a, b) => a.id - b.id);
    const reads = [], starts = new Set();
    for (const record of records) {
      assert.equal(record.error, undefined, `HTTP 본문 읽기 실패: ${record.error}`);
      assert.ok(record.doc && record.connection, "요청의 문서·연결 표지가 있어야 한다");
      assert.equal(record.status, 200);
      assert.ok(record.bytes < 1024 * 1024);
      assert.equal(record.body?.kind, "threads");
      assert.ok(record.body.threads.length <= 200);
      let read;
      if (record.after === null) {
        const start = `${record.doc}/${record.connection}`;
        assert.ok(!starts.has(start), `같은 events 연결의 중복 첫 요청: ${start}`);
        starts.add(start);
        read = { first: record.id, doc: record.doc, connection: record.connection, ids: [], next: null, pages: 0, seen: new Set() };
        reads.push(read);
      } else {
        // 이 고정 fixture의 겹친 읽기는 같은 목록 접두사를 갖는다. 먼저 기다리는 읽기에 잇되 응답을 버리지 않는다.
        read = reads.find(r => r.doc === record.doc && r.next === record.after);
        assert.ok(read, `앞 응답의 next_after 없이 요청한 커서: ${record.after}`);
      }
      assert.ok(!read.seen.has(record.after), `한 읽기 안의 반복 커서: ${record.after}`);
      read.seen.add(record.after);
      read.ids.push(...record.body.threads.map(t => `${t.project}/${t.id}`));
      read.next = record.body.next_after ?? null;
      read.pages++;
      // 취소·재연결로 덜 읽은 목록도 정확한 접두사여야 한다. 끝난 읽기는 전체 개수·차례를 모두 검사한다.
      assert.deepEqual(read.ids, expected.slice(0, read.ids.length));
      if (read.next === null) {
        assert.equal(read.ids.length, expected.length);
        assert.equal(new Set(read.ids).size, expected.length);
      }
      log({ event: "cursor-link", request: record.id, read: read.first, doc: read.doc, connection: read.connection, after: record.after, next: read.next });
    }
    log({ event: "reads", reads: reads.map(({ seen, ids, ...r }) => ({ ...r, count: ids.length, unique: new Set(ids).size })) });
    return reads;
  };
  return { onPage, reads };
}
