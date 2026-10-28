// 머신 정보 패널의 자동 배정 비중과 독점 예약(t-1265). 값은 host 상태 사건의 MachineView가 정본이다.
import { useState } from "react";
import type { DeviceRole, MachineView } from "./gen/proto";
import { call } from "./api";

const PERIODS: [string, number | null][] = [["풀 때까지", null], ["1시간", 3_600_000], ["4시간", 14_400_000], ["하루", 86_400_000]];

function weightText(weight: number | null | undefined, cpus: number | undefined) {
  if (weight === undefined || weight === null) return `${cpus ?? 1} (CPU 수)`;
  return weight === 0 ? "0 (자동 배정 안 함)" : String(weight);
}

export function MachineAssignment({ machine, role }: { machine: MachineView; role: DeviceRole }) {
  const [weight, setWeight] = useState("");
  const [holder, setHolder] = useState("");
  const [note, setNote] = useState("");
  const [period, setPeriod] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const locked = busy || role === "reader";
  const cpus = machine.health?.cpus;
  const r = machine.reservation;
  const run = async (body: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try { await body(); } catch (e) { setError(e instanceof Error ? e.message : String(e)); } finally { setBusy(false); }
  };
  const saveWeight = (value: number | null) => run(async () => {
    await call({ op: "machine_weight_set", machine: machine.name, weight: value });
    setWeight("");
  });
  const reserve = () => run(async () => {
    const ms = PERIODS[period][1];
    await call({ op: "machine_reserve", machine: machine.name, holder: holder.trim(), note: note.trim() || undefined, until_ms: ms === null ? undefined : Date.now() + ms });
    setHolder("");
    setNote("");
  });
  const parsed = Number(weight);
  const weightOk = weight.trim() !== "" && Number.isInteger(parsed) && parsed >= 0 && parsed <= 1024;
  return <section aria-label="자동 배정" className="machine-assignment" aria-busy={busy}>
    <h3>자동 배정</h3>
    <div className="machine-assignment-row" data-testid="machine-weight">
      <span>비중</span>
      <strong>{weightText(machine.weight, cpus)}</strong>
      <input type="number" min={0} max={1024} inputMode="numeric" value={weight} placeholder={String(cpus ?? 1)} disabled={locked} onChange={e => setWeight(e.target.value)} aria-label={`${machine.name} 비중`} />
      <button type="button" disabled={locked || !weightOk} onClick={() => void saveWeight(parsed)}>저장</button>
      {machine.weight !== undefined && machine.weight !== null && <button type="button" className="ghost" disabled={locked} onClick={() => void saveWeight(null)}>CPU 수로</button>}
    </div>
    {r ? <div className="machine-assignment-row machine-reservation" data-testid="machine-reservation">
      <span>독점 예약</span>
      <strong>{r.holder}</strong>
      <small>{[r.note, r.until_ms ? `${new Date(r.until_ms).toLocaleString()}까지` : null, `${r.by} · ${r.project}`].filter(Boolean).join(" · ")}</small>
      <button type="button" disabled={locked} onClick={() => void run(() => call({ op: "machine_release", machine: machine.name }))}>풀기</button>
    </div> : <form className="machine-assignment-row machine-reserve-form" data-testid="machine-reserve-form" onSubmit={e => { e.preventDefault(); void reserve(); }}>
      <span>독점 예약</span>
      <input value={holder} placeholder="t-0001 · d-0001" disabled={locked} onChange={e => setHolder(e.target.value)} aria-label="예약 주인" />
      <input value={note} placeholder="까닭" disabled={locked} onChange={e => setNote(e.target.value)} aria-label="예약 까닭" />
      <select value={period} disabled={locked} onChange={e => setPeriod(Number(e.target.value))} aria-label="예약 기간">
        {PERIODS.map(([label], i) => <option key={label} value={i}>{label}</option>)}
      </select>
      <button type="submit" disabled={locked || !holder.trim()}>걸기</button>
    </form>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
