// 아이디어 목록(t-1260, 기획 t-1255 결정 6): 활동 막대 「아이디어」의 사이드바 칸. 다듬는 중을 먼저, 완료는 아래에 둔다. 고르면 본문이
// 노트·대화다. 「+」는 제목만 받아 새 아이디어를 만든다(전용 thread는 처음 말할 때 뜬다).
// 줄은 사이드바 thread 줄(Sidebar ThreadRow 두 줄 모드)과 같은 칸이다: 앞 표지 칸·「번호 · 제목」·둘째 줄 왼쪽 상태 말, 오른쪽 참조.
import { Lightbulb, Plus } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { ApiError, call, onIdeasChanged } from "./api";
import type { IdeaSummary } from "./gen/proto";
import { IDEA_STATE, ideaGroups, ideaRefs } from "./ideasModel";
import { ago } from "./machineDetail";
import { onRowKey, ThreadName, useMinute } from "./Sidebar";

export type Ideas = { list: IdeaSummary[] | null; error: string | null; reload: () => void };

/** host 아이디어 목록(웹 events `ideas`로 다시 읽는다) */
export function useIdeas(up: boolean): Ideas {
  const [list, setList] = useState<IdeaSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rev, setRev] = useState(0);
  const reload = useCallback(() => setRev((n) => n + 1), []);
  useEffect(() => onIdeasChanged(reload), [reload]);
  useEffect(() => {
    if (!up) return;
    let live = true;
    call({ op: "idea_list" }).then(
      (r) => { if (live && r.kind === "ideas") { setList(r.ideas); setError(null); } },
      (e: unknown) => { if (live) setError(e instanceof ApiError ? e.message : String(e)); },
    );
    return () => { live = false; };
  }, [up, rev]);
  return { list, error, reload };
}

export function IdeasPanel({ ideas, selected, canControl, onOpen }: {
  ideas: Ideas;
  /** 본문에 연 아이디어(`n-0001`) */
  selected: string | null;
  canControl: boolean;
  onOpen: (id: string) => void;
}) {
  const [adding, setAdding] = useState(false);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const create = async () => {
    const t = title.trim();
    if (!t || busy) return;
    setBusy(true);
    setProblem(null);
    try {
      const r = await call({ op: "idea_create", title: t, text: "" });
      if (r.kind === "idea") {
        setTitle("");
        setAdding(false);
        ideas.reload();
        onOpen(r.note.idea.short_id);
      }
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const groups = ideaGroups(ideas.list ?? []);
  const now = useMinute();
  return (
    <section className="ideas-panel" aria-label="아이디어 목록" data-testid="ideas-panel">
      {canControl && (adding ? (
        <form className="ideas-new" onSubmit={(e) => { e.preventDefault(); void create(); }}>
          <input className="input" autoFocus value={title} placeholder="제목" aria-label="새 아이디어 제목" disabled={busy} data-testid="idea-new-title"
            onChange={(e) => setTitle(e.target.value)} onKeyDown={(e) => { if (e.key === "Escape") { setAdding(false); setTitle(""); } }} />
          <button type="submit" className="btn btn-primary" disabled={busy || !title.trim()} data-testid="idea-new-save">만들기</button>
        </form>
      ) : (
        <button type="button" className="sidebar-row-action ideas-add" data-testid="idea-new" onClick={() => setAdding(true)}>
          <span className="sidebar-mark" aria-hidden="true"><Plus /></span>새 아이디어
        </button>
      ))}
      {problem && <p className="banner err" role="alert">{problem}</p>}
      {ideas.error && <p className="banner err" role="alert">{ideas.error}</p>}
      {ideas.list === null && !ideas.error && <p className="tree-state" role="status">불러오는 중</p>}
      {ideas.list?.length === 0 && <p className="tree-state" data-testid="ideas-empty">아이디어 없음</p>}
      {groups.map((g) => (
        <section key={g.name} className="ideas-group" aria-label={g.name}>
          <h3 className="ideas-group-head">{g.name}<span className="ideas-group-count">{g.items.length}</span></h3>
          <ul className="workspace-list">
            {g.items.map((i) => <IdeaRow key={i.id} idea={i} selected={selected === i.short_id} now={now} onOpen={onOpen} />)}
          </ul>
        </section>
      ))}
    </section>
  );
}

/** thread 줄(두 줄 모드)과 같은 칸: 전구 표지 · 「n-0001 · 제목」 / 상태 말 · 고친 때 … 넘긴 할 일이나 전용 thread */
function IdeaRow({ idea: i, selected, now, onOpen }: { idea: IdeaSummary; selected: boolean; now: number; onOpen: (id: string) => void }) {
  const open = () => onOpen(i.short_id);
  const refs = ideaRefs(i);
  const when = ago(i.updated_ms, now);
  return (
    <li className={`workspace workspace-group pane-item idea-item is-${i.status}${selected ? " is-selected" : ""}`}>
      <div className="workspace-header">
        <div className="pane-select workspace-select" role="button" tabIndex={0} aria-current={selected ? "true" : undefined}
          title={[`${i.short_id} · ${i.title}`, `${IDEA_STATE[i.status]} · ${when}`, refs].filter(Boolean).join("\n")}
          data-testid="idea-row" data-id={i.short_id} onClick={open} onKeyDown={(e) => onRowKey(e, open)}>
          <span className="sidebar-mark" aria-hidden="true"><Lightbulb /></span>
          <span className="workspace-copy is-two-line">
            <span className="workspace-line"><ThreadName id={i.short_id} title={i.title} /></span>
            <span className="workspace-place thread-place">
              <span className="thread-state idea-state">{IDEA_STATE[i.status]}<span className="idea-when">{when}</span></span>
              {refs && <span className="thread-machine">{refs}</span>}
            </span>
          </span>
        </div>
      </div>
    </li>
  );
}
