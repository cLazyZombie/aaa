// 기획 대화(t-1894 2-3, t-2002 확정 시안 library/mock): 기획 문서 화면 오른쪽 칸. 저장소의 기획 대화(주제)마다 대화 thread 하나이고,
// 기록 사이에 host 사건(새 대화·main에 올림·사람 고침·구현 맡기기)을 끼운다. 말은 host가 thread 준비 뒤 보낸다(`design_say`).
// 저장은 바로 main에 들어가므로 「main에 반영」·쉼·branch 표시는 없다.
import { ArrowUp, Check, ChevronDown, Clock, Ellipsis, FileText, Hammer, MessageSquare, Pencil, Plus, Send, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { ApiError, call, uploadDesignChatImage } from "./api";
import { ChatComposer, restorePending, savePending, type PendingSend } from "./ChatComposer";
import { ChatPanel } from "./Chat";
import { chatImageDir, designMarks, modelChoices, savesOf, slugSuggest, splitContext, takeImages, SLUG } from "./designModel";
import type { ChatMessage, DesignBranchView, DesignRepoView, OwnerChoice, PaneView, ThreadView } from "./gen/proto";
import { ProviderError } from "./ProviderError";
import { paneAgentStatus } from "./sidebarModel";

const MARK_ICON: Record<string, typeof FileText> = { open: MessageSquare, save: ArrowUp, push: ArrowUp, edit: Pencil, remove: Trash2, apply: ArrowUp, apply_wait: Clock, apply_cancel: X, apply_fail: X, handoff: Hammer, relay: Send };

const clock = (ms: number) => new Date(ms).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit", hour12: false });

/** 대화 고르기 줄: 지금 고른 대화·thread 상태, 저장 수, 마지막 저장 때 */
function chatState(b: DesignBranchView, current: boolean, threads: readonly ThreadView[]): string {
  const thread = b.thread ? threads.find((t) => t.id === b.thread) : undefined;
  const state = current ? "지금" : !thread || thread.status === "resolved" || thread.status === "failed" ? "끝남" : thread.status === "parked" ? "쉬는 중" : "열림";
  const saves = savesOf(b.events);
  return [state, `저장 ${saves}`, b.head && b.updated_ms ? `마지막 ${clock(b.updated_ms)}` : null].filter(Boolean).join(" · ");
}

export function DesignChat({ project, repo, branch, threads, panes, canControl, viewing, picked, onUnpick, active, onPick, onNew, onChanged, onError, onOpenLink }: {
  /** 그림을 올릴 프로젝트(읽은 문서의 프로젝트, 없으면 대화 thread의 프로젝트) */
  project: string | null;
  repo: DesignRepoView;
  /** 고른 대화(주제). 없으면 대화가 없다 */
  branch: DesignBranchView | null;
  threads: ThreadView[];
  panes: PaneView[];
  canControl: boolean;
  /** 보는 문서(저장소 기준 경로): 보낼 말 앞에 붙인다 */
  viewing: string | null;
  /** 「대화로 정하기」로 고른 블록: 보낼 말 앞에 붙이고 보낸 뒤 비운다 */
  picked?: { path: string; section: string; title: string } | null;
  onUnpick?: () => void;
  active: boolean;
  onPick: (slug: string) => void;
  onNew: () => void;
  /** 트리·대화 다시 읽기 */
  onChanged: () => void;
  onError: (e: string | null) => void;
  onOpenLink?: (href: string) => boolean;
}) {
  const [menu, setMenu] = useState(false);
  const menuButton = useRef<HTMLButtonElement>(null);
  const menuSurface = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    // RowMenu와 같이 capture 단계에서 닫되 바깥에서 누른 곳의 동작은 막지 않는다.
    const onPointer = (event: PointerEvent) => {
      const target = event.target as Node;
      if (menuSurface.current?.contains(target) || menuButton.current?.contains(target)) return;
      setMenu(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      event.preventDefault();
      setMenu(false);
      menuButton.current?.focus();
    };
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [menu]);
  const [more, setMore] = useState(false);
  const [dialog, setDialog] = useState<"end" | null>(null);
  const thread = branch?.thread ? threads.find((t) => t.id === branch.thread) ?? null : null;
  const pane = thread?.pane ? panes.find((p) => p.id === thread.pane && p.state === "running") ?? null : null;
  const harness = pane?.agent?.harness === "codex" ? "codex" : "claude";
  const chatable = pane?.agent?.harness === "claude" || pane?.agent?.harness === "codex";
  const owner = branch ? `design:${repo.repo}/${branch.slug}` : `design:${repo.repo}`;
  const [pending, setPending] = useState<PendingSend[]>(() => restorePending(owner));
  useEffect(() => setPending(restorePending(owner)), [owner]);
  useEffect(() => savePending(owner, pending), [owner, pending]);
  const [messages, setMessages] = useState<ChatMessage[] | null>(null);
  const [sent, setSent] = useState(0);
  const status = pane ? paneAgentStatus(pane) : undefined;
  // 보는 문서를 이 대화가 고친 적이 없다(다른 대화·사람이 고친 main 문서)
  const foreign = !!branch && !!viewing
    && !branch.events.some((e) => (e.kind === "save" || e.kind === "create" || e.kind === "edit") && e.text === viewing);
  const uploadTo = project ?? thread?.project ?? null;
  const marks = useMemo(() => branch ? designMarks(branch.events).map((m) => {
    const Icon = MARK_ICON[m.kind] ?? FileText;
    return { key: m.key, after: m.after, node: <span className={`design-mark-chip is-${m.kind}`} data-testid="design-mark"><Icon aria-hidden="true" />{m.text}</span> };
  }) : [], [branch]);
  return (
    <div className="design-chat" data-testid="design-chat">
      <header className="design-chat-head">
        <div className="design-pick">
          <button ref={menuButton} type="button" className="design-pick-btn" aria-haspopup="menu" aria-expanded={menu} data-testid="design-pick" onClick={() => setMenu((v) => !v)}>
            <MessageSquare className="i" aria-hidden="true" /><strong>{branch?.title ?? "기획 대화"}</strong><ChevronDown className="i" aria-hidden="true" />
          </button>
          {thread && <a className="design-thread-chip" data-testid="design-chat-thread" href={`#thread=${encodeURIComponent(thread.id)}`}>{thread.id}</a>}
          {menu && (
            <div ref={menuSurface} className="design-pick-menu" role="menu" data-testid="design-pick-menu">
              <p className="design-pick-label">{repo.repo} 기획 대화</p>
              {repo.branches.map((b) => (
                <button key={b.slug} type="button" role="menuitem" className={`design-pick-row${b.slug === branch?.slug ? " is-current" : ""}`} data-testid="design-pick-row"
                  onClick={() => { setMenu(false); onPick(b.slug); }}>
                  <MessageSquare className="i" aria-hidden="true" />
                  <span className="design-pick-text"><strong>{b.title}</strong><span>{chatState(b, b.slug === branch?.slug, threads)}</span></span>
                  {b.slug === branch?.slug && <Check className="i" aria-hidden="true" />}
                </button>
              ))}
              {canControl && (
                <button type="button" role="menuitem" className="design-pick-row is-new" data-testid="design-new" onClick={() => { setMenu(false); onNew(); }}>
                  <Plus className="i" aria-hidden="true" />새 대화
                </button>
              )}
            </div>
          )}
        </div>
        <span className="spacer" />
        {canControl && <button type="button" className="btn small ghost design-new-chat" data-testid="design-new-chat" onClick={onNew}><Plus className="i" aria-hidden="true" />새 대화</button>}
        {branch && (
          <div className="design-more">
            <button type="button" className="btn icon" aria-label="대화 메뉴" aria-expanded={more} data-testid="design-chat-more" onClick={() => setMore((v) => !v)}><Ellipsis className="i" aria-hidden="true" /></button>
            {more && (
              <div className="design-pick-menu is-right" role="menu">
                {thread && <a role="menuitem" className="design-pick-row" href={`#thread=${encodeURIComponent(thread.id)}`} onClick={() => setMore(false)}>thread {thread.id} · {thread.machine}</a>}
                {canControl && <button type="button" role="menuitem" className="design-pick-row is-danger" data-testid="design-discard-open" onClick={() => { setMore(false); setDialog("end"); }}><X className="i" aria-hidden="true" />대화 끝내기</button>}
              </div>
            )}
          </div>
        )}
      </header>
      <div className="design-chat-body">
        {!branch ? (
          <div className="design-chat-empty" data-testid="design-chat-empty">
            <p>대화 없음</p>
            {canControl && <button type="button" className="btn" onClick={onNew}><Plus className="i" aria-hidden="true" />새 대화</button>}
          </div>
        ) : (
          <>
            <ProviderError agent={pane?.agent} />
            {pane && chatable ? (
              <ChatPanel active={active} pane={pane.id} status={status} compacting={pane.agent?.compacting ?? false} compaction={pane.agent?.context?.compaction ?? null}
                readNow={sent} onMessages={setMessages} pending={pending} harness={harness} onOpenLink={onOpenLink} marks={marks} contextOf={splitContext}
                onDismissPending={(id) => setPending((list) => list.filter((p) => p.id !== id))} />
            ) : (
              <div className="design-chat-idle" data-testid="design-chat-idle">
                {marks.map((m) => <div key={m.key} className="chat-mark-row">{m.node}</div>)}
                {pending.map((p) => {
                  const c = splitContext(p.text);
                  return (
                    <div key={p.id} className="chat-mark-group">
                      {c.chips.length > 0 && <div className="chat-context-chips">{c.chips.map((chip, n) => <span key={n} className="chat-context-chip">{chip.label} {chip.path ? <code>{chip.value}</code> : chip.value}</span>)}</div>}
                      <div className="chat-msg user chat-bubble is-pending">{c.text}</div>
                    </div>
                  );
                })}
                {branch.pending > 0 && <p className="chat-note" role="status">thread 준비 중 · 보낼 말 {branch.pending}개</p>}
                {pending.length === 0 && branch.pending === 0 && <p className="chat-note">{pane ? "대화 기록을 읽지 못하는 harness다" : thread ? "잇는 중…" : "아직 대화가 없다."}</p>}
              </div>
            )}
            {branch.chat_error && <p className="banner err" role="alert">thread를 띄우지 못했다: {branch.chat_error}</p>}
          </>
        )}
      </div>
      {branch && (viewing || branch.edited.length > 0 || picked) && (
        <div className="design-context" data-testid="design-context">
          {viewing && <span className={`design-context-chip${foreign ? " is-foreign" : ""}`} data-testid="design-viewing"><FileText aria-hidden="true" />보는 문서 <code>{viewing.split("/").slice(1).join("/") || viewing}</code>{foreign && <span className="design-context-note">이 대화가 고치지 않음</span>}</span>}
          {branch.edited.length > 0 && <span className="design-context-chip"><Pencil aria-hidden="true" />사람이 고침 <code>{branch.edited.map((p) => p.slice(p.lastIndexOf("/") + 1)).join(", ")}</code></span>}
          {picked && (
            <span className="design-context-chip is-picked" data-testid="design-picked">고른 블록 <code>{picked.section} · {picked.title}</code>
              {onUnpick && <button type="button" className="design-context-x" aria-label="고른 블록 빼기" onClick={onUnpick}><X aria-hidden="true" /></button>}
            </span>
          )}
        </div>
      )}
      {branch && canControl && (
        <ChatComposer key={owner} active={active} owner={owner} pane={pane?.id ?? ""} touch={false}
          imagesOff={uploadTo ? null : "문서를 연 뒤 그림을 붙인다"}
          // 영상은 「고치기 → 그림」으로 붙인다: 대화 말의 「(그림: …)」은 그림만이다
          uploadImage={uploadTo ? (file) => /^image\/(png|jpeg|gif|webp)$/.test(file.type)
            ? uploadDesignChatImage(uploadTo, repo.repo, branch.slug, chatImageDir(repo.folder ?? "", viewing), file)
            : Promise.reject(new Error("대화에는 그림(PNG·JPEG·GIF·WebP)만 붙인다")) : undefined}
          agentLabel={harness === "codex" ? "Codex" : "Claude"} harness={harness}
          pending={pending} setPending={setPending}
          connected working={status === "working"} compacting={pane?.agent?.compacting ?? false} asking={status === "blocked"}
          context={pane?.agent?.context ?? null} messages={messages}
          // 사람이 고친 문서 알림도 여기서 붙인다: host가 따로 붙이면 보낸 말 확인(대기 말 지우기)이 글과 맞지 않는다
          // 입력창이 올린 그림(main에 commit한 저장소 경로)은 초안의 `@경로`를 떼어 「(그림: …)」으로 붙인다
          prepare={(text) => {
            const images = takeImages(text);
            return [
              branch.edited.length > 0 ? `(문서 고침: ${branch.edited.join(", ")})` : null,
              viewing ? `(보는 문서: ${viewing})` : null,
              picked ? `(고른 블록: ${picked.section} · ${picked.title})` : null,
              ...images.paths.map((p) => `(그림: ${p})`),
              images.text,
            ].filter((x): x is string => x !== null).join("\n");
          }}
          onSend={(text) => call({ op: "design_say", repo: repo.repo, branch: branch.slug, text }).then(() => {
            onError(null);
            onUnpick?.();
            onChanged();
            return true as const;
          }, (e: unknown) => (e instanceof ApiError ? e.message : String(e)))}
          onCtrlEnter={() => false} onInputTrace={() => {}} onCancelSend={() => Promise.resolve(null)}
          onSent={() => setSent((n) => n + 1)} onInspect={() => {}} />
      )}
      {dialog === "end" && branch && <EndDialog repo={repo.repo} branch={branch} onClose={() => setDialog(null)} onDone={() => { setDialog(null); onChanged(); }} />}
    </div>
  );
}

/** 대화 끝내기: 대화 thread를 끝내고 기록을 지운다. main에 저장한 글은 그대로다 */
function EndDialog({ repo, branch, onClose, onDone }: { repo: string; branch: DesignBranchView; onClose: () => void; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    try {
      await call({ op: "design_discard", repo, branch: branch.slug });
      onDone();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}>
      <div className="modal design-dialog" role="dialog" aria-modal="true" aria-label="기획 대화 끝내기" data-testid="design-discard-dialog">
        <header className="modal-header"><h2 className="modal-title"><X className="i" aria-hidden="true" />기획 대화 끝내기 <code>{branch.title}</code></h2></header>
        <div className="modal-body">
          <p className="design-discard-what">대화 thread를 끝내고 대화 기록을 지운다. main에 저장한 글은 그대로다(되돌리기는 문서 「이력」에서).</p>
          {problem && <p className="confirm-error" role="alert">{problem}</p>}
        </div>
        <footer className="modal-footer">
          <span className="spacer" />
          <button type="button" className="btn" disabled={busy} onClick={onClose}>닫기</button>
          <button type="button" className="btn primary" disabled={busy} data-testid="design-discard-submit" onClick={() => void submit()}>끝내기</button>
        </footer>
      </div>
    </div>
  );
}

/** 새 기획 대화: 주제·이름·모델. 대화의 저장은 바로 main에 들어간다 */
export function NewChatDialog({ repo, onClose, onDone }: { repo: DesignRepoView; onClose: () => void; onDone: (slug: string) => void }) {
  const [title, setTitle] = useState("");
  const [slug, setSlug] = useState("");
  const [touched, setTouched] = useState(false);
  const [profile, setProfile] = useState("");
  const [owners, setOwners] = useState<OwnerChoice[]>([]);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    call({ op: "assignable", check: null }).then((r) => { if (r.kind === "assignable") setOwners(r.owners); }, () => {});
  }, []);
  const names = modelChoices(owners, repo.machine, "").filter(Boolean);
  const name = touched ? slug : slugSuggest(title);
  const taken = repo.branches.some((b) => b.slug === name);
  const ok = title.trim() !== "" && SLUG.test(name) && !taken;
  const submit = async () => {
    setBusy(true);
    try {
      await call({ op: "design_branch_open", repo: repo.repo, slug: name, title: title.trim(), ...(profile ? { profile } : {}) });
      onDone(name);
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}>
      <div className="modal design-dialog" role="dialog" aria-modal="true" aria-label="새 기획 대화" data-testid="design-new-dialog">
        <header className="modal-header"><h2 className="modal-title"><Plus className="i" aria-hidden="true" />새 기획 대화 <code>{repo.repo}</code></h2></header>
        <div className="modal-body design-new">
          <label>주제<input className="input" value={title} autoFocus data-testid="design-new-title" onChange={(e) => setTitle(e.target.value)} /></label>
          <label>이름
            <input className="input" value={name} spellCheck={false} data-testid="design-new-slug"
              onChange={(e) => { setTouched(true); setSlug(e.target.value); }} />
          </label>
          {name !== "" && !SLUG.test(name) && <p className="confirm-error" role="alert">영문 소문자·숫자·-</p>}
          {taken && <p className="confirm-error" role="alert">이미 있는 대화</p>}
          <div className="design-new-row">
            <label>모델
              <select className="input" value={profile} data-testid="design-new-model" onChange={(e) => setProfile(e.target.value)}>
                <option value="">프로젝트 기본</option>
                {names.map((n) => <option key={n} value={n}>{n}</option>)}
              </select>
            </label>
          </div>
          <p className="design-hand-note">대화의 저장은 바로 {repo.repo} main에 들어간다(저장소 문서 검사를 지나야 한다).</p>
          {problem && <p className="confirm-error" role="alert">{problem}</p>}
        </div>
        <footer className="modal-footer">
          <span className="spacer" />
          <button type="button" className="btn" disabled={busy} onClick={onClose}>닫기</button>
          <button type="button" className="btn primary" disabled={busy || !ok} data-testid="design-new-submit" onClick={() => void submit()}>시작</button>
        </footer>
      </div>
    </div>
  );
}
