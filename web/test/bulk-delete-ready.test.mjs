import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pendingDeletionCleared } from "../e2e/lib/bulk-delete-ready.mjs";

test("마지막 폴더 사건이 없어도 실제 정리 완료에서만 끝낸다", { timeout: 2000 }, async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-delete-ready-"));
  const pending = path.join(root, "pending");
  const batch = path.join(pending, "batch");
  const log = path.join(root, "host.log");
  fs.mkdirSync(batch, { recursive: true });
  fs.writeFileSync(path.join(batch, "report.md"), "삭제할 실제 사본");
  fs.writeFileSync(log, "");
  const watchers = new Map();
  t.after(() => {
    for (const watcher of watchers.values()) watcher.close();
    fs.rmSync(root, { recursive: true, force: true });
  });
  const watch = (file, check) => {
    const watcher = new EventEmitter();
    watcher.closed = false;
    watcher.close = () => { watcher.closed = true; watcher.removeAllListeners(); };
    watcher.on("change", check);
    watchers.set(file, watcher);
    return watcher;
  };
  let finished = false;
  const done = pendingDeletionCleared(pending, log, (promise, ms, what) => {
    assert.equal(ms, 20_000);
    assert.equal(what, "삭제 대기 사본의 실제 제거");
    return promise;
  }, watch).then(() => { finished = true; });
  // 마지막 디렉터리 제거 전 사건은 왔지만, 제거 뒤 폴더 사건은 오지 않는 순서다.
  watchers.get(pending).emit("change");
  fs.appendFileSync(log, "다른 host 사건\n");
  watchers.get(log)?.emit("change");
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(finished, false, "사본이 남았으면 폴더/로그 사건만으로 성공하지 않는다");
  fs.rmSync(batch, { recursive: true });
  fs.appendFileSync(log, "보고서 삭제 대기 정리 완료\n");
  watchers.get(log)?.emit("change");
  await done;
  assert.deepEqual(fs.readdirSync(pending), []);
  assert.ok([...watchers.values()].every(watcher => watcher.closed), "완료 뒤 감시를 닫는다");
});
