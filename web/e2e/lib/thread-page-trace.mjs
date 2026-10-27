// E2E12의 실제 HTTP를 문서·events 연결·커서로 잇는다. URL·쿠키·인증 값은 기록하지 않는다.
import assert from "node:assert/strict";
import fs from "node:fs";

// page.evaluate로도 독립 직렬화된다. 시계의 반복 환산값 대신 Document 수명을 식별한다.
export function threadTraceDocument() {
  return document.__aaaThreadTraceDocument ??= crypto.getRandomValues(new Uint32Array(4)).join("-");
}

export function traceThreadRequests() {
  // init도 독립 직렬화되므로 위 getter와 같은 Document 칸을 처음 만든다.
  const doc = document.__aaaThreadTraceDocument ??= crypto.getRandomValues(new Uint32Array(4)).join("-");
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
  const pending = [], requests = new WeakMap(), active = new Set(), bodies = new Set();
  const retired = new Set();
  const endings = new Map(), completion = new Set();
  let next = 0;
  const log = row => fs.appendFileSync(file, JSON.stringify({ at: Date.now(), engine, ...row }) + "\n");
  const settle = (record, result) => {
    if (!active.delete(record)) return;
    record.resolve(result);
  };
  const onPage = page => {
    const retire = doc => {
      for (const record of [...active]) if (record.page === page && record.data.doc !== doc) {
        retired.add(record.data.doc);
        if (record.reading) continue;
        log({ ...record.data, event: "navigation-aborted" });
        settle(record, { ...record.data, cancelled: "navigation" });
      }
    };
    page.on("framenavigated", frame => {
      if (frame !== page.mainFrame()) return;
      // hash 이동은 같은 Document다. 실제 문서 표지가 바뀐 경우만 옛 요청을 끝낸다.
      void frame.evaluate(threadTraceDocument).then(retire)
        .catch(error => log({ event: "navigation-observer-closed", error: error.message.split("\n")[0] }));
    });
    page.on("close", () => {
      for (const record of [...active]) if (record.page === page)
        settle(record, { ...record.data, error: "본문 완료 전 page.close" });
    });
    page.on("console", msg => {
      if (!msg.text().startsWith("THREAD_TRACE ")) return;
      const row = JSON.parse(msg.text().slice(13));
      log(row);
      if (row.event === "document") retire(row.doc);
    });
    page.on("request", request => {
      if (!request.url().endsWith("/api/v1/call") || request.postDataJSON()?.op !== "threads") return;
      const headers = request.headers();
      const record = { id: ++next, doc: headers["x-e2e-document"], connection: headers["x-e2e-connection"],
        fetch: headers["x-e2e-request"], after: request.postDataJSON().after ?? null };
      const entry = { data: record, page, response: null, reading: false };
      pending.push(new Promise(resolve => { entry.resolve = resolve; }));
      requests.set(request, entry);
      active.add(entry);
      log({ ...record, event: "request" });
    });
    page.on("response", response => {
      const entry = requests.get(response.request());
      if (entry) entry.response = response;
    });
    page.on("requestfinished", request => {
      const entry = requests.get(request);
      if (!entry || !active.has(entry)) return;
      const record = entry.data, response = entry.response;
      entry.reading = true;
      // response는 헤더 사건이다. 실제 다운로드 완료 전 body()를 시작하면
      // navigation으로 중단된 요청의 protocol Promise가 browser.close까지 남는다.
      let bodyReceived = false;
      const body = response.body().then(bytes => {
        bodyReceived = true;
        const result = { ...record, status: response.status(), bytes: bytes.length, body: JSON.parse(bytes).ok };
        log({ ...record, event: "response", status: result.status, bytes: result.bytes,
          count: result.body?.threads?.length, next: result.body?.next_after ?? null,
          ids: result.body?.threads?.map(t => `${t.project}/${t.id}`) });
        if (result.body?.kind === "threads" && !result.body.next_after) {
          endings.set(record.doc, (endings.get(record.doc) ?? 0) + 1);
          for (const check of completion) check();
        }
        return result;
      }).catch(async error => {
        log({ ...record, event: "body-error", error: String(error) });
        // requestfinished 뒤에도 body protocol 호출과 navigation은 경쟁한다.
        // 오류 문구로 성공 처리하지 않고 실제 Document 교체로만 꼬리를 구별한다.
        if (!bodyReceived && !retired.has(record.doc)) {
          try { retire(await page.evaluate(threadTraceDocument)); }
          catch { /* page.close는 기존 오류로 남긴다. */ }
        }
        if (!bodyReceived && retired.has(record.doc)) {
          log({ ...record, event: "body-unavailable-after-navigation", error: String(error) });
          return { ...record, cancelled: "navigation" };
        }
        return { ...record, error: String(error) };
      });
      bodies.add(body);
      void body.then(result => { bodies.delete(body); settle(entry, result); });
    });
    page.on("requestfailed", request => {
      const entry = requests.get(request);
      if (!entry || !active.has(entry)) return;
      const error = request.failure()?.errorText ?? String(request.failure());
      log({ ...entry.data, event: "request-failed", error });
      settle(entry, { ...entry.data, error });
    });
  };
  const reads = async expected => {
    const records = (await Promise.all(pending)).sort((a, b) => a.id - b.id);
    const reads = [], starts = new Set();
    for (const record of records) {
      assert.equal(record.error, undefined, `HTTP 본문 읽기 실패: ${record.error}`);
      assert.ok(record.doc && record.connection, "요청의 문서·연결 표지가 있어야 한다");
      let read;
      if (record.after === null) {
        const start = `${record.doc}/${record.connection}`;
        assert.ok(!starts.has(start), `같은 events 연결의 중복 첫 요청: ${start}`);
        starts.add(start);
        read = { first: record.id, doc: record.doc, connection: record.connection, ids: [], next: "aborted-before-body", pages: 0, seen: new Set() };
        reads.push(read);
      } else {
        // 이 고정 fixture의 겹친 읽기는 같은 목록 접두사를 갖는다. 먼저 기다리는 읽기에 잇되 응답을 버리지 않는다.
        read = reads.find(r => r.doc === record.doc && r.next === record.after);
        assert.ok(read, `앞 응답의 next_after 없이 요청한 커서: ${record.after}`);
      }
      assert.ok(!read.seen.has(record.after), `한 읽기 안의 반복 커서: ${record.after}`);
      read.seen.add(record.after);
      if (record.cancelled) {
        assert.equal(record.cancelled, "navigation");
        assert.ok(retired.has(record.doc), "실제 문서 교체로 끝난 꼬리만 취소로 분류한다");
        assert.deepEqual(read.ids, expected.slice(0, read.ids.length));
        if (record.after !== null) read.next = "navigation-aborted";
        log({ event: "cancelled-tail", request: record.id, read: read.first, doc: read.doc, after: record.after });
        continue;
      }
      assert.equal(record.status, 200);
      assert.ok(record.bytes < 1024 * 1024);
      assert.equal(record.body?.kind, "threads");
      assert.ok(record.body.threads.length <= 200);
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
  // route.fulfill의 완료는 브라우저 response.body 수집 완료가 아니다. 실제 본문까지 받은 목록을 센다.
  const completed = (doc, count) => new Promise(resolve => {
    log({ event: "completion-wait", doc, count, collected: Object.fromEntries(endings) });
    const check = () => {
      if ((endings.get(doc) ?? 0) < count) return;
      completion.delete(check);
      log({ event: "completion-reached", doc, count, collected: Object.fromEntries(endings) });
      resolve();
    };
    completion.add(check);
    check();
  });
  // 문서를 바꾸기 전에 이미 완료한 응답의 protocol body 작업을 먼저 마친다.
  const drain = async () => { while (bodies.size) await Promise.all([...bodies]); };
  return { onPage, reads, completed, drain, record: log };
}
