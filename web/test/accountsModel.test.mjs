// 설정 「계정」 규칙(src/accountsModel.ts, 관리 GUI W5a, plan §8.351). 권한·메일 싣기·상태 판정의 정본은 host다
// (account_meta/tests.rs, m13 `host_relogin_checks_the_email_and_asks_before_switching`).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { renewalAccounts, canAccounts, day, groupAccounts, healthWord, loginText, validAlias } from "../src/accountsModel.ts";

describe("계정", () => {
  it("웹 로그인·기본 변경은 1.79 이상 host만", () => {
    for (const v of [undefined, null, "1.78", "2.79"]) assert.equal(canAccounts(v), false, String(v));
    for (const v of ["1.79", "1.90"]) assert.equal(canAccounts(v), true, v);
  });
  it("별칭은 host와 같은 꼴만", () => {
    for (const ok of ["work", "work-claude_2", "a".repeat(64)]) assert.equal(validAlias(ok), true, ok);
    for (const bad of ["", "a b", "x@y", "a".repeat(65), "한글"]) assert.equal(validAlias(bad), false, bad);
  });
  it("상태 낱말: 끊김은 만료와 갱신 거부, 곧 만료는 남은 날을 올림해 보인다", () => {
    const now = Date.UTC(2026, 9, 21);
    assert.deepEqual(healthWord("ok", undefined, now), { text: "정상", tone: "ok" });
    assert.deepEqual(healthWord("expiring", now + 2.5 * 86_400_000, now), { text: "곧 만료(3일 남음)", tone: "warn" });
    assert.deepEqual(healthWord("expired", now - 1, now), { text: "끊김(만료)", tone: "bad" });
    assert.deepEqual(healthWord("rejected", undefined, now), { text: "끊김(갱신 거부)", tone: "bad" });
    // 옛 host는 상태를 주지 않는다: 배지를 그리지 않는다
    assert.deepEqual(healthWord(undefined, undefined, now), { text: "", tone: "none" });
  });
  it("날짜만 보이고 모르면 「모름」", () => {
    assert.equal(day(undefined), "모름");
    assert.match(day(Date.UTC(2026, 9, 21, 12)), /^2026-10-2[01]$/);
  });
  it("harness마다 묶고 기본 계정을 표시한다. 계정 없는 harness도 묶음이 있다", () => {
    const g = groupAccounts(
      [
        { name: "b", harness: "codex", version: 1 },
        { name: "a", harness: "codex", version: 2 },
      ],
      [{ harness: "codex", account: "b" }],
    );
    assert.deepEqual(g.map((x) => [x.harness, x.rows.map((r) => [r.name, r.isDefault])]), [["claude", []], ["codex", [["a", false], ["b", true]]]]);
  });
  it("로그인 글: 인증은 host Mac 브라우저에서, 다시 로그인은 기록된 메일을 알리고, 다른 메일이면 고르기를 기다린다", () => {
    const base = { id: "1", account: "work", harness: "codex" };
    const waiting = loginText({ ...base, state: "waiting", expect_email: "work@example.com" });
    assert.equal(waiting.done, false);
    assert.deepEqual(waiting.lines, ["host Mac 브라우저에서 인증을 마치세요. 인증 주소는 이 화면에 보이지 않습니다.", "이 메일로 로그인하세요: work@example.com"]);
    assert.equal(loginText({ ...base, state: "waiting" }).lines.length, 1, "새 별칭은 확인할 메일이 없다");
    const other = loginText({ ...base, state: "mismatch", expect_email: "work@example.com", email: "other@example.com" });
    assert.equal(other.done, false);
    assert.equal(other.tone, "warn");
    assert.ok(other.lines.includes("로그인한 메일: other@example.com"));
    for (const state of ["succeeded", "kept", "cancelled", "failed", "lost"]) assert.equal(loginText({ ...base, state }).done, true, state);
  });
});

it("갱신 메뉴는 만료·갱신 거부·곧 만료만 포함한다", () => {
  const rows = ["ok", "expiring", "expired", "rejected", undefined].map((health, n) => ({ name: `a${n}`, harness: "codex", health, version: 1 }));
  assert.deepEqual(renewalAccounts(rows).map(a => a.health), ["expired", "rejected", "expiring"]);
  assert.equal(rows.length, 5);
});
