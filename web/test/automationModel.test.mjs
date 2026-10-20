// 자동화 상세·변환·막힌 큐 판정(src/automationModel.ts, 관리 GUI W1, plan §8.303).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { archiveDirs, canPreviewConvert, convertGroups, legacyFiles, legacyMission, legacyRule, lastSkip, ruleTargets, ruleTone, stuckBatch } from "../src/automationModel.ts";

const rule = (over) => ({ enabled: true, error: null, queue: null, last_result: null, last_skip: null, ...over });
const inflight = (over) => ({ id: "automation-1", events: 2, at_ms: 1, thread: "t-0001", unknown: true, ...over });

describe("자동화 상세", () => {
  it("변환 미리보기는 판을 알린 1.58 이상 host에만 보낸다: 옛 host는 dry_run을 버리고 실제로 변환한다", () => {
    for (const v of [undefined, null, "", "1.57", "1.5", "2.58", "x.y"]) assert.equal(canPreviewConvert(v), false, String(v));
    for (const v of ["1.58", "1.59", "1.100"]) assert.equal(canPreviewConvert(v), true, v);
  });
  it("이전 형식: routines·missions 바로 아래 md만, 임무는 front matter로 시작하는 글", () => {
    assert.equal(legacyRule({ file: "routines/check.md" }), true);
    assert.equal(legacyRule({ file: "missions/watch.md" }), true);
    assert.equal(legacyRule({ file: "automations/check.toml" }), false);
    assert.equal(legacyRule({ file: "routines/.converted/check.md" }), false);
    assert.equal(legacyMission({ file_text: '+++\nprofile="x"\n+++\n본문' }), true);
    assert.equal(legacyMission({ file_text: 'profile = "x"\n' }), false);
    // 옛 임무의 조건 줄과 임무는 같은 파일 하나다
    const rules = [{ file: "routines/check.md" }, { file: "missions/watch.md" }, { file: "automations/new.toml" }];
    const missions = [{ name: "watch", file_text: "+++\n" }, { name: "next", file_text: "profile='x'\n" }];
    assert.deepEqual(legacyFiles(rules, missions), ["missions/watch.md", "routines/check.md"]);
  });
  it("막힌 큐는 보낸 결과를 모르는 묶음만이며 줄 색에서 마지막 결과보다 먼저다. 꺼짐·파일 오류가 그보다 먼저다", () => {
    assert.equal(stuckBatch(rule({ queue: { pending: 1, inflight: inflight({ unknown: false }) } })), null);
    assert.equal(stuckBatch(rule({ queue: null })), null);
    const stuck = rule({ queue: { pending: 3, inflight: inflight() }, last_result: { status: "done" } });
    assert.equal(stuckBatch(stuck).id, "automation-1");
    assert.equal(ruleTone(stuck), "stuck");
    assert.equal(ruleTone({ ...stuck, enabled: false }), "off");
    assert.equal(ruleTone({ ...stuck, error: "깨짐" }), "failed");
    assert.equal(ruleTone(rule({ last_result: { status: "waiting" } })), "waiting");
    assert.equal(ruleTone(rule({ last_result: { status: "이상한" } })), "ready");
  });
  it("대상: 동작 낱말, 명령 머신, 승인 범위의 후보·profile(조건 줄 제외), 보내던 thread", () => {
    const targets = ruleTargets({ action: "새 thread · 점검", command: "", machine: null, start_scope: "조건: every 10m\n시작 후보:\n- mac-a · 작업 폴더(저장소 없음)\nprofile: fake\n", queue: { pending: 0, inflight: inflight({ thread: "t-0009" }) } });
    assert.deepEqual(targets, ["새 thread · 점검", "mac-a · 작업 폴더(저장소 없음)", "profile: fake", "보내던 thread: t-0009"]);
    assert.deepEqual(ruleTargets({ action: "coordinator 안내", command: "git status", machine: null, start_scope: null, queue: null }), ["coordinator 안내", "명령 실행 머신: host 머신"]);
  });
  it("변환 계획 묶음: 임무 파일이 있을 때만 <임무>-events·-schedule을 생기는 규칙으로, 보존 사본·스키마·긴 본문은 따로", () => {
    const files = [
      "automations/automation.schema.json",
      "automations/check.toml",
      "automations/watch-events.toml",
      "automations/watch-schedule.toml",
      "automations/other-events.toml",
      "automations/long.md",
      "missions/mission.schema.json",
      "missions/watch.md",
      "missions/watch.toml",
      "routines/.converted/check.md",
      "missions/.converted/watch.md",
    ];
    const g = Object.fromEntries(convertGroups(files).map((x) => [x.key, x.files]));
    assert.deepEqual(g.rules, ["automations/check.toml", "automations/other-events.toml"]);
    assert.deepEqual(g.derived, ["automations/watch-events.toml", "automations/watch-schedule.toml"]);
    assert.deepEqual(g.missions, ["missions/watch.md", "missions/watch.toml"]);
    assert.deepEqual(g.bodies, ["automations/long.md"]);
    assert.deepEqual(g.archived, ["routines/.converted/check.md", "missions/.converted/watch.md"]);
    assert.deepEqual(g.schema, ["automations/automation.schema.json", "missions/mission.schema.json"]);
    assert.deepEqual(archiveDirs(files), ["routines/.converted/", "missions/.converted/"]);
    assert.deepEqual(convertGroups([]), []);
  });
  it("마지막 건너뜀은 host 값이 먼저, 없으면 기록의 첫 건너뜀·대기", () => {
    const runs = [{ id: "a", status: "done" }, { id: "b", status: "waiting" }, { id: "c", status: "skipped" }];
    assert.equal(lastSkip({ last_skip: null }, runs).id, "b");
    assert.equal(lastSkip({ last_skip: { id: "z", status: "skipped" } }, runs).id, "z");
    assert.equal(lastSkip({ last_skip: null }, [{ id: "a", status: "done" }]), null);
  });
});
