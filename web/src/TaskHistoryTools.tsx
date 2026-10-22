// 활성 할 일과 기록이 같은 검색칸·저장소 메뉴·상태 칩을 쓴다.
import { useState } from "react";
import { ChevronDown, CircleDashed, Folder, FolderGit2, Layers, Search } from "lucide-react";
import { RowMenu, type RowMenuItem } from "./RowMenu";
import { useMediaQuery } from "./herdr/useMediaQuery";
import { REPO_DIR } from "./tasksModel";

export type HistoryChoice = { key: string; label: string; n: number };
export function TaskHistoryTools({ query, onQuery, repo, onRepo, choices, filters, filter, onFilter, prefix = "task-log" }: {
  query: string; onQuery: (query: string) => void; repo: string | null; onRepo: (repo: string | null) => void;
  choices: readonly HistoryChoice[]; filters: readonly { id: string; label: string }[]; filter: string; onFilter: (filter: string) => void; prefix?: string;
}) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const coarse = useMediaQuery("(pointer: coarse)");
  const label = repo === null ? "전체" : choices.find((c) => c.key === repo)?.label ?? (repo || "정하지 않음");
  const items: RowMenuItem[] = [
    { id: "repo-all", label: "전체", icon: Layers, checked: repo === null, run: () => onRepo(null) },
    ...choices.map((c, i) => ({ id: `repo-${c.key || "none"}`, label: c.label, hint: String(c.n), icon: c.key === "" ? CircleDashed : c.key === REPO_DIR ? Folder : FolderGit2, divider: i === 0, checked: repo === c.key, run: () => onRepo(c.key) })),
  ];
  return <>
    <TaskSearchInput value={query} onChange={onQuery} label="기록 검색" testId={`${prefix}-search`} />
    <div className="tasks-chips">
      <button type="button" className={`task-chip tasks-repo${repo !== null ? " is-on" : ""}`} aria-haspopup="menu" aria-expanded={anchor !== null} data-testid={`${prefix}-repo`} onClick={(e) => setAnchor(e.currentTarget)}>
        {coarse ? (repo === null ? "저장소" : label) : `저장소: ${label}`}<ChevronDown aria-hidden="true" />
      </button>
      <div className="tasks-filters" role="group" aria-label="기록 거르기">
        {filters.map((f) => <button key={f.id} type="button" className={`task-chip tasks-filter${filter === f.id ? " is-on" : ""}`} aria-pressed={filter === f.id} data-testid={`${prefix}-filter-${f.id}`} onClick={() => onFilter(f.id)}>{f.label}</button>)}
      </div>
    </div>
    {anchor && <RowMenu anchor={anchor} title="저장소로 거르기" items={items} onClose={() => setAnchor(null)} />}
  </>;
}

/** 설정에서 빠진 옛 저장소도 기록에 있으면 고를 수 있다. */
export function historyChoices(configured: readonly string[], historical: readonly HistoryChoice[]): HistoryChoice[] {
  const keys = [...configured];
  for (const c of historical) if (!keys.includes(c.key)) keys.push(c.key);
  return keys.map((key) => historical.find((c) => c.key === key) ?? { key, label: key || "정하지 않음", n: 0 });
}

export function TaskSearchInput({ value, onChange, label, testId }: { value: string; onChange: (value: string) => void; label: string; testId: string }) {
  return <label className="tasks-search"><Search aria-hidden="true" /><input className="input" type="search" value={value} placeholder="ID·제목·메모·담당 검색" aria-label={label} data-testid={testId} onChange={(e) => onChange(e.target.value)} /></label>;
}
