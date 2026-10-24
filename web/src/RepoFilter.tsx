// 할 일·보고서의 저장소 칩과 이 기기 기억을 함께 쓴다.
import { useEffect, useState } from "react";
import { ChevronDown, CircleDashed, Folder, FolderGit2, Layers } from "lucide-react";
import { RowMenu } from "./RowMenu";
import { useMediaQuery } from "./herdr/useMediaQuery";
import { logIgnoredFailure } from "./errorDiagnostic";
import { REPO_DIR } from "./tasksModel";

export type RepoChoice = { key: string; label: string; n: number };
export function useRepoFilter(scope: "tasks" | "reports", project: string) {
  const key = `aaa:${scope}:repo:${project}`;
  const [state, setState] = useState<{ key: string; value: string | null }>({ key: "", value: null });
  useEffect(() => {
    let value = null;
    try { value = project ? localStorage.getItem(key) : null; }
    catch (error) { logIgnoredFailure(error, "저장소 거르기 읽기"); }
    setState({ key, value });
  }, [key, project]);
  const setValue = (value: string | null) => {
    setState({ key, value });
    try {
      if (!project) return;
      if (value === null) localStorage.removeItem(key);
      else localStorage.setItem(key, value);
    } catch (error) { logIgnoredFailure(error, "저장소 거르기 저장"); }
  };
  return [state.key === key ? state.value : null, setValue] as const;
}

export function RepoFilter({ value, choices, total, onChange, testId }: {
  value: string | null; choices: readonly RepoChoice[]; total: number;
  onChange: (value: string | null) => void; testId: string;
}) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const coarse = useMediaQuery("(pointer: coarse)");
  const label = value === null ? "전체" : choices.find((c) => c.key === value)?.label ?? value;
  return <>
    <button type="button" className={`task-chip tasks-repo${value !== null ? " is-on" : ""}`}
      aria-haspopup="menu" aria-expanded={anchor !== null} data-testid={testId} data-repo={value ?? ""}
      onClick={(e) => setAnchor(e.currentTarget)}>
      {coarse ? (value === null ? "저장소" : label) : `저장소: ${label}`}<ChevronDown aria-hidden="true" />
    </button>
    {anchor && <RowMenu anchor={anchor} title="저장소로 거르기" onClose={() => setAnchor(null)} items={[
      { id: "repo-all", label: "전체", hint: String(total), icon: Layers, checked: value === null, run: () => onChange(null) },
      ...choices.map((c, i) => ({ id: `repo-${c.key || "none"}`, label: c.label, hint: String(c.n), icon: c.key === "" ? CircleDashed : c.key === REPO_DIR ? Folder : FolderGit2, divider: i === 0, checked: value === c.key, run: () => onChange(c.key) })),
    ]} />}
  </>;
}
