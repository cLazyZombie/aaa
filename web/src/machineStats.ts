// 머신 패널의 CPU %·MEM %(10-19 사용자 「웹 UI에 머신에 cpu %, mem % 를 보여줄수 있어? 간략하게」, plan §8.243, t-0393 시안 B안).
// 값과 높음 판정(그리기는 MachineMeters.tsx). 값은 agent 건강 보고(MachineHealth `cpu_pct`·`mem_used_bytes`·`mem_total_bytes`, 15초마다)다.
// 끊긴 머신은 값이 오래돼 그리지 않는다.
import type { MachineView } from "./gen/proto";

/** 이 값 이상이면 빨갛게(구독 사용량 HIGH_PERCENT 80과 달리 머신은 늘 바쁘게 쓰므로 90) */
export const HIGH_STAT = 90;

export interface MachineStatsValue {
  cpu: number | null;
  mem: number | null;
  /** title: 값과 그 바탕(메모리 바이트·부하·몇 초 전) */
  detail: string;
}

/** 연결 중인 머신의 값. 끊겼거나 보고가 없으면 null이고, 옛 agent는 값 칸이 없어 cpu·mem이 null이다 */
export function machineStats(m: MachineView, now = Date.now()): MachineStatsValue | null {
  const h = m.health;
  if (m.connection !== "connected" || !h) return null;
  const cpu = h.cpu_pct ?? null;
  const used = h.mem_used_bytes ?? null;
  const total = h.mem_total_bytes ?? null;
  const mem = used !== null && total ? Math.round((Math.min(used, total) / total) * 100) : null;
  const gb = (b: number) => `${(b / 2 ** 30).toFixed(1)}GB`;
  const parts = [
    cpu === null ? "CPU 모름" : `CPU ${cpu}%`,
    mem === null ? "메모리 모름" : `메모리 ${gb(used!)} / ${gb(total!)} 사용(${mem}%)`,
    h.load_x100 === null ? null : `부하 ${(h.load_x100 / 100).toFixed(2)}/${h.cpus}`,
    `${Math.max(0, Math.round((now - h.at_ms) / 1000))}초 전`,
  ];
  return { cpu, mem, detail: parts.filter(Boolean).join(" · ") };
}

export const isHigh = (v: number | null) => v !== null && v >= HIGH_STAT;

/** 활동 막대 머신 단추 배지: CPU나 MEM이 높은 연결 중 머신 수(접힌 사이드바에서도 보인다) */
export function highMachines(machines: MachineView[]): number {
  return machines.filter((m) => {
    const s = machineStats(m);
    return s !== null && (isHigh(s.cpu) || isHigh(s.mem));
  }).length;
}
