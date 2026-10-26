// 시험 전용 비밀 fixture 규약 v1. 운영 인증에는 쓰지 않는다.
import { createHash, randomBytes } from "node:crypto";

export function assertOpaque(value) {
  if (typeof value !== "string" || value.length !== 96 || !/^(?:[g-v](?:0[0-9]|1[0-5])){32}$/.test(value))
    throw new Error("인증 fixture는 fixture-secret token/freshToken으로 만든 opaque 값이어야 한다");
  return value;
}

export function token(seed) {
  return assertOpaque(Array.from(createHash("sha256").update(seed, "utf8").digest(),
    b => String.fromCharCode(103 + Math.floor(b / 16)) + String(b % 16).padStart(2, "0")).join(""));
}

export const freshToken = () => token(randomBytes(32).toString("hex"));
