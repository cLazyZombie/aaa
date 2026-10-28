// 아이디어 목록(t-1260, 기획 t-1255 결정 6): 활동 막대 「아이디어」의 사이드바 칸. 다듬는 중을 먼저, 완료는 아래에 둔다. 고르면 본문이
// 노트·대화다. 「+」는 제목만 받아 새 아이디어를 만든다(전용 thread는 처음 말할 때 뜬다).
import { Plus } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { ApiError, call, onIdeasChanged } from "./api";
import type { IdeaSummary } from "./gen/proto";
import { ideaGroups, ideaLine } from "./ideasModel";

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
  return (
    <section className="ideas-panel" aria-label="아이디어 목록" data-testid="ideas-panel">
      {canControl && (
        <div className="ideas-tools">
          {adding ? (
            <form className="ideas-new" onSubmit={(e) => { e.preventDefault(); void create(); }}>
              <input className="input" autoFocus value={title} placeholder="제목" aria-label="새 아이디어 제목" disabled={busy} data-testid="idea-new-title"
                onChange={(e) => setTitle(e.target.value)} onKeyDown={(e) => { if (e.key === "Escape") { setAdding(false); setTitle(""); } }} />
              <button type="submit" className="btn btn-primary" disabled={busy || !title.trim()} data-testid="idea-new-save">만들기</button>
            </form>
          ) : (
            <button type="button" className="ghost small" data-testid="idea-new" onClick={() => setAdding(true)}>
              <Plus className="i" aria-hidden="true" />새 아이디어
            </button>
          )}
        </div>
      )}
      {problem && <p className="banner err" role="alert">{problem}</p>}
      {ideas.error && <p className="banner err" role="alert">{ideas.error}</p>}
      {ideas.list === null && !ideas.error && <p className="tree-state" role="status">불러오는 중</p>}
      {ideas.list?.length === 0 && <p className="tree-state" data-testid="ideas-empty">아이디어 없음</p>}
      {groups.map((g) => (
        <div key={g.name} className="ideas-group" role="group" aria-label={g.name}>
          <div className="ideas-group-head">{g.name}</div>
          {g.items.map((i) => (
            <button key={i.id} type="button" className="idea-row" aria-current={selected === i.short_id ? "true" : undefined}
              data-testid="idea-row" data-id={i.short_id} onClick={() => onOpen(i.short_id)}>
              <span className="idea-row-title">{i.title}</span>
              <span className="idea-row-meta">{ideaLine(i)}</span>
            </button>
          ))}
        </div>
      ))}
    </section>
  );
}
