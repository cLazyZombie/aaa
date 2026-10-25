import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { EventEmitter } from "node:events";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";
import { seedHost } from "../e2e/lib/bulk-fixture.mjs";

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-bulk-fixture-"));
  fs.mkdirSync(`${root}/s/host`, { recursive: true });
  const file = `${root}/s/host/host.db`;
  let holder = new DatabaseSync(file);
  holder.exec("PRAGMA journal_mode=WAL; CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT); BEGIN IMMEDIATE; INSERT INTO meta VALUES('host','held')");
  t.after(() => { holder?.close(); fs.rmSync(root, { recursive: true, force: true }); });
  const events = [];
  const host = new EventEmitter();
  const lab = {
    root,
    stop: async () => {
      // 고정 시간 대신 별도 task의 종료 사건에서 쓰기 잠금을 놓는다.
      await new Promise(resolve => setImmediate(resolve));
      holder.exec("COMMIT"); holder.close(); holder = null;
      events.push("stopped");
    },
    start: () => {
      events.push("started");
      fs.writeFileSync(`${root}/host.log`, "");
      setImmediate(() => {
        fs.appendFileSync(`${root}/host.log`, "웹 listen\n");
        setImmediate(() => { fs.appendFileSync(`${root}/host.log`, "agent 연결\n"); events.push("ready"); });
      });
      return host;
    },
    inTime: promise => promise,
  };
  return { lab, file, events, host };
}

test("살아 있는 writer의 잠금은 실패하고 비동기 종료 뒤 fixture는 한 번 확정된다", { timeout: 5000 }, async t => {
  const { lab, file, events, host } = fixture(t);
  const write = db => {
    events.push("write");
    db.prepare("INSERT INTO meta VALUES(?,?)").run("fixture", "ready");
  };
  await assert.rejects(seedHost(write, { ...lab, stop: async () => {} }), error => {
    assert.match(error.message, /fixture DB 준비 실패.*database is locked/);
    assert.match(error.cause.message, /database is locked/);
    return true;
  });
  assert.deepEqual(events, [], "잠긴 DB는 callback·host 시작에 도달하지 않는다");
  await seedHost(write, lab);
  assert.deepEqual(events, ["stopped", "write", "started", "ready"]);
  assert.equal(host.listenerCount("exit"), 0);
  const db = new DatabaseSync(file, { readOnly: true });
  try { assert.deepEqual(db.prepare("SELECT key FROM meta ORDER BY key").all().map(row => row.key), ["fixture", "host"]); }
  finally { db.close(); }
});

test("fixture 준비 오류는 부분 쓰기를 rollback하고 원인과 경로를 보존한다", { timeout: 5000 }, async t => {
  const { lab, file, events } = fixture(t);
  const cause = new Error("fixture write failure");
  await assert.rejects(seedHost(db => {
    db.exec("INSERT INTO meta VALUES('partial','discard')");
    throw cause;
  }, lab), error => error.cause === cause && error.message.includes(file) && error.message.includes(cause.message));
  assert.deepEqual(events, ["stopped"], "미완성 fixture로 host를 다시 띄우지 않는다");
  const db = new DatabaseSync(file);
  try {
    assert.equal(db.prepare("SELECT count(*) AS n FROM meta WHERE key='partial'").get().n, 0);
    db.exec("BEGIN IMMEDIATE; ROLLBACK");
  } finally { db.close(); }
});

test("재시작 host의 조기 종료를 숨기지 않는다", { timeout: 5000 }, async t => {
  const { lab, host } = fixture(t);
  await assert.rejects(seedHost(() => {}, { ...lab, start: () => {
    fs.writeFileSync(`${lab.root}/host.log`, "웹 listen\n");
    setImmediate(() => host.emit("exit", 17, null));
    return host;
  } }), /fixture host 종료: 17\/null/);
  assert.equal(host.listenerCount("exit"), 0);
});
