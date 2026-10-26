import { useState } from "react";
import { ArrowDownWideNarrow, ArrowUpDown } from "lucide-react";
import { RowMenu } from "./RowMenu";

/** 완료 탭과 같은 칩·메뉴/휴대폰 시트를 쓴다. */
export function TaskSortChip<K extends string>({ value, choices, onChange, testId, subtitle }: {
  value: K; choices: readonly { key: K; label: string; hint: string }[];
  onChange: (value: K) => void; testId: string; subtitle: string;
}) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const selected = choices.find(s => s.key === value)!;
  return <>
    <button type="button" className="task-chip tasks-sort" aria-haspopup="menu" aria-expanded={anchor !== null}
      aria-label={`정렬: ${selected.label}, ${selected.hint}`} title={`${selected.label}순 · ${selected.hint}`} data-testid={testId} onClick={e => setAnchor(e.currentTarget)}>
      <ArrowDownWideNarrow aria-hidden="true" />{selected.label}
    </button>
    {anchor && <RowMenu anchor={anchor} title="정렬" subtitle={subtitle} onClose={() => setAnchor(null)} items={choices.map(s => ({
      id: s.key, label: s.label, hint: s.hint, icon: ArrowUpDown, checked: s.key === value, run: () => onChange(s.key),
    }))} />}
  </>;
}
