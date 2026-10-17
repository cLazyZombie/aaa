// 대화 보기의 입력창(herdr-web-ui src/components/Composer.tsx의 글 칸·보내기·빠른 답과 PaneTerminal.tsx 2885~2950줄
// `composer-queue`(이 기기에 붙잡은 메시지), 모양 herdr/Composer.css 그대로, 문구는 참고 i18n.ko.ts). 차이 목록 C2, plan §8.115.
// 보내는 길은 터미널 입력줄과 같다(Term `sendTerminalLine`: 글 + Enter, 여러 줄은 bracketed paste). aaa는 host 대기열이 없어
// 작업 중이어도 바로 입력하고 harness가 다음 턴으로 줄 세운다(plan §8.100) — 참고의 「턴이 끝나면 저절로 전송」 자리다.
// 「대기 메시지 자동 전송」을 끄면 작업 중에 쓴 메시지는 이 기기에 붙잡혀(참고 messageQueue.ts) 「지금 보내기」를 누를 때 간다.
// 다른 곳: 참고의 슬래시 명령·파일 멘션·그림 첨부·받아쓰기·제안 칩·크기 손잡이·멈춤 단추·상태 줄은 옮기지 않았다.
import {
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
} from "react";
import { ArrowUp, Clock, LoaderCircle, TriangleAlert, X } from "lucide-react";
import "./herdr/Composer.css";
import { messageQueues } from "./herdr/messageQueue.ts";
import { quickReplyButtons } from "./preferences";
import { useChatShared, useTerminalToggle } from "./settings";

/** 참고 `MAX_COMPOSER_CHARS` */
const MAX_COMPOSER_CHARS = 100_000;

export interface ChatComposerProps {
  /** 붙잡은 메시지의 주인(머신:pane, 참고 `queueOwner`) */
  owner: string;
  /** 「{agent}에게 메시지…」 */
  agentLabel: string;
  connected: boolean;
  /** agent가 작업 중(참고 `composerDelivery`의 queue 자리) */
  working: boolean;
  /** 글 + Enter를 보낸다. false는 보내지 못함(끊김), 약속은 true(받음)·글(거절 이유) */
  onSend: (text: string) => false | Promise<true | string>;
  /** 보낸 뒤: 대화를 바로 다시 읽는다(참고 「every send」 읽기) */
  onSent: () => void;
}

export function ChatComposer({
  owner,
  agentLabel,
  connected,
  working,
  onSend,
  onSent,
}: ChatComposerProps) {
  const { autoSendQueued, quickReplies, showQuickReplies } = useChatShared();
  const enterSends = useTerminalToggle("enterSends");
  const [text, setText] = useState("");
  const textRef = useRef(text);
  textRef.current = text;
  const [sending, setSending] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const queued = useSyncExternalStore(messageQueues.subscribe, () =>
    messageQueues.read(owner),
  );
  const [queueSending, setQueueSending] = useState<string | null>(null);
  const [queueError, setQueueError] = useState<{
    id: string;
    text: string;
  } | null>(null);
  const replies = showQuickReplies ? quickReplyButtons({ quickReplies }) : [];

  /** 보내고 결과를 글로: null은 받음 */
  const deliver = async (message: string): Promise<string | null> => {
    const sent = onSend(message);
    if (sent === false)
      return "전송하지 못했습니다. 다시 연결한 후 시도하세요.";
    try {
      const result = await sent;
      if (result !== true) return result;
      onSent();
      return null;
    } catch {
      return "전송 여부를 확인하지 못했습니다. 다시 보내기 전에 터미널을 확인하세요.";
    }
  };

  const send = (given?: string): void => {
    const message = (given ?? textRef.current).trim();
    if (message === "" || sending) return;
    setNote(null);
    // 참고 composerSend: 작업 중이고 자동 전송이 꺼져 있으면 붙잡는다(hold)
    if (working && !autoSendQueued) {
      messageQueues.add(owner, message);
      if (given === undefined) setText("");
      return;
    }
    setSending(true);
    void deliver(message).then((failed) => {
      setSending(false);
      if (failed !== null) setNote(failed);
      // 보내는 동안 더 친 글은 남는다: 보낸 그 글일 때만 비운다
      else if (given === undefined && textRef.current.trim() === message)
        setText("");
    });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    // IME는 제 키를 갖는다. WebKit은 확정 Enter를 compositionend 뒤 229로 보낼 수 있다
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229)
      return;
    if (event.key !== "Enter") return;
    const shouldSend = enterSends
      ? !event.shiftKey && !event.metaKey && !event.ctrlKey
      : (event.metaKey || event.ctrlKey) && !event.shiftKey;
    if (!shouldSend) return;
    event.preventDefault();
    send();
  };

  const sendQueued = (id: string, message: string): void => {
    if (queueSending !== null || !messageQueues.beginSend(owner, id)) return;
    setQueueSending(id);
    setQueueError(null);
    void deliver(message)
      .then((failed) => {
        if (failed === null) messageQueues.remove(owner, id);
        else setQueueError({ id, text: failed });
      })
      .finally(() => {
        messageQueues.endSend(owner, id);
        setQueueSending(null);
      });
  };

  const placeholder = connected
    ? `${agentLabel}에게 메시지…`
    : "재연결 중… 메시지는 여기에 보관되고 대기열에 넣지 않습니다";
  return (
    <>
      {queued.length > 0 && (
        <section
          className={`composer-queue${working ? "" : " is-ready"}`}
          aria-label="대기 메시지"
          data-testid="chat-queue"
        >
          <div className="composer-queue-heading">
            <strong className="visually-hidden">{`대기 메시지 ${queued.length}개`}</strong>
            <span className="composer-queue-mark" aria-hidden="true">
              <Clock />
            </span>
            <span className="composer-queue-caption">
              {working
                ? "에이전트가 준비될 때까지 보관"
                : "보관된 메시지. 확인하고 보내세요"}
              {queued.length > 1 && ` · 메시지 ${queued.length}개`}
            </span>
          </div>
          {messageQueues.isUnsaved(owner) && (
            <p className="composer-queue-error" role="status">
              대기 메시지를 저장하지 못했습니다. 이 탭을 유지하거나 새로고침
              전에 메시지를 복사하세요.
            </p>
          )}
          <ol className="composer-queue-list">
            {queued.map((message, index) => (
              <li className="composer-queue-item" key={message.id}>
                <textarea
                  className="composer-queue-text"
                  value={message.text}
                  rows={Math.min(4, message.text.split("\n").length)}
                  aria-label={`대기 메시지 ${index + 1}`}
                  maxLength={MAX_COMPOSER_CHARS}
                  disabled={messageQueues.isSending(message.id)}
                  onChange={(event) =>
                    messageQueues.edit(owner, message.id, event.target.value)
                  }
                />
                <div className="composer-queue-actions">
                  <button
                    type="button"
                    className="composer-queue-send"
                    disabled={
                      !connected ||
                      queueSending !== null ||
                      message.text.trim().length === 0
                    }
                    onClick={() => sendQueued(message.id, message.text)}
                  >
                    지금 보내기
                  </button>
                  <button
                    type="button"
                    className="composer-queue-discard"
                    disabled={messageQueues.isSending(message.id)}
                    onClick={() => messageQueues.remove(owner, message.id)}
                  >
                    {/* 휴대폰은 X를 그리고, 말은 단추 이름으로 남는다 */}
                    <span className="composer-queue-discard-text">버리기</span>
                    <X aria-hidden="true" />
                  </button>
                </div>
                {queueError?.id === message.id && (
                  <p className="composer-queue-error" role="status">
                    {queueError.text}
                  </p>
                )}
              </li>
            ))}
          </ol>
        </section>
      )}
      <div
        className="composer"
        role="group"
        aria-label="메시지 입력창"
        data-testid="chat-composer"
      >
        {replies.length > 0 && (
          <div className="composer-quick" role="group" aria-label="빠른 답장">
            {replies.map((reply, index) => (
              <button
                key={`${index}:${reply}`}
                type="button"
                className="composer-quick-reply"
                title={`“${reply}” 보내기`}
                disabled={!connected || sending}
                onClick={() => send(reply)}
              >
                {reply}
              </button>
            ))}
          </div>
        )}
        {/* 메시지가 가지 못한 까닭: 상자 위끝에, 상자와 함께 읽힌다 */}
        {note !== null && (
          <div className="composer-note" role="alert">
            <TriangleAlert className="composer-note-icon" aria-hidden="true" />
            <span className="composer-note-text">{note}</span>
          </div>
        )}
        <div className="composer-surface">
          <textarea
            className="composer-text"
            rows={1}
            maxLength={MAX_COMPOSER_CHARS}
            value={text}
            placeholder={placeholder}
            aria-label="메시지"
            // agent에게 쓰는 글은 대개 산문이다: 휴대폰 자판의 고침·제안·문장 대문자를 둔다(터미널 입력줄은 끈다)
            spellCheck
            autoCapitalize="sentences"
            autoCorrect="on"
            onKeyDown={onKeyDown}
            onChange={(event) => {
              setText(event.target.value);
              setNote(null);
            }}
          />
          <div className="composer-controls composer-controls-right">
            <button
              type="button"
              className="composer-action composer-send"
              aria-label="메시지 보내기"
              title={sending ? "보내는 중…" : "메시지 보내기"}
              aria-busy={sending || undefined}
              disabled={!connected || sending || text.trim().length === 0}
              // focus는 글 칸에 남는다: 떠나면 휴대폰 자판이 닫혀 누르는 사이 단추가 옮겨 간다
              onPointerDown={(event) => event.preventDefault()}
              onClick={() => send()}
            >
              {sending ? (
                <LoaderCircle
                  className="composer-send-progress"
                  aria-hidden="true"
                />
              ) : (
                <ArrowUp aria-hidden="true" />
              )}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
