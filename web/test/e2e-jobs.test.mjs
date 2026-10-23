import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { planJobs, specDurations } from "../e2e/lib/jobs.mjs";

const source = fs.readFileSync(new URL("../e2e/specs/37-chat-pages.mjs", import.meta.url), "utf8");
const steps = [...source.matchAll(/^ {2}await step\("((?:[^"\\]|\\.)*)"/gm)].map((m) => JSON.parse(`"${m[1]}"`));
const spec = { name: "37-chat-pages", steps };
const options = { repeat: 2, workers: 8, durations: { [spec.name]: 120 }, only: null };

test("긴 대화의 모든 기존 조합을 반복마다 정확히 한 번 실행한다", () => {
  const jobs = planJobs([spec], options);
  assert.equal(steps.length, 12);
  assert.equal(jobs.length, 24);
  for (const r of [1, 2]) for (const name of steps) {
    assert.equal(jobs.filter((j) => j.r === r && new RegExp(j.filter).test(name)).length, 1, name);
  }
  assert.ok(jobs.every((j) => j.weight === 10));
});

test("단일 worker는 기존 순서·실험실이고 grep은 선택한 조합만 분할한다", () => {
  const sequential = planJobs([spec], { ...options, workers: 1 });
  assert.equal(sequential.length, 2);
  assert.ok(sequential.every((j) => j.filter === null && j.part === null));
  const selected = planJobs([spec], { ...options, only: /WebKit 390/ });
  assert.equal(selected.length, 4);
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

test("셸 대화 직접 실행·재개의 모든 조합을 독립 실험실에서 한 번씩 실행한다", () => {
  const body = fs.readFileSync(new URL("../e2e/specs/93-shell-chat.mjs", import.meta.url), "utf8");
  const names = [...body.matchAll(/^ {2}await step\("((?:[^"\\]|\\.)*)"/gm)].map((m) => JSON.parse(`"${m[1]}"`));
  const shell = { name: "93-shell-chat", steps: names };
  const cases = [...body.matchAll(/await harnessCase\("(codex|claude)", "(chromium|webkit)", (1280|390)(, check)?\)/g)]
    .map((m) => [m[4] ? "direct" : "resume", ...m.slice(1, 4)].join(":"));
  const expected = ["direct", "resume"].flatMap((mode) => ["codex", "claude"].flatMap((harness) => ["chromium", "webkit"]
    .flatMap((engine) => [1280, 390].map((width) => `${mode}:${harness}:${engine}:${width}`))));
  assert.deepEqual(cases.sort(), expected.sort());
  assert.equal(names.length, 16);
  const jobs = planJobs([shell], { ...options, repeat: 1 });
  assert.equal(jobs.length, 16);
  for (const name of names) assert.equal(jobs.filter((j) => new RegExp(j.filter).test(name)).length, 1, name);
  const selected = planJobs([shell], { ...options, repeat: 1, only: /^Claude.*WebKit 390$/ });
  assert.equal(selected.length, 1);
  assert.match(selected[0].filter, /Claude/);
  const sequential = planJobs([shell], { ...options, repeat: 1, workers: 1 });
  assert.equal(sequential.length, 1);
  assert.equal(sequential[0].filter, null);
});
