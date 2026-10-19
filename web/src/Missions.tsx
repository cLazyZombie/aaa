// 기존 배지·확인 창과 같은 작은 임무 보기. Controller 승인만 API로 보낸다.
import { useState } from "react";
import { FileText, Pause, Play, RotateCw, ShieldCheck, Terminal } from "lucide-react";
import { RowMenu, type RowMenuItem } from "./RowMenu";
import type { MissionView, DeviceRole, FrontRequest } from "./gen/proto";
import { call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";

export function Missions({ missions, role, onOpen, onReload }: { missions: MissionView[]; role: DeviceRole; onOpen: (id: string | null) => void; onReload: () => void }) {
  const [menu, setMenu] = useState<{ key: string; anchor: HTMLElement } | null>(null);
  const current = menu && missions.find((m) => `${m.project}/${m.name}` === menu.key);
  const [show, setShow] = useState<MissionView | null>(null);
  const [approve, setApprove] = useState<MissionView | null>(null);
  const [error, setError] = useState("");
  const invoke = async (op: "mission_start" | "mission_pause" | "mission_resume" | "mission_reload", m: MissionView) => {
    try { setError(""); await call({ op, project: m.project, name: m.name } as FrontRequest); onReload(); setShow(null); }
    catch (e) { setError(String(e)); }
  };
  const actions = (m: MissionView): RowMenuItem[] => {
    const items: RowMenuItem[] = [];
    if (m.thread?.pane) items.push({ id: "open", label: "열기", icon: Terminal, run: () => onOpen(m.thread?.pane ?? null) });
    items.push({ id: "view", label: `임무 보기${m.stale ? " · 낡음" : ""}`, icon: FileText, run: () => { setError(""); setShow(m); } });
    if (role !== "reader") {
      const start = !m.thread || m.thread.status === "resolved";
      const resume = m.state === "멈춤";
      items.push({ id: "run", label: start ? "시작" : resume ? "이어 가기" : "멈춤", icon: start || resume ? Play : Pause,
        run: () => void invoke(start ? "mission_start" : resume ? "mission_resume" : "mission_pause", m) });
      items.push({ id: "reload", label: "다시 읽기", icon: RotateCw, run: () => void invoke("mission_reload", m) });
      if (!m.approved && !m.error) items.push({ id: "approve", label: "승인 검토", icon: ShieldCheck, run: () => setApprove(m) });
    }
    return items;
  };
  return <>
    {missions.map((m) => {
      const key = `${m.project}/${m.name}`;
      return <span key={key} className="mission-menu" data-testid={`mission-${m.project}-${m.name}`}>
        <button type="button" className="eval-badge" data-state={m.state} aria-haspopup="menu" aria-expanded={menu?.key === key}
          title={`${m.title} · ${m.state}`} onClick={(e) => setMenu(menu?.key === key ? null : { key, anchor: e.currentTarget })}>
          특수 ● {m.title} · {m.state} · {m.wakes_today}/{m.max_wakes}
        </button>
      </span>;
    })}
    {menu && current && <RowMenu anchor={menu.anchor} title={current.title} items={actions(current)} onClose={() => setMenu(null)}
      header={<div className="mission-summary"><strong>{current.title}</strong><p>{current.project} · 일한 분 {current.work_minutes} · post 처리 {current.handled_posts}/{current.posts} · 입력 {current.pending}</p></div>} />}
    {error && <span role="alert">{error}</span>}
    {show && <ConfirmDialog title={`${show.title} · ${show.state}`} body={`적용 판 ${show.applied_sha ?? "없음"} · 파일 판 ${show.file_sha ?? "없음"}\n승인 ${show.approved ? "됨" : "필요"}\n다음 reset ${show.next_reset_ms ? new Date(show.next_reset_ms).toLocaleString() : "없음"}\n사용량 대기 ${show.usage_wait_until_ms ? new Date(show.usage_wait_until_ms).toLocaleString() : "없음"}\n\n차이\n${show.diff || "없음"}\n\n적용 사본\n${show.applied_text}\n\n후보 파일\n${show.file_text}${show.error ? `\n오류: ${show.error}` : ""}`} confirmLabel="확인" onConfirm={async () => { setShow(null); }} onClose={() => setShow(null)} />}
    {approve && <ConfirmDialog title={`${approve.title} · 임무 승인`} body={`첫 시작·repo·예산 올림을 승인한다.\n승인 hash ${approve.approval_sha256}\n\n${approve.file_text}`} confirmLabel="승인" onConfirm={async () => { await call({ op: "mission_approve", project: approve.project, name: approve.name, approval_sha256: approve.approval_sha256 }); setApprove(null); onReload(); }} onClose={() => setApprove(null)} />}
  </>;
}
