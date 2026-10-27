import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { phase, measured, withCasePhases } from '../e2e/lib/phases.mjs';

test('계측은 원래 실패를 보존하고 준비·보조 관측·미완료 정리를 따로 남긴다', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aaa-phases-'));
  const original = new Error('원래 실패');
  try {
    await assert.rejects(withCasePhases('fixture', async () => {
      phase('prepare', 'fixture');
      assert.equal(await measured('observe', 'sample', async () => 17), 17);
      phase('contract', 'real-event');
      phase('cleanup', 'owned-resources');
      await measured('observe', 'failure', async () => { throw original; });
    }, dir), error => error === original);
    const rows = fs.readdirSync(dir).flatMap(name => fs.readFileSync(path.join(dir, name), 'utf8').trim().split('\n').map(JSON.parse));
    assert.equal(rows.at(-1).event, 'end');
    assert.equal(rows.at(-1).ok, false);
    assert.equal(rows.at(-1).category, 'cleanup');
    assert.deepEqual(rows.filter(row => row.event === 'phase').map(row => row.category), ['contract', 'prepare', 'contract']);
    assert.deepEqual(rows.filter(row => row.event === 'span').map(row => row.ok), [true, false]);
    assert.ok(rows.every((row, index) => index === 0 || row.elapsed_ms >= rows[index - 1].elapsed_ms));
    // 기록 폴더가 파일이어도 진단 오류로 원래 오류를 바꾸지 않는다.
    const occupied = path.join(dir, 'file');
    fs.writeFileSync(occupied, 'x');
    await assert.rejects(withCasePhases('fixture', async () => { throw original; }, occupied), error => error === original);
  } finally { fs.rmSync(dir, { recursive: true }); }
});
