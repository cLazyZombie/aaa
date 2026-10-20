// 「안전」·「profile」·작업 폴더 규칙(src/settingsModel.ts, 관리 GUI W5c, plan §8.353). 웹이 바꾸는 key·custom 거부·판 대조·절대 경로의
// 정본은 host다(crates/aaa-host/src/settings_admin/tests.rs). 여기는 화면이 보낼 요청을 정하는 규칙이다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { absolutePath, argsFromText, canSettings, canSwitch, draftOf, needsYoloConfirm, profileChange, profileNameOk } from "../src/settingsModel.ts";

const row = (over = {}) => ({
  name: "opus",
  harness: "claude",
  model: "opus",
  effort: "high",
  description: "",
  args: ["--verbose"],
  env_names: [],
  builtin: false,
  custom: false,
  launch: [],
  ...over,
});

describe("관리 W5c", () => {
  it("웹 profile·safety·작업 폴더·hook 관리는 1.81 이상 host만", () => {
    for (const v of [undefined, null, "1.80", "2.81"]) assert.equal(canSettings(v), false, String(v));
    for (const v of ["1.81", "1.90"]) assert.equal(canSettings(v), true, v);
  });
  it("yolo를 켤 때만 한 번 더 묻는다", () => {
    assert.equal(needsYoloConfirm("yolo", "on"), true);
    for (const [k, v] of [
      ["yolo", "off"],
      ["yolo", null],
      ["start_threads", "auto"],
    ])
      assert.equal(needsYoloConfirm(k, v), false, `${k} ${v}`);
  });
  it("인자는 줄마다 하나라 빈칸이 든 인자도 하나로 간다", () => {
    assert.deepEqual(argsFromText("--settings\n {\"a\": 1} \n\n-v\n"), ["--settings", '{"a": 1}', "-v"]);
  });
  it("기본 profile을 고치면 같은 이름으로 더해 대신한다", () => {
    const builtin = row({ name: "codex", harness: "codex", model: "", effort: "", args: [], builtin: true });
    const c = profileChange(builtin, { ...draftOf(builtin), effort: "high" });
    assert.deepEqual(c, { change: "add", name: "codex", harness: "codex", model: "", effort: "high", description: "", args: [] });
    assert.equal(profileChange(row(), draftOf(row())).change, "edit");
  });
  it("harness를 바꾸면 그 harness가 받지 않는 model·effort는 비운다", () => {
    const c = profileChange(row(), { ...draftOf(row()), harness: "shell" });
    assert.deepEqual([c.harness, c.model, c.effort, c.args], ["shell", "", "", ["--verbose"]]);
    // codex는 claude의 effort 중 받는 값만 남긴다
    assert.equal(profileChange(row({ effort: "max" }), { ...draftOf(row({ effort: "max" })), harness: "codex" }).effort, "max");
  });
  it("profile 이름은 host `validate_name`과 같다", () => {
    for (const ok of ["opus", "gpt-5.5", "a_b"]) assert.equal(profileNameOk(ok), true, ok);
    for (const bad of ["", "-x", ".x", "a b", "a/b", "x".repeat(41)]) assert.equal(profileNameOk(bad), false, bad);
  });
  it("작업 폴더는 절대 경로만(host `absolute`와 같다)", () => {
    for (const ok of ["/work", " /work ", "C:\\work", "d:/work"]) assert.equal(absolutePath(ok), true, ok);
    for (const bad of ["", "~/work", "work", "./w", "C:work", "\\\\server\\w"]) assert.equal(absolutePath(bad), false, bad);
  });
  it("전환은 같은 화면에서 점검을 통과한 그 경로·mount일 때만", () => {
    const checked = { path: "/work/next", mount: "" };
    assert.equal(canSwitch(null, "/work/next", ""), false);
    assert.equal(canSwitch(checked, "/work/next", ""), true);
    assert.equal(canSwitch(checked, " /work/next ", " "), true);
    assert.equal(canSwitch(checked, "/work/other", ""), false);
    assert.equal(canSwitch(checked, "/work/next", "/Volumes/W"), false);
  });
});
