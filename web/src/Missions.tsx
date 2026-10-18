// 기존 배지·확인 창과 같은 작은 임무 보기. Controller 승인만 API로 보낸다.
import { useState } from "react";
import type { MissionView, DeviceRole, FrontRequest } from "./gen/proto";
import { call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";

export function Missions({ missions, role, onOpen, onReload }: { missions: MissionView[]; role: DeviceRole; onOpen: (id: string | null) => void; onReload: () => void }) {
  const [show, setShow] = useState<MissionView | null>(null);
  const [approve, setApprove] = useState<MissionView | null>(null);
  const [error, setError] = useState("");
  const invoke = async (op: "mission_start" | "mission_pause" | "mission_resume" | "mission_reload", m: MissionView) => {
    try { setError(""); await call({ op, project: m.project, name: m.name } as FrontRequest); onReload(); setShow(null); }
    catch (e) { setError(String(e)); }
  };
  return <>
    {missions.map((m) => <details key={`${m.project}/${m.name}`} className="mission-menu" data-testid={`mission-${m.project}-${m.name}`}>
      <summary className="eval-badge" data-state={m.state}>특수 ● {m.title} · {m.state} · {m.wakes_today}/{m.max_wakes}</summary>
      <div className="mission-actions">
        <p>{m.project} · 일한 분 {m.work_minutes} · post 처리 {m.handled_posts}/{m.posts} · 입력 {m.pending}</p>
        {m.thread?.pane && <button onClick={() => onOpen(m.thread?.pane ?? null)}>열기</button>}
        <button onClick={() => { setError(""); setShow(m); }}>임무 보기{m.stale ? " · 낡음" : ""}</button>
        {role !== "reader" && <>
          <button onClick={() => void invoke((!m.thread || m.thread.status === "resolved") ? "mission_start" : m.state === "멈춤" ? "mission_resume" : "mission_pause", m)}>{(!m.thread || m.thread.status === "resolved") ? "시작" : m.state === "멈춤" ? "이어 가기" : "멈춤"}</button>
          <button onClick={() => void invoke("mission_reload", m)}>다시 읽기</button>
          {!m.approved && !m.error && <button onClick={() => setApprove(m)}>승인 검토</button>}
        </>}
      </div>
    </details>)}
    {error && <span role="alert">{error}</span>}
    {show && <ConfirmDialog title={`${show.title} · ${show.state}`} body={`적용 판 ${show.applied_sha ?? "없음"} · 파일 판 ${show.file_sha ?? "없음"}\n승인 ${show.approved ? "됨" : "필요"}\n다음 reset ${show.next_reset_ms ? new Date(show.next_reset_ms).toLocaleString() : "없음"}\n사용량 대기 ${show.usage_wait_until_ms ? new Date(show.usage_wait_until_ms).toLocaleString() : "없음"}\n\n차이\n${show.diff || "없음"}\n\n적용 사본\n${show.applied_text}\n\n후보 파일\n${show.file_text}${show.error ? `\n오류: ${show.error}` : ""}`} confirmLabel="확인" onConfirm={async () => { setShow(null); }} onClose={() => setShow(null)} />}
    {approve && <ConfirmDialog title={`${approve.title} · 임무 승인`} body={`첫 시작·repo·예산 올림을 승인한다.\n승인 hash ${approve.approval_sha256}\n\n${approve.file_text}`} confirmLabel="승인" onConfirm={async () => { await call({ op: "mission_approve", project: approve.project, name: approve.name, approval_sha256: approve.approval_sha256 }); setApprove(null); onReload(); }} onClose={() => setApprove(null)} />}
  </>;
}
