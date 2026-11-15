// 기획 문서 보기(t-1894, 확정 계획 plan §2): 사이드바 「기획」 트리에서 고른 문서 하나. 1단계는 읽기 전용이고 origin 기본 branch의 글이다.
// 그림·영상은 같은 commit의 저장소 파일이고, 문서 안 다른 문서 링크(상대 경로 .md)는 앱 안에서 연다.
import "./DesignView.css";
import { RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { ApiError, call } from "./api";
import { designFileUrl, designTarget, isMarkdown, resolveDocPath, splitTarget } from "./designModel";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import type { DesignDoc, DesignRepoView } from "./gen/proto";
import { OpenFileContext } from "./herdr/filePaths.ts";
import { useLightbox } from "./Lightbox";
import { ago } from "./machineDetail";
import { ReportMarkdown } from "./ReportsView";
import { useMinute } from "./Sidebar";

export function DesignView({ shown, target, repos, onOpen, onReload }: {
  shown: boolean;
  /** `<저장소>/<경로>`, 없으면 고른 문서 없음 */
  target: string | null;
  /** 트리(문서 안 링크가 가리키는 문서가 있는지 본다). 트리를 다시 읽어 판이 바뀌면 문서도 그 판으로 다시 읽는다 */
  repos: DesignRepoView[] | null;
  onOpen: (target: string) => void;
  /** 트리 다시 읽기(origin을 받는다) */
  onReload: () => void;
}) {
  const at = target ? splitTarget(target) : null;
  const key = at ? `${at.repo}/${at.path}` : null;
  const treeCommit = repos?.find((r) => r.repo === at?.repo)?.tree?.commit ?? null;
  // 받은 문서와 그것을 부른 주소(host가 경로를 정리해 돌려줘도 그 주소의 문서다)
  const [doc, setDoc] = useState<{ key: string; doc: DesignDoc } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [rev, setRev] = useState(0);
  const lightbox = useLightbox();
  const now = useMinute();
  useEffect(() => {
    if (!at || !key) return;
    let live = true;
    setBusy(true);
    setError(null);
    call({ op: "design_read", repo: at.repo, path: at.path }).then(
      (r) => { if (live && r.kind === "design_doc") setDoc({ key, doc: r.doc }); },
      (e: unknown) => { if (live) { setDoc(null); setError(e instanceof ApiError ? e.message : String(e)); } },
    ).finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [key, rev, treeCommit]); // eslint-disable-line react-hooks/exhaustive-deps
  const current = doc && doc.key === key ? doc.doc : null;
  const docs = repos?.find((r) => r.repo === at?.repo)?.tree?.docs;
  const image = useCallback((src: string) => {
    if (!current) return null;
    const path = resolveDocPath(current.path, src);
    return path && !isMarkdown(path) ? designFileUrl(current.project, current.repo, path, current.commit) : null;
  }, [current]);
  /** 문서 안 경로: 문서 폴더 기준, 없으면 저장소 기준으로 찾아 트리에 있는 문서면 연다 */
  const openPath = useCallback((raw: string) => {
    if (!current) return;
    const tries = [resolveDocPath(current.path, raw), resolveDocPath("x", raw)];
    const hit = tries.find((p) => p && isMarkdown(p) && (docs ?? []).some((d) => d.path === p));
    if (hit) onOpen(designTarget(current.repo, hit));
  }, [current, docs, onOpen]);
  return (
    <div className={`design-screen${shown ? "" : " is-hidden"}`} inert={!shown} data-testid="design-view" data-shown={shown ? "true" : "false"}>
      <div className="thread-view design-view">
        <header className="term-head design-head">
          <DrawerToggle />
          <HeadNote />
          <div className="term-title design-title">
            {at ? <span className="design-crumb" data-testid="design-crumb">{at.repo} / {at.path.split("/").join(" / ")}</span> : <strong>기획</strong>}
          </div>
          {at && (
            <button type="button" className="btn icon" aria-label="새로 읽기" title="새로 읽기" disabled={busy} data-testid="design-doc-reload" onClick={() => { onReload(); setRev((n) => n + 1); }}>
              <RefreshCw className={`i${busy ? " is-spinning" : ""}`} aria-hidden="true" />
            </button>
          )}
          <HeadMore />
        </header>
        {!at && <p className="design-empty" data-testid="design-none">고른 문서 없음</p>}
        {error && <div className="banner err" role="alert">{error}</div>}
        {current && (
          <div className="design-body" data-testid="design-body">
            <div className="design-meta" data-testid="design-meta">
              <span title={current.commit}>판 <code>{current.commit.slice(0, 7)}</code></span>
              <span>{ago(current.committed_ms, now)}</span>
            </div>
            <div className="design-text">
              <OpenFileContext.Provider value={openPath}>
                <ReportMarkdown base={null} image={image} text={current.text} onImages={lightbox.open} captions />
              </OpenFileContext.Provider>
            </div>
          </div>
        )}
      </div>
      {lightbox.node}
    </div>
  );
}
