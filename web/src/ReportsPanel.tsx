import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Trash2 } from "lucide-react";
import type { DeviceRole, FrontResponse, LibraryReport } from "./gen/proto";
import { call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { ReportLink } from "./LibraryLink";
import { formatBytes } from "./formatBytes";

function storageSize(bytes: number): string {
  return bytes >= 1024 ** 3 ? `${Math.round(bytes / 1024 ** 3 * 10) / 10} GB` : formatBytes(bytes);
}

/**
 * 보고서를 새 창으로 연다. 사용자 동작에서 먼저 창을 연다(휴대폰 popup 차단). 만료·폐기돼도 pairing 권한으로 열린다.
 * 창이 막히면 던진다.
 */
export async function openReportWindow(r: { project: string; name: string }): Promise<void> {
  const windowRef = window.open("about:blank", "_blank");
  if (windowRef) windowRef.opener = null;
  try {
    const v = await call({ op: "library_open", project: r.project, name: r.name });
    if (v.kind === "library" && windowRef) windowRef.location.href = v.base;
    else {
      windowRef?.close();
      throw new Error("보고서를 열 창이 차단됐다. 브라우저의 팝업을 허용한다");
    }
  } catch (e) {
    windowRef?.close();
    throw e;
  }
}

/** 보고서를 낸 thread 번호(사용자 요청 10-20 「보고서에 항상 스레드 번호」): 이름 왼쪽. 근원을 모르는 보고서는 그리지 않는다 */
export function ReportThread({ id }: { id: string | undefined }) {
  return id ? <span className="report-tag report-thread" data-testid="report-thread">{id}</span> : null;
}

type Tab = "mine" | "work";

const keyOf = (r: { project: string; name: string }) => `${r.project}/${r.name}`;
const when = (ms: number) => new Date(ms).toLocaleString("ko-KR");

/**
 * 목록·크기·링크 상태는 host 정본이다. 파일 경로나 등록 원본 경로를 웹에 싣지 않는다. 「열기」는 본문에서(`onView`), 「새 창」은 따로.
 * 두 탭(사용자 결정 10-19, plan §8.256): 「내 보고서」는 coordinator가 `library publish --to-user`로 보낸 웹 보고서이고 안 본 것에
 * 점과 수를 단다(연 순간 host가 봄으로 적는다). 「작업 보고」는 thread 사본·그 밖의 publish·import이고 골라서 한꺼번에 지운다.
 * 지우기는 확인 창 한 번 뒤 host 사본·공유 링크·목록에서 영구히 지우며, 진행 중(끝나지 않은) thread의 사본은 고를 수 없다.
 */
export function ReportsPanel({ role, rev, onView }: { role: DeviceRole; rev: number; onView: (r: LibraryReport) => void }) {
  const [reports, setReports] = useState<LibraryReport[] | null>(null);
  const [usage, setUsage] = useState<Extract<FrontResponse, { kind: "library_usage" }> | null>(null);
  const [problem, setProblem] = useState("");
  const [tab, setTab] = useState<Tab>("mine");
  const [opening, setOpening] = useState<string | null>(null);
  const [picked, setPicked] = useState<ReadonlySet<string>>(new Set());
  const [doomed, setDoomed] = useState<LibraryReport[] | null>(null);
  const [linkOpen, setLinkOpen] = useState<string | null>(null);
  const load = useCallback(async () => {
    setProblem("");
    try {
      const [r, u] = await Promise.all([
        call({ op: "library_reports", project: null }), call({ op: "library_usage" }),
      ]);
      if (u.kind === "library_usage") setUsage(u);
      if (r.kind === "library_reports") {
        setReports(r.reports);
        // 사라졌거나 진행 중이 된 것은 고른 데서 뺀다
        const can = new Set(r.reports.filter((x) => !x.busy).map(keyOf));
        setPicked((p) => (([...p].every((k) => can.has(k))) ? p : new Set([...p].filter((k) => can.has(k)))));
      }
    } catch (e) { setProblem(String(e)); }
  }, []);
  useEffect(() => { void load(); }, [load, rev]);
  const open = async (r: LibraryReport) => {
    setOpening(keyOf(r));
    try { await openReportWindow(r); }
    catch (e) { setProblem(e instanceof Error ? e.message : String(e)); }
    finally { setOpening(null); }
  };
  const titleLink = (r: LibraryReport, className?: string) => <a className={className} href={`#report=${encodeURIComponent(keyOf(r))}`} onClick={(e) => {
    // 보조 키·가운데 단추는 브라우저의 새 탭 동작을 지킨다. 보통 클릭은 목록의 굴린 자리도 보존한다.
    if (e.button === 0 && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      onView(r);
    }
  }}>{r.title ?? keyOf(r)}</a>;
  const remove = async (rows: LibraryReport[]) => {
    const r = await call({ op: "library_delete", reports: rows.map((x) => ({ project: x.project, name: x.name })) });
    if (r.kind !== "library_pruned") throw new Error("host가 지우기에 다른 답을 했다");
    const gone = new Set(r.removed);
    setReports((all) => all?.filter((x) => !gone.has(keyOf(x))) ?? null);
    setPicked((p) => new Set([...p].filter((k) => !gone.has(k))));
    setDoomed(null);
    void load();
  };
  const canEdit = role !== "reader";
  // host는 프로젝트 하나라(plan §8.291) 프로젝트로 거르지 않는다
  const shown = reports ?? [];
  const mine = shown.filter((r) => r.to_user);
  const work = shown.filter((r) => !r.to_user);
  const unseen = (reports ?? []).filter((r) => r.unseen).length;
  const pickedRows = work.filter((r) => picked.has(keyOf(r)));
  const setLink = (r: LibraryReport, link: LibraryReport["link"]) =>
    setReports((rows) => rows?.map((row) => row.project === r.project && row.name === r.name ? { ...row, link } : row) ?? null);
  const tabButton = (t: Tab, label: string, badge: ReactNode) => (
    <button type="button" role="tab" id={`reports-tab-${t}`} aria-selected={tab === t} aria-controls="reports-tabpanel"
      className={`reports-tab${tab === t ? " is-on" : ""}`} data-testid={`reports-tab-${t}`} onClick={() => setTab(t)}>
      {label}{badge}
    </button>
  );
  return <div className="reports-panel" data-testid="reports-panel" data-status={reports === null ? "loading" : "ready"} data-tab={tab}>
    <div className="reports-tabs" role="tablist" aria-label="보고서 종류">
      {tabButton("mine", "내 보고서", unseen > 0 && <span className="reports-count is-new" data-testid="reports-unseen" aria-label={`안 본 보고서 ${unseen}`}>{unseen > 99 ? "99+" : unseen}</span>)}
      {tabButton("work", "작업 보고", reports !== null && <span className="reports-count">{(reports ?? []).filter((r) => !r.to_user).length}</span>)}
    </div>
    <div className="reports-toolbar">
      <button className="ghost small" onClick={() => void load()}>새로고침</button>
    </div>
    {usage && <p className="muted" data-testid="reports-usage">합계 {storageSize(usage.size)} / {usage.max_size ? storageSize(usage.max_size) : "상한 꺼짐"}, 기한 {usage.max_days ? `${usage.max_days}일` : "꺼짐"}</p>}
    {usage && usage.max_size > 0 && usage.protected_size > usage.max_size && <p role="alert">보호 사본만으로 상한을 넘었다. 자동 삭제하지 않는다</p>}
    {problem && <p role="alert">{problem}</p>}
    <div id="reports-tabpanel" role="tabpanel" aria-labelledby={`reports-tab-${tab}`}>
    {reports === null ? <p>보고서를 읽고 있다…</p> : tab === "mine" ? (
      mine.length === 0 ? <p className="muted">coordinator가 보낸 보고서가 없다. thread 사본과 그 밖에 올린 보고서는 「작업 보고」에 있다</p> : mine.map((r) => {
        const key = keyOf(r);
        return <article className={`report-card report-mine${r.unseen ? " is-unseen" : ""}`} key={key} data-testid={`report-${r.project}-${r.name}`} data-unseen={r.unseen ? "true" : "false"}>
          <div className="report-heading">
            <strong>{r.unseen && <span className="report-new-dot" role="img" aria-label="안 봄" />}<ReportThread id={r.from_thread} />{titleLink(r)}</strong>
            <span>{formatBytes(r.size)}{r.live ? " · 실시간" : ""}</span>
          </div>
          <p className="muted">{r.title ? `${key} · ` : ""}보냄 {when(r.updated_at_ms)}</p>
          {r.problem && <p role="alert">{r.problem}</p>}
          <div className="report-actions">
            <button className="small" data-testid="report-open" onClick={() => onView(r)}>열기</button>
            <button className="ghost small" disabled={opening !== null} onClick={() => void open(r)}>새 창</button>
            {canEdit && <button className="ghost small report-delete" data-testid="report-delete" onClick={() => setDoomed([r])}><Trash2 className="i" aria-hidden="true" />지우기</button>}
          </div>
          {canEdit && <ReportLink project={r.project} name={r.name} link={r.link} onChange={(link) => setLink(r, link)} />}
        </article>;
      })
    ) : (
      <>
        {canEdit && <div className="reports-work-tools">
          <span data-testid="reports-picked"><strong>{pickedRows.length}개 선택</strong>{pickedRows.length > 0 && <span className="muted"> · {formatBytes(pickedRows.reduce((n, r) => n + r.size, 0))}</span>}</span>
          <span className="spacer" />
          <button className="ghost small" data-testid="reports-pick-done" disabled={!work.some((r) => !r.busy)}
            onClick={() => setPicked(new Set(work.filter((r) => !r.busy).map(keyOf)))}>끝난 것 모두 고르기</button>
          <button className="small danger" data-testid="reports-delete" disabled={pickedRows.length === 0} onClick={() => setDoomed(pickedRows)}>
            <Trash2 className="i" aria-hidden="true" />지우기
          </button>
        </div>}
        {work.length === 0 ? <p className="muted">작업 보고가 없다</p> : work.map((r) => {
          const key = keyOf(r);
          const on = picked.has(key);
          return <div className={`report-row${on ? " is-picked" : ""}`} key={key} data-testid={`report-${r.project}-${r.name}`} data-busy={r.busy ? "true" : "false"}>
            <div className="report-row-main">
              {canEdit && <input type="checkbox" className="report-pick" aria-label={`${key} 고르기`} checked={on} disabled={r.busy}
                title={r.busy ? "진행 중 thread의 보고는 지울 수 없다" : undefined}
                onChange={(e) => setPicked((p) => {
                  const next = new Set(p);
                  if (e.target.checked) next.add(key); else next.delete(key);
                  return next;
                })} />}
              <ReportThread id={r.from_thread} />
              {titleLink(r, "report-row-name")}
              {r.busy && <span className="report-tag is-busy">진행 중</span>}
              <span className="muted report-row-meta">{formatBytes(r.size)}{r.live ? " · 실시간" : ""} · {when(r.updated_at_ms)}</span>
              <span className="report-row-actions">
                <button className="ghost small" data-testid="report-open" onClick={() => onView(r)}>열기</button>
                <button className="ghost small" disabled={opening !== null} onClick={() => void open(r)}>새 창</button>
                {canEdit && <button className="ghost small" aria-expanded={linkOpen === key} onClick={() => setLinkOpen((k) => k === key ? null : key)}>공유 링크</button>}
              </span>
            </div>
            {r.problem && <p role="alert">{r.problem}</p>}
            {canEdit && linkOpen === key && <ReportLink project={r.project} name={r.name} link={r.link} onChange={(link) => setLink(r, link)} />}
          </div>;
        })}
      </>
    )}
    </div>
    {doomed && <ConfirmDialog
      title={doomed.length === 1 ? "보고서를 지울까?" : `보고서 ${doomed.length}개를 지울까?`}
      body={`${doomed.length === 1 ? keyOf(doomed[0]) : `${doomed.slice(0, 3).map(keyOf).join(", ")}${doomed.length > 3 ? ` 외 ${doomed.length - 3}개` : ""}`}를 host 사본·공유 링크·목록에서 영구히 지운다.${doomed.some((r) => r.name.startsWith("t-")) ? " thread 사본은 회수한 보고 글도 함께 지운다." : ""} 되돌릴 수 없다.`}
      confirmLabel="영구히 지우기"
      onConfirm={() => remove(doomed)}
      onClose={() => setDoomed(null)}
    />}
  </div>;
}
