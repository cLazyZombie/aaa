// 조작 기기 passkey(src/passkey.ts, 관리 GUI W7). 서명·challenge·등록 시한의 정본은 host다(crates/aaa-host/src/passkey/tests.rs).
// 여기는 WebAuthn 바이트 칸의 base64url과 화면이 등록을 여는 규칙이다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { canRegister, fromB64url, minutesLeft, toB64url } from "../src/passkey.ts";

describe("관리 W7 passkey", () => {
  it("base64url은 채움 없이 왕복한다(host는 URL_SAFE_NO_PAD)", () => {
    for (const n of [0, 1, 2, 3, 31, 32, 65]) {
      const bytes = Uint8Array.from({ length: n }, (_, i) => (i * 37 + 250) % 256);
      const text = toB64url(bytes);
      assert.doesNotMatch(text, /[+/=]/);
      assert.deepEqual([...fromB64url(text)], [...bytes]);
    }
    assert.equal(toB64url(new Uint8Array([0xfb, 0xff])), "-_8");
  });
  it("등록은 passkey가 없고 짝지은 지 10분이 안 된 공개 주소 화면만", () => {
    const now = 1_000_000;
    const state = (over = {}) => ({ rp_id: "mac-a.example.ts.net", registered: false, register_until_ms: now + 90_000, ...over });
    assert.equal(canRegister(state(), now), true);
    assert.equal(minutesLeft(state(), now), 2);
    assert.equal(canRegister(state({ registered: true }), now), false);
    assert.equal(canRegister(state({ register_until_ms: null }), now), false);
    assert.equal(canRegister(state({ register_until_ms: now }), now), false);
    assert.equal(canRegister(null, now), false);
    assert.equal(canRegister(undefined, now), false);
  });
});
