// 머신 패널 머리줄 오른쪽의 agent 수·CPU·MEM(plan §8.245, §8.254 사용자 결정 10-19 「1안」). 셋 다 lucide 아이콘(Bot·Cpu·MemoryStick)
// + 숫자이고, CPU·MEM 아래에 2px 막대를 둔다(agent는 막대 자리만 비운다). 칸마다 title이 풀이한다. 값·높음 판정은 machineStats.ts다.
import type { CSSProperties } from "react";
import { Bot, Cpu, MemoryStick, type LucideIcon } from "lucide-react";
import { isHigh, type MachineStatsValue } from "./machineStats";
import "./MachineMeters.css";

function Meter({ stat, icon: Icon, value, bar, title }: { stat: string; icon: LucideIcon; value: number | null; bar: boolean; title: string }) {
  // 모름(옛 agent)은 「—」, agent 0은 흐린 「0」: 둘 다 흐리게 두어 줄맞춤을 지킨다
  const empty = value === null || (!bar && value === 0);
  return (
    <span className={`machine-meter${bar && isHigh(value) ? " is-high" : ""}${empty ? " is-empty" : ""}`} data-stat={stat} title={title}>
      <Icon aria-hidden="true" />
      <span className="machine-meter-value">{value ?? "—"}</span>
      {bar && <span className="machine-meter-bar" style={{ "--fill": `${value ?? 0}%` } as CSSProperties} />}
    </span>
  );
}

/** 에이전트 패널과 같은 기준으로 센 그 머신의 agent(일하는 중·답 기다림 포함) */
export type MachineAgents = { count: number; working: number; waiting: number };

/** 머리줄 오른쪽(+ 앞) agent·CPU·MEM */
export function MachineMeters({ stats, agents, name }: { stats: MachineStatsValue; agents: MachineAgents; name: string }) {
  const agentText = `agent ${agents.count} · 일하는 중 ${agents.working} · 답 기다림 ${agents.waiting}`;
  return (
    <span className="machine-meters" role="img" title={`${agentText}\n${stats.detail}`} aria-label={`${name} ${agentText} · ${stats.detail}`} data-testid={`machine-stats-${name}`}>
      <Meter stat="agents" icon={Bot} value={agents.count} bar={false} title={agentText} />
      <Meter stat="cpu" icon={Cpu} value={stats.cpu} bar title={stats.cpu === null ? "CPU 모름(옛 agent)" : `CPU ${stats.cpu}%`} />
      <Meter stat="mem" icon={MemoryStick} value={stats.mem} bar title={stats.mem === null ? "메모리 모름(옛 agent)" : `메모리 ${stats.mem}%`} />
    </span>
  );
}
