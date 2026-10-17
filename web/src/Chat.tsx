// harness 대화 기록 보기(§8.27). 기록은 읽기만 하고 보내는 길은 입력창 ChatComposer(plan §8.115)다. agent가 Claude·Codex 기록에서 골라 준 줄을 보인다.
// 도구 결과 본문은 agent가 빼고 보낸다. 생각은 설정 「생각 표시」(공유)를 켰을 때만 청해 접힌 「생각」 줄로 보인다
// (herdr-web-ui ChatView.tsx `ThinkingRow`, plan §8.114). 다시 읽기(차이 목록 C3, 참고 conversationRefresh.ts·status.ts
// `statusEdgeRead`): aaa는 기록 변경을 push하지 않으니 참고의 push 없는 판처럼 2초마다, 그리고 agent 상태가 작업 중에 들고 날 때
// (턴 시작·끝)와 이 화면이 보낸 뒤(`readNow`)에는 기다리지 않고 바로 읽는다.
import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { ApiError, call } from "./api";
import type { ChatView } from "./gen/proto";
import { useChatShared } from "./settings";
import { statusEdgeRead, type AgentStatus } from "./herdr/status.ts";

/** push 없는 대화의 읽기 간격(참고 conversationRefresh.ts `INVALIDATION_MS`) */
const INVALIDATION_MS = 2000;

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

export function ChatPanel({ pane, status, readNow = 0 }: { pane: string; status?: AgentStatus; readNow?: number }) {
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

  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const load = async () => {
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
      if (alive) timer = window.setTimeout(load, INVALIDATION_MS);
    };
    void load();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [pane, showThinking, pollKey, readNow]);

  // 맨 아래를 보고 있었으면 새 줄이 오면 따라 내려간다
  useEffect(() => {
    const el = list.current;
    if (el && atBottom.current) el.scrollTop = el.scrollHeight;
  }, [chat]);

  return (
    <div
      className="chat"
      ref={list}
      data-testid="chat"
      onScroll={(e) => {
        const el = e.currentTarget;
        atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      }}
    >
      <p className="chat-note">
        {chat?.harness === "codex" ? "Codex" : "Claude"}가 남긴 대화 기록이다. 도구 결과{showThinking ? "는" : "와 생각은"} 빼고 보인다.
        {chat?.truncated && " 앞부분은 생략했다."}
      </p>
      {error && <div className="banner warn">{error}</div>}
      {chat?.messages.map((m, i) =>
        m.role === "thinking" ? (
          <ThinkingRow key={i} text={m.text} />
        ) : (
          <div key={i} className={`chat-msg ${m.role}`} data-role={m.role}>
            {m.text}
          </div>
        ),
      )}
      {chat && chat.messages.length === 0 && !error && <p className="chat-note">아직 대화가 없다.</p>}
    </div>
  );
}
