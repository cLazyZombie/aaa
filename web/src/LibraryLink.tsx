import { useRef, useState } from "react";
import type { LinkView, LibraryView } from "./gen/proto";
import { ApiError, call } from "./api";
import { copyText } from "./herdr/clipboard";
import { QrCode } from "./herdr/QrCode";

/** 링크는 host 상태를 그대로 보여 준다. 폐기·만료 후에도 앱 안의 2시간 보기 경로는 유효하다. */
export function LibraryLink({ library, onChange }: { library: LibraryView; onChange: (l: LibraryView) => void }) {
  return <ReportLink project={library.project} name={library.thread} link={library.link ?? null} onChange={(link) => onChange({ ...library, link })} />;
}

/** 전체 보고서와 thread 화면이 같은 링크 줄·조작을 쓴다. */
export function ReportLink({ project, name, link, onChange }: { project: string; name: string; link: LinkView | null; onChange: (link: LinkView) => void }) {
  const [busy, setBusy] = useState(false);
  const [qr, setQr] = useState(false);
  const [message, setMessage] = useState("");
  const anchor = useRef<HTMLAnchorElement>(null);
  const active = link !== null && !link.revoked && link.expires_at_ms > Date.now();
  const url = link ? link.url ?? new URL(link.base, window.location.origin).href : "";
  const change = async (revoke: boolean) => {
    setBusy(true);
    setMessage("");
    try {
      const r = await call(revoke && link
        ? { op: "library_link_revoke", id: link.id }
        : { op: "library_link", project, thread: name, days: null });
      if (r.kind === "library_link") {
        onChange(r);
        setQr(false);
        setMessage(revoke ? "링크를 폐기했다" : "새 링크를 만들었다");
      }
    } catch (e) { setMessage(e instanceof ApiError ? e.message : String(e)); }
    finally { setBusy(false); }
  };
  return <div className="library-link" data-testid="library-link" data-state={active ? "active" : link?.revoked ? "revoked" : link ? "expired" : "none"}>
    <div className="library-link-row">
      <strong>링크</strong>
      {active ? <a ref={anchor} href={url} target="_blank" rel="noopener noreferrer" aria-label="보고서 공유 링크">{url}</a>
        : <span className="muted">{link?.revoked ? "폐기됨" : link ? "만료됨" : "아직 없다"}</span>}
    </div>
    {link && <span className="muted">만료 {new Date(link.expires_at_ms).toLocaleString("ko-KR")}</span>}
    <div className="library-link-actions">
      {active ? <>
        <button className="ghost small" disabled={busy} onClick={async () => setMessage(await copyText(url, anchor.current) ? "링크를 복사했다" : "링크를 선택했다. 길게 눌러 복사한다")}>링크 복사</button>
        <button className="ghost small" disabled={busy} aria-expanded={qr} onClick={() => setQr(!qr)}>QR 코드</button>
        <button className="ghost small" disabled={busy} onClick={() => void change(true)}>링크 폐기</button>
      </> : <button className="ghost small" disabled={busy} onClick={() => void change(false)}>새 링크 만들기</button>}
    </div>
    {active && qr && <QrCode value={url} label="보고서 공유 링크 QR 코드" />}
    {message && <span role="status">{message}</span>}
  </div>;
}
