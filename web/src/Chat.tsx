import { AutoDecisionLines } from "./AutoMode";
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
import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp, Brain, Check, ChevronDown, ChevronRight, Clock, Copy, Eye, Globe, Hammer, LoaderCircle, Minimize2, Search, SquarePen, Terminal, TriangleAlert, Wrench } from "lucide-react";
import { ApiError, call, watchChat } from "./api";
import type { ChatMessage, ChatView, ContextCompaction } from "./gen/proto";
import { compactionText } from "./contextUsage";
import { useChatShared } from "./settings";
import { statusEdgeRead, type AgentStatus } from "./herdr/status.ts";
import { linkFilePaths } from "./FilePathLink";
import { Markdown, OpenLinkContext } from "./Markdown";
import { OpenFileContext } from "./herdr/filePaths.ts";
import { copyText } from "./herdr/clipboard.ts";
import { imagePaths } from "./herdr/composerImages";
import { shouldFoldUserMessage } from "./herdr/userMessageFold.ts";
import { ConversationRefresh } from "./herdr/conversationRefresh.ts";
import { delivered, type PendingSend } from "./ChatComposer";
import { receiptSeen } from "./inputReceipt";
import { chatAnchorTop, chatIsAtEnd, chatRowIds, chatScrollAnchorAt, readChatPosition, rememberChatPosition } from "./chatScroll";
import { unwrapPasted } from "./pastedContent";
import { ChatRow } from "./ChatRow";
import { usePageVisible } from "./pageVisible";
import { mergeChatPage } from "./chatPages";
import { steerLabel, type SteerState } from "./steerQueued";
import { groupChatRows, groupExpanded, readExpandedGroups, rememberExpandedGroups, rowIndexById, summarizeToolGroup, toggleExpanded, toolGroupKind, type ChatDisplayRow, type ToolGroupKind } from "./toolGroups";
import { ChatFindBar, useChatFind, type OlderOutcome } from "./ChatFind";

/** 기록 줄 요소(말마다 하나, 끝에 대기 말). 머리 안내·경고는 줄이 아니다 */
function chatRows(el: HTMLElement): NodeListOf<HTMLElement> {
  return el.querySelectorAll<HTMLElement>(":scope > .chat-transcript > :not(.chat-note):not(.banner)");
}
const SCROLL_KEYS = ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "];

/** 대화 정리 중(plan §8.309): 대화 끝의 구분선. t3code `ContextCompactionTimelineRow`(MessagesTimeline.tsx 1980~2010줄, 가로선·
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

/** 대화가 정리된 자리(plan §8.309, 사용자 10-20 「대화 정리됨 구분선으로 남겨」). herdr-web-ui ChatView.tsx 618~623줄 `chat-compact`
 * 그대로: 조용한 구분선이고 요약은 누르면 펼친다. 기록에 시각이 없어 시각은 없다. 요약이 없는(Codex) 정리는 구분선만이다 */
function CompactedRow({ text, sizes }: { text: string; sizes?: ContextCompaction | null }) {
  // 마지막 정리에는 전후 컨텍스트 크기를 붙인다(plan §8.321, t3code 「Context compacted A → B tokens」)
  const delta = sizes ? <span className="chat-compact-sizes" data-testid="chat-compact-sizes"> · {compactionText(sizes.before, sizes.after)}</span> : null;
  if (text.trim() === "")
    return (
      <div className="chat-compact" data-role="compact" role="separator" aria-label="대화 정리됨">
        <span className="chat-compact-head">대화 정리됨{delta}</span>
      </div>
    );
  return (
    <details className="chat-compact" data-role="compact">
      <summary className="chat-compact-head">대화 정리됨{delta}</summary>
      <div className="chat-compact-text"><Markdown>{text}</Markdown></div>
    </details>
  );
}

/** 사람이 친 harness 명령(plan §8.323, herdr-web-ui conversation.ts가 숨기는 명령 echo를 조용한 한 줄로). `/compact`는 정리 자리가 대신한다 */
function CommandRow({ text }: { text: string }) {
  return (
    <div className="chat-msg chat-command" data-role="command" data-testid="chat-command">
      <Terminal aria-hidden="true" />
      <code>{text}</code>
    </div>
  );
}

/** 명령의 결과 글(herdr-web-ui conversation.ts 345~352줄 local_command → notice). 여섯 줄이 넘으면 첫 줄만 보이고 누르면 펼친다 */
function CommandOutputRow({ text }: { text: string }) {
  const lines = text.split("\n");
  if (lines.length <= 6)
    return <div className="chat-msg chat-command-output" data-role="command_output" data-testid="chat-command-output"><pre>{text}</pre></div>;
  return (
    <details className="chat-msg chat-command-output" data-role="command_output" data-testid="chat-command-output">
      <summary>{lines[0]!.trim() || "결과"} <span className="chat-command-more">· {lines.length}줄</span></summary>
      <pre>{text}</pre>
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

// 참고 t3code `toolGroupSummaryIconName`(MessagesTimeline.tsx:3834)
const GROUP_ICONS: Readonly<Record<ToolGroupKind, typeof Terminal>> = {
  command: Terminal, read: Eye, edit: SquarePen, "code-search": Search, search: Globe, other: Wrench, reasoning: Brain, mixed: Hammer,
};

/**
 * 이어진 도구·생각 줄 묶음(plan §8.304, 참고 t3code `WorkGroupHeader`·`ExpandedWorkGroupEntries`): 그림과 요약 한 줄, 누르면 그 줄들을
 * 펼친다. 펼친 목록은 높이 min(18rem, 50dvh) 안에서 굴리고, 그 끝을 보고 있었으면 새 줄을 따라간다(참고 shouldFollowWorkGroupAppend).
 * agent가 작업 중이고 기록 맨 끝 묶음이면 요약 대신 마지막 줄을 빛나게 보인다(참고 `work-live`)
 */
function ToolGroup({ messages, expanded, live, onToggle, children }: {
  messages: ChatMessage[];
  expanded: boolean;
  live: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  const Icon = GROUP_ICONS[toolGroupKind(messages)];
  const last = messages.at(-1)!;
  const label = live ? (last.role === "tool" ? last.text : "생각 중") : summarizeToolGroup(messages);
  const list = useRef<HTMLDivElement>(null);
  // 펼치면 맨 위부터 보인다. 목록이 다 들어가면 끝을 보는 것으로 친다: 그 뒤 붙는 줄을 따라간다(참고 appendState.follow)
  const atEnd = useRef(false);
  useLayoutEffect(() => {
    const el = list.current;
    if (el) atEnd.current = el.scrollHeight - el.clientHeight <= 4;
  }, [expanded]);
  const count = useRef(messages.length);
  useLayoutEffect(() => {
    const el = list.current;
    if (el && messages.length > count.current && atEnd.current) el.scrollTop = el.scrollHeight;
    count.current = messages.length;
  }, [messages.length]);
  const listId = useId();
  return (
    <div className={`chat-tool-group${live ? " is-live" : ""}`} data-role="tools">
      <button type="button" className="chat-tool-group-head" aria-expanded={expanded} aria-controls={expanded ? listId : undefined} onClick={onToggle}>
        <Icon className="chat-tool-group-icon" aria-hidden="true" />
        <span className="chat-tool-group-label">{label}</span>
      </button>
      {expanded && (
        <div ref={list} id={listId} className="chat-tool-group-list" role="region" aria-label="도구 호출" tabIndex={0}
          onScroll={(e) => {
            const el = e.currentTarget;
            atEnd.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 4;
          }}>
          {children}
        </div>
      )}
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
  active,
  text,
  label,
  className = "icon-button chat-copy",
  children,
  onResult,
}: {
  active: boolean;
  text: string;
  label: string;
  className?: string;
  children?: ReactNode;
  onResult: (ok: boolean) => void;
}) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!active) { setCopied(false); return; }
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [active, copied]);
  const copy = async () => {
    const ok = await copyText(text);
    setCopied(ok);
    onResult(ok);
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
function UserMessage({ active, text, pending = false, sending = false }: { active: boolean; text: string; pending?: boolean; sending?: boolean }) {
  const canFold = useMemo(() => shouldFoldUserMessage(text), [text]);
  const [open, setOpen] = useState(false);
  const capped = canFold && !open;
  const body = useRef<HTMLDivElement>(null);
  const [clipped, setClipped] = useState(false);
  useLayoutEffect(() => {
    if (!active) return;
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
  }, [active, capped]);
  // 대화 안 찾기가 잘린 아래의 찾음으로 갈 때 편다(ChatFind.tsx, 참고 t3code `data-thread-find-fold`·beforematch)
  useEffect(() => {
    const node = body.current;
    if (!active || !capped || node === null) return;
    const reveal = () => setOpen(true);
    node.addEventListener("beforematch", reveal);
    return () => node.removeEventListener("beforematch", reveal);
  }, [active, capped]);
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
    <div ref={card} className={`chat-msg user chat-bubble${capped ? " is-capped" : ""}${clipped ? " is-folded" : ""}${pending ? " is-pending" : ""}${sending ? " is-sending" : ""}`} data-role="user">
      <div ref={body} className="chat-bubble-body" data-chat-find-text="" data-chat-find-fold={capped ? "" : undefined}>
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

/** 대기 말(보냈지만 기록에 사람 글로 오르지 않은 글, plan §8.247·t-1069)의 밑줄: 어디까지 갔는지, 기록에 없으면 터미널을 보라고.
 * `stage`는 이 화면이 띄운 말의 단계이고(ChatComposer `PendingSend`), `queued`는 기록의 harness 대기열 말(Claude)이다 */
interface Pending {
  harness: "claude" | "codex";
  stage: PendingSend["stage"] | "queued";
  /** agent가 작업 중: 전달된 말은 지금 턴 뒤에 들어간다 */
  working: boolean;
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

/** 말풍선 밑 표지: 아이콘·글·경고 여부. 참고 Telegram·WhatsApp의 시계(보내는 중) → 체크(서버 받음), Slack의 흐린 보내는 중 */
function pendingLook({ stage, working, stale }: Pick<Pending, "stage" | "working" | "stale">): { icon: ReactNode; text: string; warn?: boolean } {
  if (stale) return { icon: <TriangleAlert aria-hidden="true" />, text: "대화 기록에 없습니다 · 터미널 입력창을 확인하세요", warn: true };
  if (stage === "sending") return { icon: <LoaderCircle className="chat-pending-spin" aria-hidden="true" />, text: "보내는 중" };
  if (stage === "stuck") return { icon: <TriangleAlert aria-hidden="true" />, text: "전달 확인이 늦습니다 · 입력창의 ×로 멈출 수 있습니다", warn: true };
  if (stage === "unknown") return { icon: <Clock aria-hidden="true" />, text: "새로고침 전에 보낸 글 · 기록을 기다립니다" };
  if (stage === "queued" || working) return { icon: <Clock aria-hidden="true" />, text: "대기 중 · 지금 작업이 끝나면 들어갑니다" };
  return { icon: <Check aria-hidden="true" />, text: "전달됨 · 기록을 기다립니다" };
}

function PendingTag({ harness, stage, working, stale, onDismiss, steer }: Pending) {
  const busy = steer !== undefined && (steer.state.phase === "sending" || steer.state.phase === "sent");
  const failed = steer?.state.phase === "failed" ? steer.state : null;
  const look = pendingLook({ stage, working, stale });
  return (
    <>
      <div className={`chat-pending-tag${look.warn ? " is-stale" : ""}`} data-testid="chat-pending" data-stage={stage} data-stale={stale || undefined} role="status">
        {look.icon}
        <span>{look.text}</span>
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
function UserTurn({ active, text, pane, onOpenFile, pending }: { active: boolean; text: string; pane: string; onOpenFile?: (path: string) => void; pending?: Pending }) {
  const { note, onResult } = useCopyResult();
  return (
    <article className="chat-turn chat-turn-user">
      {text.trimStart().startsWith("[aaa inbox] ") && <div className="chat-inbox-label" data-testid="chat-inbox-label">aaa inbox 안내</div>}
      {onOpenFile && <UserImages pane={pane} text={text} onOpenFile={onOpenFile} />}
      <div className="chat-user-row">
        <UserMessage active={active} text={text} pending={pending !== undefined} sending={pending?.stage === "sending" || pending?.stage === "stuck"} />
        <div className="chat-turn-meta">
          <CopyButton active={active} text={text} label="메시지 복사" onResult={onResult} />
        </div>
      </div>
      {pending && <PendingTag {...pending} />}
      {note}
    </article>
  );
}

// 참고 ChatView UserImages·ImageThumb. @는 첨부 표시이고 실제 파일 경로에 속하지 않는다.
// URL은 같은 pane의 기존 파일 API가 준 capability만 쓴다(머신·file_roots·인증은 그대로).
function UserImages({ pane, text, onOpenFile }: { pane: string; text: string; onOpenFile: (path: string) => void }) {
  const paths = useMemo(() => imagePaths(text), [text]);
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
function AgentTurn({ active, text }: { active: boolean; text: string }) {
  const { note, onResult } = useCopyResult();
  return (
    <article className="chat-turn chat-turn-agent">
      <div className="chat-msg assistant" data-role="assistant" data-chat-find-text="">
        <Markdown>{text}</Markdown>
      </div>
      <div className="chat-turn-meta chat-agent-meta">
        <CopyButton active={active} className="chat-meta-btn" text={text} label="마크다운으로 복사" onResult={onResult}>
          <span className="chat-meta-fmt">MD</span>
        </CopyButton>
        <CopyButton active={active} className="chat-meta-btn chat-meta-plain" text={plainText(text)} label="일반 텍스트로 복사" onResult={onResult}>
          <span className="chat-meta-fmt">TXT</span>
          <span className="chat-meta-word">일반 텍스트</span>
        </CopyButton>
      </div>
      {note}
    </article>
  );
}

export function ChatPanel({
  active = true,
  onBytes,
  coordinator = false,
  pane,
  status,
  compacting = false,
  compaction = null,
  readNow = 0,
  onOpenFile,
  onOpenLink,
  onMessages,
  pending: pendingSends = [],
  harness = "claude",
  onDismissPending,
  steer,
  findRequest = 0,
}: {
  active?: boolean;
  onBytes?: (bytes: number) => void;
  coordinator?: boolean;
  pane: string;
  status?: AgentStatus;
  /** agent가 대화를 정리하는 중(plan §8.309): 대화 끝에 「대화 정리 중…」 줄 */
  compacting?: boolean;
  /** 이 기록의 마지막 정리 전후 컨텍스트 크기(plan §8.321): 마지막 「대화 정리됨」 줄에 붙인다 */
  compaction?: ContextCompaction | null;
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
  /** 머리 돋보기·팔레트 「대화에서 찾기」가 올린다: 바뀌면 찾기 칸을 연다(plan §8.308) */
  findRequest?: number;
}) {
  const visible = usePageVisible();
  const reading = active && visible;
  const readingRef = useRef(reading);
  readingRef.current = reading;
  const [chat, setChat] = useState<ChatView | null>(null);
  // 기록을 그리는 commit에서 이미 수신된 낙관적 말을 뺀다. Composer의 상태 정리 effect까지
  // 기다리면 기록 말과 낙관적 말이 한 frame 함께 나타난다. 수신 추적·저장은 Composer가 계속 맡는다.
  const pending = useMemo(() => pendingSends.filter(p => !receiptSeen(p, chat?.messages ?? null)), [pendingSends, chat]);
  const [error, setError] = useState<string | null>(null);
  const chatRef = useRef<ChatView | null>(null);
  const epoch = useRef(0);
  const olderBusy = useRef(false);
  const [olderState, setOlderState] = useState<"idle" | "loading" | "failed">("idle");
  const commitPage = (page: ChatView, older = false) => {
    const previous = chatRef.current?.history;
    // 새 파일에는 이전 대화의 기준 줄·앞 쪽 위치가 없다. 맨 위를 읽던 상태까지 넘기면
    // 최신 쪽을 받자마자 앞 쪽을 자동으로 채워 가상화된 최신 본문을 다시 숨긴다.
    // 돌아오는 중 파일이 바뀐 경우도 같은 경계다. 같은 파일의 append·prepend는 자리를 지킨다.
    if (page.history && ((previous && previous !== page.history)
      || (restoring.current && restoreFrom && !restoreFrom.rowId.startsWith(`${page.history}:`)))) {
      cancelRestore.current?.();
      restoring.current = false;
      setPositioning(false);
      anchor.current = null;
      atBottom.current = true;
      hideJump();
    }
    const next = mergeChatPage(chatRef.current, page, older);
    chatRef.current = next;
    setChat(next);
    onBytes?.(new TextEncoder().encode(JSON.stringify(next)).byteLength);
  };
  const list = useRef<HTMLDivElement>(null);
  const touchY = useRef<number | null>(null);
  // 굴림(plan §8.266, t3code ChatView.tsx:4004~4030·4443~4505, MessagesTimeline.tsx:871~955·1112~1141, chatScroll.ts):
  // thread(pane)마다 읽던 자리를 기억한다. 끝을 따라가던 곳이면 끝으로, 아니면 기억한 줄의 그 거리로 돌아오고 「맨 아래로」 단추를
  // 바로 보인다. 이 화면이 보낸 뒤(`readNow`)는 사용자 결정 §8.250대로 끝으로 간다(참고는 보낸 말을 위에 고정하는 anchoring-new-turn)
  const [restoreFrom, setRestoreFrom] = useState(() => {
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
    if (active && statusEdgeRead(seen.current, status)) setPollKey((key) => key + 1);
    seen.current = status;
  }, [active, status]);

  const [refresh] = useState(() => new ConversationRefresh());
  const [pushes, setPushes] = useState(false);
  useEffect(() => {
    chatRef.current = null;
    setChat(null);
    onBytes?.(0);
  }, [pane, showThinking, onBytes]);
  useEffect(() => {
    if (!reading) return;
    let alive = true;
    const generation = ++epoch.current;
    olderBusy.current = false;
    setOlderState("idle");
    refresh.setRead(async () => {
      try {
        const r = await call({ op: "pane_chat", pane, thinking: showThinking, compact: true, commands: true });
        if (!alive || !readingRef.current) return;
        if (r.kind === "chat") {
          // 최신 쪽이 통째로 앞으로 이동했어도 사이 기록을 버리지 않는다(참고 ChatView gap fill).
          const held = chatRef.current;
          let newest: ChatView = r;
          if (r.history && held?.history === r.history) {
            const known = new Set(held.messages.flatMap((m) => m.id ? [m.id] : []));
            let gap = r;
            while (gap.before && !gap.messages.some((m) => m.id && known.has(m.id))) {
              const previous = await call({ op: "pane_chat", pane, thinking: showThinking, compact: true, commands: true, before: gap.before });
              if (!alive || !readingRef.current || epoch.current !== generation) return;
              if (previous.kind !== "chat" || previous.history !== r.history) break;
              if (previous.before === gap.before) throw new Error("앞 기록 위치가 전진하지 않습니다");
              newest = mergeChatPage(newest, previous, true);
              gap = previous;
            }
          }
          // 앞 쪽을 읽다가 pane을 떠났다면 기억한 row까지 다시 채운 뒤 한 번에 복원한다.
          if (restoring.current && restoreFrom?.rowId.startsWith(`${r.history}:`)
            && !held?.messages.some(m => m.id === restoreFrom.rowId)) {
            while (newest.before && !newest.messages.some((m) => m.id === restoreFrom.rowId)) {
              const previous = await call({ op: "pane_chat", pane, thinking: showThinking, compact: true, commands: true, before: newest.before });
              if (!alive || !readingRef.current || epoch.current !== generation) return;
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
  }, [reading, pane, showThinking, refresh, onMessages, onBytes]);
  // 처음·보낸 뒤·턴 시작과 끝은 읽기를 끊지 않고 바로 읽는다(읽는 곳은 위 effect가 먼저 둔다)
  useEffect(() => {
    if (reading) refresh.refresh();
  }, [reading, pane, showThinking, pollKey, readNow, refresh]);

  // 앞 쪽 하나를 받아 붙인다. 찾기의 「앞 기록도 찾기」(plan §8.310)는 결과를 보고 다음 쪽을 부른다. 이미 받는 중이면 그 결과를 기다린다
  const olderRun = useRef<Promise<OlderOutcome> | null>(null);
  const pageOlder = (keepFollowing = false): Promise<OlderOutcome> => {
    if (olderBusy.current && olderRun.current) return olderRun.current;
    const run = readOlder(keepFollowing);
    olderRun.current = run;
    return run;
  };
  const loadOlder = () => void pageOlder();
  const readOlder = async (keepFollowing: boolean): Promise<OlderOutcome> => {
    if (!reading) return "gone";
    const before = chatRef.current?.before;
    if (!before || !chatRef.current?.history) return "done";
    const generation = epoch.current;
    olderBusy.current = true;
    // 화면보다 짧은 최신 쪽의 단추로 읽는 사람도 앞 쪽을 보려는 것이다. 찾기가 받는 앞 쪽은 보던 자리(끝 따라가기 포함)를 두고 붙인다
    if (!keepFollowing) atBottom.current = false;
    setOlderState("loading");
    try {
      const response = await refresh.page(async () => {
        if (epoch.current !== generation) throw new Error("이 대화 읽기는 끝났습니다");
        return call({ op: "pane_chat", pane, thinking: showThinking, compact: true, commands: true, before });
      });
      if (!readingRef.current || epoch.current !== generation) return "gone";
      if (response.kind !== "chat" || !response.history) {
        throw new Error("이 host는 앞 기록 읽기를 지원하지 않습니다");
      }
      if (response.history !== chatRef.current?.history) throw new Error("대화 기록이 바뀌었습니다. 대화를 다시 열어 주세요");
      if (response.before === before) throw new Error("앞 기록 위치가 전진하지 않습니다");
      const el = list.current;
      if (el && !atBottom.current) {
        keepAnchor(el);
        const top = el.getBoundingClientRect().top;
        const rows = Array.from(chatRows(el));
        const index = rows.findIndex((row) => row.getBoundingClientRect().bottom > top);
        const row = rows[index];
        const rowId = rowIdsRef.current[index];
        // 안내 아래 첫 줄은 viewport보다 아래일 수 있다. 그 음수 거리도 그대로 보존한다.
        if (row && rowId) anchor.current = { rowId, offsetWithinRow: top - row.getBoundingClientRect().top, scrollOffset: el.scrollTop };
      }
      commitPage(response, true);
      setOlderState("idle");
      setError(null);
      return "loaded";
    } catch (e) {
      if (!readingRef.current || epoch.current !== generation) return "gone";
      setOlderState("failed");
      setError(e instanceof Error ? e.message : String(e));
      return "failed";
    } finally {
      if (epoch.current === generation) olderBusy.current = false;
    }
  };
  // 빈/도구 전용 쪽도 다음 cursor로 진행한다. 짧은 화면은 스크롤 사건이 없어도 앞 쪽을 채운다.
  useEffect(() => {
    const el = list.current;
    if (reading && el && el.scrollTop < 400 && (!atBottom.current || el.scrollHeight <= el.clientHeight) && olderState === "idle" && chat?.before) loadOlder();
  }, [reading, chat, olderState]);

  // 줄 id(chatScroll.ts): React key이자 읽던 자리의 기준이다. 차례 번호를 key로 쓰면 agent가 기록 앞을 자를 때마다 같은 DOM에
  // 다음 말이 들어가 보던 화면이 밀린다(사용자 10-19 16:5x 「위를 보고 있으면 거기에 멈춰 있게」)
  // 이어진 도구·생각 줄은 한 줄(묶음)이다(plan §8.304). 묶음 id는 첫 줄 id고, 찾을 때는 묶음 안 어느 줄 id로도 찾는다(`rowAt`)
  const displayRows = useMemo(() => {
    const messages = chat?.messages ?? [];
    return groupChatRows(messages, chatRowIds(messages));
  }, [chat]);
  const rowIds = useMemo(
    () => [...displayRows.map((row) => row.id), ...pending.map((p) => `pending:${p.id}`)],
    [displayRows, pending],
  );
  const rowAt = useMemo(() => {
    const at = rowIndexById(displayRows);
    pending.forEach((p, n) => at.set(`pending:${p.id}`, displayRows.length + n));
    return at;
  }, [displayRows, pending]);
  const rowAtRef = useRef(rowAt);
  rowAtRef.current = rowAt;
  // 펼친 묶음은 pane마다 기억한다(참고 rememberedPosition.disclosures.workGroups). 펼치고 접어도 보던 줄 고정(holdAnchor)이
  // 머리를 제자리에 두고, 끝을 보던 사람은 끝에 남는다(참고 suspendEndScrollMaintenanceForDisclosure)
  const [expandedGroups, setExpandedGroups] = useState(() => readExpandedGroups(pane));
  const toggleGroup = (row: Extract<ChatDisplayRow, { kind: "tools" }>) => {
    const next = toggleExpanded(row, expandedGroups);
    rememberExpandedGroups(pane, next);
    setExpandedGroups(next);
  };
  // 「지금 보내기」는 대기 묶음 맨 끝 말 하나에 단다(키가 대기열 전부를 보낸다). 기록의 대기 말(Claude) 뒤에 이 화면이 붙든
  // 대기 말이 온다. 기록에 없는(stale) 말은 대기열에 있는지 모르므로 세지 않는다
  const steerTarget = useMemo(() => {
    const fresh = pending.filter(delivered);
    const messages = chat?.messages ?? [];
    let lastQueued = -1;
    messages.forEach((m, i) => { if (m.pending) lastQueued = i; });
    const count = messages.filter((m) => m.pending).length + fresh.length;
    if (count === 0) return null;
    const last = fresh.at(-1);
    return { count, row: last ? `pending-${last.id}` : `message-${lastQueued}` };
  }, [active, chat, pending]);
  const steerFor = (row: string): SteerView | undefined =>
    steer && steerTarget?.row === row ? { ...steer, count: steerTarget.count } : undefined;
  const rowIdsRef = useRef(rowIds);
  rowIdsRef.current = rowIds;
  // 가상화 판정은 화면 줄(묶음은 한 줄) 수로 한다. row와 끝 이동이 같은 판정을 쓴다(t-0474)
  const virtual = displayRows.length > 300;
  // 대화 안 찾기(plan §8.308): 받아 둔 기록을 센다. 가상화로 화면 밖인 줄은 그 자리로 굴려 꺼낸다. 사람 말·답은 묶이지 않아
  // 그 말의 id가 곧 화면 줄 id다(도구 묶음은 찾지 않는다)
  const chatMessages = useMemo(() => chat?.messages ?? [], [chat]);
  const messageRowIds = useMemo(() => chatRowIds(chatMessages), [chatMessages]);
  const find = useChatFind({ enabled: active, container: list, messages: chatMessages, rowIds: messageRowIds, canOpen: onOpenFile !== undefined, openRequest: findRequest,
    more: Boolean(chat?.before || chat?.truncated), loadEarlier: chat?.history && chat.before ? () => pageOlder(true) : null });
  // 맨 아래가 아니면 보던 줄과 그 줄 안 거리를 지킨다(plan §8.262, t3code timelineScrollAnchoring `rowId`·`offsetWithinRow`):
  // 새 말·앞 자름·입력창 높이·폭이 바뀌어도 그리기 전에 그 줄을 같은 거리로 되돌린다. 브라우저 굴림 고정(overflow-anchor)은 끈다
  // (iOS Safari에는 없고, 이것과 겹치면 두 번 옮긴다). 같은 자리를 이 pane의 읽던 자리로도 기억한다(plan §8.266, 참고 MessagesTimeline
  // handleScroll의 rememberTimelinePosition). 기억한 자리로 돌아가는 동안은 둘 다 쉰다
  const anchor = useRef<{ rowId: string; offsetWithinRow: number; scrollOffset: number } | null>(null);
  const keepAnchor = (el: HTMLDivElement) => {
    if (restoring.current) return;
    const rows = chatRows(el);
    const ids = rowIdsRef.current;
    if (rows.length !== ids.length) return;
    const base = el.getBoundingClientRect().top - el.scrollTop;
    const topAt = (index: number) => rows[index]!.getBoundingClientRect().top - base;
    const at = chatScrollAnchorAt(ids, topAt, el.scrollTop);
    // 맨 위 안내를 읽을 때 첫 메시지까지의 빈 거리도 보존한다.
    if (at && at.rowId === ids[0] && el.scrollTop < topAt(0)) at.offsetWithinRow = el.scrollTop - topAt(0);
    anchor.current = at ?? null;
    if (at) rememberChatPosition(pane, { ...at, atEnd: atBottom.current || (chatIsAtEnd(el) && !el.style.getPropertyValue("--chat-tail-space")) }, atBottom.current);
  };
  // 같은 commit의 행별 초기 실측은 같은 shell 목록을 쓴다. 본문 mount/가림은 shell을 바꾸지 않는다.
  // prepend·기록 교체·대기 말 변경 때 rowIds가 바뀌므로 DOM mutation 뒤 처음 한 번만 다시 조회한다.
  const anchorRows = useRef<{ ids: readonly string[]; el: HTMLDivElement; rows: NodeListOf<HTMLElement> } | null>(null);
  const holdAnchor = (el: HTMLDivElement) => {
    const at = anchor.current;
    if (at === null || restoring.current) return;
    // 자체 쓰기는 곧바로 되읽어 lastScrollTop에 둔다. 1px도 실제 이동이므로 허용 오차로 덮지 않는다.
    // scroll 사건보다 먼저 ResizeObserver가 와도 아직 반영 안 된 손의 이동을 지킨다.
    if (el.scrollTop !== lastScrollTop.current) return;
    const index = rowAtRef.current.get(at.rowId) ?? -1;
    if (anchorRows.current?.ids !== rowIdsRef.current || anchorRows.current.el !== el)
      anchorRows.current = { ids: rowIdsRef.current, el, rows: chatRows(el) };
    const row = index < 0 ? undefined : anchorRows.current.rows[index];
    if (row === undefined) return;
    const top = row.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop;
    const target = chatAnchorTop(top, index === 0, at);
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
    if (!active || !layoutActive.current || !el || el.scrollTop !== lastScrollTop.current) return;
    if (atBottom.current) scrollToEnd(false);
    else holdAnchor(el);
  };
  // 한 페이지의 행 높이는 같은 ResizeObserver 묶음에서 모두 잰 뒤 보던 줄을 한 번 맞춘다.
  // 행마다 전체 목록을 찾고 scrollTop을 쓰면 앞 페이지 commit 중 layout을 반복 강제한다.
  const rowSizes = useRef(new Map<Element, () => void>());
  const rowObserver = useRef<ResizeObserver | null>(null);
  const observeRowSize = useCallback((el: HTMLDivElement, measure: () => void) => {
    const observer = rowObserver.current ??= new ResizeObserver(entries => {
      for (const entry of entries) rowSizes.current.get(entry.target)?.();
      resizeRow.current();
    });
    rowSizes.current.set(el, measure);
    observer.observe(el);
    // mount·가상 본문 복귀 직후의 실측은 같은 layout 단계에서 보정해야 한다.
    // 첫 observer 통지까지 미루면 그 사이 scroll 사건이 중간 위치를 새 기준으로 기억한다.
    resizeRow.current();
    return () => {
      rowSizes.current.delete(el);
      observer.unobserve(el);
      if (rowSizes.current.size === 0) {
        observer.disconnect();
        rowObserver.current = null;
      }
    };
  }, []);
  const layoutActive = useRef(false);
  useLayoutEffect(() => {
    const el = list.current;
    if (!active || !el) return;
    // 받아 둔 과거 위치도 기존 두 frame 복원으로 지킨다. 가상 row의 재측정 중간 위치를 완료로 알리지 않는다.
    if (chatRef.current && !atBottom.current && anchor.current) {
      restoring.current = true;
      setPositioning(true);
      setRestoreFrom({ ...anchor.current, atEnd: false });
    }
    // display:none 동안 브라우저가 scrollTop을 줄여도 마지막으로 읽던 위치가 기준이다.
    el.scrollTop = lastScrollTop.current;
    lastScrollTop.current = el.scrollTop;
    if (!restoring.current) {
      if (atBottom.current) scrollToEnd(false);
      else holdAnchor(el);
    }
    layoutActive.current = true;
    return () => {
      layoutActive.current = false;
      if (jumpTimer.current !== null) window.clearTimeout(jumpTimer.current);
      jumpTimer.current = null;
    };
  }, [active]);
  // 기억한 자리로(참고 MessagesTimeline 871~955줄): 기록을 처음 그린 뒤 그 줄 위 + 거리로 굴리고, 그림·글꼴로 높이가 바뀌어도
  // 두 frame 동안 같을 때까지 맞춘다. 줄이 없어졌으면 굴림 거리로. 손이 굴리면(휠·끌기·누름·굴림 키) 그 자리에서 그친다
  useLayoutEffect(() => {
    const el = list.current;
    if (!active || el === null || !restoring.current || restoreFrom === null || chat === null) return;
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
    const index = rowAtRef.current.get(restoreFrom.rowId) ?? -1;
    const target = () => {
      if (anchorRows.current?.ids !== rowIdsRef.current || anchorRows.current.el !== el)
        anchorRows.current = { ids: rowIdsRef.current, el, rows: chatRows(el) };
      const row = index < 0 ? undefined : anchorRows.current.rows[index];
      if (row === undefined) return restoreFrom.scrollOffset;
      const top = el.scrollTop + row.getBoundingClientRect().top - el.getBoundingClientRect().top;
      const offset = chatAnchorTop(top, index === 0, restoreFrom);
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
        anchor.current = { rowId: restoreFrom.rowId, offsetWithinRow: restoreFrom.offsetWithinRow, scrollOffset: el.scrollTop };
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
  }, [active, chat, restoreFrom]);
  /** 끝으로 가서 따라간다(참고 scrollToEnd). 단추는 부드럽게, 보낸 뒤는 바로 */
  const scrollToEnd = (animated: boolean) => {
    cancelRestore.current?.();
    atBottom.current = true;
    hideJump();
    const el = list.current;
    if (el !== null) {
      el.style.removeProperty("--chat-tail-space");
      // 가상 row는 이동하면서 추정 높이가 실측 높이로 바뀐다. WebKit의 smooth 이동 중
      // row 실측이 즉시 끝 보정을 끼우면 옛 애니메이션이 다시 위로 옮겨 따라가기를 잃는다.
      // 목적지가 계속 바뀌는 긴 대화는 바로 끝으로 간 뒤 실측마다 끝을 맞춘다.
      el.scrollTo({ top: el.scrollHeight, behavior: animated && !virtual ? "smooth" : "auto" });
      lastScrollTop.current = el.scrollTop;
    }
  };
  // 이 화면이 보낸 뒤는 끝으로(사용자 결정 §8.250 「입력 보낸 뒤에는 맨 아래로」). 처음 값은 지난 보내기다
  const sentSeen = useRef(readNow);
  useLayoutEffect(() => {
    if (!active || readNow === sentSeen.current) return;
    sentSeen.current = readNow;
    scrollToEnd(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 보낸 때만
  }, [active, readNow]);

  // 맨 아래를 보고 있었으면 새 줄이 오면 따라 내려간다(그리기 전에, 참고 herdr-web-ui ChatView.tsx useLayoutEffect). 아니면 보던 줄을 지킨다
  useLayoutEffect(() => {
    const el = list.current;
    if (!active || el === null) return;
    // 기록이 짧아져 브라우저가 옛 위치를 새 끝으로 줄인 것은 손의 이동이 아니다.
    // 늦은 scroll 사건이 앞 쪽 읽기 중 따라가기를 다시 켜지 않게, 그리기 직후 반영한다.
    const end = Math.max(0, el.scrollHeight - el.clientHeight);
    if (lastScrollTop.current > end && Math.abs(el.scrollTop - end) <= 1) lastScrollTop.current = el.scrollTop;
    if (el.scrollTop !== lastScrollTop.current) return;
    if (atBottom.current) {
      el.style.removeProperty("--chat-tail-space");
      el.scrollTop = el.scrollHeight;
      lastScrollTop.current = el.scrollTop;
    }
    else holdAnchor(el);
  }, [active, chat, pending]);
  // 입력창이 커지거나(여러 줄·보내기 상태·알림) 키보드가 오르거나 창이 좁아지면 보기가 굴림 사건 없이 줄어든다. 폭·글자가 바뀌어
  // 대화가 다시 접혀도 그렇다. 맨 아래를 보던 사람은 바로 맨 아래에 남는다. 높이만 는 것(접힌 말 펼침)은 손이 한 일이라 따라가지 않는다
  // (참고 herdr-web-ui `aa624cb` ChatView.tsx ResizeObserver, plan §8.250)
  useEffect(() => {
    const el = list.current;
    if (!active || el === null) return;
    let lane = "";
    const observer = new ResizeObserver((entries) => {
      if (el.scrollTop !== lastScrollTop.current) return;
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
  }, [active]);

  return (
    <OpenFileContext.Provider value={onOpenFile ?? null}>
      <OpenLinkContext.Provider value={onOpenLink ?? null}>
      <div className="chat-frame">
      <ChatFindBar {...find.bar} />
      <div
        className="chat"
        ref={list}
        data-testid={active ? "chat" : undefined}
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
          if (!active || !layoutActive.current || el.clientHeight === 0) return;
          const manual = el.scrollTop !== lastScrollTop.current;
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
          if (manual || atBottom.current || el.scrollTop === 0) keepAnchor(el);
          if (el.scrollTop < 400 && olderState === "idle") loadOlder();
        }}
      >
        <div className="chat-transcript">
          <p className="chat-note" data-testid={active ? "chat-history-start" : undefined}>
            {chat?.history && !chat.before ? "처음이다" : chat?.before ? (
              <button type="button" className="chat-older" disabled={olderState === "loading"} onClick={loadOlder}>
                {olderState === "loading" ? "앞 기록 불러오는 중…" : olderState === "failed" ? "앞 기록 다시 불러오기" : "앞 기록 더 보기"}
              </button>
            ) : chat?.truncated ? "앞 기록을 읽지 못함(host·agent가 옛 판)" : null}
          </p>
          {error && <div className="banner warn">{error}</div>}
          {chat && displayRows.map((row, n) => {
            let lastCompact = chat.messages.length - 1;
            while (lastCompact >= 0 && chat.messages[lastCompact]!.role !== "compact") lastCompact--;
            const message = (i: number, key: string) => {
              const m = chat.messages[i]!;
              return m.role === "thinking" ? (
                <ThinkingRow key={key} text={m.text} />
              ) : m.role === "compact" ? (
                <CompactedRow key={key} text={m.text} sizes={i === lastCompact ? compaction : null} />
              ) : m.role === "command" ? (
                <CommandRow key={key} text={m.text} />
              ) : m.role === "command_output" ? (
                <CommandOutputRow key={key} text={m.text} />
              ) : m.role === "user" ? (
                <UserTurn active={active} key={key} text={unwrapPasted(m.text)} pane={pane} onOpenFile={onOpenFile} pending={m.pending ? { harness, stage: "queued", working: status === "working", steer: steerFor(`message-${i}`) } : undefined} />
              ) : m.role === "assistant" ? (
                <AgentTurn active={active} key={key} text={m.text} />
              ) : (
                <div key={key} className={`chat-msg ${m.role}`} data-role={m.role}>
                  {linkFilePaths(m.text, onOpenFile ?? null)}
                </div>
              );
            };
            return (
              <ChatRow active={active} key={row.id} id={row.id} role={row.kind === "tools" ? "tools" : chat.messages[row.index]!.role} virtual={virtual} observeSize={observeRowSize}>
                {row.kind === "tools" ? (
                  <ToolGroup messages={row.indices.map((i) => chat.messages[i]!)} expanded={groupExpanded(row, expandedGroups)}
                    live={status === "working" && n === displayRows.length - 1} onToggle={() => toggleGroup(row)}>
                    {row.indices.map((i, k) => message(i, row.memberIds[k]!))}
                  </ToolGroup>
                ) : message(row.index, row.id)}
              </ChatRow>
            );
          })}
          {coordinator && <AutoDecisionLines />}
          {pending.map((p) => (
            <UserTurn active={active} key={`pending-${p.id}`} text={p.text} pane={pane} onOpenFile={onOpenFile}
              pending={{ harness, stage: p.stage, working: status === "working", stale: p.stale, onDismiss: onDismissPending && (() => onDismissPending(p.id)), steer: steerFor(`pending-${p.id}`) }} />
          ))}
          {chat && chat.messages.length === 0 && pending.length === 0 && !error && !compacting && <p className="chat-note">아직 대화가 없다.</p>}
          {compacting && <CompactingRow />}
        </div>
        {/* 「맨 아래로」(참고 ChatView.tsx:8074 「Scroll to end」): 대화 아래 가운데에 뜬다. 입력창 focus를 뺏지 않는다 */}
        {showJump && (
          <div className="chat-jump">
            <button type="button" className="chat-jump-button" data-testid={active ? "chat-jump" : undefined} onPointerDown={(e) => e.preventDefault()} onClick={() => scrollToEnd(true)}>
              <ArrowDown aria-hidden="true" />
              맨 아래로
            </button>
          </div>
        )}
      </div>
      </div>
      </OpenLinkContext.Provider>
    </OpenFileContext.Provider>
  );
}
