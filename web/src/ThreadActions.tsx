// thread 화면 머리의 「thread 관리」(관리 GUI W4, plan §8.333): 끝내기·다시 열기·다시 시작·brief 다시 보내기·Next·요약 고치기.
// 끝내기 창은 host가 미리 읽은 잃을 수 있는 것(커밋하지 않은 파일·병합되지 않은 변경·사본 상태)을 보이고, 위험하거나 모르면
// 「worktree 남기기」를 켠 채로 연다. 거부·검사는 host가 한다: 화면은 그 글을 창 안에 보인다. 조작 기기만 그린다.
import "./AutomationPanel.css";
import "./ThreadActions.css";

import { useEffect, useRef, useState } from "react";
import { BookOpenText, FilePen, ListChecks, MoreHorizontal, RotateCcw, Send, SquareCheckBig, Undo2 } from "lucide-react";
import type { ProfileView, ResolvePreview, ThreadView } from "./gen/proto";
import { ApiError, call, me } from "./api";
import { AutomationSheet } from "./AutomationSheet";
import { ConfirmDialog } from "./ConfirmDialog";
import { RowMenu, type RowMenuItem } from "./RowMenu";
import { canOperate, resolveDefaults, resolveRequest, threadActions } from "./operateModel.ts";

type Open = "resolve" | "reopen" | "restart" | "brief" | "next" | "summary";
const say = (e: unknown) => (e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));

export function ThreadActions({ t, running }: { t: ThreadView; running: boolean }) {
  const button = useRef<HTMLButtonElement>(null);
  // undefined: 아직 모름(`me` 응답 전). 끝내기 창은 알 때까지 기다린다
  const [frontApi, setFrontApi] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    me().then((m) => live && setFrontApi(m?.front_api ?? null), () => undefined);
    return () => {
      live = false;
    };
  }, []);
  const [menu, setMenu] = useState(false);
  const [open, setOpen] = useState<Open | null>(null);
  const can = threadActions(t, running);
  const items: RowMenuItem[] = [
    ...(can.resolve ? [{ id: "resolve", label: "끝내기", icon: SquareCheckBig, run: () => setOpen("resolve") }] : []),
    ...(can.reopen ? [{ id: "reopen", label: "다시 열기", icon: Undo2, run: () => setOpen("reopen") }] : []),
    ...(can.restart ? [{ id: "restart", label: "다시 시작", icon: RotateCcw, run: () => setOpen("restart") }] : []),
    ...(can.brief ? [{ id: "brief", label: "brief 다시 보내기", icon: BookOpenText, run: () => setOpen("brief") }] : []),
    { id: "next", label: "Next", hint: t.next.length > 0 ? `${t.next.length}줄` : undefined, icon: ListChecks, divider: can.resolve || can.reopen || can.restart || can.brief, run: () => setOpen("next") },
    { id: "summary", label: "작업 요약 고치기", icon: FilePen, run: () => setOpen("summary") },
  ];
  const close = () => setOpen(null);
  return (
    <>
      <button ref={button} type="button" className="ghost small thread-manage" data-testid="thread-manage" aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu((m) => !m)}>
        <MoreHorizontal aria-hidden="true" />
        <span className="thread-manage-label">관리</span>
      </button>
      {menu && button.current && <RowMenu anchor={button.current} title="thread 관리" subtitle={`${t.project}/${t.id}`} items={items} onClose={() => setMenu(false)} />}
      {open === "resolve" && <ResolveDialog t={t} frontApi={frontApi} onClose={close} />}
      {open === "reopen" && (
        <ConfirmDialog
          title="끝낸 thread 다시 열기"
          body={`${t.id}를 열린 thread로 되돌린다. 아무것도 띄우지 않고, 지운 worktree는 돌아오지 않는다.`}
          confirmLabel="다시 열기"
          onConfirm={async () => {
            await call({ op: "thread_resolve", project: t.project, thread: t.id, keep_worktree: false, reopen: true, skip_copy: false, discard_uncopied: false });
            close();
          }}
          onClose={close}
        />
      )}
      {open === "restart" && <RestartDialog t={t} onClose={close} />}
      {open === "brief" && (
        <ConfirmDialog
          title="brief 다시 보내기"
          body={`${t.id}의 agent pane에 brief 안내를 다시 보낸다(자동으로는 다시 보내지 않는다).`}
          confirmLabel="다시 보내기"
          onConfirm={async () => {
            await call({ op: "thread_brief", project: t.project, thread: t.id });
            close();
          }}
          onClose={close}
        />
      )}
      {open === "next" && <NextDialog t={t} running={running} onClose={close} />}
      {open === "summary" && <SummaryDialog t={t} onClose={close} />}
    </>
  );
}

function Count({ n, unit }: { n: number | null; unit: string }) {
  return <strong>{n === null ? "모름" : `${n}${unit}`}</strong>;
}

function ResolveDialog({ t, frontApi, onClose }: { t: ThreadView; frontApi: string | null | undefined; onClose: () => void }) {
  const preview = canOperate(frontApi);
  const [p, setP] = useState<ResolvePreview | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  // host 판을 알기 전과 미리보기를 읽는 동안은 끝내기를 막는다(미리보기 없이 끝내지 않게)
  const loading = frontApi === undefined || (preview && p === null && readError === null);
  // 정리가 끊긴 thread는 처음 끝낼 때 고른 대로 이어 간다(host `resolve_pending`): 여기서 고를 것이 없다
  const resuming = t.cleanup_pending === true;
  const [keep, setKeep] = useState(true);
  const [skipCopy, setSkipCopy] = useState(false);
  const [discard, setDiscard] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const touched = useRef(false);
  useEffect(() => {
    if (!preview) return;
    let live = true;
    call({ op: "thread_resolve_preview", project: t.project, thread: t.id })
      .then((r) => {
        if (!live || r.kind !== "resolve_preview") return;
        setP(r);
        if (!touched.current) setKeep(resolveDefaults(r).keep);
      })
      .catch((e: unknown) => live && setReadError(say(e)));
    return () => {
      live = false;
    };
  }, [preview, t.project, t.id]);
  const worktree = p ? p.worktree : t.thread_kind === "worktree" && !!t.cwd;
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      await call(resolveRequest(t, { keep: worktree && keep, skipCopy, discard }));
      onClose();
    } catch (e) {
      setError(say(e));
      setBusy(false);
    }
  };
  return (
    <AutomationSheet
      title="thread 끝내기"
      subtitle={`${t.project}/${t.id} · ${t.title}`}
      onClose={onClose}
      busy={busy}
      testid="resolve-dialog"
      footer={
        <>
          <button type="button" className="btn" disabled={busy} onClick={onClose}>
            취소
          </button>
          <button type="button" className="btn btn-danger" data-testid="resolve-confirm" disabled={busy || loading} onClick={() => void run()}>
            {busy ? "끝내는 중…" : "끝내기"}
          </button>
        </>
      }
    >
      <p className="op-lead">마지막 사본을 받고 세션을 닫은 뒤 worktree를 정리한다. 보고와 결과물 사본은 남는다.</p>
      {loading && (
        <p className="op-muted" role="status" data-testid="resolve-loading">
          그 머신에서 worktree 상태를 읽는 중…
        </p>
      )}
      {frontApi !== undefined && !preview && <p className="op-muted">이 host는 끝내기 전 확인을 모른다: worktree를 남기는 것이 기본이다.</p>}
      {resuming && (
        <p className="op-warn" data-testid="resolve-resuming">
          정리가 끊긴 thread다: 처음 끝낼 때 고른 대로(남기기·버리기) 이어 간다.
        </p>
      )}
      {readError && <p className="op-warn">미리 읽지 못했다: {readError}</p>}
      {p && (
        <dl className="op-facts" data-testid="resolve-facts">
          {p.worktree && (
            <>
              <dt>커밋하지 않은 파일</dt>
              <dd>
                <Count n={p.uncommitted} unit="개" />
              </dd>
              <dt>base 대비 바뀐 파일</dt>
              <dd>
                <Count n={p.changed} unit="개" />
              </dd>
            </>
          )}
          {p.branch && (
            <>
              <dt>branch</dt>
              <dd>
                <span className="mono">{p.branch}</span> · {p.merged ? "PR 병합됨(지운다)" : "병합 안 됨(남긴다)"}
              </dd>
            </>
          )}
          <dt>사본</dt>
          <dd>{p.copy === "complete" ? "다 왔다" : p.copy === "partial" ? "빠진 것이 있다" : p.copy === "failed" ? "실패(다시 받는 중)" : "끝낼 때 받는다"}</dd>
        </dl>
      )}
      {p && p.risks.length > 0 && (
        <ul className="op-risks" data-testid="resolve-risks">
          {p.risks.map((r) => (
            <li key={r}>{r}</li>
          ))}
        </ul>
      )}
      <fieldset className="op-options" disabled={busy} hidden={resuming}>
        {worktree && (
          <label className="op-check">
            <input
              type="checkbox"
              data-testid="resolve-keep"
              checked={keep}
              onChange={(e) => {
                touched.current = true;
                setKeep(e.target.checked);
              }}
            />
            <span>worktree와 branch 남기기</span>
          </label>
        )}
        <label className="op-check">
          <input type="checkbox" checked={skipCopy} onChange={(e) => setSkipCopy(e.target.checked)} />
          <span>마지막 사본 없이 끝내기(사본을 만들 수 없을 때, worktree는 남는다)</span>
        </label>
        {worktree && !keep && p?.copy === "partial" && (
          <label className="op-check">
            <input type="checkbox" checked={discard} onChange={(e) => setDiscard(e.target.checked)} />
            <span>사본에 빠진 것이 있어도 worktree 지우기</span>
          </label>
        )}
      </fieldset>
      {error && (
        <p className="confirm-error" role="alert">
          {error}
        </p>
      )}
    </AutomationSheet>
  );
}

function RestartDialog({ t, onClose }: { t: ThreadView; onClose: () => void }) {
  const [resume, setResume] = useState(false);
  const [profiles, setProfiles] = useState<ProfileView[] | null>(null);
  const [profile, setProfile] = useState(t.profile);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    call({ op: "profiles", machine: t.machine })
      .then((r) => live && r.kind === "profiles" && setProfiles(r.profiles))
      .catch(() => live && setProfiles([]));
    return () => {
      live = false;
    };
  }, [t.machine]);
  const names = [...new Set([t.profile, ...(profiles ?? []).map((p) => p.name)])];
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      await call({ op: "thread_restart", project: t.project, thread: t.id, resume, profile: resume || profile === t.profile ? null : profile });
      onClose();
    } catch (e) {
      setError(say(e));
      setBusy(false);
    }
  };
  return (
    <AutomationSheet
      title="thread 다시 시작"
      subtitle={`${t.project}/${t.id} · ${t.profile}@${t.machine}`}
      onClose={onClose}
      busy={busy}
      testid="restart-dialog"
      footer={
        <>
          <button type="button" className="btn" disabled={busy} onClick={onClose}>
            취소
          </button>
          <button type="button" className="btn btn-primary" data-testid="restart-confirm" disabled={busy} onClick={() => void run()}>
            {busy ? "띄우는 중…" : "다시 시작"}
          </button>
        </>
      }
    >
      <fieldset className="op-options" disabled={busy}>
        <label className="op-check">
          <input type="radio" name="restart-mode" checked={!resume} onChange={() => setResume(false)} />
          <span>새 대화: task·지침을 다시 읽어 새 brief로 띄운다</span>
        </label>
        <label className="op-check">
          <input type="radio" name="restart-mode" data-testid="restart-resume" checked={resume} onChange={() => setResume(true)} />
          <span>이어 가기: 저장된 대화를 잇는다(brief 안내 없음)</span>
        </label>
        <label className="op-field">
          <span>profile</span>
          <select value={profile} disabled={resume || profiles === null} onChange={(e) => setProfile(e.target.value)} data-testid="restart-profile">
            {names.map((n) => (
              <option key={n} value={n}>
                {n}
                {n === t.profile ? " (지금)" : ""}
              </option>
            ))}
          </select>
        </label>
      </fieldset>
      {error && (
        <p className="confirm-error" role="alert">
          {error}
        </p>
      )}
    </AutomationSheet>
  );
}

function NextDialog({ t, running, onClose }: { t: ThreadView; running: boolean; onClose: () => void }) {
  const [lines, setLines] = useState<string[]>(t.next);
  const [add, setAdd] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ask = async (line: number | null, more: string | null, key: string) => {
    setBusy(key);
    setError(null);
    setNote(null);
    try {
      const r = await call({ op: "thread_next", project: t.project, thread: t.id, line, add: more });
      if (r.kind === "next") {
        setLines(r.lines);
        if (line !== null) setNote(`${line}번째 줄을 보냈다${r.message?.state === "queued" ? "(차례 기다림)" : ""}`);
        if (more !== null) setAdd("");
      }
    } catch (e) {
      setError(say(e));
    } finally {
      setBusy(null);
    }
  };
  useEffect(() => {
    void ask(null, null, "load");
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <AutomationSheet title="Next" subtitle={`${t.project}/${t.id}`} onClose={onClose} busy={busy !== null && busy !== "load"} testid="next-dialog">
      {lines.length === 0 ? (
        <p className="op-muted">Next 줄이 없다</p>
      ) : (
        <ol className="op-next" data-testid="next-lines">
          {lines.map((l, i) => (
            <li key={`${i}:${l}`}>
              <span className="op-next-text">{l}</span>
              <button type="button" className="btn small" disabled={busy !== null || !running} title={running ? "이 줄을 thread에 메시지로 보낸다" : "agent pane이 돌지 않는다"} onClick={() => void ask(i + 1, null, `send:${i}`)}>
                <Send aria-hidden="true" />
                보내기
              </button>
            </li>
          ))}
        </ol>
      )}
      <form
        className="op-add"
        onSubmit={(e) => {
          e.preventDefault();
          if (add.trim()) void ask(null, add.trim(), "add");
        }}
      >
        <input value={add} onChange={(e) => setAdd(e.target.value)} placeholder="Next 줄 더하기(한 줄)" aria-label="Next 줄 더하기" disabled={busy !== null} data-testid="next-add" />
        <button type="submit" className="btn" disabled={busy !== null || !add.trim()}>
          더하기
        </button>
      </form>
      {note && (
        <p className="op-note" role="status">
          {note}
        </p>
      )}
      {error && (
        <p className="confirm-error" role="alert">
          {error}
        </p>
      )}
    </AutomationSheet>
  );
}

function SummaryDialog({ t, onClose }: { t: ThreadView; onClose: () => void }) {
  const [text, setText] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    call({ op: "thread_summary", project: t.project, thread: t.id })
      .then((r) => live && r.kind === "thread_summary" && setText(r.text ?? ""))
      // 읽지 못하면 칸을 잠근 채 둔다: 보지 못한 요약을 통째로 덮어쓰지 않게
      .catch((e: unknown) => live && setError(say(e)));
    return () => {
      live = false;
    };
  }, [t.project, t.id]);
  const save = async () => {
    if (text === null) return;
    setBusy(true);
    setError(null);
    try {
      await call({ op: "thread_summary_set", project: t.project, thread: t.id, text });
      onClose();
    } catch (e) {
      setError(say(e));
      setBusy(false);
    }
  };
  return (
    <AutomationSheet
      title="작업 요약 고치기"
      subtitle={`${t.project}/${t.id}`}
      onClose={onClose}
      busy={busy}
      testid="summary-dialog"
      footer={
        <>
          <button type="button" className="btn" disabled={busy} onClick={onClose}>
            취소
          </button>
          <button type="button" className="btn btn-primary" data-testid="summary-save" disabled={busy || text === null} onClick={() => void save()}>
            {busy ? "저장 중…" : "저장"}
          </button>
        </>
      }
    >
      <p className="op-muted">요약 전체를 다시 쓴다(Markdown, `# 목표` 같은 절).</p>
      <textarea className="op-textarea" aria-label="작업 요약" value={text ?? ""} disabled={text === null || busy} onChange={(e) => setText(e.target.value)} rows={14} data-testid="summary-text" />
      {error && (
        <p className="confirm-error" role="alert">
          {error}
        </p>
      )}
    </AutomationSheet>
  );
}
