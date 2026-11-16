// 옛 아이디어 목록(t-1260): 기획 트리 「옛 아이디어」 묶음과 아이디어 본문이 읽는다(t-1894 4단계에서 「아이디어」 패널을 기획에 합쳤다).
import { useCallback, useEffect, useState } from "react";
import { ApiError, call, onIdeasChanged } from "./api";
import type { IdeaSummary } from "./gen/proto";

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
