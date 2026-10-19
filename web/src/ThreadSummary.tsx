// thread 작업 요약(plan §8.259): 「작업」 트리 thread 줄의 hover 카드(마우스 기기만)와 thread 화면 머리의 「요약」 단추.
// 카드는 순간 훑어보는 것이라 6줄 안팎이다: 목표, 지금(3줄), 최근 바뀐 것 한 줄, 상태(10-19 사용자 「길면 안 된다」).
// 정한 것·지난 변경·끝남·요약 뒤 지시는 단추 창에서만, 접어 둔다. 카드 모양·지연·자리는 t3code SidebarThreadTooltip
// (150ms, 닫힘 0, 400ms 안 다음 줄은 바로, 오른쪽·위 끝·4px), 단추 창은 데스크톱 팝오버·휴대폰 아래 시트(RowMenu와 같은 틀).
// 휴대폰에는 hover 대안(길게 누름)을 두지 않는다(10-19 사용자): 단추만 있다.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { CircleCheck, ClipboardList, History, ListTodo, Target, TriangleAlert, Undo2, X } from "lucide-react";
import type { SummaryEntry, ThreadSummaryView, ThreadView } from "./gen/proto";
import { ApiError, call } from "./api";
import { useMediaQuery } from "./herdr/useMediaQuery.ts";
import { SHEET_QUERY, useSheetSwipe } from "./herdr/sheets.ts";
import { useSheetHistory } from "./herdr/sheetHistory.ts";
import { GROUP_WORD } from "./sidebarModel";
import { cardLines, decidedLines, hoverPlace, popoverPlace, summaryStale, whoWord } from "./summaryCard";
import "./ThreadSummary.css";

/** 요약이 바뀌었는지 가르는 열쇠: 요약 시각·요약 뒤 지시 수·보고(Decisions) */
const versionOf = (t: ThreadView) => `${t.summary?.at_ms ?? "-"}:${t.summary?.pending ?? "-"}:${t.report_hash ?? "-"}`;
const cache = new Map<string, { version: string; view: ThreadSummaryView }>();

/** 요약을 읽는다. 같은 판이면 기억한 것을 쓴다(hover마다 묻지 않게) */
function useSummary(t: ThreadView | undefined, enabled: boolean): { view: ThreadSummaryView | null; error: string | null } {
  const key = t ? `${t.project}/${t.id}` : "";
  const version = t ? versionOf(t) : "";
  const hit = cache.get(key);
  const [state, setState] = useState<{ view: ThreadSummaryView | null; error: string | null }>({
    view: hit?.version === version ? hit.view : null,
    error: null,
  });
  useEffect(() => {
    if (!enabled || !t) return;
    const have = cache.get(key);
    if (have?.version === version) {
      setState({ view: have.view, error: null });
      return;
    }
    let live = true;
    call({ op: "thread_summary", project: t.project, thread: t.id }).then(
      (r) => {
        if (!live || r.kind !== "thread_summary") return;
        const { kind: _, ...view } = r;
        cache.set(key, { version, view });
        setState({ view, error: null });
      },
      (e: unknown) => live && setState({ view: null, error: e instanceof ApiError ? e.message : String(e) }),
    );
    return () => {
      live = false;
    };
  }, [enabled, key, version, t?.project, t?.id]);
  return state;
}

const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });

function Chip({ e }: { e: SummaryEntry }) {
  if (!e.who && !e.at) return null;
  return <span className={`summary-chip is-${e.who === "사용자" ? "user" : e.who === "agent" ? "agent" : "coord"}`}>{[whoWord(e.who), e.at].filter(Boolean).join(" ")}</span>;
}

function Status({ t, v }: { t: ThreadView; v: ThreadSummaryView }) {
  return (
    <p className="summary-status">
      <span className={`thread-state is-${t.group}`}>{GROUP_WORD[t.group] ?? t.group}</span>
      {v.pending > 0 ? (
        <span className="summary-warn">
          <TriangleAlert aria-hidden="true" />
          {v.text === null ? `요약 없음 · 지시 ${v.pending}개` : `요약 뒤 지시 ${v.pending}개`}
        </span>
      ) : (
        <span className="summary-dim">{v.at_ms !== null ? `요약 ${clock(v.at_ms)}` : "요약 없음"}</span>
      )}
    </p>
  );
}

/** hover 카드 본문: 6줄 안팎, 줄마다 한 줄(넘치면 …) */
export function SummaryCard({ t, v }: { t: ThreadView; v: ThreadSummaryView }) {
  const c = cardLines(v, t.title);
  return (
    <div className="summary-card" data-testid="summary-card">
      <p className={`summary-goal${c.auto ? " is-auto" : ""}`}>
        <Target aria-hidden="true" />
        <span>{c.goal}</span>
      </p>
      {c.now.length > 0 && (
        <ul className="summary-now">
          {c.now.map((x) => (
            <li key={x}>{x}</li>
          ))}
          {c.moreNow > 0 && <li className="summary-more">+{c.moreNow}</li>}
        </ul>
      )}
      {c.latest && (
        <p className="summary-latest">
          <History aria-hidden="true" />
          <span>{c.latest.text}</span>
          <Chip e={c.latest} />
        </p>
      )}
      <Status t={t} v={v} />
    </div>
  );
}

function Fold({ icon, title, count, tone, children }: { icon: ReactNode; title: string; count: number; tone?: string; children: ReactNode }) {
  if (count === 0) return null;
  return (
    <details className={`summary-fold${tone ? ` is-${tone}` : ""}`}>
      <summary>
        {icon}
        <span>{title}</span>
        <span className="summary-count">{count}</span>
      </summary>
      {children}
    </details>
  );
}

function Entries({ list }: { list: SummaryEntry[] }) {
  return (
    <ul>
      {list.map((e) => (
        <li key={`${e.at}${e.text}`} className={e.dropped ? "is-dropped" : undefined}>
          <span className="summary-text">{e.text}</span>
          <Chip e={e} />
        </li>
      ))}
    </ul>
  );
}

/** 단추 창 본문: 목표·지금은 펼치고, 나머지는 접어 둔다 */
export function SummaryDetail({ t, v }: { t: ThreadView; v: ThreadSummaryView }) {
  const c = cardLines(v, t.title);
  const decided = decidedLines(v);
  return (
    <div className="summary-detail">
      <p className={`summary-goal${c.auto ? " is-auto" : ""}`}>
        <Target aria-hidden="true" />
        <span>{c.goal}</span>
      </p>
      {v.origin && <p className="summary-origin">처음: {v.origin}</p>}
      {c.auto && <p className="summary-dim summary-note">아직 요약이 없어 할 일 첫 줄을 보인다</p>}
      {v.now.length > 0 && (
        <section className="summary-block">
          <h4>
            <ListTodo aria-hidden="true" />
            지금 할 것
          </h4>
          <ul className="summary-now">
            {v.now.map((x) => (
              <li key={x}>{x}</li>
            ))}
          </ul>
        </section>
      )}
      <Status t={t} v={v} />
      <div className="summary-folds">
        <Fold icon={<CircleCheck aria-hidden="true" />} title="정한 것" count={decided.length}>
          <Entries list={decided} />
        </Fold>
        <Fold icon={<History aria-hidden="true" />} title="바뀐 것" count={v.changed.length}>
          <Entries list={v.changed} />
        </Fold>
        <Fold icon={<Undo2 aria-hidden="true" />} title="끝난 것" count={v.done.length}>
          <ul>
            {v.done.map((x) => (
              <li key={x}>{x}</li>
            ))}
          </ul>
        </Fold>
        <Fold icon={<TriangleAlert aria-hidden="true" />} title={v.text === null ? "받은 지시" : "요약 뒤 지시"} count={v.pending} tone="warn">
          <ul>
            {v.pending_lines.map((l, i) => (
              <li key={i}>
                <span className="summary-text">{l.text}</span>
                {l.at_ms !== null && <span className="summary-chip is-coord">{clock(l.at_ms)}</span>}
              </li>
            ))}
            {v.pending > v.pending_lines.length && <li className="summary-more">+{v.pending - v.pending_lines.length}</li>}
          </ul>
        </Fold>
      </div>
      <p className="summary-foot">
        {v.at_ms !== null ? `${v.by ?? ""} ${clock(v.at_ms)}에 씀`.trim() : "요약 없음"} · 지시 기록 {v.follow_ups}개
      </p>
    </div>
  );
}

const HOVER_QUERY = "(hover: hover) and (pointer: fine)";
const OPEN_MS = 150;
const WARM_MS = 400;
let lastClosed = 0;

/** thread 줄 hover 카드(마우스 기기만). 줄의 title tooltip을 대신한다 */
export function useSummaryHover(t: ThreadView | undefined, suppressed: boolean) {
  const mouse = useMediaQuery(HOVER_QUERY);
  const enabled = mouse && t !== undefined;
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  // 400ms 안에 다음 줄로 옮기면 지연·등장 효과 없이 바로(t3code Tooltip `timeout`·`data-instant`)
  const [instant, setInstant] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const close = () => {
    window.clearTimeout(timer.current);
    setAnchor((a) => {
      if (a) lastClosed = Date.now();
      return null;
    });
  };
  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => {
    if (!anchor) return;
    // 굴리면 줄이 움직이므로 닫는다. 줄에 focus가 있으면 둔다(t3code tooltip.tsx `dismissUnfocused`)
    const onScroll = () => {
      if (!anchor.contains(document.activeElement)) close();
    };
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", close);
    };
  }, [anchor]);
  const open = anchor !== null && !suppressed;
  const { view } = useSummary(t, open);
  const handlers = enabled
    ? {
        onMouseEnter: (e: React.MouseEvent<HTMLElement>) => {
          const el = e.currentTarget;
          window.clearTimeout(timer.current);
          const warm = Date.now() - lastClosed < WARM_MS;
          timer.current = window.setTimeout(() => {
            setInstant(warm);
            setAnchor(el);
          }, warm ? 0 : OPEN_MS);
        },
        onMouseLeave: close,
        onMouseDown: close,
      }
    : {};
  const card = open && t && view ? <HoverCard anchor={anchor} t={t} v={view} instant={instant} /> : null;
  return { handlers, card, enabled };
}

function HoverCard({ anchor, t, v, instant }: { anchor: HTMLElement; t: ThreadView; v: ThreadSummaryView; instant: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{ top: number; left: number } | null>(null);
  useLayoutEffect(() => {
    // 크기는 offset으로 잰다: 등장 효과(scale .98)가 걸린 rect로 재면 자리가 어긋난다
    const card = { width: ref.current!.offsetWidth, height: ref.current!.offsetHeight };
    setPlace(hoverPlace(anchor.getBoundingClientRect(), card, { width: innerWidth, height: innerHeight }));
  }, [anchor, v]);
  return createPortal(
    <div ref={ref} className={`summary-hover${instant ? " is-instant" : ""}`} role="tooltip" style={place ?? { visibility: "hidden", top: 0, left: 0 }} data-testid="summary-hover">
      <SummaryCard t={t} v={v} />
    </div>,
    document.body,
  );
}

/** thread 화면 머리의 「요약」 단추와 그 창(데스크톱 팝오버, 휴대폰 아래 시트) */
export function SummaryButton({ t }: { t: ThreadView }) {
  const sheet = useMediaQuery(SHEET_QUERY);
  const btn = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const stale = summaryStale(t.summary);
  const label = stale ? "작업 요약 · 반영 안 된 지시 있음" : "작업 요약";
  return (
    <>
      <button
        ref={btn}
        type="button"
        className={`ghost small summary-btn${stale ? " is-stale" : ""}`}
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label={label}
        title={label}
        data-testid="summary-open"
        onClick={() => setOpen(!open)}
      >
        <ClipboardList aria-hidden="true" />
        <span className="summary-btn-label">요약</span>
        {stale && <span className="summary-dot" aria-hidden="true" />}
      </button>
      {open && (sheet ? <SummarySheet t={t} onClose={() => setOpen(false)} /> : <SummaryPopover t={t} anchor={btn.current!} onClose={() => setOpen(false)} />)}
    </>
  );
}

function Body({ t }: { t: ThreadView }) {
  const { view, error } = useSummary(t, true);
  if (error) return <p className="summary-error" role="alert">{error}</p>;
  if (!view) return <p className="summary-dim">읽는 중…</p>;
  return <SummaryDetail t={t} v={view} />;
}

function SummaryPopover({ t, anchor, onClose }: { t: ThreadView; anchor: HTMLElement; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{ top: number; left: number } | null>(null);
  // 자리는 단추 아래·폭으로만 정해진다(높이는 CSS max-height가 맡는다): 글이 늦게 와도 다시 잴 것이 없다
  useLayoutEffect(() => {
    const card = { width: ref.current!.offsetWidth, height: ref.current!.offsetHeight };
    setPlace(popoverPlace(anchor.getBoundingClientRect(), card, { width: innerWidth, height: innerHeight }));
  }, [anchor]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!ref.current?.contains(target) && !anchor.contains(target)) onClose();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("resize", onClose);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("resize", onClose);
    };
  }, [anchor, onClose]);
  return createPortal(
    <div ref={ref} className="summary-pop" role="dialog" aria-label={`${t.id} 작업 요약`} style={place ?? { visibility: "hidden", top: 0, left: 0 }} data-testid="summary-pop">
      <div className="summary-pop-head">
        <span className="summary-pop-title">{t.title}</span>
        <span className="summary-dim">{t.id}</span>
      </div>
      <Body t={t} />
    </div>,
    document.body,
  );
}

function SummarySheet({ t, onClose }: { t: ThreadView; onClose: () => void }) {
  const surface = useRef<HTMLDivElement>(null);
  useSheetSwipe(surface, onClose);
  // 뒤로 가기는 시트를 닫는다(그 밑 화면이 아니라)
  useSheetHistory(true, onClose);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return createPortal(
    <div className="modal-scrim" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={surface} className="modal row-sheet summary-sheet" role="dialog" aria-modal="true" aria-label={`${t.id} 작업 요약`} data-testid="summary-sheet">
        <span className="row-sheet-grip" aria-hidden="true" />
        <div className="summary-sheet-head">
          <span className="row-sheet-title">{t.title}</span>
          <button type="button" className="ghost icon small" aria-label="닫기" onClick={onClose}>
            <X aria-hidden="true" />
          </button>
        </div>
        <div className="summary-sheet-scroll">
          <Body t={t} />
        </div>
      </div>
    </div>,
    document.body,
  );
}
