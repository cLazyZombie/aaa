import assert from "node:assert/strict";
import { test } from "node:test";
import { gzipSync } from "node:zlib";
import { decodeBase64, inflateGzip, PAYLOAD_MAX, rawLength, WIRE_MAX } from "../src/terminalInflate.ts";
import { TerminalReceiver } from "../src/terminalReceive.ts";

const encoded = (bytes) => Buffer.from(bytes).toString("base64");
const packed = (bytes, stream = 1) => JSON.stringify({ t: "snapshot", replay: encoded(gzipSync(bytes, { level: 1 })), compression: { codec: "gzip", raw_len: bytes.length }, cols: 120, rows: 36, readonly: false, stream, history_more: true });
const identity = (bytes) => JSON.stringify({ t: "snapshot", replay: encoded(bytes), cols: 120, rows: 36, readonly: false, stream: 1, history_more: false });
const turn = () => new Promise((resolve) => queueMicrotask(resolve));
class WorkerStub {
  onmessage = null;
  onerror = null;
  sent = [];
  terminated = false;
  postMessage(message) { this.sent.push(message); }
  terminate() { this.terminated = true; }
  answer(data) { this.onmessage?.({ data }); }
  async finish() {
    const { id, data, rawLen } = this.sent.at(-1);
    this.answer({ id, bytes: await inflateGzip(decodeBase64(data), rawLen) });
  }
}
function fixture(supported = true) {
  const worker = new WorkerStub(); const consumed = []; let failures = 0;
  const receiver = new TerminalReceiver((item) => consumed.push(item), () => failures++, () => worker);
  worker.answer({ ready: supported });
  return { worker, receiver, consumed, failures: () => failures };
}

test("실제 gzip: 일반·색상·한글·binary 왕복과 768 KiB 경계", async () => {
  for (const bytes of [Buffer.from("plain\r\n".repeat(2000)), Buffer.from("\x1b[31m한글 줄 접힘\x1b[m\r\n".repeat(2000)), Buffer.from(Array.from({ length: 32768 }, (_, i) => i % 256)), Buffer.alloc(PAYLOAD_MAX)]) {
    assert.deepEqual(Buffer.from(await inflateGzip(new Uint8Array(gzipSync(bytes)), bytes.length)), bytes);
  }
});

test("metadata·base64·CRC·잘림·초과·폭탄 거부", async () => {
  for (const value of [null, {}, { codec: "zstd", raw_len: 1 }, { codec: "gzip", raw_len: -1 }, { codec: "gzip", raw_len: 1.5 }, { codec: "gzip", raw_len: PAYLOAD_MAX + 1 }, { codec: "gzip", raw_len: "3" }]) assert.throws(() => rawLength(value));
  for (const value of [null, "?===", "YQ", "a".repeat(WIRE_MAX + 4)]) assert.throws(() => decodeBase64(value));
  const bytes = Buffer.from("한글과 색\x1b[31m".repeat(3000));
  const gzip = gzipSync(bytes, { level: 1 });
  for (const len of [bytes.length - 1, bytes.length + 1, PAYLOAD_MAX + 1]) await assert.rejects(inflateGzip(new Uint8Array(gzip), len));
  for (let cut = 0; cut < gzip.length; cut++) await assert.rejects(inflateGzip(new Uint8Array(gzip.subarray(0, cut)), bytes.length));
  const crc = Buffer.from(gzip); crc[crc.length - 8] ^= 1;
  await assert.rejects(inflateGzip(new Uint8Array(crc), bytes.length));
  await assert.rejects(inflateGzip(new Uint8Array(Buffer.concat([gzip, Buffer.from([0])])), bytes.length));
  await assert.rejects(inflateGzip(new Uint8Array(Buffer.concat([gzip, gzip])), bytes.length));
  const bomb = new Uint8Array(gzipSync(Buffer.alloc(16 * 1024 * 1024), { level: 1 }));
  await assert.rejects(inflateGzip(bomb, 1));
});

test("취소는 다음 입력·출력 읽기 전에 끝나고 결과를 내보내지 않는다", async () => {
  const abort = new AbortController(); abort.abort();
  await assert.rejects(inflateGzip(new Uint8Array(gzipSync(Buffer.alloc(65536))), 65536, abort.signal));
});

test("Worker 해제 gate: snapshot→binary→history→새 stream 순서, 해제 전 소비/ACK 없음", async () => {
  const f = fixture(); assert.equal(await f.receiver.ready, true);
  const bytes = Buffer.from("기록\x1b[31m\r\n".repeat(1000));
  f.receiver.push(packed(bytes));
  f.receiver.push(Uint8Array.from([65, 66]).buffer);
  f.receiver.push(JSON.stringify({ t: "history", id: 7, data: encoded(bytes), lines: 1000, found: true, more: true }));
  f.receiver.push(packed(bytes, 2));
  assert.equal(f.consumed.length, 0);
  assert.equal(f.worker.sent.length, 1);
  await f.worker.finish(); await turn();
  assert.deepEqual(f.consumed.map((item) => "binary" in item ? "binary" : item.message.t), ["snapshot", "binary", "history"]);
  assert.deepEqual(Buffer.from(f.consumed[0].payload), bytes);
  assert.equal(f.worker.sent.length, 2);
  await f.worker.finish(); await turn();
  assert.equal(f.consumed.at(-1).message.stream, 2);
  assert.equal(f.failures(), 0); f.receiver.close();
});

test("연결 종료·pane 교체는 Worker를 끝내고 늦은 결과·후속 출력을 버린다", async () => {
  const f = fixture(); await f.receiver.ready;
  f.receiver.push(packed(Buffer.alloc(32768)));
  const id = f.worker.sent[0].id;
  f.receiver.push(Uint8Array.from([1]).buffer);
  f.receiver.close();
  assert.equal(f.worker.terminated, true);
  f.worker.answer({ id, bytes: new Uint8Array(32768) }); await turn();
  assert.equal(f.consumed.length, 0); assert.equal(f.failures(), 0);
});

test("구 host identity·지원 없는 브라우저·Worker 실패와 협상 없는 압축", async () => {
  for (const supported of [false, true]) {
    const f = fixture(supported); assert.equal(await f.receiver.ready, supported);
    f.receiver.push(identity(Buffer.from("옛 host")));
    assert.equal(Buffer.from(f.consumed[0].payload).toString(), "옛 host");
    f.receiver.close();
  }
  const f = fixture(false); await f.receiver.ready;
  f.receiver.push(packed(Buffer.alloc(8192)));
  assert.equal(f.failures(), 1); assert.equal(f.consumed.length, 0);
  const worker = new WorkerStub();
  const receiver = new TerminalReceiver(() => {}, () => assert.fail("연결 전 지원 실패는 identity"), () => worker);
  worker.onerror(); assert.equal(await receiver.ready, false); receiver.close();
});

test("재연결의 확인된 gzip은 새 Worker 준비 사건 뒤에만 해제를 시작한다", async () => {
  const worker = new WorkerStub(); const consumed = [];
  const receiver = new TerminalReceiver((item) => consumed.push(item), () => assert.fail("지원된 재연결"), () => worker, true);
  receiver.push(packed(Buffer.alloc(8192)));
  receiver.push(Uint8Array.from([65]).buffer);
  assert.equal(worker.sent.length, 0);
  worker.answer({ ready: true }); await receiver.ready; await turn();
  assert.equal(worker.sent.length, 1); assert.equal(consumed.length, 0);
  await worker.finish(); await turn();
  assert.equal(consumed.length, 2); receiver.close();
});

test("연결별 대기 바이트·원시 JSON 상한·해제 오류는 한 번만 실패하고 회수한다", async () => {
  const f = fixture(); await f.receiver.ready;
  f.receiver.push(packed(Buffer.alloc(600000)));
  f.receiver.push(packed(Buffer.alloc(600000)));
  assert.equal(f.failures(), 1); assert.equal(f.worker.terminated, true);
  f.receiver.push("invalid"); assert.equal(f.failures(), 1);
  const big = fixture(); await big.receiver.ready;
  big.receiver.push(packed(Buffer.alloc(PAYLOAD_MAX)));
  assert.equal(big.failures(), 1);
  const bad = fixture(); await bad.receiver.ready;
  bad.receiver.push(packed(Buffer.alloc(8192)));
  bad.worker.answer({ id: bad.worker.sent[0].id, error: true }); await turn();
  assert.equal(bad.failures(), 1); assert.equal(bad.worker.terminated, true);
  const encoded = fixture(); await encoded.receiver.ready;
  const forged = JSON.stringify({ t: "history", id: 1, data: "A".repeat(600000), compression: { codec: "gzip", raw_len: 0 }, lines: 1, found: true, more: true });
  encoded.receiver.push(forged); encoded.receiver.push(forged);
  assert.equal(encoded.failures(), 1, "작은 raw_len으로 큰 압축 문자열을 여러 개 쌓지 못한다");
});
