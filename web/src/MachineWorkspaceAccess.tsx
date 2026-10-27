// 머신 정보 패널 내용. 열기·닫기·화면 이동은 MachineInfoPanel이 맡는다.
import { useEffect, useState } from "react";
import type { DeviceRole, WorkspaceAccessView } from "./gen/proto";
import { call, onTasksChanged } from "./api";

export function MachineWorkspaceAccess({ machineId, role }: { machineId: string; role: DeviceRole }) {
  const [access, setAccess] = useState<WorkspaceAccessView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    let live = true;
    let sequence = 0;
    setAccess(null);
    setError(null);
    const load = () => {
      const request = ++sequence;
      void call({ op: "workspace_access", machine: machineId }).then(r => {
        if (live && request === sequence && r.kind === "workspace_access") { setAccess(r); setError(null); }
      }).catch(e => { if (live && request === sequence) setError(String(e.message ?? e)); });
    };
    load();
    const off = onTasksChanged(load);
    window.addEventListener("focus", load);
    return () => { live = false; off(); window.removeEventListener("focus", load); };
  }, [machineId]);
  const toggle = async (workspace: string, enabled: boolean) => {
    if (!access || saving) return;
    setSaving(true);
    setError(null);
    try {
      const r = await call({ op: "workspace_access_set", project: access.project, machine: machineId, workspace, enabled, revision: access.revision });
      if (r.kind === "workspace_access") setAccess(r);
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setSaving(false); }
  };
  return (
<section aria-label="workspace 자동 배정" className="machine-workspaces" aria-busy={saving}>
        <h3>자동 배정할 workspace</h3>
        {role === "reader" && <p>보기 권한 · 변경은 Owner만 가능합니다.</p>}
        {!access && !error && <p role="status">허용 설정 읽는 중</p>}
        {access?.workspaces.map(row => <label key={row.workspace} className="machine-workspace-row">
          <input type="checkbox" checked={row.enabled} disabled={saving || role === "reader"} onChange={e => void toggle(row.workspace, e.target.checked)} aria-label={`${row.workspace} 자동 배정 허용`} />
          <span><strong>{row.workspace}</strong><small>{row.workspace === "작업 폴더" ? "저장소 없는 일 · 이 머신의 작업 root" : row.paths.length ? row.paths.join(" · ") : "이 머신에 등록된 저장소 경로 없음"}</small></span>
          <span>{row.enabled ? "켜짐" : "꺼짐"}</span>
        </label>)}
        {error && <p role="alert">{error}</p>}
      </section>
  );
}
