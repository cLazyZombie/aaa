import { splitAppIds } from "./appIds";
import { ignoredFailure } from "./errorDiagnostic.ts";
// 활동 막대 「알림함」 패널(관리 GUI W4, plan §8.343): coordinator 알림함(inbox)·흐름 사건·post. 알림함은 늘 `peek`로 읽는다:
// 화면이 읽어도 「봤다」가 되지 않아 coordinator 묶음 알림(nudge)이 그대로 간다. 끝내기는 조작 기기만. 사용자가 답할 요청 post는
// 「답할 것」이 다루므로(plan §8.345) post 탭과 알림함 탭 모두 수만 보이고 그리로 보낸다(같은 자료를 두 곳에 두지 않는다). post 목록은 조작 기기만 읽는다.
import "./InboxPanel.css";

import { useCallback, useEffect, useRef, useState } from "react";
import type { FlowEvent, InboxItem, PostEnvelope } from "./gen/proto";
import { ApiError, call, me, onInboxChanged } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { canOperate, inboxOpen, isUserRequest, needsAnswer } from "./operateModel.ts";

type Tab = "inbox" | "events" | "posts";
const say = (e: unknown) => (e instanceof ApiError || e instanceof Error ? e.message : String(e));
const when = (ms: number) => {
  const d = new Date(ms);
  const today = new Date().toDateString() === d.toDateString();
  return today ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }) : d.toLocaleDateString([], { month: "numeric", day: "numeric" });
};
const THREAD_ID = /^t-\d+$/;

export function InboxPanel({ canControl, onThread, onAnswers }: {
  canControl: boolean;
  /** 항목·사건의 thread를 연다 */
  onThread?: (project: string, id: string) => void;
  /** 「답할 것」으로(있을 때만 단추를 그린다) */
  onAnswers?: () => void;
}) {
  const [tab, setTab] = useState<Tab>("inbox");
  const tabs: Array<{ id: Tab; label: string }> = [
    { id: "inbox", label: "알림함" },
    { id: "events", label: "흐름 사건" },
    ...(canControl ? [{ id: "posts" as const, label: "post" }] : []),
  ];
  return (
    <div className="inbox-panel" data-testid="inbox-panel">
      <div className="inbox-tabs" role="tablist" aria-label="알림함">
        {tabs.map((t) => (
          <button key={t.id} type="button" role="tab" aria-selected={tab === t.id} className={tab === t.id ? "is-on" : undefined} onClick={() => setTab(t.id)} data-testid={`inbox-tab-${t.id}`}>
            {t.label}
          </button>
        ))}
      </div>
      {tab === "inbox" && <InboxList canControl={canControl} onThread={onThread} onAnswers={onAnswers} />}
      {tab === "events" && <EventList onThread={onThread} />}
      {tab === "posts" && <PostList onAnswers={onAnswers} />}
    </div>
  );
}

function InboxList({ canControl, onThread, onAnswers }: { canControl: boolean; onThread?: (project: string, id: string) => void; onAnswers?: () => void }) {
  const [all, setAll] = useState(false);
  const [items, setItems] = useState<InboxItem[] | null>(null);
  const [omitted, setOmitted] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  // 「모두 끝내기」 창을 연 때 보인 항목(창이 떠 있는 동안 새로 온 항목은 끝내지 않는다)
  const [askAll, setAskAll] = useState<string[] | null>(null);
  const loadId = useRef(0);
  const load = useCallback(() => {
    const id = ++loadId.current;
    return call({ op: "inbox", all, peek: true })
      .then((r) => {
        if (id !== loadId.current || r.kind !== "inbox") return;
        setItems(r.items);
        setOmitted(r.omitted_open + (all ? r.omitted_done : 0));
        setError(null);
      })
      .catch((e: unknown) => { if (id === loadId.current) setError(say(e)); });
  }, [all]);
  useEffect(() => {
    load();
    return onInboxChanged(load);
  }, [load]);
  const done = async (ids: string[], every: boolean) => {
    setBusy(every ? "all" : ids[0]!);
    setError(null);
    try {
      await call({ op: "inbox_done", ids, all: every });
      load();
    } catch (e) {
      setError(say(e));
    } finally {
      setBusy(null);
    }
  };
  // 사용자가 답할 요청 post는 「답할 것」에서 답하고 끝낸다(plan §8.345): 여기서는 끝내지 않은 수만 보이고 그리로 보낸다
  const answers = (items ?? []).filter((i) => !i.done && isUserRequest(i)).length;
  const mine = (items ?? []).filter((i) => !isUserRequest(i));
  const open = mine.filter((i) => !i.done);
  // 끝내지 않은 것이 먼저, 그 안에서는 최근 것이 위
  const shown = [...mine].sort((a, b) => Number(a.done) - Number(b.done) || b.created_at_ms - a.created_at_ms);
  return (
    <div className="inbox-body">
      <div className="inbox-tools">
        <label className="inbox-check">
          <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} />
          끝낸 것도
        </label>
        {canControl && open.length > 0 && (
          <button type="button" className="btn small" disabled={busy !== null} onClick={() => setAskAll(open.map((i) => i.id))} data-testid="inbox-done-all">
            모두 끝내기
          </button>
        )}
      </div>
      {error && <p className="inbox-error" role="alert">{error}</p>}
      {answers > 0 && (
        <p className="inbox-answers" data-testid="inbox-answers">
          사용자가 답할 요청 {answers}건은 「답할 것」에서 다룬다.
          {onAnswers && (
            <button type="button" className="link" onClick={onAnswers}>
              답할 것 열기
            </button>
          )}
        </p>
      )}
      {items === null ? (
        <p className="inbox-empty">읽는 중…</p>
      ) : shown.length === 0 ? (
        <p className="inbox-empty" data-testid="inbox-empty">알림함이 비었다</p>
      ) : (
        <ul className="inbox-list" data-testid="inbox-list">
          {shown.map((i) => {
            const report = ["new report", "report needs user"].includes(i.event)
              ? splitAppIds(i.summary, id => id.kind === "report").find(id => typeof id !== "string")?.target : undefined;
            return <li key={i.id} className={i.done ? "is-done" : undefined} data-testid="inbox-item">
              <div className="inbox-line">
                <span className="inbox-kind">{i.item_kind}</span>
                {THREAD_ID.test(i.subject) && onThread ? (
                  <button type="button" className="link inbox-subject" onClick={() => onThread(i.project, i.subject)}>
                    {i.subject}
                  </button>
                ) : (
                  <span className="inbox-subject">{i.subject}</span>
                )}
                <span className="inbox-event">{i.event}</span>
                {report && <a href={`#report=${encodeURIComponent(`${i.project}/${report}`)}`}>{report}</a>}
                <time className="inbox-time">{when(i.created_at_ms)}</time>
              </div>
              <p className="inbox-summary">{i.summary}</p>
              {i.body && (
                <details className="inbox-detail">
                  <summary>자세히</summary>
                  <pre>{i.body}</pre>
                </details>
              )}
              {canControl && !i.done && (
                <div className="inbox-actions">
                  <button type="button" className="btn small" disabled={busy !== null} onClick={() => void done([i.id], false)} aria-label={`${i.subject} ${i.event} 끝내기`}>
                    끝내기
                  </button>
                </div>
              )}
            </li>;
          })}
        </ul>
      )}
      {omitted > 0 && <p className="inbox-empty">응답 크기 한도로 {omitted}개를 싣지 못했다(`aaa inbox list`)</p>}
      {askAll && (
        <ConfirmDialog
          title="알림함 모두 끝내기"
          body={`보이는 끝내지 않은 ${askAll.length}개를 끝낸다. 창을 연 뒤 새로 온 항목은 그대로 둔다. 끝낸 항목은 「끝낸 것도」에서 다시 볼 수 있다.`}
          confirmLabel="모두 끝내기"
          onConfirm={async () => {
            // 본 것만 끝낸다(`all`은 그 사이 온, 보지 않은 항목까지 끝내 coordinator 묶음 알림에서 빠진다)
            await call({ op: "inbox_done", ids: askAll, all: false });
            ++loadId.current;
            const finished = new Set(askAll);
            setItems(rows => rows?.map(i => finished.has(i.id) ? { ...i, done: true } : i) ?? null);
            void load();
          }}
          onClose={() => setAskAll(null)}
        />
      )}
    </div>
  );
}

function EventList({ onThread }: { onThread?: (project: string, id: string) => void }) {
  const [events, setEvents] = useState<FlowEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    // 최근 N개(`tail`)는 1.77 host만 안다: 옛 host는 모르는 칸을 버리고 가장 오래된 것을 준다
    me()
      .then((m) => {
        if (!canOperate(m?.front_api)) throw new Error("이 host는 최근 흐름 사건 읽기를 모른다(`aaa events`)");
        return call({ op: "events", project: null, after: 0, limit: 100, tail: true });
      })
      .then((r) => {
        if (r.kind !== "events") return;
        setEvents(r.events);
        setError(null);
      })
      .catch((e: unknown) => setError(say(e)));
  }, []);
  useEffect(() => {
    load();
    // 알림함과 함께 바뀌는 일이 많다(보고·끝냄·실패): 그때 다시 읽는다
    return onInboxChanged(load);
  }, [load]);
  return (
    <div className="inbox-body">
      <div className="inbox-tools">
        <span className="inbox-hint">최근 100개</span>
        <button type="button" className="btn small" onClick={load}>
          새로 읽기
        </button>
      </div>
      {error && <p className="inbox-error" role="alert">{error}</p>}
      {events === null ? (
        <p className="inbox-empty">읽는 중…</p>
      ) : events.length === 0 ? (
        <p className="inbox-empty">흐름 사건이 없다</p>
      ) : (
        <ul className="inbox-list" data-testid="event-list">
          {[...events].reverse().map((e) => (
            <li key={`${e.project}:${e.seq}`} data-testid="event-item" data-seq={e.seq} data-at={e.at_ms}>
              <div className="inbox-line">
                <span className="inbox-kind">{e.kind}</span>
                {e.thread && onThread ? (
                  <button type="button" className="link inbox-subject" onClick={() => onThread(e.project, e.thread!)}>
                    {e.thread}
                  </button>
                ) : (
                  <span className="inbox-subject">{e.thread ?? e.project}</span>
                )}
                <time className="inbox-time">{when(e.at_ms)}</time>
              </div>
              {e.detail && <p className="inbox-summary">{e.detail}</p>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function PostList({ onAnswers }: { onAnswers?: () => void }) {
  const [posts, setPosts] = useState<PostEnvelope[] | null>(null);
  const [muted, setMuted] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(() => {
    call({ op: "post_list" })
      .then((r) => {
        if (r.kind !== "posts") return;
        setPosts(r.posts);
        setMuted(r.muted ?? []);
        setError(null);
      })
      .catch((e: unknown) => setError(say(e)));
  }, []);
  useEffect(load, [load]);
  const unmute = async (who: string) => {
    setBusy(true);
    try {
      await call({ op: "post_unmute", thread: who });
      load();
    } catch (e) {
      setError(say(e));
    } finally {
      setBusy(false);
    }
  };
  const answers = (posts ?? []).filter(needsAnswer).length;
  const shown = (posts ?? []).filter((p) => !needsAnswer(p));
  return (
    <div className="inbox-body">
      <div className="inbox-tools">
        <span className="inbox-hint">최근 post(받는 쪽·전달 상태)</span>
        <button type="button" className="btn small" onClick={load}>
          새로 읽기
        </button>
      </div>
      {error && <p className="inbox-error" role="alert">{error}</p>}
      {muted.length > 0 && (
        <ul className="inbox-muted" data-testid="post-muted">
          {muted.map((who) => (
            <li key={who}>
              <span>
                <strong>{who}</strong> post 음소거됨
              </span>
              <button type="button" className="btn small" disabled={busy} onClick={() => void unmute(who)}>
                음소거 풀기
              </button>
            </li>
          ))}
        </ul>
      )}
      {answers > 0 && (
        <p className="inbox-answers" data-testid="post-answers">
          사용자가 답할 요청 {answers}건은 「답할 것」에서 다룬다.
          {onAnswers && (
            <button type="button" className="link" onClick={onAnswers}>
              답할 것 열기
            </button>
          )}
        </p>
      )}
      {posts === null ? (
        <p className="inbox-empty">읽는 중…</p>
      ) : shown.length === 0 ? (
        <p className="inbox-empty">post가 없다</p>
      ) : (
        <ul className="inbox-list" data-testid="post-list">
          {shown.map((p) => (
            <li key={p.id} data-testid="post-item">
              <div className="inbox-line">
                <span className="inbox-kind">{p.post_type}</span>
                <span className="inbox-subject">
                  {p.from.thread ?? p.from.role} → {p.to}
                </span>
                <span className="inbox-event">{p.state}</span>
                <time className="inbox-time">{when(p.created_ms)}</time>
              </div>
              <p className="inbox-summary">{p.title}</p>
              {p.body && (
                <details className="inbox-detail">
                  <summary>본문</summary>
                  <pre>{p.body}</pre>
                </details>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** 활동 막대 배지: 끝내지 않은 알림함 항목 수. 열 때와 host `inbox` 사건마다 다시 읽는다(`peek`라 「봤다」가 되지 않는다) */
export function useInboxBadge(): number {
  const [count, setCount] = useState(0);
  useEffect(() => {
    let live = true;
    let loading = false;
    let again = false;
    const load = () => {
      if (loading) {
        again = true;
        return;
      }
      loading = true;
      call({ op: "inbox", all: false, peek: true })
        .then((r) => live && r.kind === "inbox" && setCount(inboxOpen(r.items, r.omitted_open)))
        .catch(ignoredFailure("InboxPanel.tsx:340", () => undefined))
        .finally(() => {
          loading = false;
          if (again && live) {
            again = false;
            load();
          }
        });
    };
    load();
    const off = onInboxChanged(load);
    return () => {
      live = false;
      off();
    };
  }, []);
  return count;
}
