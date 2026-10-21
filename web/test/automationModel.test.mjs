// 자동화 상세·변환·막힌 큐 판정(src/automationModel.ts, 관리 GUI W1, plan §8.303).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { archiveDirs, canPreviewConvert, cardTone, cardWord, convertGroups, legacyFiles, legacyRule, lastSkip, ruleTargets, ruleTone, ruleWord, shortTrigger, stuckBatch } from "../src/automationModel.ts";

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
    // 옛 임무의 조건 줄과 임무는 같은 파일 하나다
    const rules = [{ file: "routines/check.md" }, { file: "missions/watch.md" }, { file: "automations/new.toml" }];
    assert.deepEqual(legacyFiles(rules), ["missions/watch.md", "routines/check.md"]);
  });
  it("막힌 큐는 보낸 결과를 모르는 묶음만이며 줄 색에서 마지막 결과보다 먼저다. 파일 오류·꺼짐이 그보다 먼저다", () => {
    assert.equal(stuckBatch(rule({ queue: { pending: 1, inflight: inflight({ unknown: false }) } })), null);
    assert.equal(stuckBatch(rule({ queue: null })), null);
    const stuck = rule({ queue: { pending: 3, inflight: inflight() }, last_result: { status: "done" } });
    assert.equal(stuckBatch(stuck).id, "automation-1");
    assert.equal(ruleTone(stuck), "stuck");
    assert.equal(ruleTone({ ...stuck, enabled: false }), "off");
    assert.equal(ruleTone({ ...stuck, error: "깨짐" }), "file-error");
    assert.equal(ruleTone(rule({ last_result: { status: "waiting" } })), "waiting");
    assert.equal(ruleTone(rule({ last_result: { status: "이상한" } })), "ready");
  });
  it("정상 파일의 지난 시작 실패는 실행 실패이며 start:config라는 기록 이름도 파일 오류가 아니다", () => {
    for (const id of ["start:config", "flow:error", "automation:error", "manual:1"]) {
      const failed = rule({ last_result: { id, status: "failed", detail: "host_profiles.v1 없음" } });
      assert.equal(ruleTone(failed), "failed");
      assert.equal(ruleWord[ruleTone(failed)], "실행 실패");
    }
    const broken = rule({ enabled: false, error: "TOML 구문 오류", last_result: { status: "done" } });
    assert.equal(ruleTone(broken), "file-error");
    assert.equal(ruleWord[ruleTone(broken)], "파일 오류");
    assert.equal(ruleWord[ruleTone(rule({ last_result: { status: "waiting" } }))], "대기");
  });
  it("대상: 동작 낱말, 명령 머신, 승인 범위의 후보·profile(조건 줄 제외), 보내던 thread", () => {
    const targets = ruleTargets({ action: "새 thread · 점검", command: "", machine: null, start_scope: "조건: every 10m\n시작 후보:\n- mac-a · 작업 폴더(저장소 없음)\nprofile: fake\n", queue: { pending: 0, inflight: inflight({ thread: "t-0009" }) } });
    assert.deepEqual(targets, ["새 thread · 점검", "mac-a · 작업 폴더(저장소 없음)", "profile: fake", "보내던 thread: t-0009"]);
    assert.deepEqual(ruleTargets({ action: "coordinator 안내", command: "git status", machine: null, start_scope: null, queue: null }), ["coordinator 안내", "명령 실행 머신: host 머신"]);
  });
  it("변환 계획은 규칙·brief·기억·보존 사본을 묶는다", () => {
    const g = Object.fromEntries(convertGroups(["automations/check.toml", "automations/check.brief.md", "automations/check.memory/memory.md", "missions/.converted/watch.md"]).map((x) => [x.key,x.files]));
    assert.deepEqual(g.rules,["automations/check.toml"]);
    assert.deepEqual(g.memory,["automations/check.memory/memory.md"]);
    assert.deepEqual(g.bodies,["automations/check.brief.md"]);
    assert.deepEqual(archiveDirs(g.archived),["missions/.converted/"]);
  });
  it("마지막 건너뜀은 host 값이 먼저, 없으면 기록의 첫 건너뜀·대기", () => {
    const runs = [{ id: "a", status: "done" }, { id: "b", status: "waiting" }, { id: "c", status: "skipped" }];
    assert.equal(lastSkip({ last_skip: null }, runs).id, "b");
    assert.equal(lastSkip({ last_skip: { id: "z", status: "skipped" } }, runs).id, "z");
    assert.equal(lastSkip({ last_skip: null }, [{ id: "a", status: "done" }]), null);
  });
});

describe("자동화 카드(t-0628)", () => {
  it("표지는 파일 오류 → 막힘 → 승인 필요 → 꺼짐 → 실행 실패 → 켜짐 중 먼저 맞는 하나다. 처리됨·건너뜀·대기는 켜짐이다", () => {
    const stuck = { pending: 1, inflight: inflight() };
    assert.equal(cardTone(rule({ error: "깨짐", queue: stuck })), "error");
    // 막힘·승인 필요는 활동 막대 배지가 센다: 꺼져 있어도 카드에 단다
    assert.equal(cardTone(rule({ enabled: false, queue: stuck, start_approved: false })), "stuck");
    assert.equal(cardTone(rule({ enabled: false, start_approved: false, last_result: { status: "failed" } })), "approve");
    assert.equal(cardTone(rule({ enabled: false, last_result: { status: "failed" } })), "off");
    assert.equal(cardTone(rule({ last_result: { status: "failed" } })), "error");
    assert.equal(cardTone(rule({ start_approved: false })), "approve");
    assert.equal(cardTone(rule({ command: "make check", approved: false })), "approve");
    assert.equal(cardTone(rule({ command: "make check", approved: true })), "on");
    for (const status of ["done", "skipped", "waiting"]) assert.equal(cardTone(rule({ last_result: { status }, start_approved: true })), "on", status);
    assert.deepEqual(Object.keys(cardWord).sort(), ["approve", "error", "off", "on", "stuck"]);
  });
  it("조건 한 줄: 일정은 사람 말, 저장소 경로는 이름만, 그 밖은 host 글 그대로", () => {
    const t = (trigger, schedule = "") => shortTrigger({ trigger, schedule });
    assert.equal(t("every 10m"), "10분마다");
    assert.equal(t("every 2d"), "2일마다");
    assert.equal(t("every 1h"), "1시간마다");
    assert.equal(t("daily 04:00"), "매일 04:00");
    assert.equal(t("daily 04:30 명령 출력 변화"), "매일 04:30 명령 출력 변화");
    assert.equal(t("/srv/work/aaa main CI (failed)"), "aaa main CI (failed)");
    assert.equal(t("/srv/work/app main, dev Git (pushed)"), "app main, dev Git (pushed)");
    assert.equal(t("app main CI (failed)"), "app main CI (failed)");
    assert.equal(t("사건 report_ready · repos app"), "사건 report_ready · repos app");
    assert.equal(t("on pr: checks-failed, review"), "on pr: checks-failed, review");
    assert.equal(t("", "every 30m"), "30분마다");
    assert.equal(t("", ""), "조건 정보 없음");
  });
});
