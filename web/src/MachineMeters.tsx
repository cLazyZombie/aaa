// 머신 패널 머리줄 오른쪽의 CPU·MEM 막대(plan §8.245, t-0393 시안 B안). 구독 사용량 칩(UsageMeters `usage-chip`)과 같은 값 + 2px 막대 둘이고
// C·M 머리글자는 title이 푼다. 값·높음 판정은 machineStats.ts다.
import type { CSSProperties } from "react";
import { isHigh, type MachineStatsValue } from "./machineStats";
import "./MachineMeters.css";

function Meter({ label, value }: { label: string; value: number | null }) {
  return (
    <span className={`machine-meter${isHigh(value) ? " is-high" : ""}${value === null ? " is-empty" : ""}`} data-stat={label}>
      <span className="machine-meter-label">{label}</span>
      <span className="machine-meter-value">{value ?? "—"}</span>
      <span className="machine-meter-bar" style={{ "--fill": `${value ?? 0}%` } as CSSProperties} />
    </span>
  );
}

/** 머리줄 오른쪽(+ 앞) CPU·MEM 막대 둘 */
export function MachineMeters({ stats, name }: { stats: MachineStatsValue; name: string }) {
  return (
    <span className="machine-meters" role="img" title={stats.detail} aria-label={`${name} ${stats.detail}`} data-testid={`machine-stats-${name}`}>
      <Meter label="C" value={stats.cpu} />
      <Meter label="M" value={stats.mem} />
    </span>
  );
}
