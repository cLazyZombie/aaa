import { test } from "node:test";
import assert from "node:assert/strict";
import { rememberedSignIn, rememberSignIn } from "../src/signedIn.ts";

function storage() {
  const map = new Map();
  return { map, getItem: (k) => (map.has(k) ? map.get(k) : null), setItem: (k, v) => map.set(k, String(v)), removeItem: (k) => map.delete(k) };
}

test("확인한 기기만 기억하고 null이면 지운다", () => {
  const s = storage();
  assert.equal(rememberedSignIn(s), null);
  rememberSignIn({ role: "controller", device: "D1", front_api: "1.2", passkey: { registered: true } }, s);
  assert.deepEqual(JSON.parse(s.map.get("aaa:signed-in")), { role: "controller", device: "D1" });
  assert.deepEqual(rememberedSignIn(s), { role: "controller", device: "D1" });
  rememberSignIn(null, s);
  assert.equal(s.map.has("aaa:signed-in"), false);
  assert.equal(rememberedSignIn(s), null);
});

test("모르는 역할·빈 기기·깨진 값·막힌 저장소는 기억이 없는 것으로 본다", () => {
  for (const raw of ['{"role":"admin","device":"D1"}', '{"role":"reader","device":""}', '{"role":"reader"}', "{", "null", "[]"]) {
    const s = storage();
    s.setItem("aaa:signed-in", raw);
    assert.equal(rememberedSignIn(s), null, raw);
  }
  const blocked = { getItem() { throw new Error("SecurityError"); }, setItem() { throw new Error("SecurityError"); }, removeItem() { throw new Error("SecurityError"); } };
  assert.equal(rememberedSignIn(blocked), null);
  assert.doesNotThrow(() => rememberSignIn({ role: "reader", device: "D2" }, blocked));
  assert.equal(rememberedSignIn(null), null);
});
