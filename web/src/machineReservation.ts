// 머신 독점 예약(docs/machine-assignment.md)의 목록 표시 문구(t-1367 시안 1A·2A). 그리기는 Sidebar `MachineStrip`·ActivityBar다.
// 값은 host 상태 사건의 MachineView.reservation이 정본이고, 풀림 판정도 host가 한다(웹은 받은 그대로 보인다).
import type { MachineReservation, MachineView } from "./gen/proto";

const pad = (n: number) => String(n).padStart(2, "0");

/** 끝 시각: 오늘이면 「18:00까지」, 다른 날이면 「11-01 18:00까지」, 없으면 「풀 때까지」(기기 정보의 기간 이름과 같다) */
export function reservationUntil(untilMs: number | undefined, now = Date.now()): string {
  if (untilMs === undefined) return "풀 때까지";
  const at = new Date(untilMs);
  const today = new Date(now);
  const time = `${pad(at.getHours())}:${pad(at.getMinutes())}`;
  const sameDay = at.getFullYear() === today.getFullYear() && at.getMonth() === today.getMonth() && at.getDate() === today.getDate();
  return sameDay ? `${time}까지` : `${pad(at.getMonth() + 1)}-${pad(at.getDate())} ${time}까지`;
}

/** 이름 아래 줄의 주인 뒤 글: 까닭 · 끝 시각 */
export function reservationDetail(r: MachineReservation, now = Date.now()): string {
  return [r.note, reservationUntil(r.until_ms, now)].filter(Boolean).join(" · ");
}

/** 활동 막대 머신 단추의 자물쇠: 독점 예약이 걸린 머신 수(끊긴 머신도 센다: 예약은 연결과 무관하게 새 시작을 막는다) */
export function reservedMachines(machines: MachineView[]): number {
  return machines.filter((m) => m.reservation).length;
}
