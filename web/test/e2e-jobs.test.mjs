import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { planJobs, shardSpecs, specDurations } from "../e2e/lib/jobs.mjs";

const source = fs.readFileSync(new URL("../e2e/specs/37-chat-pages.mjs", import.meta.url), "utf8");
const steps = [...source.matchAll(/^ {2}await step\("((?:[^"\\]|\\.)*)"/gm)].map((m) => JSON.parse(`"${m[1]}"`));
const spec = { name: "37-chat-pages", steps };
const options = { repeat: 2, workers: 8, durations: { [spec.name]: 120 }, only: null };

test("shard는 모든 spec을 정확히 한 번 넣고 단계 상태와 입력 목록을 보존한다", () => {
  const specs = Array.from({ length: 7 }, (_, i) => ({ name: `spec-${i}`, steps: ["준비", "그 결과 사용"] }));
  const durations = { "spec-0": 90, "spec-1": 60, "spec-2": 30 };
  const groups = [1, 2, 3].map(i => shardSpecs(specs, `${i}/3`, durations));
  assert.deepEqual(groups.flat().map(s => s.name).sort(), specs.map(s => s.name).sort());
  assert.ok(groups.every(g => g.length > 0));
  assert.deepEqual(shardSpecs([...specs].reverse(), "1/3", durations).map(s => s.name).sort(), groups[0].map(s => s.name).sort());
  for (const s of groups.flat()) assert.equal(s, specs.find(original => original.name === s.name));
  assert.equal(shardSpecs(specs, null, durations), specs);
});

test("균등한 긴 spec을 분산하고 잘못된 shard를 빈 통과로 만들지 않는다", () => {
  const specs = ["a", "b", "c", "d", "e", "f"].map(name => ({ name }));
  const durations = { a: 100, b: 100, c: 100, d: 10, e: 10, f: 10 };
  for (const i of [1, 2, 3]) {
    assert.equal(shardSpecs(specs, `${i}/3`, durations).reduce((sum, s) => sum + durations[s.name], 0), 110);
  }
  for (const bad of ["0/3", "4/3", "1/0", "1/7", "1", "a/3", "1/9007199254740993"])
    assert.throws(() => shardSpecs(specs, bad, durations), /shard/);
});

test("새 기록과 긴 대화의 네 경계를 반복마다 정확히 한 번 실행한다", () => {
  const jobs = planJobs([spec], options);
  assert.deepEqual(steps, [
    "새 기록 생성 전후: Chromium claude",
    "새 기록 생성 전후: WebKit codex",
    "긴 대화 앞 쪽: Chromium 1280 claude",
    "긴 대화 앞 쪽: WebKit 390 codex",
  ]);
  assert.equal(jobs.length, 8);
  for (const r of [1, 2]) for (const name of steps) {
    assert.equal(jobs.filter((j) => j.r === r && new RegExp(j.filter).test(name)).length, 1, name);
  }
  assert.ok(jobs.every((j) => j.weight === 30));
});

test("단일 worker는 기존 순서·실험실이고 grep은 선택한 조합만 분할한다", () => {
  const sequential = planJobs([spec], { ...options, workers: 1 });
  assert.equal(sequential.length, 2);
  assert.ok(sequential.every((j) => j.filter === null && j.part === null));
  const selected = planJobs([spec], { ...options, only: /WebKit 390/ });
  assert.equal(selected.length, 2);
  assert.ok(selected.every((j) => steps.filter((n) => new RegExp(j.filter).test(n)).length === 1));
});

test("상태를 이어 쓰는 spec은 분할하지 않고 긴 작업부터 배치한다", () => {
  const dependent = { name: "23-composer-attachments", steps: ["준비", "보내기"] };
  const jobs = planJobs([spec, dependent], { ...options, repeat: 1, durations: { [spec.name]: 120, [dependent.name]: 70 } });
  assert.equal(jobs[0].s, dependent);
  assert.equal(jobs[0].filter, null);
  assert.equal(jobs.filter((j) => j.s === dependent).length, 1);
});

test("분할 작업의 시간은 반복별 합계이며 실제 전체 wall time과 구별한다", () => {
  const runs = [{ name: "x", r: 1, ms: 10 }, { name: "x", r: 1, ms: 20 }, { name: "x", r: 2, ms: 40 }];
  assert.deepEqual(specDurations(runs, "x", 2), [30, 40]);
});

test("단계 선택의 정규식은 이름의 문장 부호를 문자 그대로 비교한다", () => {
  const name = "이름 (괄호)+ [대괄호] .*? $^ {경계} \\";
  const jobs = planJobs([{ ...spec, steps: [name] }], { ...options, repeat: 1 });
  const pattern = new RegExp(jobs[0].filter);
  assert.ok(pattern.test(name));
  assert.ok(!pattern.test(`${name} 다른 단계`));
  assert.ok(!pattern.test(`다른 단계 ${name}`));
});

test("셸 대화의 두 harness 수명은 서로 격리하고 직접 실행부터 재개까지 이어 간다", () => {
  const body = fs.readFileSync(new URL("../e2e/specs/93-shell-chat.mjs", import.meta.url), "utf8");
  const names = [...body.matchAll(/^ {2}await step\("((?:[^"\\]|\\.)*)"/gm)].map((m) => JSON.parse(`"${m[1]}"`));
  const shell = { name: "93-shell-chat", steps: names };
  assert.equal(names.length, 2);
  const jobs = planJobs([shell], { ...options, repeat: 1 });
  assert.equal(jobs.length, 2);
  for (const name of names) assert.equal(jobs.filter((j) => new RegExp(j.filter).test(name)).length, 1, name);
  const selected = planJobs([shell], { ...options, repeat: 1, only: /Claude.*WebKit 390$/ });
  assert.equal(selected.length, 1);
  assert.match(selected[0].filter, /Claude/);
  const sequential = planJobs([shell], { ...options, repeat: 1, workers: 1 });
  assert.equal(sequential.length, 1);
  assert.equal(sequential[0].filter, null);
});
