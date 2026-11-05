// 자동화 편집 폼의 고르기 칸(t-1538): profile은 모델 → 생각 수준을 골라 붙이고, 머신·저장소는 host가 아는 목록에서 칩·드롭다운으로 고른다.
// 목록은 host의 `assignable`·`profiles`·`project_settings`에서 한 번 읽는다. 목록을 못 읽으면 부르는 쪽이 글 칸으로 돌아간다.
// 칩 모양은 설정 보기 흐름 카드(AutomationDetail FlowCards)와 같다.
import { useEffect, useMemo, useState } from "react";
import { ChevronLeft, X } from "lucide-react";
import { call } from "./api";
import { ignoredFailure } from "./errorDiagnostic.ts";
import type { OwnerChoice, ProfileView } from "./gen/proto";
import {
  effortsOf, machineChoices, matching, modelsOf, moveEarlier, profileChip, profileChoices, repoNames,
  type Choices, type MachineChoice, type ProfileChoice,
} from "./automationChoices";

/** 고르기 목록. 읽는 중이면 null, 읽지 못한 칸은 빈 목록이다 */
export function useAutomationChoices(project: string): Choices | null {
  const [choices, setChoices] = useState<Choices | null>(null);
  useEffect(() => {
    let live = true;
    void Promise.allSettled([
      call({ op: "profiles", machine: null }),
      call({ op: "assignable", project, check: null }),
      call({ op: "project_settings", project }),
    ]).then(([p, o, s]) => {
      if (!live) return;
      const host: ProfileView[] = p.status === "fulfilled" && p.value.kind === "profiles" ? p.value.profiles : [];
      const owners: OwnerChoice[] | null = o.status === "fulfilled" && o.value.kind === "assignable" ? o.value.owners : null;
      const repos = s.status === "fulfilled" && s.value.kind === "project_settings" ? repoNames(s.value.repos) : [];
      setChoices({ profiles: profileChoices(host, owners), machines: machineChoices(owners), repos });
    }).catch(ignoredFailure("AutomationPickers.tsx:30", () => undefined));
    return () => { live = false; };
  }, [project]);
  return choices;
}

/**
 * profile 칩 글: 이름 · 모델 · 생각 수준. 고르기 목록에 없는 이름은 「모델 모름」이다: host가 아는 profile(기본 profile·끊긴 머신의 profile)일 수
 * 있어 잘못이라고 단정하지 않는다. 풀지 못하는 profile은 host 검사 경고(check.warnings)가 알린다
 */
export function ProfileChipText({ chip }: { chip: ProfileChoice }) {
  return <>{chip.name}<small>{chip.known ? `${chip.model} · ${chip.effort}` : "모델 모름"}</small></>;
}

/** profile 후보: 차례가 있는 칩 + 모델 → 생각 수준(→ 같은 조합이 여럿이면 이름) 고르기 */
export function ProfilePicker({ label, value, choices, onChange, testid }: { label: string; value: string[]; choices: ProfileChoice[]; onChange: (v: string[]) => void; testid?: string }) {
  const models = useMemo(() => modelsOf(choices), [choices]);
  const [model, setModel] = useState(models[0] ?? "");
  const efforts = useMemo(() => effortsOf(choices, model), [choices, model]);
  const [effort, setEffort] = useState(efforts[0] ?? "");
  const found = matching(choices, model, effort);
  const [pick, setPick] = useState("");
  const chosen = found.find((c) => c.name === pick) ?? found.find((c) => !value.includes(c.name)) ?? found[0];
  useEffect(() => { if (!models.includes(model)) setModel(models[0] ?? ""); }, [models, model]);
  useEffect(() => { if (!efforts.includes(effort)) setEffort(efforts[0] ?? ""); }, [efforts, effort]);
  const add = () => { if (chosen && !value.includes(chosen.name)) onChange([...value, chosen.name]); };
  return <fieldset className="automation-picker" data-testid={testid}>
    <legend>{label}</legend>
    <div className="automation-chips" data-testid={testid && `${testid}-chips`}>
      {value.length === 0 && <span className="automation-dim">없음</span>}
      {[...new Set(value)].map((name, i) => {
        const chip = profileChip(name, choices);
        return <span key={name} className={`automation-chip${i === 0 ? " is-first" : ""}${chip.known ? "" : " is-nometa"}`} data-testid="automation-profile-chip">
          <span className="automation-chip-n" aria-label={`${i + 1}번째`}>{i + 1}</span>
          <ProfileChipText chip={chip} />
          {i > 0 && <button type="button" className="automation-chip-btn" aria-label={`${name} 앞으로`} title="앞으로" onClick={() => onChange(moveEarlier(value, i))}><ChevronLeft aria-hidden="true" /></button>}
          <button type="button" className="automation-chip-btn" aria-label={`${name} 빼기`} title="빼기" onClick={() => onChange(value.filter((v) => v !== name))}><X aria-hidden="true" /></button>
        </span>;
      })}
    </div>
    <div className="automation-picker-row">
      <label className="automation-field"><span>모델</span>
        <select className="input" value={model} onChange={(e) => { setModel(e.target.value); setPick(""); }} data-testid="automation-profile-model">{models.map((m) => <option key={m} value={m}>{m}</option>)}</select></label>
      <label className="automation-field"><span>생각 수준</span>
        <select className="input" value={effort} onChange={(e) => { setEffort(e.target.value); setPick(""); }} data-testid="automation-profile-effort">{efforts.map((x) => <option key={x} value={x}>{x}</option>)}</select></label>
      {found.length > 1 && <label className="automation-field"><span>profile</span>
        <select className="input" value={chosen?.name ?? ""} onChange={(e) => setPick(e.target.value)} data-testid="automation-profile-name">{found.map((c) => <option key={c.name} value={c.name}>{c.name}</option>)}</select></label>}
      <button type="button" className="btn" disabled={!chosen || value.includes(chosen.name)} onClick={add} data-testid="automation-profile-add">
        {chosen && value.includes(chosen.name) ? "고름" : chosen ? `${chosen.name} 더하기` : "더하기"}</button>
    </div>
  </fieldset>;
}

/** 여러 개 고르기: 누르면 켜고 끄는 칩. 목록에 없는 기존 값은 켜진 채 표지를 달고, 누르면 뺀다 */
export function ChipSelect({ label, options, value, onChange, testid, empty }: { label: string; options: Array<{ value: string; dim?: string }>; value: string[]; onChange: (v: string[]) => void; testid?: string; empty?: string }) {
  const known = new Set(options.map((o) => o.value));
  const all = [...options, ...[...new Set(value)].filter((v) => !known.has(v)).map((v) => ({ value: v, dim: "목록에 없음", unknown: true }))];
  const on = new Set(value);
  return <fieldset className="automation-picker" data-testid={testid}>
    <legend>{label}{empty && <small className="automation-dim">{value.length === 0 ? ` · ${empty}` : ""}</small>}</legend>
    <div className="automation-chips">
      {all.map((o) => <button key={o.value} type="button" role="checkbox" aria-checked={on.has(o.value)} className={`automation-chip is-toggle${"unknown" in o ? " is-unknown" : ""}${o.dim && !("unknown" in o) ? " is-dim" : ""}`}
        onClick={() => onChange(on.has(o.value) ? value.filter((v) => v !== o.value) : [...value, o.value])}>{o.value}{o.dim && <small>{o.dim}</small>}</button>)}
    </div>
  </fieldset>;
}

/** 하나 고르기: 빈 값(host 기본)과 목록. 목록에 없는 기존 값은 「목록에 없음」으로 남긴다 */
export function SelectOne({ label, options, value, onChange, empty, testid }: { label: string; options: string[]; value: unknown; onChange: (v: string) => void; empty: string; testid?: string }) {
  const v = typeof value === "string" ? value : "";
  return <label className="automation-field"><span>{label}</span>
    <select className="input" value={v} onChange={(e) => onChange(e.target.value)} data-testid={testid}>
      <option value="">{empty}</option>
      {options.map((o) => <option key={o} value={o}>{o}</option>)}
      {v && !options.includes(v) && <option value={v}>{v}(목록에 없음)</option>}
    </select></label>;
}

export const machineOptions = (machines: MachineChoice[]) => machines.map((m) => ({ value: m.name, dim: m.connected ? undefined : "끊김" }));
