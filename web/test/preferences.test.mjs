// 공유 설정 동기화의 순수 규칙(src/preferences.ts). herdr-web-ui `src/lib/preferencesSync.test.ts`를 옮겼다.
// 참고의 기기 설정(terminalFontSize 등)은 aaa에서 공유 설정 store에 없으므로 그 줄은 빠지고, aaa가 아직 없는 공유 이름
// (showThinking·agentMarks)은 「읽지 못하는 값」 쪽 시험으로 간다. node(24, 형 지우기)로 바로 돈다: `npm test`.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { DEFAULT_USAGE, preferencesChange, sharedSettings, takePreferences } from "../src/preferences.ts";

const DEFAULT = { theme: "dark", palette: "amber", sidebarRows: "two", ...DEFAULT_USAGE };
const light = { ...DEFAULT, theme: "light" };
const base = (settings, pins) => ({ settings: sharedSettings(settings), pins });

describe("공유 설정 동기화", () => {
  it("바뀐 공유 설정만, 고정은 바뀌었을 때만 보낸다", () => {
    const known = base(DEFAULT, ["local:w1:p1"]);
    assert.equal(preferencesChange(known, { settings: DEFAULT, pins: ["local:w1:p1"] }), null);
    assert.deepEqual(preferencesChange(known, { settings: light, pins: ["local:w1:p1"] }), { settings: { theme: "light" } });
    assert.deepEqual(preferencesChange(known, { settings: DEFAULT, pins: [] }), { settings: {}, pins: [] });
    assert.deepEqual(preferencesChange(known, { settings: { ...DEFAULT, usageOrder: ["claude"] }, pins: ["local:w1:p1"] }), {
      settings: { usageOrder: ["claude"] },
    });
  });

  it("host가 고정을 갖지 않은 동안 이 화면 고정은 첫 변경에 함께 가고, 고정만으로는 기다린다", () => {
    const unknown = base(DEFAULT, null);
    assert.equal(preferencesChange(unknown, { settings: DEFAULT, pins: ["local:w1:p1"] }), null);
    assert.deepEqual(preferencesChange(unknown, { settings: light, pins: ["local:w1:p1"] }), { settings: { theme: "light" }, pins: ["local:w1:p1"] });
  });

  it("host가 가진 공유 설정을 받는다", () => {
    const mine = { ...light, usageCount: "left" };
    const taken = takePreferences(base(mine, null), { settings: mine, pins: [] }, { revision: 3, settings: { theme: "dark", palette: "catppuccin" } }, null);
    assert.deepEqual(taken.patch, { theme: "dark", palette: "catppuccin" });
    assert.equal(taken.pins, null);
    assert.deepEqual(taken.base, { settings: { ...sharedSettings(mine), theme: "dark", palette: "catppuccin" }, pins: null });
    // 받은 뒤에는 보낼 것이 없다
    assert.equal(preferencesChange(taken.base, { settings: { ...mine, ...taken.patch }, pins: [] }), null);
  });

  it("여기서 바꾸고 아직 안 보낸 값은 남아, host 값 위로 나간다", () => {
    // 화면이 어두움으로 열렸고, 문서가 오기 전에 사람이 밝음을 골랐다
    const taken = takePreferences(base(DEFAULT, null), { settings: light, pins: [] }, { revision: 1, settings: { theme: "dark", usageCount: "left" } }, null);
    assert.deepEqual(taken.patch, { usageCount: "left" });
    assert.equal(taken.base.settings.theme, "dark");
    assert.deepEqual(preferencesChange(taken.base, { settings: { ...light, usageCount: "left" }, pins: [] }), { settings: { theme: "light" }, pins: [] });
  });

  it("이미 보낸 값보다 다른 곳의 나중 쓰기가 이기고, 문서의 나머지는 받는다", () => {
    // 밝음이 가는 동안 다른 화면이 팔레트를 바꾸고, 다시 테마를 어두움으로 돌렸다
    const inFlight = { settings: { theme: "light" } };
    const concurrent = takePreferences(
      base(DEFAULT, []),
      { settings: light, pins: [] },
      { revision: 2, settings: { theme: "light", palette: "catppuccin" }, pins: [] },
      inFlight,
    );
    assert.deepEqual(concurrent.patch, { theme: "light", palette: "catppuccin" });
    const overtaken = takePreferences(base(DEFAULT, []), { settings: light, pins: [] }, { revision: 3, settings: { theme: "dark" }, pins: [] }, inFlight);
    assert.deepEqual(overtaken.patch, { theme: "dark" });
    assert.equal(preferencesChange(overtaken.base, { settings: { ...light, theme: "dark" }, pins: [] }), null);
  });

  it("읽지 못하는 값(모르는 값·aaa에 없는 설정)은 두고, 제 것을 되돌려 보내지 않는다", () => {
    const remote = { revision: 1, settings: { theme: "purple", agentMarks: "yes", showThinking: false, palette: "catppuccin" } };
    const taken = takePreferences(base(DEFAULT, null), { settings: DEFAULT, pins: [] }, remote, null);
    assert.deepEqual(taken.patch, { palette: "catppuccin" });
    assert.equal(preferencesChange(taken.base, { settings: { ...DEFAULT, palette: "catppuccin" }, pins: [] }), null);
  });

  it("host가 처음 갖는 고정에 이 화면 고정을 합치고, 그 뒤로는 마지막 쓰기를 따른다", () => {
    const first = takePreferences(base(DEFAULT, null), { settings: DEFAULT, pins: ["local:w1:p1"] }, { revision: 1, settings: {}, pins: ["pc:w2:p3", "local:w1:p1"] }, null);
    assert.deepEqual(first.pins, ["pc:w2:p3", "local:w1:p1"]);
    assert.deepEqual(first.base.pins, ["pc:w2:p3", "local:w1:p1"]);
    const joined = takePreferences(base(DEFAULT, null), { settings: DEFAULT, pins: ["local:w1:p1"] }, { revision: 1, settings: {}, pins: ["pc:w2:p3"] }, null);
    assert.deepEqual(joined.pins, ["pc:w2:p3", "local:w1:p1"]);
    // 합친 것은 이 화면의 변경이다: 나간다
    assert.deepEqual(preferencesChange(joined.base, { settings: DEFAULT, pins: joined.pins }), { settings: {}, pins: ["pc:w2:p3", "local:w1:p1"] });
    const later = takePreferences(base(DEFAULT, ["pc:w2:p3"]), { settings: DEFAULT, pins: ["pc:w2:p3"] }, { revision: 2, settings: {}, pins: [] }, null);
    assert.deepEqual(later.pins, []);
    // 여기서 고정하고 아직 안 보낸 것은 남는다
    const pinning = takePreferences(base(DEFAULT, ["pc:w2:p3"]), { settings: DEFAULT, pins: ["pc:w2:p3", "local:w1:p1"] }, { revision: 2, settings: {}, pins: [] }, null);
    assert.equal(pinning.pins, null);
    assert.deepEqual(pinning.base.pins, []);
  });

  it("보기 권한 기기는 모두 받는다", () => {
    const taken = takePreferences(base(DEFAULT, ["a:b"]), { settings: light, pins: ["a:b", "c:d"] }, { revision: 5, settings: { theme: "dark" }, pins: [] }, null, true);
    assert.deepEqual(taken.patch, { theme: "dark" });
    assert.deepEqual(taken.pins, []);
  });
});
