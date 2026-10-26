// 기준 실행과 날짜별 p50의 대표 실행을 혼동하거나 결측을 0초로 표시하는 회귀를 막는다.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { referencePoint, referenceNotice } from '../src/workspaceSelection.ts';
const at = Date.parse('2026-10-25T16:00:00Z');
const run = { run_id:'selected', observed_at_ms:at, machine:'mac-a', profile:'verify', outcome:'succeeded', verification_ms:0, build_ms:null };
const point = { run_id:'median-representative', at_ms:Date.parse('2026-10-25T15:00:00Z'), machine:'mac-a', profile:'verify', samples:2, value:200 };
test('시각별은 실행 ID, 날짜별은 시간대·머신·profile 묶음을 표시한다', () => {
  assert.equal(referencePoint([point],run,false,false),undefined);
  assert.equal(referencePoint([point],{...run,run_id:point.run_id},false,false),point);
  assert.equal(referencePoint([point],run,true,false),point);
  assert.equal(referencePoint([{...point,at_ms:Date.parse('2026-10-26T00:00:00Z')}],run,true,true),undefined);
  assert.equal(referencePoint([{...point,machine:'mac-b'}, {...point,profile:'release'}],run,true,false),undefined);
  assert.equal(referencePoint([point],{...run,outcome:'failed'},true,false),undefined);
});
test('필터 밖·결측·p50 제외를 설명하며 실제 0초는 강조한다', () => {
  assert.equal(referenceNotice(run,true,true,'verify',false),null);
  assert.match(referenceNotice(run,true,true,'build',false),/빌드 시간이 미수집/);
  assert.match(referenceNotice(run,true,false,'verify',true),/현재 그래프 범위 밖/);
  assert.match(referenceNotice({...run,outcome:'failed'},true,true,'verify',true),/p50에서 제외/);
  assert.match(referenceNotice(undefined,true,false,'verify',false),/찾을 수 없습니다/);
});
