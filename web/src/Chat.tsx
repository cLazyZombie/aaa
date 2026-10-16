// harness 대화 기록 보기(§8.27, 읽기 전용). agent가 Claude·Codex 기록에서 골라 준 줄을 보인다.
// 생각·도구 결과 본문은 agent가 빼고 보낸다. 보이는 동안 3초마다 다시 읽는다.
import { useEffect, useRef, useState } from "react";
import { ApiError, call } from "./api";
import type { ChatView } from "./gen/proto";

export function ChatPanel({ pane }: { pane: string }) {
  const [chat, setChat] = useState<ChatView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);

  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const load = async () => {
      try {
        const r = await call({ op: "pane_chat", pane });
        if (!alive) return;
        if (r.kind === "chat") {
          setChat(r);
          setError(null);
        }
      } catch (e) {
        if (alive) setError(e instanceof ApiError ? e.message : String(e));
      }
      if (alive) timer = window.setTimeout(load, 3000);
    };
    void load();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [pane]);

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
        {chat?.harness === "codex" ? "Codex" : "Claude"}가 남긴 대화 기록이다(읽기 전용). 생각과 도구 결과는 빼고 보인다.
        {chat?.truncated && " 앞부분은 생략했다."}
      </p>
      {error && <div className="banner warn">{error}</div>}
      {chat?.messages.map((m, i) => (
        <div key={i} className={`chat-msg ${m.role}`} data-role={m.role}>
          {m.text}
        </div>
      ))}
      {chat && chat.messages.length === 0 && !error && <p className="chat-note">아직 대화가 없다.</p>}
    </div>
  );
}
