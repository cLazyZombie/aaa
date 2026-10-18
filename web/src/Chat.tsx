// harness 대화 기록 보기(§8.27). 기록은 읽기만 하고 보내는 길은 입력창 ChatComposer(plan §8.130)다. agent가 Claude·Codex 기록에서 골라 준 줄을 보인다.
// 도구 결과 본문은 agent가 빼고 보낸다. 생각은 설정 「생각 표시」(공유)를 켰을 때만 청해 접힌 「생각」 줄로 보인다
// (herdr-web-ui ChatView.tsx `ThinkingRow`, plan §8.129). 다시 읽기(차이 목록 C3, 참고 ChatView·conversationRefresh.ts·status.ts
// `statusEdgeRead`, plan §8.187): host가 기록 변화를 알리면(`chat_changed`, 그 머신 agent가 기록 파일을 본다) 바로 읽고(2초에 한
// 번까지) 놓친 것은 10초마다, host가 알릴 수 없는 pane(옛 agent)이나 연결이 끊긴 동안은 2초마다 읽는다. agent 상태가 작업 중에
// 들고 날 때(턴 시작·끝)와 이 화면이 보낸 뒤(`readNow`)에는 기다리지 않고 바로 읽는다. 가려진 탭은 알림으로 읽지 않는다.
// 글 속 파일 경로는 그 pane의 파일 보기로 연다(참고 ChatView는 pane마다 `OpenFileContext`, 찾는 곳은 그 pane이 지금 있는 폴더,
// plan §8.171). 보기 권한 화면은 열 곳이 없어 글 그대로다. 사람·답 말은 참고처럼 Markdown으로 그린다(참고 `UserMessage`의
// `chat-bubble-body`·`Turn`의 답, plan §8.179): 사람 말은 무채색 카드, 답은 카드 없이. 도구 줄은 글 그대로 경로만 잇는다(`linkFilePaths`).
// 말 둘레도 참고대로다(plan §8.182): 대화 기둥 폭(`.chat-transcript`), 긴 사람 말 접기(`UserMessage`, userMessageFold.ts), 사람 말 복사·답의
// 마크다운/일반 텍스트 복사(`CopyButton`, 마우스면 올렸을 때). 참고와 다른 곳: 기록에 시각이 없어 시각은 없고, 접힘은 이 화면 안에서만 기억한다.
// 문구는 참고 i18n.ko.ts.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Check, ChevronDown, ChevronRight, Copy } from "lucide-react";
import { ApiError, call, watchChat } from "./api";
import type { ChatView } from "./gen/proto";
import { useChatShared } from "./settings";
import { statusEdgeRead, type AgentStatus } from "./herdr/status.ts";
import { linkFilePaths } from "./FilePathLink";
import { Markdown } from "./Markdown";
import { OpenFileContext } from "./herdr/filePaths.ts";
import { copyText } from "./herdr/clipboard.ts";
import { shouldFoldUserMessage } from "./herdr/userMessageFold.ts";
import { ConversationRefresh } from "./herdr/conversationRefresh.ts";

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
function UserMessage({ text }: { text: string }) {
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
    <div ref={card} className={`chat-msg user chat-bubble${capped ? " is-capped" : ""}${clipped ? " is-folded" : ""}`} data-role="user">
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

/** 사람 말: 카드와 그 복사(마우스면 카드 옆, 터치면 아래) */
function UserTurn({ text }: { text: string }) {
  const { note, onResult } = useCopyResult();
  return (
    <article className="chat-turn chat-turn-user">
      <div className="chat-user-row">
        <UserMessage text={text} />
        <div className="chat-turn-meta">
          <CopyButton text={text} label="메시지 복사" onResult={onResult} />
        </div>
      </div>
      {note}
    </article>
  );
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
  readNow = 0,
  onOpenFile,
}: {
  pane: string;
  status?: AgentStatus;
  readNow?: number;
  /** 글 속 파일 경로를 연다(조작 기기만) */
  onOpenFile?: (path: string) => void;
}) {
  const [chat, setChat] = useState<ChatView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
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
    refresh.setRead(async () => {
      try {
        const r = await call({ op: "pane_chat", pane, thinking: showThinking });
        if (!alive) return;
        if (r.kind === "chat") {
          setChat(r);
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
      unwatch();
      refresh.stop();
      refresh.setPushes(false);
      setPushes(false);
    };
  }, [pane, showThinking, refresh]);
  // 처음·보낸 뒤·턴 시작과 끝은 읽기를 끊지 않고 바로 읽는다(읽는 곳은 위 effect가 먼저 둔다)
  useEffect(() => {
    refresh.refresh();
  }, [pane, showThinking, pollKey, readNow, refresh]);

  // 맨 아래를 보고 있었으면 새 줄이 오면 따라 내려간다
  useEffect(() => {
    const el = list.current;
    if (el && atBottom.current) el.scrollTop = el.scrollHeight;
  }, [chat]);

  return (
    <OpenFileContext.Provider value={onOpenFile ?? null}>
      <div
        className="chat"
        ref={list}
        data-testid="chat"
        data-pushes={pushes}
        onScroll={(e) => {
          const el = e.currentTarget;
          atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        <div className="chat-transcript">
          <p className="chat-note">
            {chat?.harness === "codex" ? "Codex" : "Claude"}가 남긴 대화 기록이다. 도구 결과{showThinking ? "는" : "와 생각은"} 빼고 보인다.
            {chat?.truncated && " 앞부분은 생략했다."}
          </p>
          {error && <div className="banner warn">{error}</div>}
          {chat?.messages.map((m, i) =>
            m.role === "thinking" ? (
              <ThinkingRow key={i} text={m.text} />
            ) : m.role === "user" ? (
              <UserTurn key={i} text={m.text} />
            ) : m.role === "assistant" ? (
              <AgentTurn key={i} text={m.text} />
            ) : (
              <div key={i} className={`chat-msg ${m.role}`} data-role={m.role}>
                {linkFilePaths(m.text, onOpenFile ?? null)}
              </div>
            ),
          )}
          {chat && chat.messages.length === 0 && !error && <p className="chat-note">아직 대화가 없다.</p>}
        </div>
      </div>
    </OpenFileContext.Provider>
  );
}
