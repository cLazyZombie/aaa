// 선택 계측. 시험의 대기/시한/단언은 건드리지 않고 runner의 단조 시각을 기록한다.
import { AsyncLocalStorage } from 'node:async_hooks';
import { randomUUID } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const current = new AsyncLocalStorage();
class CasePhases {
  constructor(name, directory) {
    this.name = name;
    this.started = performance.now();
    this.previous = this.started;
    this.category = 'contract';
    this.label = 'body';
    this.nextSpan = 0;
    if (!directory) return;
    try {
      fs.mkdirSync(directory, { recursive: true });
      this.file = fs.openSync(path.join(directory, `web-${process.pid}-${randomUUID()}.jsonl`), 'wx');
      this.write('start', this.started);
    } catch (error) { console.error(`시험 단계 계측 시작 실패: ${error}`); }
  }
  write(event, now, fields = {}) {
    if (this.file === undefined) return;
    try {
      fs.writeSync(this.file, JSON.stringify({ event, case: this.name, category: this.category, label: this.label,
        at_ms: Date.now(), elapsed_ms: now - this.started, duration_ms: now - this.previous,
        pid: process.pid, ...fields }) + '\n');
    } catch (error) {
      console.error(`시험 단계 계측 쓰기 실패: ${error}`);
      this.close();
    }
  }
  mark(category, label) {
    if (this.file === undefined) return;
    const now = performance.now();
    this.write('phase', now);
    this.previous = now;
    this.category = category;
    this.label = label;
  }
  close() {
    if (this.file === undefined) return;
    const file = this.file;
    this.file = undefined;
    try { fs.closeSync(file); }
    catch (error) { console.error(`시험 단계 계측 닫기 실패: ${error}`); }
  }
}

export async function withCasePhases(name, fn, directory = process.env.AAA_TEST_PHASES) {
  if (!directory) return fn();
  const phases = new CasePhases(name, directory);
  let ok = false;
  try {
    const result = await current.run(phases, fn);
    ok = true;
    return result;
  } finally {
    phases.write('end', performance.now(), { ok });
    phases.close();
  }
}

export function phase(category, label) { current.getStore()?.mark(category, label); }

// 브라우저 안에서 이미 읽은 구간이다. Node wall/phase 시간과 합산하지 않는다.
export function sample(category, label, duration_ms) {
  current.getStore()?.write('sample', performance.now(), { category, label, duration_ms });
}

// 보조 span은 phase와 겹친다. 합산 비용에서 빼거나 category 합계에 다시 더하지 않는다.
export async function measured(category, label, fn) {
  const phases = current.getStore();
  if (phases?.file === undefined) return fn();
  const started = performance.now();
  const span = ++phases.nextSpan;
  phases.write('span-start', started, { category, label, span, duration_ms: 0 });
  let ok = false;
  try {
    const result = await fn();
    ok = true;
    return result;
  } finally {
    const now = performance.now();
    phases.write('span', now, { category, label, span, duration_ms: now - started, ok });
  }
}
