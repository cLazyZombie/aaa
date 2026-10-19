// 사이드바 폭(src/sidebarWidth.ts, plan §8.257): herdr-web-ui 규칙(240~520px, 창 절반, ←→ 16px, Home·End)과 host 공유 설정 값.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { clampSidebarWidth, keyedSidebarWidth, parseSidebarWidth, sidebarMaxWidth } from "../src/sidebarWidth.ts";
import { preferencesChange, sanitizeSidebarShared, sharedSettings, takePreferences } from "../src/preferences.ts";

describe("사이드바 폭", () => {
  it("240~520px이고 창의 절반을 넘지 않는다", () => {
    assert.equal(clampSidebarWidth(100), 240);
    assert.equal(clampSidebarWidth(900), 520);
    assert.equal(clampSidebarWidth(333.4), 333);
    assert.equal(sidebarMaxWidth(800), 400);
    assert.equal(clampSidebarWidth(480, 800), 400);
    // 아주 좁은 창도 최소 폭은 지킨다
    assert.equal(sidebarMaxWidth(300), 240);
  });

  it("손잡이 키: ←→ 16px, Home 최소, End 이 창의 최대, 그 밖의 키는 손잡이 것이 아니다", () => {
    assert.equal(keyedSidebarWidth(320, "ArrowLeft"), 304);
    assert.equal(keyedSidebarWidth(320, "ArrowRight"), 336);
    assert.equal(keyedSidebarWidth(250, "ArrowLeft"), 240);
    assert.equal(keyedSidebarWidth(320, "Home"), 240);
    assert.equal(keyedSidebarWidth(320, "End", 1000), 500);
    assert.equal(keyedSidebarWidth(320, "Enter"), null);
  });

  it("공유 설정 값: 수만 폭이고 한도로 자르며, 없거나 못 읽으면 기본 폭(null)", () => {
    assert.equal(parseSidebarWidth(400), 400);
    assert.equal(parseSidebarWidth(9999), 520);
    assert.equal(parseSidebarWidth("400"), null);
    assert.equal(parseSidebarWidth(null), null);
    assert.equal(parseSidebarWidth(Number.NaN), null);
    assert.deepEqual(sanitizeSidebarShared({ paneOrder: ["a:1"], sidebarWidth: 360 }), { paneOrder: ["a:1"], sidebarWidth: 360 });
    assert.deepEqual(sanitizeSidebarShared({}), { paneOrder: [], sidebarWidth: null });
  });

  it("다른 화면이 끈 폭을 받고, 여기서 끈 폭과 되돌림(null)은 보낸다", () => {
    const mine = { paneOrder: [], sidebarWidth: null };
    const known = { settings: sharedSettings(mine), pins: [] };
    const taken = takePreferences(known, { settings: mine, pins: [] }, { revision: 2, settings: { sidebarWidth: 400 } }, null);
    assert.equal(taken.patch.sidebarWidth, 400);
    // 읽지 못하는 값은 받지 않는다
    assert.equal(takePreferences(known, { settings: mine, pins: [] }, { revision: 3, settings: { sidebarWidth: "wide" } }, null).patch.sidebarWidth, undefined);
    assert.deepEqual(preferencesChange(known, { settings: { ...mine, sidebarWidth: 360 }, pins: [] }).settings, { sidebarWidth: 360 });
    const wide = { settings: sharedSettings({ ...mine, sidebarWidth: 360 }), pins: [] };
    assert.deepEqual(preferencesChange(wide, { settings: mine, pins: [] }).settings, { sidebarWidth: null });
  });
});
