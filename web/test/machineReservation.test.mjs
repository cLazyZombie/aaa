// 머신 독점 예약의 목록 문구(src/machineReservation.ts, t-1367): 끝 시각은 오늘이면 시각만, 다른 날이면 날짜도, 없으면 「풀 때까지」.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { reservationDetail, reservationUntil, reservedMachines } from "../src/machineReservation.ts";

const now = new Date(2026, 9, 31, 9, 30).getTime();

describe("머신 독점 예약 문구", () => {
  it("끝 시각: 오늘·다른 날·없음", () => {
    assert.equal(reservationUntil(new Date(2026, 9, 31, 18, 5).getTime(), now), "18:05까지");
    assert.equal(reservationUntil(new Date(2026, 10, 1, 9, 0).getTime(), now), "11-01 09:00까지");
    assert.equal(reservationUntil(undefined, now), "풀 때까지");
  });
  it("까닭이 없으면 끝 시각만, 예약 머신 수는 끊긴 머신도 센다", () => {
    assert.equal(reservationDetail({ project: "p", holder: "d-0486", note: "빌드 측정", by: "사용자" }, now), "빌드 측정 · 풀 때까지");
    assert.equal(reservationDetail({ project: "p", holder: "t-1234", by: "t-1234", until_ms: new Date(2026, 9, 31, 18, 0).getTime() }, now), "18:00까지");
    const r = { project: "p", holder: "d-1", by: "사용자" };
    assert.equal(reservedMachines([{ reservation: r, connection: "connected" }, { connection: "connected" }, { reservation: r, connection: "disconnected" }]), 2);
  });
});
