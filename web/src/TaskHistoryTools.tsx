// 활성 할 일과 완료 목록이 같은 검색줄·필터 칩을 쓴다.
import { useState, type ReactNode } from "react";
import { ChevronDown, CircleDashed, Layers, Search } from "lucide-react";
import { RowMenu } from "./RowMenu";

export type HistoryChoice = { key: string; label: string; n: number };
export function TaskChoiceChip({ label, value, choices, onChange, testId }: {
  label: string; value: string | null; choices: readonly { key: string; label: string; n?: number }[];
  onChange: (value: string | null) => void; testId: string;
}) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const picked = choices.find((c) => c.key === value)?.label ?? value;
  return <>
    <button type="button" className={`task-chip tasks-repo${value !== null ? " is-on" : ""}`} aria-haspopup="menu" aria-expanded={anchor !== null} data-testid={testId} onClick={(e) => setAnchor(e.currentTarget)}>
      {label}: {value === null ? "전체" : picked || "정하지 않음"}<ChevronDown aria-hidden="true" />
    </button>
    {anchor && <RowMenu anchor={anchor} title={`${label}로 거르기`} onClose={() => setAnchor(null)} items={[
      { id: "all", label: "전체", icon: Layers, checked: value === null, run: () => onChange(null) },
      ...choices.map((c) => ({ id: c.key || "none", label: c.label, icon: CircleDashed, hint: c.n === undefined ? undefined : String(c.n), checked: value === c.key, run: () => onChange(c.key) })),
    ]} />}
  </>;
}

/** 정렬은 거르기 칩 밖에 둔다: 넓으면 줄 끝, 휴대폰은 맨 앞이라 칩을 굴리지 않아도 보인다. */
export function TaskFilterChips({ repo, sort, label, children }: { repo: ReactNode; sort?: ReactNode; label: string; children: ReactNode }) {
  return <div className="tasks-chips">{sort}{repo}<div className="tasks-filters" role="group" aria-label={label}>{children}</div></div>;
}

/** 설정에서 빠진 옛 저장소도 완료 보관에 있으면 고를 수 있다. */
export function historyChoices(configured: readonly string[], historical: readonly HistoryChoice[]): HistoryChoice[] {
  const keys = [...configured];
  for (const c of historical) if (!keys.includes(c.key)) keys.push(c.key);
  return keys.map((key) => historical.find((c) => c.key === key) ?? { key, label: key || "정하지 않음", n: 0 });
}

export function TaskSearchInput({ value, onChange, label, testId }: { value: string; onChange: (value: string) => void; label: string; testId: string }) {
  return <label className="tasks-search"><Search aria-hidden="true" /><input className="input" type="search" value={value} placeholder="ID·제목·메모·담당 검색" aria-label={label} data-testid={testId} onChange={(e) => onChange(e.target.value)} /></label>;
}
