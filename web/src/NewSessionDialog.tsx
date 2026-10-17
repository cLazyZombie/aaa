// 머신 머리 +의 새 pane 창(herdr-web-ui aa624cb src/components/NewSessionDialog.tsx를 옮김, 모양 herdr/NewSessionDialog.css 그대로,
// 문구는 참고 i18n.ko.ts). agent(셸 또는 이 머신 agent.toml profile)·폴더·이름을 고르고 「시작」하면 그 pane을 연다. 마지막 고른 agent와
// 머신마다 최근 폴더(참고 recent-directories.ts)를 기억한다. 다른 곳: 참고의 workspace 자리는 aaa pane이고, 탭 모드·폴더 찾아보기
// (머신 폴더 목록 API가 없다)·주소 초안(?new=)·agent가 못 떴을 때 「열기」는 뺐다. plan §8.110.
import { useEffect, useRef, useState, type MouseEvent } from "react";
import { X } from "lucide-react";

import "./herdr/NewSessionDialog.css";

import type { MachineView } from "./gen/proto";
import { ApiError, call } from "./api";
import { AgentPicker, rememberAgent, rememberedAgent, type AgentKind } from "./AgentPicker";
import { RecentFolderChips } from "./RecentFolderChips";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap.ts";
import { useSheetSwipe } from "./herdr/sheets.ts";
import { passingFolder, recentDirectories } from "./herdr/recent-directories.ts";

/** pane 이름 한도(aaa-proto `MAX_PANE_LABEL_CHARS`) */
const MAX_LABEL_CHARS = 80;
/** recent folders offered: one row a phone swipes sideways; a wider screen fits what one row holds and lists the rest under "+N" */
const RECENT_FOLDERS_SHOWN = 8;

function directoryBasename(value: string): string {
  const trimmed = value.replace(/\\/g, "/").replace(/\/+$/, "");
  return trimmed.split("/").pop() || value;
}

export interface NewSessionDialogProps {
  machine: MachineView;
  onClose: () => void;
  onCreated: (paneId: string) => void;
}

export function NewSessionDialog({ machine, onClose, onCreated }: NewSessionDialogProps) {
  const machineId = machine.id;
  const [agents, setAgents] = useState<AgentKind[]>([]);
  const [agentKind, setAgentKind] = useState(rememberedAgent);
  const [cwd, setCwd] = useState("");
  const [name, setName] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recentFolders, setRecentFolders] = useState(() => recentDirectories.read(machineId));
  const shownFolders = recentFolders.filter((path) => !passingFolder(path)).slice(0, RECENT_FOLDERS_SHOWN);
  const firstFieldRef = useRef<HTMLButtonElement>(null);
  const surface = useFocusTrap<HTMLFormElement>(true, { initialFocus: firstFieldRef });
  useSheetSwipe(surface, pending ? null : onClose);

  useEffect(() => {
    const remembered = rememberedAgent();
    let cancelled = false;
    call({ op: "profiles", machine: machineId })
      .then((r) => {
        if (cancelled || r.kind !== "profiles") return;
        const next = r.profiles.map((p) => ({ kind: p.name, label: p.name, harness: p.harness }));
        setAgents(next);
        // 이 머신에 없는 profile을 기억하고 있으면 셸로
        if (remembered && !next.some((agent) => agent.kind === remembered)) setAgentKind("");
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      cancelled = true;
    };
  }, [machineId]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || nativeModalOver(surface.current)) return;
      event.preventDefault();
      if (!pending) onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose, pending]); // eslint-disable-line react-hooks/exhaustive-deps

  const selectedAgent = agents.find((agent) => agent.kind === agentKind);
  const pendingLabel = selectedAgent ? `${selectedAgent.label} 시작 중… 최대 60초` : "셸 시작 중…";

  const start = async (): Promise<void> => {
    if (pending) return;
    setPending(true);
    setError(null);
    try {
      rememberAgent(agentKind);
      const directory = cwd.trim();
      // 참고의 이름 칸 placeholder(폴더 이름)가 비운 이름의 값이다
      const label = name.trim() || (directory ? directoryBasename(directory) : null);
      const r = await call({
        op: "pane_run",
        machine: machineId,
        name: label,
        argv: [],
        cwd: directory || null,
        cols: null,
        rows: null,
        ...(agentKind ? { profile: agentKind } : {}),
      });
      if (r.kind !== "pane") throw new Error(`뜻밖의 답 ${r.kind}`);
      setRecentFolders(recentDirectories.remember(machineId, [directory], true));
      setPending(false);
      onCreated(r.id);
    } catch (reason: unknown) {
      setPending(false);
      setError(reason instanceof ApiError ? reason.message : reason instanceof Error ? reason.message : String(reason));
    }
  };

  const closeFromScrim = (event: MouseEvent<HTMLDivElement>): void => {
    if (!pending && event.target === event.currentTarget) onClose();
  };

  return (
    <div className="modal-scrim new-session-scrim" onMouseDown={closeFromScrim}>
      <form
        ref={surface}
        className="modal new-session-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-session-title"
        tabIndex={-1}
        onSubmit={(event) => {
          event.preventDefault();
          void start();
        }}
      >
        <header className="modal-header">
          <h2 className="modal-title" id="new-session-title">{`새 pane · ${machine.name}`}</h2>
          <button type="button" className="icon-button" aria-label="창 닫기" disabled={pending} onClick={onClose}>
            <X aria-hidden="true" />
          </button>
        </header>
        <div className="modal-body">
          <div className="field">
            <span className="field-label" id="new-session-agent">
              에이전트
            </span>
            <AgentPicker ref={firstFieldRef} agents={agents} value={agentKind} disabled={pending} labelledBy="new-session-agent" onChange={setAgentKind} />
          </div>
          <div className="field">
            <label className="field-label" htmlFor="new-session-cwd">
              폴더
            </label>
            <div className="new-session-cwd">
              <input id="new-session-cwd" className="input" value={cwd} disabled={pending} autoComplete="off" onChange={(event) => setCwd(event.target.value)} />
            </div>
            <span className="field-hint">절대 경로 또는 ~/…</span>
          </div>
          {shownFolders.length > 0 && (
            <div className="field new-session-recents">
              <span className="field-label" id="new-session-recents">
                최근 폴더
              </span>
              <RecentFolderChips folders={shownFolders} disabled={pending} labelledBy="new-session-recents" basename={directoryBasename} onPick={setCwd} />
            </div>
          )}
          <label className="field">
            <span className="field-label">이름</span>
            <input
              className="input"
              value={name}
              maxLength={MAX_LABEL_CHARS}
              disabled={pending}
              autoComplete="off"
              placeholder={cwd.trim() ? directoryBasename(cwd.trim()) : ""}
              onChange={(event) => setName(event.target.value)}
            />
            <span className="field-hint">pane 이름 (선택)</span>
          </label>
          {pending && (
            <p className="new-session-note" role="status">
              {pendingLabel}
            </p>
          )}
          {error && (
            <p className="field-hint new-session-error" role="alert">
              {error}
            </p>
          )}
        </div>
        <footer className="modal-footer">
          <button type="button" className="btn btn-ghost" disabled={pending} onClick={onClose}>
            취소
          </button>
          <button type="submit" className="btn btn-primary" disabled={pending}>
            {pending ? "시작 중…" : "시작"}
          </button>
        </footer>
      </form>
    </div>
  );
}
