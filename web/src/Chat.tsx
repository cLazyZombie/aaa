// harness 대화 기록 보기(§8.27). 기록은 읽기만 하고 보내는 길은 입력창 ChatComposer(plan §8.130)다. agent가 Claude·Codex 기록에서 골라 준 줄을 보인다.
// 도구 결과 본문은 agent가 빼고 보낸다. 생각은 설정 「생각 표시」(공유)를 켰을 때만 청해 접힌 「생각」 줄로 보인다
// (herdr-web-ui ChatView.tsx `ThinkingRow`, plan §8.129). 다시 읽기(차이 목록 C3, 참고 ChatView·conversationRefresh.ts·status.ts
// `statusEdgeRead`, plan §8.191): host가 기록 변화를 알리면(`chat_changed`, 그 머신 agent가 기록 파일을 본다) 바로 읽고(2초에 한
// 번까지) 놓친 것은 10초마다, host가 알릴 수 없는 pane(옛 agent)이나 연결이 끊긴 동안은 2초마다 읽는다. agent 상태가 작업 중에
// 들고 날 때(턴 시작·끝)와 이 화면이 보낸 뒤(`readNow`)에는 기다리지 않고 바로 읽는다. 가려진 탭은 알림으로 읽지 않는다.
// 글 속 파일 경로는 그 pane의 파일 보기로 연다(참고 ChatView는 pane마다 `OpenFileContext`, 찾는 곳은 그 pane이 지금 있는 폴더,
// plan §8.171). 보기 권한 화면은 열 곳이 없어 글 그대로다. 휴대폰에서는 같은 host 보고서·앱 주소 링크도 새 창 대신 앱 안에서 연다(`onOpenLink`, plan §8.300). 사람·답 말은 참고처럼 Markdown으로 그린다(참고 `UserMessage`의
// `chat-bubble-body`·`Turn`의 답, plan §8.179): 사람 말은 무채색 카드, 답은 카드 없이. 도구 줄은 글 그대로 경로만 잇는다(`linkFilePaths`).
// 말 둘레도 참고대로다(plan §8.182): 대화 기둥 폭(`.chat-transcript`), 긴 사람 말 접기(`UserMessage`, userMessageFold.ts), 사람 말 복사·답의
// 마크다운/일반 텍스트 복사(`CopyButton`, 마우스면 올렸을 때). 참고와 다른 곳: 기록에 시각이 없어 시각은 없고, 접힘은 이 화면 안에서만 기억한다.
// 문구는 참고 i18n.ko.ts.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp, Check, ChevronDown, ChevronRight, Clock, Copy, LoaderCircle, Minimize2, TriangleAlert } from "lucide-react";
import { ApiError, call, watchChat } from "./api";
import type { ChatMessage, ChatView } from "./gen/proto";
import { useChatShared } from "./settings";
import { statusEdgeRead, type AgentStatus } from "./herdr/status.ts";
import { linkFilePaths } from "./FilePathLink";
import { Markdown, OpenLinkContext } from "./Markdown";
import { OpenFileContext } from "./herdr/filePaths.ts";
import { copyText } from "./herdr/clipboard.ts";
import { shouldFoldUserMessage } from "./herdr/userMessageFold.ts";
import { ConversationRefresh } from "./herdr/conversationRefresh.ts";
import type { PendingSend } from "./ChatComposer";
import { chatIsAtEnd, chatRowIds, chatScrollAnchor, readChatPosition, rememberChatPosition } from "./chatScroll";
import { unwrapPasted } from "./pastedContent";
import { ChatRow } from "./ChatRow";
import { mergeChatPage } from "./chatPages";
import { steerLabel, type SteerState } from "./steerQueued";

/** 기록 줄 요소(말마다 하나, 끝에 대기 말). 머리 안내·경고는 줄이 아니다 */
function chatRows(el: HTMLElement): NodeListOf<HTMLElement> {
  return el.querySelectorAll<HTMLElement>(":scope > .chat-transcript > :not(.chat-note):not(.banner)");
}
const SCROLL_KEYS = ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "];

/** 대화 정리 중(plan §8.306): 대화 끝의 구분선. t3code `ContextCompactionTimelineRow`(MessagesTimeline.tsx 1980~2010줄, 가로선·
 * Minimize2·「Compacting context」 반짝임)를 옮겼다. 굴림 기준 줄(`chatRows`)에 들지 않게 chat-note다: 정리가 끝나면 사라진다 */
function CompactingRow() {
  return (
    <div className="chat-note chat-compacting" role="status" data-testid="chat-compacting">
      <span className="chat-compacting-rule" aria-hidden="true" />
      <span className="chat-compacting-label">
        <Minimize2 aria-hidden="true" />
        대화 정리 중…
      </span>
      <span className="chat-compacting-rule" aria-hidden="true" />
    </div>
  );
}

/** 대화가 정리된 자리(plan §8.306, 사용자 10-20 「대화 정리됨 구분선으로 남겨」). herdr-web-ui ChatView.tsx 618~623줄 `chat-compact`
 * 그대로: 조용한 구분선이고 요약은 누르면 펼친다. 기록에 시각이 없어 시각은 없다. 요약이 없는(Codex) 정리는 구분선만이다 */
function CompactedRow({ text }: { text: string }) {
  if (text.trim() === "")
    return (
      <div className="chat-compact" data-role="compact" role="separator" aria-label="대화 정리됨">
        <span className="chat-compact-head">대화 정리됨</span>
      </div>
    );
  return (
    <details className="chat-compact" data-role="compact">
      <summary className="chat-compact-head">대화 정리됨</summary>
      <div className="chat-compact-text"><Markdown>{text}</Markdown></div>
    </details>
  );
}

/** 참고 `ThinkingRow`: 「생각」 머리만 보이고 누르면 펼친다 */
function ThinkingRow({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="chat-msg thinking work-row-thinking" data-role="thinking">
      <button type="button" className="work-row-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="work-row-caret" aria-hidden="true">
          {open ? <ChevronDown /> : <ChevronRight />}
        </span>
        <span className="work-row-name">생각</span>
      </button>
      {open && <div className="work-thinking-text">{text}</div>}
    </div>
  );
}

/** 참고 `plainText`: 답을 일반 텍스트로 복사할 때 Markdown 표시를 뗀다 */
function plainText(markdown: string): string {
  return markdown
    .replace(/```[^\n]*\n([\s\S]*?)```/g, "$1")
    .replace(/\[([^\]]+)\]\((?:https?:\/\/|mailto:)[^)]+\)/gi, "$1")
    .replace(/(?:\*\*|__|~~|`)(.*?)(?:\*\*|__|~~|`)/g, "$1")
    .replace(/^#{1,3}\s+/gm, "")
    .replace(/^>\s?/gm, "");
}

/** 참고 `CopyButton`: 복사하고 잠깐 「복사됨」 */
function CopyButton({
  text,
  label,
  className = "icon-button chat-copy",
  children,
  onResult,
}: {
  text: string;
  label: string;
  className?: string;
  children?: ReactNode;
  onResult: (ok: boolean) => void;
}) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    const ok = await copyText(text);
    setCopied(ok);
    onResult(ok);
    if (ok) window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <button type="button" className={copied ? `${className} is-copied` : className} onClick={() => void copy()} aria-label={copied ? "복사됨" : label} title={copied ? "복사됨" : label}>
      {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
      {children}
    </button>
  );
}

/** 복사가 안 됐을 때 그 말 아래에 한 줄(참고 `copyError`) */
function useCopyResult() {
  const [failed, setFailed] = useState(false);
  const note = failed && (
    <p className="chat-copy-error" role="alert">
      복사하지 못했습니다. 텍스트를 선택해 직접 복사하세요.
    </p>
  );
  return { note, onResult: (ok: boolean) => setFailed(!ok) };
}

/** 참고 `UserMessage`: 긴 말은 몇 줄만 보이고 「전체 메시지 보기」로 펼친다. 잘린 곳은 흐리게 */
function UserMessage({ text, pending = false }: { text: string; pending?: boolean }) {
  const canFold = useMemo(() => shouldFoldUserMessage(text), [text]);
  const [open, setOpen] = useState(false);
  const capped = canFold && !open;
  const body = useRef<HTMLDivElement>(null);
  const [clipped, setClipped] = useState(false);
  useLayoutEffect(() => {
    const node = body.current;
    if (!capped || node === null) {
      setClipped(false);
      return;
    }
    const measure = () => setClipped(node.scrollHeight > node.clientHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [capped]);
  const card = useRef<HTMLDivElement>(null);
  const folding = useRef(false);
  const toggle = () => {
    folding.current = open;
    setOpen(!open);
  };
  // 「접기」는 긴 말 아래에 있다: 접은 뒤 카드 머리가 화면 위로 나갔으면 다시 보이게 한다(참고)
  useLayoutEffect(() => {
    if (!folding.current) return;
    folding.current = false;
    const node = card.current;
    const view = node?.closest(".chat");
    if (node && view && node.getBoundingClientRect().top < view.getBoundingClientRect().top) node.scrollIntoView({ block: "start" });
  }, [open]);
  return (
    <div ref={card} className={`chat-msg user chat-bubble${capped ? " is-capped" : ""}${clipped ? " is-folded" : ""}${pending ? " is-pending" : ""}`} data-role="user">
      <div ref={body} className="chat-bubble-body">
        <Markdown>{text}</Markdown>
      </div>
      {(open || clipped) && (
        <div className="chat-bubble-foot">
          <button type="button" className="chat-bubble-fold" aria-expanded={open} onClick={toggle}>
            {open ? "접기" : "전체 메시지 보기"}
          </button>
        </div>
      )}
    </div>
  );
}

/** 대기 말(작업 중에 보내 harness 대기열에 있는 글, plan §8.247)의 밑줄: 언제 읽히는지, 기록에 없으면 터미널을 보라고 */
interface Pending {
  harness: "claude" | "codex";
  stale?: boolean;
  onDismiss?: () => void;
  /** 대기 묶음 맨 끝 말에만: 「지금 보내기」(plan §8.271) */
  steer?: SteerView;
}

/** 대기 메시지 「지금 보내기」 단추와 그 결과 줄(ChatPanel이 맨 끝 대기 말에 준다) */
export interface SteerView {
  state: SteerState;
  /** 대기 말 수(stale 빼고): 둘 이상이면 「N개 지금 보내기」 */
  count: number;
  /** 끈 까닭(질문 중·다른 글을 보내는 중, 단추 title). null이면 누를 수 있다 */
  blocked: string | null;
  /** 단추 아래 조용한 안내(질문 중). 잠깐 끄는 까닭(보내는 중)은 깜박이지 않게 title로만 둔다 */
  hint: string | null;
  onSteer: () => void;
  onInspect?: () => void;
}

const STEER_CUTS = { claude: "Claude가 하던 일을 끊을 수 있습니다", codex: "Codex가 하던 도구 호출을 끊습니다" };

function PendingTag({ harness, stale, onDismiss, steer }: Pending) {
  const busy = steer !== undefined && (steer.state.phase === "sending" || steer.state.phase === "sent");
  const failed = steer?.state.phase === "failed" ? steer.state : null;
  return (
    <>
      <div className={`chat-pending-tag${stale ? " is-stale" : ""}`} data-testid="chat-pending" data-stale={stale || undefined}>
        {stale ? <TriangleAlert aria-hidden="true" /> : <Clock aria-hidden="true" />}
        <span>{stale ? "대화 기록에 없습니다 · 터미널 입력창을 확인하세요" : "대기 중"}</span>
        {stale && onDismiss && <button type="button" className="chat-pending-dismiss" onClick={onDismiss}>지우기</button>}
        {steer && (
          // 글을 다시 보내지 않고 host가 고른 키만 보낸다(Claude sendNow, Codex Esc). 그 키는 대기열 전부를 보낸다
          <button type="button" className="chat-pending-steer" data-testid="chat-pending-steer" data-phase={steer.state.phase}
            aria-busy={busy || undefined} disabled={busy || steer.blocked !== null}
            aria-label={steer.count > 1 ? `대기 메시지 ${steer.count}개 지금 보내기` : "대기 메시지 지금 보내기"}
            title={steer.blocked ?? `기다리지 않고 지금 읽게 합니다. ${STEER_CUTS[harness]} (빈 입력창에서 Ctrl+Enter와 같음)`}
            onClick={steer.onSteer}>
            {busy ? <LoaderCircle className="chat-pending-steer-spin" aria-hidden="true" /> : <ArrowUp aria-hidden="true" />}
            <span>{busy ? "보내는 중…" : steerLabel(steer.count)}</span>
          </button>
        )}
      </div>
      {steer && (failed || steer.hint) && (
        <div className={`chat-pending-note${failed?.inspect ? "" : " is-quiet"}`} role={failed ? "alert" : "status"} data-testid="chat-pending-note">
          <span className="chat-pending-note-text">{failed?.inspect && <TriangleAlert aria-hidden="true" />}{failed?.text ?? steer.hint}</span>
          {failed?.inspect && steer.onInspect && <button type="button" className="chat-pending-dismiss" onClick={steer.onInspect}>터미널 보기</button>}
        </div>
      )}
    </>
  );
}

/** 사람 말: 카드와 그 복사(마우스면 카드 옆, 터치면 아래) */
function UserTurn({ text, pane, onOpenFile, pending }: { text: string; pane: string; onOpenFile?: (path: string) => void; pending?: Pending }) {
  const { note, onResult } = useCopyResult();
  return (
    <article className="chat-turn chat-turn-user">
      {onOpenFile && <UserImages pane={pane} text={text} onOpenFile={onOpenFile} />}
      <div className="chat-user-row">
        <UserMessage text={text} pending={pending !== undefined} />
        <div className="chat-turn-meta">
          <CopyButton text={text} label="메시지 복사" onResult={onResult} />
        </div>
      </div>
      {pending && <PendingTag {...pending} />}
      {note}
    </article>
  );
}

// 참고 ChatView UserImages·ImageThumb. @는 첨부 표시이고 실제 파일 경로에 속하지 않는다.
// URL은 같은 pane의 기존 파일 API가 준 capability만 쓴다(머신·file_roots·인증은 그대로).
const IMAGE_MENTION = /(?:^|\s)@(\S+\.(?:png|jpe?g|gif|webp))(?=\s|$)/gi;
function UserImages({ pane, text, onOpenFile }: { pane: string; text: string; onOpenFile: (path: string) => void }) {
  const paths = useMemo(() => [...new Set([...text.matchAll(IMAGE_MENTION)].map((m) => m[1]!))], [text]);
  return paths.length === 0 ? null : <div className="chat-user-images">
    {paths.map((path) => <ImageThumb key={path} pane={pane} path={path} onOpenFile={onOpenFile} />)}
  </div>;
}
function ImageThumb({ pane, path, onOpenFile }: { pane: string; path: string; onOpenFile: (path: string) => void }) {
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    void call({ op: "pane_file", pane, path }).then((r) => {
      if (!alive) return;
      if (r.kind !== "pane_file" || r.info?.kind !== "image" || r.url === null || r.info.size > 20 * 1024 * 1024) {
        setError("이미지를 표시할 수 없음");
      } else setUrl(r.url);
    }).catch((e) => { if (alive) setError(e instanceof Error ? e.message : "이미지를 표시할 수 없음"); });
    return () => { alive = false; };
  }, [pane, path]);
  return <button type="button" className="chat-user-image" aria-label="첨부 그림 확대" title={error ?? path}
    disabled={url === null || error !== null} data-state={error ? "error" : url ? "ready" : "loading"}
    onClick={(e) => { e.currentTarget.focus({ preventScroll: true }); onOpenFile(path); }}>
    {url && !error ? <img src={url} alt={path.split(/[\\/]/).at(-1)} loading="lazy" onError={() => setError("이미지를 표시할 수 없음")} />
      : <span role="status">{error ?? "그림 여는 중…"}</span>}
  </button>;
}

/** 답: 카드 없이 Markdown, 그 아래 마크다운·일반 텍스트 복사(참고 `chat-agent-meta`) */
function AgentTurn({ text }: { text: string }) {
  const { note, onResult } = useCopyResult();
  return (
    <article className="chat-turn chat-turn-agent">
      <div className="chat-msg assistant" data-role="assistant">
        <Markdown>{text}</Markdown>
      </div>
      <div className="chat-turn-meta chat-agent-meta">
        <CopyButton className="chat-meta-btn" text={text} label="마크다운으로 복사" onResult={onResult}>
          <span className="chat-meta-fmt">MD</span>
        </CopyButton>
        <CopyButton className="chat-meta-btn chat-meta-plain" text={plainText(text)} label="일반 텍스트로 복사" onResult={onResult}>
          <span className="chat-meta-fmt">TXT</span>
          <span className="chat-meta-word">일반 텍스트</span>
        </CopyButton>
      </div>
      {note}
    </article>
  );
}

export function ChatPanel({
  pane,
  status,
  compacting = false,
  readNow = 0,
  onOpenFile,
  onOpenLink,
  onMessages,
  pending = [],
  harness = "claude",
  onDismissPending,
  steer,
}: {
  pane: string;
  status?: AgentStatus;
  /** agent가 대화를 정리하는 중(plan §8.306): 대화 끝에 「대화 정리 중…」 줄 */
  compacting?: boolean;
  readNow?: number;
  /** 글 속 파일 경로를 연다(조작 기기만) */
  onOpenFile?: (path: string) => void;
  /** 글 속 링크를 앱 안에서 연다(열었으면 true, 아니면 새 창 그대로) */
  onOpenLink?: (href: string) => boolean;
  onMessages?: (messages: ChatMessage[]) => void;
  /** 이 화면이 작업 중에 보내 아직 기록에 없는 글(ChatComposer): 기록 끝에 대기 말로 */
  pending?: PendingSend[];
  harness?: "claude" | "codex";
  onDismissPending?: (id: number) => void;
  /** 대기 메시지 「지금 보내기」(plan §8.271). 없으면 단추를 그리지 않는다(쉬는 agent·보기 권한·옛 host) */
  steer?: Omit<SteerView, "count">;
}) {
  const [chat, setChat] = useState<ChatView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const chatRef = useRef<ChatView | null>(null);
  const epoch = useRef(0);
  const olderBusy = useRef(false);
  const [olderState, setOlderState] = useState<"idle" | "loading" | "failed">("idle");
  const commitPage = (page: ChatView, older = false) => {
    const next = mergeChatPage(chatRef.current, page, older);
    chatRef.current = next;
    setChat(next);
  };
  const list = useRef<HTMLDivElement>(null);
  const touchY = useRef<number | null>(null);
  // 굴림(plan §8.266, t3code ChatView.tsx:4004~4030·4443~4505, MessagesTimeline.tsx:871~955·1112~1141, chatScroll.ts):
  // thread(pane)마다 읽던 자리를 기억한다. 끝을 따라가던 곳이면 끝으로, 아니면 기억한 줄의 그 거리로 돌아오고 「맨 아래로」 단추를
  // 바로 보인다. 이 화면이 보낸 뒤(`readNow`)는 사용자 결정 §8.250대로 끝으로 간다(참고는 보낸 말을 위에 고정하는 anchoring-new-turn)
  const [restoreFrom] = useState(() => {
    const remembered = readChatPosition(pane);
    return remembered?.atEnd === false ? remembered : null;
  });
  const restoring = useRef(restoreFrom !== null);
  const [positioning, setPositioning] = useState(restoreFrom !== null);
  const cancelRestore = useRef<(() => void) | null>(null);
  const atBottom = useRef(restoreFrom === null);
  // 자동 굴림도 실제 위치를 즉시 기억한다. 첫 scroll 사건이 손의 위 이동과 합쳐져도 방향을 놓치지 않는다.
  const lastScrollTop = useRef(0);
  const [showJump, setShowJump] = useState(restoreFrom !== null);
  // 단추는 늦게 보이고 바로 숨는다(참고 showScrollDebouncer 150ms: 자리를 잡는 동안 깜빡이지 않게)
  const jumpTimer = useRef<number | null>(null);
  const hideJump = () => {
    if (jumpTimer.current !== null) window.clearTimeout(jumpTimer.current);
    jumpTimer.current = null;
    setShowJump(false);
  };
  const soonShowJump = () => {
    if (jumpTimer.current !== null) return;
    jumpTimer.current = window.setTimeout(() => {
      jumpTimer.current = null;
      setShowJump(true);
    }, 150);
  };
  useEffect(() => () => {
    if (jumpTimer.current !== null) window.clearTimeout(jumpTimer.current);
  }, []);
  const { showThinking } = useChatShared();
  // 상태가 작업 중에 들고 나면 바로 읽는다(참고 ChatView 882~886줄)
  const [pollKey, setPollKey] = useState(0);
  const seen = useRef(status);
  useEffect(() => {
    if (statusEdgeRead(seen.current, status)) setPollKey((key) => key + 1);
    seen.current = status;
  }, [status]);

  const [refresh] = useState(() => new ConversationRefresh());
  const [pushes, setPushes] = useState(false);
  useEffect(() => {
    let alive = true;
    const generation = ++epoch.current;
    chatRef.current = null;
    setChat(null);
    olderBusy.current = false;
    setOlderState("idle");
    refresh.setRead(async () => {
      try {
        const r = await call({ op: "pane_chat", pane, thinking: showThinking });
        if (!alive) return;
        if (r.kind === "chat") {
          // 최신 쪽이 통째로 앞으로 이동했어도 사이 기록을 버리지 않는다(참고 ChatView gap fill).
          const held = chatRef.current;
          let newest: ChatView = r;
          if (r.history && held?.history === r.history) {
            const known = new Set(held.messages.flatMap((m) => m.id ? [m.id] : []));
            let gap = r;
            while (gap.before && !gap.messages.some((m) => m.id && known.has(m.id))) {
              const previous = await call({ op: "pane_chat", pane, thinking: showThinking, before: gap.before });
              if (!alive || epoch.current !== generation) return;
              if (previous.kind !== "chat" || previous.history !== r.history) break;
              if (previous.before === gap.before) throw new Error("앞 기록 위치가 전진하지 않습니다");
              newest = mergeChatPage(newest, previous, true);
              gap = previous;
            }
          }
          // 앞 쪽을 읽다가 pane을 떠났다면 기억한 row까지 다시 채운 뒤 한 번에 복원한다.
          if (restoring.current && restoreFrom?.rowId.startsWith(`${r.history}:`)) {
            while (newest.before && !newest.messages.some((m) => m.id === restoreFrom.rowId)) {
              const previous = await call({ op: "pane_chat", pane, thinking: showThinking, before: newest.before });
              if (!alive || epoch.current !== generation) return;
              if (previous.kind !== "chat" || previous.history !== r.history) break;
              if (previous.before === newest.before) throw new Error("앞 기록 위치가 전진하지 않습니다");
              newest = mergeChatPage(newest, previous, true);
            }
          }
          commitPage(newest);
          onMessages?.(r.messages);
          setError(null);
        }
      } catch (e) {
        if (alive) setError(e instanceof ApiError ? e.message : String(e));
      }
    });
    const unwatch = watchChat(pane, (note) => {
      if ("live" in note) {
        refresh.setPushes(note.live);
        setPushes(note.live);
      } else if (document.visibilityState !== "hidden") refresh.invalidate();
    });
    return () => {
      alive = false;
      epoch.current++;
      unwatch();
      refresh.stop();
      refresh.setPushes(false);
      setPushes(false);
    };
  }, [pane, showThinking, refresh, onMessages]);
  // 처음·보낸 뒤·턴 시작과 끝은 읽기를 끊지 않고 바로 읽는다(읽는 곳은 위 effect가 먼저 둔다)
  useEffect(() => {
    refresh.refresh();
  }, [pane, showThinking, pollKey, readNow, refresh]);

  const loadOlder = async () => {
    const before = chatRef.current?.before;
    if (!before || !chatRef.current?.history || olderBusy.current) return;
    const generation = epoch.current;
    olderBusy.current = true;
    // 화면보다 짧은 최신 쪽의 단추로 읽는 사람도 앞 쪽을 보려는 것이다.
    atBottom.current = false;
    setOlderState("loading");
    try {
      const response = await refresh.page(async () => {
        if (epoch.current !== generation) throw new Error("이 대화 읽기는 끝났습니다");
        return call({ op: "pane_chat", pane, thinking: showThinking, before });
      });
      if (epoch.current !== generation) return;
      if (response.kind !== "chat" || !response.history) {
        throw new Error("이 host는 앞 기록 읽기를 지원하지 않습니다");
      }
      if (response.history !== chatRef.current?.history) throw new Error("대화 기록이 바뀌었습니다. 대화를 다시 열어 주세요");
      if (response.before === before) throw new Error("앞 기록 위치가 전진하지 않습니다");
      const el = list.current;
      if (el) {
        keepAnchor(el);
        const top = el.getBoundingClientRect().top;
        const rows = Array.from(chatRows(el));
        const index = rows.findIndex((row) => row.getBoundingClientRect().bottom > top);
        const row = rows[index];
        const rowId = rowIdsRef.current[index];
        // 안내 아래 첫 줄은 viewport보다 아래일 수 있다. 그 음수 거리도 그대로 보존한다.
        if (row && rowId) anchor.current = { rowId, offsetWithinRow: top - row.getBoundingClientRect().top };
      }
      commitPage(response, true);
      setOlderState("idle");
      setError(null);
    } catch (e) {
      if (epoch.current !== generation) return;
      setOlderState("failed");
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (epoch.current === generation) olderBusy.current = false;
    }
  };
  // 빈/도구 전용 쪽도 다음 cursor로 진행한다. 짧은 화면은 스크롤 사건이 없어도 앞 쪽을 채운다.
  useEffect(() => {
    const el = list.current;
    if (el && el.scrollTop < 400 && (!atBottom.current || el.scrollHeight <= el.clientHeight) && olderState === "idle" && chat?.before) void loadOlder();
  }, [chat, olderState]);

  // 줄 id(chatScroll.ts): React key이자 읽던 자리의 기준이다. 차례 번호를 key로 쓰면 agent가 기록 앞을 자를 때마다 같은 DOM에
  // 다음 말이 들어가 보던 화면이 밀린다(사용자 10-19 16:5x 「위를 보고 있으면 거기에 멈춰 있게」)
  const rowIds = useMemo(
    () => [...chatRowIds(chat?.messages ?? []), ...pending.map((p) => `pending:${p.id}`)],
    [chat, pending],
  );
  // 「지금 보내기」는 대기 묶음 맨 끝 말 하나에 단다(키가 대기열 전부를 보낸다). 기록의 대기 말(Claude) 뒤에 이 화면이 붙든
  // 대기 말이 온다. 기록에 없는(stale) 말은 대기열에 있는지 모르므로 세지 않는다
  const steerTarget = useMemo(() => {
    const fresh = pending.filter((p) => !p.stale);
    const messages = chat?.messages ?? [];
    let lastQueued = -1;
    messages.forEach((m, i) => { if (m.pending) lastQueued = i; });
    const count = messages.filter((m) => m.pending).length + fresh.length;
    if (count === 0) return null;
    const last = fresh.at(-1);
    return { count, row: last ? `pending-${last.id}` : `message-${lastQueued}` };
  }, [chat, pending]);
  const steerFor = (row: string): SteerView | undefined =>
    steer && steerTarget?.row === row ? { ...steer, count: steerTarget.count } : undefined;
  const rowIdsRef = useRef(rowIds);
  rowIdsRef.current = rowIds;
  // 맨 아래가 아니면 보던 줄과 그 줄 안 거리를 지킨다(plan §8.262, t3code timelineScrollAnchoring `rowId`·`offsetWithinRow`):
  // 새 말·앞 자름·입력창 높이·폭이 바뀌어도 그리기 전에 그 줄을 같은 거리로 되돌린다. 브라우저 굴림 고정(overflow-anchor)은 끈다
  // (iOS Safari에는 없고, 이것과 겹치면 두 번 옮긴다). 같은 자리를 이 pane의 읽던 자리로도 기억한다(plan §8.266, 참고 MessagesTimeline
  // handleScroll의 rememberTimelinePosition). 기억한 자리로 돌아가는 동안은 둘 다 쉰다
  const anchor = useRef<{ rowId: string; offsetWithinRow: number } | null>(null);
  const keepAnchor = (el: HTMLDivElement) => {
    if (restoring.current) return;
    const rows = chatRows(el);
    const ids = rowIdsRef.current;
    if (rows.length !== ids.length) return;
    const base = el.getBoundingClientRect().top - el.scrollTop;
    const tops = Array.from(rows, (row) => row.getBoundingClientRect().top - base);
    const at = chatScrollAnchor(ids, tops, el.scrollTop);
    // 맨 위 안내를 읽을 때 첫 메시지까지의 빈 거리도 보존한다.
    if (at && at.rowId === ids[0] && el.scrollTop < tops[0]!) at.offsetWithinRow = el.scrollTop - tops[0]!;
    anchor.current = at ? { rowId: at.rowId, offsetWithinRow: at.offsetWithinRow } : null;
    if (at) rememberChatPosition(pane, { ...at, atEnd: atBottom.current || (chatIsAtEnd(el) && !el.style.getPropertyValue("--chat-tail-space")) }, atBottom.current);
  };
  const holdAnchor = (el: HTMLDivElement) => {
    const at = anchor.current;
    if (at === null || restoring.current) return;
    // scroll 사건보다 먼저 ResizeObserver가 올 수 있다. 아직 반영 안 된 손의 이동을 덮지 않는다.
    if (Math.abs(el.scrollTop - lastScrollTop.current) > 1) return;
    const index = rowIdsRef.current.indexOf(at.rowId);
    const row = index < 0 ? undefined : chatRows(el)[index];
    if (row === undefined) return;
    const top = row.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop;
    const target = top + at.offsetWithinRow;
    // 짧은 최신 쪽 아래의 빈 자리도 읽던 화면이다(t3code anchored end space).
    const extra = target - (el.scrollHeight - el.clientHeight);
    if (extra > 0) {
      const previous = parseFloat(el.style.getPropertyValue("--chat-tail-space")) || 0;
      el.style.setProperty("--chat-tail-space", `${previous + extra}px`);
    }
    el.scrollTop = target;
    lastScrollTop.current = el.scrollTop;
  };
  const resizeRow = useRef<() => void>(() => {});
  resizeRow.current = () => {
    const el = list.current;
    if (!el || Math.abs(el.scrollTop - lastScrollTop.current) > 1) return;
    if (atBottom.current) scrollToEnd(false);
    else holdAnchor(el);
  };
  const onRowResize = useCallback(() => resizeRow.current(), []);
  // 기억한 자리로(참고 MessagesTimeline 871~955줄): 기록을 처음 그린 뒤 그 줄 위 + 거리로 굴리고, 그림·글꼴로 높이가 바뀌어도
  // 두 frame 동안 같을 때까지 맞춘다. 줄이 없어졌으면 굴림 거리로. 손이 굴리면(휠·끌기·누름·굴림 키) 그 자리에서 그친다
  useLayoutEffect(() => {
    const el = list.current;
    if (el === null || !restoring.current || restoreFrom === null || chat === null) return;
    let frame: number | null = null;
    const finish = () => {
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
      restoring.current = false;
      setPositioning(false);
      cancelRestore.current = null;
      detach();
      // 그친 자리가 보던 줄이 된다
      keepAnchor(el);
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (SCROLL_KEYS.includes(event.key) && !(event.target instanceof Element && event.target.closest("input, textarea, [contenteditable=true]"))) finish();
    };
    const detach = () => {
      el.removeEventListener("wheel", finish);
      el.removeEventListener("touchmove", finish);
      el.removeEventListener("pointerdown", finish);
      el.ownerDocument.removeEventListener("keydown", onKey);
    };
    el.addEventListener("wheel", finish, { passive: true });
    el.addEventListener("touchmove", finish, { passive: true });
    el.addEventListener("pointerdown", finish, { passive: true });
    el.ownerDocument.addEventListener("keydown", onKey);
    cancelRestore.current = finish;
    const index = rowIdsRef.current.indexOf(restoreFrom.rowId);
    const target = () => {
      const row = index < 0 ? undefined : chatRows(el)[index];
      if (row === undefined) return restoreFrom.scrollOffset;
      const offset = el.scrollTop + row.getBoundingClientRect().top - el.getBoundingClientRect().top + restoreFrom.offsetWithinRow;
      return Math.max(0, Math.min(offset, el.scrollHeight - el.clientHeight));
    };
    el.scrollTop = target();
    lastScrollTop.current = el.scrollTop;
    if (index < 0) {
      finish();
      return;
    }
    let stableFrames = 0;
    const reconcile = () => {
      const offset = target();
      if (Math.abs(el.scrollTop - offset) > 1) {
        stableFrames = 0;
        el.scrollTop = offset;
        lastScrollTop.current = el.scrollTop;
      } else if (++stableFrames >= 2) {
        finish();
        // 추정 높이의 주변 줄이 늦게 mount돼도 저장한 row가 계속 기준이다.
        anchor.current = { rowId: restoreFrom.rowId, offsetWithinRow: restoreFrom.offsetWithinRow };
        rememberChatPosition(pane, { ...restoreFrom, scrollOffset: el.scrollTop }, false);
        return;
      }
      frame = requestAnimationFrame(reconcile);
    };
    frame = requestAnimationFrame(reconcile);
    return () => {
      if (frame !== null) cancelAnimationFrame(frame);
      if (cancelRestore.current === finish) cancelRestore.current = null;
      detach();
    };
  }, [chat, restoreFrom]);
  /** 끝으로 가서 따라간다(참고 scrollToEnd). 단추는 부드럽게, 보낸 뒤는 바로 */
  const scrollToEnd = (animated: boolean) => {
    cancelRestore.current?.();
    atBottom.current = true;
    hideJump();
    const el = list.current;
    if (el !== null) {
      el.style.removeProperty("--chat-tail-space");
      el.scrollTo({ top: el.scrollHeight, behavior: animated ? "smooth" : "auto" });
      lastScrollTop.current = el.scrollTop;
    }
  };
  // 이 화면이 보낸 뒤는 끝으로(사용자 결정 §8.250 「입력 보낸 뒤에는 맨 아래로」). 처음 값은 지난 보내기다
  const sentSeen = useRef(readNow);
  useLayoutEffect(() => {
    if (readNow === sentSeen.current) return;
    sentSeen.current = readNow;
    scrollToEnd(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 보낸 때만
  }, [readNow]);

  // 맨 아래를 보고 있었으면 새 줄이 오면 따라 내려간다(그리기 전에, 참고 herdr-web-ui ChatView.tsx useLayoutEffect). 아니면 보던 줄을 지킨다
  useLayoutEffect(() => {
    const el = list.current;
    if (el === null) return;
    // 기록이 짧아져 브라우저가 옛 위치를 새 끝으로 줄인 것은 손의 이동이 아니다.
    // 늦은 scroll 사건이 앞 쪽 읽기 중 따라가기를 다시 켜지 않게, 그리기 직후 반영한다.
    const end = Math.max(0, el.scrollHeight - el.clientHeight);
    if (lastScrollTop.current > end && Math.abs(el.scrollTop - end) <= 1) lastScrollTop.current = el.scrollTop;
    if (Math.abs(el.scrollTop - lastScrollTop.current) > 1) return;
    if (atBottom.current) {
      el.style.removeProperty("--chat-tail-space");
      el.scrollTop = el.scrollHeight;
      lastScrollTop.current = el.scrollTop;
    }
    else holdAnchor(el);
  }, [chat, pending]);
  // 입력창이 커지거나(여러 줄·보내기 상태·알림) 키보드가 오르거나 창이 좁아지면 보기가 굴림 사건 없이 줄어든다. 폭·글자가 바뀌어
  // 대화가 다시 접혀도 그렇다. 맨 아래를 보던 사람은 바로 맨 아래에 남는다. 높이만 는 것(접힌 말 펼침)은 손이 한 일이라 따라가지 않는다
  // (참고 herdr-web-ui `aa624cb` ChatView.tsx ResizeObserver, plan §8.250)
  useEffect(() => {
    const el = list.current;
    if (el === null) return;
    let lane = "";
    const observer = new ResizeObserver((entries) => {
      if (Math.abs(el.scrollTop - lastScrollTop.current) > 1) return;
      let follow = false;
      for (const entry of entries) {
        if (entry.target === el) { follow = true; continue; }
        const style = getComputedStyle(entry.target);
        const next = `${entry.contentRect.width}|${style.fontSize}|${style.fontFamily}`;
        if (next !== lane) { lane = next; follow = true; }
      }
      if (follow && atBottom.current) {
        el.scrollTop = el.scrollHeight;
        lastScrollTop.current = el.scrollTop;
      }
      else if (!atBottom.current) holdAnchor(el);
    });
    observer.observe(el);
    if (el.firstElementChild !== null) observer.observe(el.firstElementChild);
    return () => observer.disconnect();
  }, []);

  return (
    <OpenFileContext.Provider value={onOpenFile ?? null}>
      <OpenLinkContext.Provider value={onOpenLink ?? null}>
      <div
        className="chat"
        ref={list}
        data-testid="chat"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === "End" && !(e.target instanceof Element && e.target.closest("input, textarea, [contenteditable=true]"))) {
            e.preventDefault();
            scrollToEnd(false);
          }
        }}
        onWheel={(e) => {
          if (e.deltaY > 0 && e.currentTarget.style.getPropertyValue("--chat-tail-space") && chatIsAtEnd(e.currentTarget)) scrollToEnd(false);
        }}
        onTouchStart={(e) => { touchY.current = e.touches[0]?.clientY ?? null; }}
        onTouchMove={(e) => {
          const next = e.touches[0]?.clientY;
          if (next !== undefined && touchY.current !== null && next < touchY.current
            && e.currentTarget.style.getPropertyValue("--chat-tail-space") && chatIsAtEnd(e.currentTarget)) scrollToEnd(false);
          touchY.current = next ?? null;
        }}
        aria-busy={chat === null || positioning || olderState === "loading"}
        data-pushes={pushes}
        onScroll={(e) => {
          // 위로 굴린 손만 따라가기를 끈다: 아래로 가는 자동 굴림은 다음 잰 높이보다 먼저 올 수 있다(참고 ChatView.tsx onScroll)
          const el = e.currentTarget;
          const manual = Math.abs(el.scrollTop - lastScrollTop.current) > 1;
          const scrollingUp = el.scrollTop < lastScrollTop.current;
          lastScrollTop.current = el.scrollTop;
          if (manual && (!atBottom.current || scrollingUp)) {
            atBottom.current = chatIsAtEnd(el);
            if (atBottom.current && el.style.getPropertyValue("--chat-tail-space")) {
              el.style.removeProperty("--chat-tail-space");
              el.scrollTop = el.scrollHeight;
              lastScrollTop.current = el.scrollTop;
            }
          }
          // 끝이면 단추를 바로 숨기고, 따라가기를 놓았으면 늦게 보인다(참고 onIsAtEndChange)
          if (atBottom.current) hideJump();
          else soonShowJump();
          // holdAnchor가 낸 scroll은 읽던 줄을 바꾸지 않는다. 가상 row의 실측이 끝날 때까지 같은 줄을 잡는다.
          if (manual || atBottom.current) keepAnchor(el);
          if (el.scrollTop < 400 && olderState === "idle") void loadOlder();
        }}
      >
        <div className="chat-transcript">
          <p className="chat-note">
            {chat?.harness === "codex" ? "Codex" : "Claude"}가 남긴 대화 기록이다. 도구 결과{showThinking ? "는" : "와 생각은"} 빼고 보인다.
            {chat?.truncated && " 앞부분은 생략했다."}
          </p>
          <p className="chat-note" data-testid="chat-history-start">
            {chat?.history && !chat.before ? "처음이다" : chat?.before ? (
              <button type="button" className="chat-older" disabled={olderState === "loading"} onClick={() => void loadOlder()}>
                {olderState === "loading" ? "앞 기록 불러오는 중…" : olderState === "failed" ? "앞 기록 다시 불러오기" : "앞 기록 더 보기"}
              </button>
            ) : chat?.truncated ? "앞 기록을 읽으려면 host와 agent를 갱신하세요." : null}
          </p>
          {error && <div className="banner warn">{error}</div>}
          {chat?.messages.map((m, i) => (
            <ChatRow key={rowIds[i]} id={rowIds[i]!} role={m.role} virtual={chat.messages.length > 300} onResize={onRowResize}>
            {m.role === "thinking" ? (
              <ThinkingRow key={rowIds[i]} text={m.text} />
            ) : m.role === "compact" ? (
              <CompactedRow key={rowIds[i]} text={m.text} />
            ) : m.role === "user" ? (
              <UserTurn key={rowIds[i]} text={unwrapPasted(m.text)} pane={pane} onOpenFile={onOpenFile} pending={m.pending ? { harness, steer: steerFor(`message-${i}`) } : undefined} />
            ) : m.role === "assistant" ? (
              <AgentTurn key={rowIds[i]} text={m.text} />
            ) : (
              <div key={rowIds[i]} className={`chat-msg ${m.role}`} data-role={m.role}>
                {linkFilePaths(m.text, onOpenFile ?? null)}
              </div>
            )}
            </ChatRow>
          ))}
          {pending.map((p) => (
            <UserTurn key={`pending-${p.id}`} text={p.text} pane={pane} onOpenFile={onOpenFile}
              pending={{ harness, stale: p.stale, onDismiss: onDismissPending && (() => onDismissPending(p.id)), steer: steerFor(`pending-${p.id}`) }} />
          ))}
          {chat && chat.messages.length === 0 && pending.length === 0 && !error && !compacting && <p className="chat-note">아직 대화가 없다.</p>}
          {compacting && <CompactingRow />}
        </div>
        {/* 「맨 아래로」(참고 ChatView.tsx:8074 「Scroll to end」): 대화 아래 가운데에 뜬다. 입력창 focus를 뺏지 않는다 */}
        {showJump && (
          <div className="chat-jump">
            <button type="button" className="chat-jump-button" data-testid="chat-jump" onPointerDown={(e) => e.preventDefault()} onClick={() => scrollToEnd(true)}>
              <ArrowDown aria-hidden="true" />
              맨 아래로
            </button>
          </div>
        )}
      </div>
      </OpenLinkContext.Provider>
    </OpenFileContext.Provider>
  );
}
