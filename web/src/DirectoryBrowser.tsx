// 폴더 보기(herdr-web-ui `src/components/DirectoryBrowser.tsx` aa624cb, plan §8.148). 파일 보기가 폴더 경로를 받으면
// 이 목록을 그린다: 한 번에 폴더 한 단계, 위 폴더·집, 숨김 항목 표시, 파일을 누르면 연다. 문구는 참고 i18n.ko.ts다.
// 참고와 다른 것: 그 pane이 도는 머신의 agent가 읽는다(`pane_dir`, 허용 폴더 안만). 참고의 폴더 고르기(`onPick`)·새 폴더·
// 걸러 보기·명령 팔레트가 고른 폴더(`paletteFolders`)는 새 세션 대화 상자와 Files 창 것이라 aaa에 쓰는 곳이 없어 옮기지 않았다.
import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowUp, House } from "lucide-react";

import { ApiError, call } from "./api.ts";
import type { DirListing } from "./gen/proto.ts";
import { formatBytes } from "./formatBytes.ts";
import { FileGlyph } from "./herdr/FileGlyph.tsx";
import { resolveWorkspaceRoot, workspaceRelativePath } from "./herdr/workspacePath.ts";
import "./herdr/DirectoryBrowser.css";

function childPath(parent: string, name: string): string {
  return parent.endsWith("/") ? `${parent}${name}` : `${parent}/${name}`;
}

/** `path`를 집 기준(`~/…`)으로(참고 `src/lib/paletteFolders.ts` `homeRelative`) */
export function homeRelative(path: string, home: string): string {
  if (path === home) return "~";
  return path.startsWith(`${home}/`) ? `~/${path.slice(home.length + 1)}` : path;
}

/** 읽지 못한 까닭(참고 문구, 허용 폴더 밖은 aaa 것) */
function problemText(reason: unknown): string {
  if (!(reason instanceof ApiError)) return "폴더를 불러올 수 없습니다.";
  if (reason.body.code === "unavailable") return "이 머신의 agent는 아직 폴더를 탐색할 수 없습니다. agent binary를 바꿔야 합니다.";
  if (reason.body.code === "forbidden")
    return "허용한 폴더 밖이라 열지 않습니다. 그 머신 agent.toml의 [agent] file_roots에 폴더를 넣으면 열 수 있습니다.";
  if (reason.body.code === "not_found") return "이 폴더를 열 수 없습니다.";
  return "폴더를 불러올 수 없습니다.";
}

export interface DirectoryBrowserProps {
  /** 처음 여는 폴더(실제 경로) */
  start: string;
  /** 읽는 pane: 그 pane이 도는 머신의 폴더다 */
  pane: string;
  /** 누른 파일(실제 경로)을 연다 */
  onOpenFile: (path: string) => void;
}

/** 폴더 한 단계씩 그 pane의 머신에서 읽는다. 지금 보이는 폴더만 가진다 */
export function DirectoryBrowser({ start, pane, onOpenFile }: DirectoryBrowserProps) {
  const [listing, setListing] = useState<DirListing | null>(null);
  const [hidden, setHidden] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const listRef = useRef<HTMLUListElement>(null);

  const open = useCallback(async (path: string, showHidden: boolean) => {
    const id = ++request.current;
    setLoading(true);
    try {
      const r = await call({ op: "pane_dir", pane, path, hidden: showHidden });
      if (id !== request.current || r.kind !== "pane_dir") return;
      setListing(r.listing);
      setError(null);
      listRef.current?.scrollTo({ top: 0 });
    } catch (reason: unknown) {
      if (id !== request.current) return;
      setError(problemText(reason));
    } finally {
      if (id === request.current) setLoading(false);
    }
  }, [pane]);

  useEffect(() => {
    void open(start, false);
  }, []);

  const path = listing?.path ?? "";
  const homeShown = listing ? homeRelative(listing.path, listing.home) : start || "~";
  // 연 폴더 기준으로 읽는다(「<연 폴더 이름>/<나머지>」)
  const shown = listing ? workspaceRelativePath(listing.path, resolveWorkspaceRoot(start, listing.home), homeShown) : homeShown;

  return (
    <div className="dir-browser" role="group" aria-label="폴더 선택" aria-busy={loading}>
      <div className="dir-browser-bar">
        <button type="button" className="icon-button" aria-label="상위 폴더" title="상위 폴더" disabled={!listing?.parent || loading} onClick={() => listing?.parent && void open(listing.parent, hidden)}>
          <ArrowUp aria-hidden="true" />
        </button>
        <button type="button" className="icon-button" aria-label="홈 폴더" title="홈 폴더" disabled={loading || (listing !== null && listing.path === listing.home)} onClick={() => void open("", hidden)}>
          <House aria-hidden="true" />
        </button>
        <span className="dir-browser-path" title={path}><span dir="ltr">{shown}</span></span>
      </div>
      {error !== null ? <p className="dir-browser-note dir-browser-error" role="alert">{error}</p> : (
        <ul className="dir-browser-list" ref={listRef}>
          {listing === null && <li className="dir-browser-note" role="status">불러오는 중…</li>}
          {listing?.directories.map((name) => (
            <li key={name}>
              <button type="button" className="dir-browser-item" disabled={loading} onClick={() => void open(childPath(path, name), hidden)}>
                <FileGlyph name={name} kind="directory" />
                <span>{name}</span>
              </button>
            </li>
          ))}
          {listing?.files.map((file) => (
            <li key={`file:${file.name}`}>
              <button type="button" className="dir-browser-item is-file" disabled={loading} onClick={() => onOpenFile(childPath(path, file.name))}>
                <FileGlyph name={file.name} />
                <span>{file.name}</span>
                <span className="dir-browser-size">{formatBytes(file.size)}</span>
              </button>
            </li>
          ))}
          {listing !== null && listing.directories.length === 0 && listing.files.length === 0 && <li className="dir-browser-note">여기에는 아무것도 없습니다</li>}
          {listing?.truncated && <li className="dir-browser-note">처음 {listing.directories.length}개 폴더만 보입니다. 더 들어가려면 나머지 경로를 입력하세요.</li>}
        </ul>
      )}
      <div className="dir-browser-footer">
        <label className="dir-browser-hidden">
          <input type="checkbox" checked={hidden} disabled={loading && listing === null} onChange={(event) => { setHidden(event.target.checked); if (listing) void open(listing.path, event.target.checked); }} />
          숨김 항목 표시
        </label>
      </div>
    </div>
  );
}
