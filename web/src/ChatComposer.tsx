// 대화 보기의 입력창(herdr-web-ui src/components/Composer.tsx의 글 칸·보내기·빠른 답과 PaneTerminal.tsx 2885~2950줄
// `composer-queue`(이 기기에 붙잡은 메시지), 모양 herdr/Composer.css 그대로, 문구는 참고 i18n.ko.ts). 차이 목록 C2, plan §8.130.
// 보내는 길은 터미널 입력줄과 같다(Term `sendTerminalLine`: 글 + Enter, 여러 줄은 bracketed paste). aaa는 host 대기열이 없어
// 작업 중이어도 바로 입력하고 harness가 다음 턴으로 줄 세운다(plan §8.100) — 참고의 「턴이 끝나면 저절로 전송」 자리다.
// 「대기 메시지 자동 전송」을 끄면 작업 중에 쓴 메시지는 이 기기에 붙잡혀(참고 messageQueue.ts) 「지금 보내기」를 누를 때 간다.
// iPad에서 입력창을 끌면 화면이 통째로 따라가던 것은 pageHold.ts가 막는다(참고에 없음).
// 글 칸 높이는 참고 `sizeTextarea`(글만큼, CSS 10줄에서 멈춤)다. 한 줄이면 보내기 단추를 같은 줄에 두는 것은 t3code `resting`
// 배치(ChatComposer.tsx `isComposerResting`, useComposerMultilinePrompt.ts)이고, 데스크톱·긴 대화로 묶은 조건 없이 글만 본다(plan §8.215).
// 다른 곳: 참고의 슬래시 명령·파일 멘션·받아쓰기·제안 칩·크기 손잡이·멈춤 단추·상태 줄은 옮기지 않았다.
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
} from "react";
import { ArrowUp, Check, Clock, ImagePlus, LoaderCircle, TriangleAlert, X } from "lucide-react";
import { uploadChatImage } from "./api";
import type { ChatMessage } from "./gen/proto";
import { imageMention, insertLeadingMention } from "./herdr/composerImages";
import { normalizeComposerImageFile } from "./herdr/composerImageMime";
import "./herdr/Composer.css";
import { composerDrafts } from "./herdr/composerDraft.ts";
import { messageQueues } from "./herdr/messageQueue.ts";
import { holdPageRef } from "./pageHold";
import { quickReplyButtons } from "./preferences";
import { useChatShared, useTerminalToggle } from "./settings";
import { useAppearance } from "./theme";

/** 참고 `MAX_COMPOSER_CHARS` */
const MAX_COMPOSER_CHARS = 100_000;
interface Attachment {
  id: number;
  file: File;
  preview: string;
  state: "uploading" | "waiting" | "ready" | "error" | "no-room";
  path?: string;
}

/** 글 칸이 지금 폭에서 몇 줄인가(높이를 auto로 둔 뒤 잰다) */
function wrappedRows(element: HTMLTextAreaElement): number {
  const style = getComputedStyle(element);
  const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
  return Math.round((element.scrollHeight - padding) / parseFloat(style.lineHeight));
}

export interface ChatComposerProps {
  /** 붙잡은 메시지의 주인(머신:pane, 참고 `queueOwner`) */
  owner: string;
  pane: string;
  touch: boolean;
  /** 「{agent}에게 메시지…」 */
  agentLabel: string;
  connected: boolean;
  /** agent가 작업 중(참고 `composerDelivery`의 queue 자리) */
  working: boolean;
  /** 글 + Enter를 보낸다. false는 보내지 못함(끊김), 약속은 true(받음)·글(거절 이유) */
  onSend: (text: string) => false | Promise<true | string>;
  /** 보낸 뒤: 대화를 바로 다시 읽는다(참고 「every send」 읽기) */
  onSent: () => void;
  messages: ChatMessage[] | null;
  onInspect: () => void;
}

export function ChatComposer({
  owner,
  pane,
  touch,
  agentLabel,
  connected,
  working,
  onSend,
  onSent,
  messages,
  onInspect,
}: ChatComposerProps) {
  const { autoSendQueued, quickReplies, showQuickReplies } = useChatShared();
  const enterSends = useTerminalToggle("enterSends");
  // 초안은 pane마다 localStorage에 둔다(참고 Composer.tsx `draftKey`): 다른 pane을 보거나 앱을 다시 열어도(새 웹) 남는다
  const draftKey = `herdr-web-ui:composer-draft:${owner}`;
  const { text } = useSyncExternalStore(composerDrafts.subscribe, () =>
    composerDrafts.read(draftKey),
  );
  const setText = useCallback(
    (value: string) => composerDrafts.set(draftKey, value),
    [draftKey],
  );
  const textRef = useRef(text);
  textRef.current = text;
  const [sending, setSending] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const picker = useRef<HTMLInputElement>(null);
  const [composing, setComposing] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const attachmentsRef = useRef(attachments);
  attachmentsRef.current = attachments;
  const sequence = useRef(0);
  const uploads = useRef(Promise.resolve());
  const removed = useRef(new Set<number>());
  const mounted = useRef(true);
  const uploading = attachments.some((a) => a.state === "uploading" || a.state === "waiting");
  const noRoom = attachments.some((a) => a.state === "no-room");
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      for (const a of attachmentsRef.current) URL.revokeObjectURL(a.preview);
    };
  }, []);
  // 참고 waitingMentions: IME가 끝나고 온전한 경로가 들어갈 자리가 있을 때 앞에 넣는다.
  useLayoutEffect(() => {
    if (composing) return;
    let value = textRef.current;
    let changed = false;
    const earlier: string[] = [];
    const box = textareaRef.current;
    let start = box?.selectionStart ?? 0;
    let end = box?.selectionEnd ?? 0;
    const next = attachments.map((a) => {
      if (!a.path) return a;
      const mention = imageMention(a.path);
      if (a.state === "ready") { earlier.push(mention); return a; }
      const placed = insertLeadingMention(value, mention, earlier);
      if (placed.missing !== undefined) return a.state === "no-room" ? a : (changed = true, { ...a, state: "no-room" as const });
      value = placed.text;
      if (start >= placed.at) start += placed.length;
      if (end >= placed.at) end += placed.length;
      earlier.push(mention);
      changed = true;
      return { ...a, state: "ready" as const };
    });
    if (changed) {
      setAttachments(next);
      textRef.current = value;
      setText(value);
      if (box) { box.value = value; box.setSelectionRange(start, end); }
    }
  }, [attachments, composing, text, setText]);

  const uploadImages = (files: File[]) => {
    if (!connected || files.length === 0) return;
    if (files.some((f) => f.size === 0 || f.size > 16 * 1024 * 1024)) {
      setNote("그림은 1바이트부터 16MiB까지 붙여 넣습니다. 이 묶음은 올리지 않았습니다.");
      return;
    }
    const added: Attachment[] = files.map((source) => {
      const file = normalizeComposerImageFile(source);
      return { id: ++sequence.current, file, preview: URL.createObjectURL(file), state: "uploading" };
    });
    setNote(null);
    setAttachments((current) => [...current, ...added]);
    for (const a of added) uploads.current = uploads.current.then(async () => {
      if (!mounted.current || removed.current.has(a.id)) return;
      try {
        const path = await uploadChatImage(pane, a.file);
        if (mounted.current && !removed.current.has(a.id)) setAttachments((current) => current.map((item) => item.id === a.id ? { ...item, path, state: "waiting" } : item));
      } catch (error) {
        if (!mounted.current || removed.current.has(a.id)) return;
        setAttachments((current) => current.map((item) => item.id === a.id ? { ...item, state: "error" } : item));
        setNote(error instanceof Error ? error.message : "그림을 올리지 못했습니다.");
      }
    });
  };
  const clearAttachments = (sent: Attachment[]) => {
    for (const a of sent) { removed.current.add(a.id); URL.revokeObjectURL(a.preview); }
    setAttachments((current) => current.filter((a) => !removed.current.has(a.id)));
  };
  const [receipt, setReceipt] = useState<{ id: number; text: string; before: number | null; state: "sending" | "waiting" | "unconfirmed" | "confirmed" } | null>(null);
  const receiptSequence = useRef(0);
  const occurrences = (text: string) => (messages ?? []).filter((m) => m.role === "user" && m.text.trim() === text.trim()).length;
  // 접수 ACK는 PTY 쓰기일 뿐이다. 같은 글이 새로 기록됐을 때만 harness 수신으로 표시한다.
  useEffect(() => {
    if (receipt && receipt.before !== null && receipt.state !== "confirmed" && occurrences(receipt.text) > receipt.before)
      setReceipt({ ...receipt, state: "confirmed" });
  }, [messages, receipt]);
  useEffect(() => {
    if (receipt?.state !== "waiting") return;
    const timer = window.setTimeout(() => setReceipt((r) => r === receipt ? { ...r, state: "unconfirmed" } : r), 10_000);
    return () => window.clearTimeout(timer);
  }, [receipt]);
  const queued = useSyncExternalStore(messageQueues.subscribe, () =>
    messageQueues.read(owner),
  );
  const [queueSending, setQueueSending] = useState<string | null>(null);
  const [queueError, setQueueError] = useState<{
    id: string;
    text: string;
  } | null>(null);
  const replies = showQuickReplies ? quickReplyButtons({ quickReplies }) : [];
  const { appearance } = useAppearance();
  const surfaceRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  // 한 줄(one)이면 단추가 글 칸 옆에, 여러 줄(more)이면 글 칸이 상자 폭을 다 쓰고 단추는 밑 줄로 간다(t3code `resting`)
  const [lines, setLines] = useState<"one" | "more">("one");
  const [boxWidth, setBoxWidth] = useState(0);

  // 참고 `boxWidth`: 폭이 바뀌면(돌림·창 크기·옆 목록) 줄넘김이 바뀌므로 다시 잰다
  useEffect(() => {
    const surface = surfaceRef.current;
    if (!surface) return;
    const observer = new ResizeObserver(() => setBoxWidth(surface.clientWidth));
    observer.observe(surface);
    return () => observer.disconnect();
  }, []);

  // 참고 `sizeTextarea`(Composer.tsx): 높이를 auto로 두고 scrollHeight만큼 준다. 그리기 전에 돌아 깜박이지 않는다.
  // 한 줄인지는 늘 단추 옆 폭(한 줄 배치의 글 칸 폭)에서 잰다: 배치를 바꿔도 판정이 같아 두 배치를 오가며 튀지 않는다
  // (t3code useComposerMultilinePrompt.ts는 펼친 폭과 비교해 그 사이 글은 한 줄 칸에서 잘린다 — 여기는 잘리지 않게 좁은 폭).
  // 글꼴은 시스템 글꼴이라 참고의 `facesLoaded`는 없고, 대화 글자 크기·밀도가 바뀌면 다시 잰다(참고 settings.chatFontSize 효과).
  useLayoutEffect(() => {
    const element = textareaRef.current;
    const surface = surfaceRef.current;
    if (!element || !surface) return;
    element.style.height = "auto";
    let next: "one" | "more";
    if (text === "") next = "one";
    else if (text.includes("\n")) next = "more";
    else if (wrappedRows(element) > 1) next = "more";
    else if (lines === "one") next = "one";
    else {
      // 펼친 폭에서 한 줄: 단추 옆 폭으로 줄여 다시 잰다(같은 그리기 안이라 화면에는 나오지 않는다)
      let beside = surface.clientWidth;
      for (const controls of surface.querySelectorAll<HTMLElement>(":scope > .composer-controls"))
        beside -= controls.offsetWidth;
      element.style.width = `${beside}px`;
      next = wrappedRows(element) > 1 ? "more" : "one";
      element.style.width = "";
    }
    if (next !== lines) {
      // 배치를 바꾼 다음 그리기 전에 이 효과가 다시 돌아 그 배치에서 높이를 준다
      setLines(next);
      return;
    }
    element.style.height = `${element.scrollHeight}px`;
  }, [text, lines, boxWidth, appearance.chatFontSize, appearance.uiFontSize, appearance.density]);

  /** 보내고 결과를 글로: null은 받음 */
  const deliver = async (message: string): Promise<string | null> => {
    const attempt = { id: ++receiptSequence.current, text: message, before: messages === null ? null : occurrences(message), state: "sending" as const };
    try {
      setReceipt(attempt);
      const sent = onSend(message);
      if (sent === false) {
        setReceipt((r) => r?.id === attempt.id ? null : r);
        return "전송하지 못했습니다. 다시 연결한 후 시도하세요.";
      }
      const result = await sent;
      if (result !== true) { setReceipt((r) => r?.id === attempt.id ? null : r); return result; }
      setReceipt((r) => r?.id === attempt.id && r.state === "sending" ? { ...r, state: "waiting" } : r);
      onSent();
      return null;
    } catch {
      setReceipt((r) => r?.id === attempt.id ? null : r);
      return "전송 여부를 확인하지 못했습니다. 다시 보내기 전에 터미널을 확인하세요.";
    }
  };

  const send = (given?: string): void => {
    const message = (given ?? textRef.current).trim();
    if (message === "" || sending || uploading || noRoom || composing || !connected) return;
    const sentAttachments = attachments;
    setNote(null);
    // 참고 composerSend: 작업 중이고 자동 전송이 꺼져 있으면 붙잡는다(hold)
    if (working && !autoSendQueued) {
      messageQueues.add(owner, message);
      if (given === undefined) { setText(""); clearAttachments(sentAttachments); }
      return;
    }
    setSending(true);
    void deliver(message).then((failed) => {
      setSending(false);
      if (failed !== null) setNote(failed);
      // 보내는 동안 더 친 글은 남는다: 보낸 그 글일 때만 비운다
      else if (given === undefined) {
        if (textRef.current.trim() === message) setText("");
        clearAttachments(sentAttachments);
      }
    });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    // IME는 제 키를 갖는다. WebKit은 확정 Enter를 compositionend 뒤 229로 보낼 수 있다
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229)
      return;
    if (event.key !== "Enter") return;
    // 터치 화면은 참고의 coarse 판별을 쓰되 사용자 요청대로 Enter는 항상 줄바꿈이다.
    if (touch) return;
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
          ref={holdPageRef}
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
      {/* 입력창을 끌어도 화면이 통째로 따라가지 않는다(pageHold.ts) */}
      <div
        ref={holdPageRef}
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
        {note === null && receipt && receipt.state !== "sending" && (
          <div className="composer-hint" role="status" data-testid="chat-delivered" data-state={receipt.state}>
            {receipt.state === "confirmed" ? <Check size={14} aria-hidden="true" /> : <Clock size={14} aria-hidden="true" />}
            <span>{receipt.state === "confirmed" ? "대화 기록에서 수신을 확인했습니다. 처리 완료 여부는 응답에서 확인하세요."
              : receipt.state === "waiting" ? "입력 전달됨 · 대화 기록에서 수신을 확인하는 중입니다."
                : "아직 처리되지 않음 · 대화 기록에서 수신을 확인하지 못했습니다. 터미널 입력창을 확인한 뒤 다시 보내세요."}</span>
            {receipt.state === "unconfirmed" && <div className="composer-receipt-actions">
              <button type="button" className="btn btn-ghost" onClick={onInspect}>입력창 확인</button>
              <button type="button" className="btn btn-ghost" disabled={text !== "" || sending} onClick={() => { setText(receipt.text); textareaRef.current?.focus(); }}>보낸 글 복원</button>
            </div>}
          </div>
        )}
        <div ref={surfaceRef} data-lines={lines} className={`composer-surface${dragging ? " is-dragging" : ""}`}
          onDragOver={(event) => {
            if (!event.dataTransfer.types.includes("Files")) return;
            event.preventDefault();
            event.dataTransfer.dropEffect = connected ? "copy" : "none";
            setDragging(connected);
          }}
          onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDragging(false); }}
          onDrop={(event) => { event.preventDefault(); setDragging(false); uploadImages(Array.from(event.dataTransfer.files)); }}
        >
          {attachments.length > 0 && <div className="composer-attachments" aria-label="첨부한 그림">
            {attachments.map((a) => <div className={`composer-attachment is-${a.state}`} key={a.id}>
              <img src={a.preview} alt={a.file.name} />
              <span className="composer-attachment-state">{a.state === "ready" ? "첨부됨" : a.state === "error" ? "실패" : a.state === "no-room" ? "글을 줄여 주세요" : "올리는 중"}</span>
              <button type="button" aria-label={`${a.file.name} 첨부 삭제`} onClick={() => {
                clearAttachments([a]);
                if (a.path) setText(textRef.current.replace(imageMention(a.path), ""));
              }}><X aria-hidden="true" /></button>
            </div>)}
          </div>}
          <textarea
            ref={textareaRef}
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
            onCompositionStart={() => setComposing(true)}
            onCompositionEnd={() => setComposing(false)}
            onPaste={(event) => {
              const images = Array.from(event.clipboardData.items).filter((item) => item.kind === "file").map((item) => item.getAsFile()).filter((file): file is File => file !== null);
              if (images.length === 0) return;
              event.preventDefault();
              uploadImages(images);
            }}
            onChange={(event) => {
              setText(event.target.value);
              setNote(null);
            }}
          />
          <input ref={picker} type="file" accept="image/png,image/jpeg,image/gif,image/webp" multiple hidden
            onChange={(event) => { const files = Array.from(event.target.files ?? []); event.target.value = ""; uploadImages(files); }} />
          <div className="composer-controls composer-controls-left">
            <button type="button" className="icon-button" aria-label="그림 첨부" title="그림 첨부" disabled={!connected || uploading}
              onPointerDown={(event) => event.preventDefault()} onClick={() => picker.current?.click()}><ImagePlus aria-hidden="true" /></button>
          </div>
          <div className="composer-controls composer-controls-right">
            <button
              type="button"
              className="composer-action composer-send"
              aria-label="메시지 보내기"
              title={sending ? "보내는 중…" : "메시지 보내기"}
              aria-busy={sending || undefined}
              disabled={!connected || sending || uploading || noRoom || composing || text.trim().length === 0}
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
