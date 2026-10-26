import { useCallback, useEffect, useState } from "react";
import { call, onWorkspaceMetricsChanged } from "./api";
import type { WorkspaceDashboard, WorkspaceQuery } from "./gen/proto";

export function workspaceParams(route: string | null): URLSearchParams {
  return new URLSearchParams(route?.startsWith("workspace:") ? route.slice(10) : "");
}
export function workspaceQuery(params: URLSearchParams): WorkspaceQuery {
  return {
    selected_run_id: params.get("run"), workspace_id: params.get("ws"), machine: params.get("machine"), profile: params.get("profile"),
    days: [1, 7, 30].includes(Number(params.get("days"))) ? Number(params.get("days")) : 7,
    granularity: params.get("by") === "day" ? "day" : "run",
    utc_offset_minutes: params.get("tz") === "utc" ? 0 : 540,
    code_axis: params.get("axis"),
  };
}
export function workspaceRoute(route: string | null, change: Record<string, string | null>): string {
  const params = workspaceParams(route);
  params.delete("branch");
  for (const [key, value] of Object.entries(change)) { if (value === null || value === "") params.delete(key); else params.set(key, value); }
  return `workspace:${params}`;
}
export function useWorkspaceData(active: boolean, up: boolean, route: string | null, revision: number) {
  const query = JSON.stringify(workspaceQuery(workspaceParams(route)));
  const [state, setState] = useState<{ data: WorkspaceDashboard | null; error: string | null; loading: boolean; key: string }>({ data: null, error: null, loading: false, key: "" });
  const [refresh, setRefresh] = useState(0);
  const reload = useCallback(() => setRefresh((n) => n + 1), []);
  useEffect(() => {
    if (!active || !up) return;
    let current = true;
    setState((s) => ({ ...s, loading: true, error: null }));
    call({ op: "workspace_dashboard", query: JSON.parse(query) as WorkspaceQuery }).then((res) => {
      if (current && res.kind === "workspace_dashboard") setState({ data: res, error: null, loading: false, key: query });
    }).catch((e: unknown) => { if (current) setState((s) => ({ ...s, loading: false, error: String(e) })); });
    return () => { current = false; };
  }, [active, up, query, revision, refresh]);
  useEffect(() => active ? onWorkspaceMetricsChanged(reload) : undefined, [active, reload]);
  useEffect(() => {
    if (!active) return;
    window.addEventListener("focus", reload);
    return () => window.removeEventListener("focus", reload);
  }, [active, reload]);
  return { ...state, data: state.key === query ? state.data : null, lastSuccess: state.data?.queried_at_ms ?? null, reload };
}
export type WorkspaceData = ReturnType<typeof useWorkspaceData>;
