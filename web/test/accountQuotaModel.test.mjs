import assert from "node:assert/strict";
import { test } from "node:test";
import { quotaAge, quotaLabel, quotaLeft, quotaState } from "../src/accountQuotaModel.ts";

test("구독 잔량: 알려진 기간만 5시간·주간, 리셋·실패·오래됨은 합성하지 않는다", () => {
  const w = { limit_id: "codex", scope: null, duration_mins: 300, used_percent: 72, observed_at_ms: 1000, resets_at_ms: 9999999, freshness: "fresh", problem: null };
  assert.equal(quotaLabel(w), "5시간");
  assert.equal(quotaLeft(w), "28% 남음");
  assert.equal(quotaLabel({ ...w, duration_mins: 10080 }), "주간");
  assert.equal(quotaLabel({ ...w, duration_mins: 15, scope: "spark" }), "15분 창 · spark");
  assert.equal(quotaLabel({ ...w, duration_mins: null }), "기타 창");
  assert.equal(quotaState(w, 121000), "fresh");
  assert.equal(quotaState(w, 121001), "stale");
  assert.equal(quotaState(w, 601001), "stale");
  assert.equal(quotaState(w, 9999998), "stale");
  assert.equal(quotaState({ ...w, resets_at_ms: null }, 601000), "stale");
  assert.equal(quotaState({ ...w, resets_at_ms: null }, 601001), "unknown");
  assert.equal(quotaState(w, 9999999), "unknown");
  assert.equal(quotaLeft(w), "28% 남음", "리셋으로 100%를 만들지 않는다");
  assert.equal(quotaState({ ...w, problem: "expired" }, 2000), "unknown");
  assert.equal(quotaState({ ...w, observed_at_ms: 32001 }, 2000), "unknown");
  assert.equal(quotaLeft({ ...w, used_percent: NaN }), "사용량 모름");
});

test("마지막 관측은 수신 시각 대신 실제 관측 경과 분을 표시한다", () => {
  assert.equal(quotaAge(1000, 60_999), "방금 관측");
  assert.equal(quotaAge(1000, 61_000), "1분 전 관측");
  assert.equal(quotaAge(1000, 3_601_000), "60분 전 관측");
  assert.equal(quotaAge(null, 1000), "관측 시각 모름");
  assert.equal(quotaAge(NaN, 1000), "관측 시각 모름");
  assert.equal(quotaAge(31_001, 1000), "관측 시각 모름");
});
