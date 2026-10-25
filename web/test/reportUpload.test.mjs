import assert from "node:assert/strict";
import test from "node:test";
import { uploadReportFiles } from "../src/reportUpload.ts";

const paths = Array.from({ length: 9 }, (_, index) => ({ path: `file-${index}`, index }));
const tick = () => new Promise(resolve => queueMicrotask(resolve));

test("최대 네 요청·역순 완료·실패 뒤 drain·명시적 재개", async () => {
  const gates = new Map(), sent = [], progress = [], completed = new Set();
  const run = uploadReportFiles(paths, completed, entry => {
    sent.push(entry.index);
    const gate = Promise.withResolvers();
    gates.set(entry.index, gate);
    return gate.promise;
  }, n => progress.push(n));
  let settled = false;
  const result = run.catch(error => error).finally(() => { settled = true; });
  assert.deepEqual(sent, [0, 1, 2, 3]);
  gates.get(3).resolve();
  await tick();
  assert.deepEqual(sent, [0, 1, 2, 3, 4]);
  gates.get(1).reject(new Error("서버 쓰기 실패 (os error 5)"));
  await tick();
  assert.equal(settled, false, "보낸 요청이 남으면 창을 풀지 않는다");
  for (const index of [4, 2, 0]) gates.get(index).resolve();
  const error = await result;
  assert.match(error.message, /4\/9.*file-1.*서버 쓰기 실패 \(os error 5\)/);
  assert.deepEqual(sent, [0, 1, 2, 3, 4], "실패 뒤 새 요청·자동 재시도 없음");
  assert.deepEqual(progress, [1, 2, 3, 4]);
  assert.deepEqual([...completed].sort(), ["file-0", "file-2", "file-3", "file-4"]);
  const resumed = [];
  await uploadReportFiles(paths, completed, async entry => { resumed.push(entry.index); }, n => progress.push(n));
  assert.deepEqual(resumed, [1, 5, 6, 7, 8], "성공한 뒷 파일도 재전송하지 않는다");
  assert.equal(completed.size, 9);
  assert.equal(progress.at(-1), 9);
  await uploadReportFiles(paths, completed, async () => { assert.fail("게시만 실패했으면 전송 없음"); }, () => {});
});

test("동시에 실패한 요청의 경로·하위 오류를 모두 남긴다", async () => {
  const causes = [new Error("권한 거부 (os error 13)"), new Error("디스크 부족 (os error 28)"), "연결 끊김", new Error("파일 잠김")];
  let sent = 0;
  await assert.rejects(uploadReportFiles(paths, new Set(), async entry => {
    sent++;
    throw causes[entry.index];
  }, () => assert.fail("실패를 성공으로 세지 않는다")), error => {
    assert(error instanceof AggregateError);
    assert.equal(error.errors.length, 4);
    for (let i = 0; i < 4; i++) {
      assert.equal(error.errors[i].cause, causes[i]);
      assert(error.message.includes(`file-${i}: ${String(causes[i]).replace(/^Error: /, "")}`));
    }
    return true;
  });
  assert.equal(sent, 4, "다음 다섯 요청은 보내지 않는다");
});
