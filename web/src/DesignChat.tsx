// 기획 대화(t-1894 2-3, 확정 시안 library/mock2): 기획 문서 화면 오른쪽 칸. 저장소의 기획 branch마다 대화 하나이고, 대화 thread의
// 기록 사이에 host 사건(새 대화·저장·올림·사람 고침·반영)을 끼운다. 말은 host가 thread 준비 뒤 보낸다(`design_say`).
import { Check, ChevronDown, Clock, Ellipsis, FileText, GitBranch, GitMerge, Hammer, MessageSquare, Pencil, Play, Plus, Send, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { ApiError, call, uploadDesignChatImage } from "./api";
import { ChatComposer, restorePending, savePending, type PendingSend } from "./ChatComposer";
import { ChatPanel } from "./Chat";
import { chatImageDir, designMarks, mergedToMain, restDays, slugSuggest, splitContext, takeImages, SLUG } from "./designModel";
import type { ChatMessage, DesignBranchView, DesignRepoView, FrontRequest, FrontResponse, OwnerChoice, PaneView, ThreadView } from "./gen/proto";
import { ideaModelChoices } from "./ideasModel";
import { ProviderError } from "./ProviderError";
import { paneAgentStatus } from "./sidebarModel";

const MARK_ICON: Record<string, typeof GitBranch> = { open: GitBranch, save: FileText, push: GitMerge, edit: Pencil, apply: GitMerge, apply_wait: Clock, apply_cancel: X, apply_fail: X, handoff: Hammer, relay: Send };

/** 대화 고르기 줄의 상태 */
function branchState(b: DesignBranchView, threads: readonly ThreadView[], now: number): string {
  if (mergedToMain(b)) return "main에 들어감";
  const apply = b.apply ? threads.find((t) => t.id === b.apply) : undefined;
  if (apply && apply.status !== "resolved" && apply.status !== "failed") return `반영 대기 ${apply.id}`;
  if (b.apply_wait) return "반영 예약";
  if (b.state === "rest") return `쉼 ${restDays(b, now)}일`;
  return "지금";
}

export function DesignChat({ project, repo, branch, threads, panes, canControl, viewing, picked, onUnpick, active, now, onPick, onNew, onChanged, onError, onOpenLink }: {
  /** 그림을 올릴 프로젝트(읽은 문서의 프로젝트, 없으면 대화 thread의 프로젝트) */
  project: string | null;
  repo: DesignRepoView;
  /** 고른 대화(기획 branch). 없으면 대화가 없다 */
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
  now: number;
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
  const [dialog, setDialog] = useState<"apply" | "discard" | null>(null);
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
  const apply = branch?.apply ? threads.find((t) => t.id === branch.apply) : undefined;
  const merged = !!branch && mergedToMain(branch);
  const applying = !merged && !!apply && apply.status !== "resolved" && apply.status !== "failed";
  const waiting = branch?.apply_wait ?? null;
  // 예약한 반영은 host가 자리 나면 띄운다: 새 thread가 보이면(또는 30초마다) 트리를 다시 읽어 예약 표시를 바꾼다
  useEffect(() => { if (waiting) onChanged(); }, [threads.length]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!waiting) return;
    const id = window.setInterval(onChanged, 30_000);
    return () => window.clearInterval(id);
  }, [!!waiting, onChanged]); // eslint-disable-line react-hooks/exhaustive-deps
  // 보는 문서를 이 대화가 고친 적이 없다(다른 대화·main의 문서): 머리의 반영 표지가 이 문서 일이 아님을 보인다
  const foreign = !!branch && !!viewing && !branch.changed.some((c) => c.path === viewing)
    && !branch.events.some((e) => (e.kind === "save" || e.kind === "create" || e.kind === "edit") && e.text === viewing);
  const uploadTo = project ?? thread?.project ?? null;
  const marks = useMemo(() => branch ? designMarks(branch.events, branch.slug).map((m) => {
    const Icon = MARK_ICON[m.kind] ?? FileText;
    return { key: m.key, after: m.after, node: <span className={`design-mark-chip is-${m.kind}`} data-testid="design-mark"><Icon aria-hidden="true" />{m.text}</span> };
  }) : [], [branch]);
  const reopen = () => {
    if (!branch) return;
    call({ op: "design_branch_open", repo: repo.repo, slug: branch.slug }).then(() => { onError(null); onChanged(); },
      (e: unknown) => onError(e instanceof ApiError ? e.message : String(e)));
  };
  return (
    <div className="design-chat" data-testid="design-chat" data-statistics={branch?.read ? "ready" : "pending"}>
      <header className="design-chat-head">
        <div className="design-pick">
          <button ref={menuButton} type="button" className="design-pick-btn" aria-haspopup="menu" aria-expanded={menu} data-testid="design-pick" onClick={() => setMenu((v) => !v)}>
            <MessageSquare className="i" aria-hidden="true" /><strong>{branch?.title ?? "기획 대화"}</strong><ChevronDown className="i" aria-hidden="true" />
          </button>
          {branch && <span className="design-branch-chip" data-testid="design-chat-branch"><GitBranch aria-hidden="true" />{branch.slug}</span>}
          {menu && (
            <div ref={menuSurface} className="design-pick-menu" role="menu" data-testid="design-pick-menu">
              <p className="design-pick-label">{repo.repo} 기획 대화</p>
              {repo.branches.map((b) => (
                <button key={b.slug} type="button" role="menuitem" className={`design-pick-row${b.slug === branch?.slug ? " is-current" : ""}`} data-testid="design-pick-row"
                  onClick={() => { setMenu(false); onPick(b.slug); }}>
                  <MessageSquare className="i" aria-hidden="true" />
                  <span className="design-pick-text"><strong>{b.title}</strong><span>design/{b.slug} · {branchState(b, threads, now)} · 고친 문서 {b.changed.length}</span></span>
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
        {branch && branch.state === "open" && (merged
          ? <span className="design-merged" data-testid="design-merged"><Check className="i" aria-hidden="true" />main에 들어감</span>
          : applying && apply
            ? <a className="design-applying" data-testid="design-applying" href={`#thread=${encodeURIComponent(apply.id)}`}>반영 대기 <code>{apply.id}</code></a>
            : waiting
              ? <button type="button" className="design-applying is-wait" data-testid="design-apply-wait" title={waiting.reason} onClick={() => setDialog("apply")}><Clock className="i" aria-hidden="true" />예약됨 · 자리 나면 시작</button>
              : canControl && <button type="button" className="btn primary design-apply" data-testid="design-apply" onClick={() => setDialog("apply")}><GitMerge className="i" aria-hidden="true" />main에 반영</button>)}
        {branch && (
          <div className="design-more">
            <button type="button" className="btn icon" aria-label="대화 메뉴" aria-expanded={more} data-testid="design-chat-more" onClick={() => setMore((v) => !v)}><Ellipsis className="i" aria-hidden="true" /></button>
            {more && (
              <div className="design-pick-menu is-right" role="menu">
                {thread && <a role="menuitem" className="design-pick-row" href={`#thread=${encodeURIComponent(thread.id)}`} onClick={() => setMore(false)}>thread {thread.id} · {thread.machine}</a>}
                {canControl && <button type="button" role="menuitem" className="design-pick-row is-danger" data-testid="design-discard-open" disabled={!branch.read} onClick={() => { setMore(false); setDialog("discard"); }}><Trash2 className="i" aria-hidden="true" />버리기</button>}
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
        ) : branch.state === "rest" ? (
          <div className="design-rest" data-testid="design-rest">
            <div className="design-rest-head"><span className="design-branch-chip is-dim"><GitBranch aria-hidden="true" />design/{branch.slug}</span><span className="design-rest-days">쉼 {restDays(branch, now)}일</span></div>
            <p>고친 문서 {branch.changed.length} · main보다 {branch.ahead} 앞 · worktree 돌려줌</p>
            {canControl && (
              <div className="design-rest-actions">
                <button type="button" className="btn primary" data-testid="design-reopen" onClick={reopen}><Play className="i" aria-hidden="true" />다시 열기</button>
                <button type="button" className="btn danger" disabled={!branch.read} onClick={() => setDialog("discard")}><Trash2 className="i" aria-hidden="true" />버리기</button>
              </div>
            )}
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
      {branch && branch.state === "open" && (viewing || branch.edited.length > 0 || picked) && (
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
      {branch && branch.state === "open" && canControl && (
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
          // 입력창이 올린 그림(그 branch에 commit한 저장소 경로)은 초안의 `@경로`를 떼어 「(그림: …)」으로 붙인다
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
      {dialog === "apply" && branch && <ApplyDialog repo={repo.repo} branch={branch} canControl={canControl} onClose={() => setDialog(null)} onChanged={onChanged} onDone={() => { setDialog(null); onChanged(); }} />}
      {dialog === "discard" && branch && <DiscardDialog repo={repo.repo} branch={branch} onClose={() => setDialog(null)} onDone={() => { setDialog(null); onChanged(); }} />}
    </div>
  );
}

function ChangedFiles({ branch, folder }: { branch: DesignBranchView; folder: string | null }) {
  const cut = folder ? `${folder}/` : "";
  return (
    <ul className="design-files" data-testid="design-files">
      {branch.changed.map((c) => (
        <li key={c.path} className="design-file-row">
          {c.status === "A" ? <Plus className="i" aria-hidden="true" /> : <FileText className="i" aria-hidden="true" />}
          <code>{c.path.startsWith(cut) ? c.path.slice(cut.length) : c.path}</code>
          <span className="spacer" />
          {c.status === "A" && <span className="design-file-new">새 문서</span>}
          {c.added !== undefined && <span className="design-plus">+{c.added}</span>}
          {c.removed !== undefined && c.status !== "A" && <span className="design-minus">−{c.removed}</span>}
        </li>
      ))}
    </ul>
  );
}

function ApplyDialog({ repo, branch: given, canControl, onClose, onChanged, onDone }: { repo: string; branch: DesignBranchView; canControl: boolean; onClose: () => void; onChanged: () => void; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  // 한도에 걸려 예약한 반영. 방금 예약했으면 답의 올린 판·예약을 덮는다(답에는 고친 파일 목록이 없다: 트리 읽기가 채운다)
  const [reserved, setReserved] = useState<DesignBranchView | null>(null);
  const branch = reserved
    ? { ...given, head: reserved.head, pushed: reserved.pushed, push_error: reserved.push_error, apply_wait: given.apply_wait ?? reserved.apply_wait }
    : given;
  const wait = branch.apply_wait ?? null;
  const pushed = !!branch.head && branch.head === branch.pushed;
  const run = async (req: FrontRequest, then: (r: FrontResponse) => void) => {
    setBusy(true);
    setProblem(null);
    try {
      then(await call(req));
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const submit = () => run({ op: "design_apply", repo, branch: branch.slug }, (r) => {
    if (r.kind === "design_branch" && r.branch.apply_wait) {
      setReserved(r.branch);
      onChanged();
    } else onDone();
  });
  const cancel = () => run({ op: "design_apply_cancel", repo, branch: branch.slug }, () => onDone());
  return (
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}>
      <div className="modal design-dialog" role="dialog" aria-modal="true" aria-label="main에 반영" data-testid="design-apply-dialog">
        <header className="modal-header"><h2 className="modal-title"><GitMerge className="i" aria-hidden="true" />main에 반영 <code>design/{branch.slug} → main</code></h2></header>
        <div className="modal-body">
          <ChangedFiles branch={branch} folder={null} />
          <ul className="design-checks">
            <li className={pushed ? "is-ok" : "is-wait"}>{pushed ? <Check className="i" aria-hidden="true" /> : <GitBranch className="i" aria-hidden="true" />}
              {pushed ? <>origin에 올림 <code>{branch.pushed?.slice(0, 7)}</code></> : "올리지 않은 저장은 먼저 올린다"}</li>
            <li className={branch.push_error ? "is-bad" : "is-ok"}>{branch.push_error ? <X className="i" aria-hidden="true" /> : <Check className="i" aria-hidden="true" />}
              저장소 기획 검사 <code>design_check</code>{branch.push_error && <span className="design-check-why">{branch.push_error}</span>}</li>
            {wait && (
              <li className="is-wait" role="status" data-testid="design-apply-reserved"><Clock className="i" aria-hidden="true" />
                <strong>예약됨 · 자리 나면 시작</strong><span className="design-check-why">{wait.reason}</span></li>
            )}
          </ul>
          {problem && <p className="confirm-error" role="alert">{problem}</p>}
        </div>
        <footer className="modal-footer">
          {wait && canControl && <button type="button" className="btn" disabled={busy} data-testid="design-apply-cancel" onClick={() => void cancel()}><X className="i" aria-hidden="true" />예약 취소</button>}
          <span className="spacer" />
          <button type="button" className="btn" disabled={busy} onClick={onClose}>닫기</button>
          {!wait && canControl && <button type="button" className="btn primary" disabled={busy} data-testid="design-apply-submit" onClick={() => void submit()}><GitMerge className="i" aria-hidden="true" />착지 창 요청</button>}
        </footer>
      </div>
    </div>
  );
}

function DiscardDialog({ repo, branch, onClose, onDone }: { repo: string; branch: DesignBranchView; onClose: () => void; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const submit = async () => {
    if (!branch.read) return;
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
      <div className="modal design-dialog" role="dialog" aria-modal="true" aria-label="기획 대화 버리기" data-testid="design-discard-dialog">
        <header className="modal-header"><h2 className="modal-title is-danger"><Trash2 className="i" aria-hidden="true" />기획 대화 버리기 <code>design/{branch.slug}</code></h2></header>
        <div className="modal-body">
          {branch.read ? <>
            <ChangedFiles branch={branch} folder={null} />
            <p className="design-discard-what"><X className="i" aria-hidden="true" />main에 없는 commit {branch.ahead}개와 branch·worktree를 지움</p>
          </> : <p role="status">branch 통계를 읽는 중</p>}
          {problem && <p className="confirm-error" role="alert">{problem}</p>}
        </div>
        <footer className="modal-footer">
          <span className="spacer" />
          <button type="button" className="btn" disabled={busy} onClick={onClose}>닫기</button>
          <button type="button" className="btn danger" disabled={busy || !branch.read} data-testid="design-discard-submit" onClick={() => void submit()}><Trash2 className="i" aria-hidden="true" />버리기</button>
        </footer>
      </div>
    </div>
  );
}

/** 새 기획 대화: 주제·branch 이름·시작 판·모델 */
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
  const names = ideaModelChoices(owners, repo.machine, "").filter(Boolean);
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
          <label>branch
            <span className="design-slug"><span>design/</span><input className="input" value={name} spellCheck={false} data-testid="design-new-slug"
              onChange={(e) => { setTouched(true); setSlug(e.target.value); }} /></span>
          </label>
          {name !== "" && !SLUG.test(name) && <p className="confirm-error" role="alert">영문 소문자·숫자·-</p>}
          {taken && <p className="confirm-error" role="alert">이미 있는 branch</p>}
          <div className="design-new-row">
            <label>시작<input className="input" value={`main ${repo.tree?.commit.slice(0, 7) ?? ""}`} readOnly /></label>
            <label>모델
              <select className="input" value={profile} data-testid="design-new-model" onChange={(e) => setProfile(e.target.value)}>
                <option value="">프로젝트 기본</option>
                {names.map((n) => <option key={n} value={n}>{n}</option>)}
              </select>
            </label>
          </div>
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
