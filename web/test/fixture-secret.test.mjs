// Node E2E 토큰이 Rust·Python과 같은 규약을 쓰고 업무 단어는 즉시 거부하는 계약.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { token, freshToken, assertOpaque } from "../../scripts/fixture-secret.mjs";

test("인증 fixture의 공통 벡터와 업무 단어 거부", () => {
  const vectors = JSON.parse(fs.readFileSync(new URL("../../scripts/fixtures/secret-vectors.json", import.meta.url), "utf8"));
  for (const v of vectors) {
    assert.equal(token(v.seed), v.token);
    assert.ok(!v.token.includes(v.seed));
  }
  for (const value of ["automation", "owner", "access_token", "refresh_token", "fixture-automation-token"])
    assert.throws(() => assertOpaque(value), e => !e.message.includes(value));
  for (const value of [token("owner") + "\n", token("owner").slice(1), "g99".repeat(32)])
    assert.throws(() => assertOpaque(value));
  assert.notEqual(freshToken(), freshToken());
});
