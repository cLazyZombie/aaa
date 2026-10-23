import { ignoredFailure } from "./errorDiagnostic.ts";
// 자동화 목록 자료(정본은 host): 사이드바 카드 목록(AutomationPanel)과 본문 설정 화면(AutomationView)이 같은 사본을 본다(t-0628).
// 패널이나 설정 화면이 보일 때만 읽는다: 열 때·포커스를 되찾을 때·보이는 동안 30초마다, 그리고 thread 사건(보고·Idle·입력)마다.
import { useCallback, useEffect, useRef, useState } from "react";
import { call, me } from "./api";
import type { AutomationTrashEntry, RoutineView, ThreadView } from "./gen/proto";

export type AutomationRow = { project: string; rules: RoutineView[]; trash: AutomationTrashEntry[] };
export type Automations = {
  rows: AutomationRow[];
  error: string | null;
  loading: boolean;
  /** undefined: 아직 묻는 중(그동안 변환 단추도 안내 글도 그리지 않는다), null: 옛 host이거나 묻지 못함 */
  frontApi: string | null | undefined;
  load: () => Promise<void>;
  setError: (e: string | null) => void;
};

export function useAutomations(active: boolean, threads: ThreadView[]): Automations {
  const [rows, setRows] = useState<AutomationRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [frontApi, setFrontApi] = useState<string | null | undefined>(undefined);
  const generation = useRef(0);
  const load = useCallback(async () => {
    const ticket = ++generation.current;
    try {
      const projects = await call({ op: "projects" });
      if (projects.kind !== "projects") throw new Error("프로젝트 목록 응답이 아니다");
      const next = await Promise.all(projects.projects.map(async (p) => {
        const result = await call({ op: "routines", project: p.name });
        if (result.kind !== "routines") throw new Error("자동화 목록 응답이 아니다");
        return { project: p.name, rules: result.routines, trash: result.trash ?? [] };
      }));
      if (ticket === generation.current) { setRows(next); setError(null); window.dispatchEvent(new Event("aaa:automations-changed")); }
    } catch (e) {
      if (ticket === generation.current) setError(String(e));
    } finally {
      if (ticket === generation.current) setLoading(false);
    }
  }, []);
  useEffect(() => {
    if (!active) return;
    void load();
    // 옛 host는 판을 알리지 않는다: 미리보기(dry_run)를 버리고 변환하므로 그때는 변환 단추를 두지 않는다
    if (frontApi === undefined) me().then((m) => setFrontApi(m?.front_api ?? null)).catch(ignoredFailure("automationData.ts:46", () => setFrontApi(null)));
    const refresh = () => { if (!document.hidden) void load(); };
    const timer = window.setInterval(refresh, 30_000);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => { ++generation.current; clearInterval(timer); window.removeEventListener("focus", refresh); document.removeEventListener("visibilitychange", refresh); };
  }, [active, load]); // eslint-disable-line react-hooks/exhaustive-deps
  // 보고·Idle·입력 처리 사건으로 남은 시간·열린 thread를 다시 읽는다
  const threadStamp = threads.map((t) => `${t.id}:${t.updated_at_ms}:${t.report_hash}:${t.group}:${t.agent?.activity}`).join("|");
  const seen = useRef(threadStamp);
  useEffect(() => {
    if (!active || seen.current === threadStamp) return;
    seen.current = threadStamp;
    void load();
  }, [active, load, threadStamp]);
  return { rows, error, loading, frontApi, load, setError };
}

/** 이 규칙이 띄워 아직 열린 thread(목록 줄에 호박색 막대) */
export const openThreads = (threads: ThreadView[], project: string, name: string): ThreadView[] =>
  threads.filter((t) => t.project === project && t.automation === name && t.group !== "resolved" && t.status !== "failed");
