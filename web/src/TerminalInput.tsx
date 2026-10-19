// 터미널 입력줄. herdr-web-ui `src/components/TerminalInput.tsx`(aa624cb)를 옮겼다.
//
// 휴대폰 키보드는 친 글을 고쳐 쓴다(받아쓰기가 구절을 고치고, IME가 음절을 마친다). 터미널은 이미 보낸 키를
// 되돌릴 수 없어 고칠 때마다 글이 더 들어갔다. 입력줄에서는 키보드 자신의 편집으로 글을 쓰고 pane에는 통째로
// 보낸 뒤 Enter를 보낸다. 자동 고침·맞춤법은 끈다(옵션·경로·branch 이름을 사전 낱말로 바꾼다).
//
// 참고와 다른 곳(plan §8.44): 받아쓰기(VoiceInput)는 aaa에 없다. 「그림」 단추를 앞에 둔다(참고는 chat 화면
// composer에 둔다. aaa chat 화면은 아직 읽기 전용이라 휴대폰에서 그림을 올릴 곳이 여기뿐이다).
import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type ClipboardEvent, type KeyboardEvent } from "react";
import { Icon } from "./icons";
import {
  acknowledgeTerminalDraft,
  readTerminalDraft,
  setTerminalDraftSending,
  subscribeTerminalDraft,
  terminalDraftSending,
  TERMINAL_LINE_LIMIT,
  writeTerminalDraft,
} from "./herdr/terminalDraft.ts";

export interface TerminalInputProps {
  owner: string;
  /** 입력줄 조합 중: 바로 입력으로 바꾸는 단추만 기다린다 */
  onComposing?: (active: boolean) => void;
  connected: boolean;
  /** true: 보냄, 줄을 비운다. 글: 글을 두고 이유를 보인다. false: 못 보냄(끊김) */
  onSend: (text: string) => false | Promise<true | string>;
  /** 빈 줄 보내기: Enter만. true: 보냄, false: 못 보냄(끊김), 글: 못 보낸 이유 */
  onEnter: () => boolean | string;
  /** 빈 입력줄의 Ctrl+Enter는 host의 공통 판정으로 보낸다. 글이 있으면 이 입력줄에 줄바꿈이다. */
  onCtrlEnter?: () => boolean | string;
  /** 입력줄이 focus를 얻음 */
  onFocus?: () => void;
  /** 「그림」 단추(aaa만) */
  onImage?: () => void;
  onPaste?: (event: ClipboardEvent<HTMLTextAreaElement>) => void;
}

/** 이만큼 자란 뒤에는 줄 안에서 넘긴다 */
const MAX_ROWS = 4;
/** 조합 중에 누른 보내기가 글자 확정을 기다리는 시간 */
const COMMIT_WAIT_MS = 300;

const DISCONNECTED = "보내지 않았다: 터미널 연결이 끊겼다";

/** 조합 중에 누른 보내기. 글자가 확정되기를 기다린다. */
interface SendAttempt {
  /** 조합이 열린 채 기한이 되면 누름을 거둔다 */
  timer: ReturnType<typeof setTimeout> | null;
  /** 누른 때 정한다: 조합이 이어져도 기다림이 늘지 않는다 */
  deadline: number;
  /** 누를 때 줄에 글이 있었다: 확정이 비면 아무것도 보내지 않는다(Enter도 아니다) */
  textSend: boolean;
}

export function TerminalInput({ owner, connected, onSend, onEnter, onCtrlEnter, onComposing, onFocus, onImage, onPaste }: TerminalInputProps) {
  const text = useSyncExternalStore(subscribeTerminalDraft, () => readTerminalDraft(owner));
  const setText = useCallback(
    (value: string | ((previous: string) => string)) => {
      // 내려간 뒤에도 owner 기록을 읽는다: 늦은 확인이 같은 pane에 새로 연 입력줄의 글을 지우면 안 된다
      const next = typeof value === "function" ? value(readTerminalDraft(owner)) : value;
      writeTerminalDraft(owner, next);
    },
    [owner],
  );
  const composing = useRef(false);
  const composingCallback = useRef(onComposing);
  composingCallback.current = onComposing;
  useEffect(
    () => () => {
      composingCallback.current?.(false);
    },
    [],
  );

  const [note, setNote] = useState<string | null>(null);
  const sending = useSyncExternalStore(subscribeTerminalDraft, () => terminalDraftSending(owner));
  const box = useRef<HTMLTextAreaElement>(null);

  // 키보드가 아직 마지막 낱말을 조합하는 중에 누른 보내기는 그 낱말을 마치고 확정되면 보낸다. 단추는 focus와
  // 선택을 가져가지 않으므로 누름만으로는 확정되지 않는다: blur가 확정한다(Chrome은 focus가 바뀌면 조합을 끝내고
  // 바로 compositionend를 낸다). 같은 동작 안에서 다시 focus하므로 키보드는 그대로다. 기한까지 확정하지 않는
  // 키보드면 누름을 거두고 알린다. 늦은 확정이 혼자 보내지 않는다.
  const attempt = useRef<SendAttempt | null>(null);
  const clearPendingSend = useCallback(() => {
    const current = attempt.current;
    if (current?.timer != null) clearTimeout(current.timer);
    attempt.current = null;
  }, []);
  useEffect(() => clearPendingSend, [clearPendingSend]);
  // 끊기면 누름을 거둔다: 기다리는 사이 다시 연결돼도 보내지 않는다
  useEffect(() => {
    if (connected || attempt.current === null) return;
    clearPendingSend();
    setNote(DISCONNECTED);
  }, [clearPendingSend, connected]);
  const sendRef = useRef<(deferred: boolean) => void>(() => {});

  /** 누른 때의 기한까지 확정을 기다린다. 그동안 「글자를 마치는 중…」 */
  const awaitCommit = useCallback((current: SendAttempt) => {
    if (current.timer !== null) clearTimeout(current.timer);
    current.timer = null;
    const left = current.deadline - Date.now();
    if (left <= 0) {
      attempt.current = null;
      setNote("글자를 마친 뒤 보낸다");
      return;
    }
    setNote("글자를 마치는 중…");
    current.timer = setTimeout(() => {
      if (attempt.current !== current) return;
      attempt.current = null;
      setNote("글자를 마친 뒤 보낸다");
    }, left);
  }, []);

  const send = useCallback(
    (deferred = false) => {
      // 미룬 보내기는 아직 기다리는 누름에만 돈다: 끊김·기한·비운 줄이 거뒀다
      const current = attempt.current;
      if (deferred && current === null) return;
      if (!connected || terminalDraftSending(owner)) {
        // 확정을 기다리던 보내기가 「글자를 마치는 중…」을 남기면 안 된다
        if (deferred) {
          clearPendingSend();
          setNote(connected ? null : DISCONNECTED);
        }
        return;
      }
      if (composing.current) {
        if (deferred) {
          // 확정 뒤 새 조합: 누른 때의 기한에서 남은 만큼만 기다린다
          awaitCommit(current!);
          return;
        }
        clearPendingSend();
        const tapped: SendAttempt = { timer: null, deadline: Date.now() + COMMIT_WAIT_MS, textSend: readTerminalDraft(owner).length > 0 };
        attempt.current = tapped;
        const element = box.current;
        if (element !== null && document.activeElement === element) {
          element.blur();
          element.focus({ preventScroll: true });
        }
        // 누르는 사이 확정됨(Chrome·Gboard): compositionend가 이미 보내기를 걸었다. 알림 없음
        if (composing.current && attempt.current === tapped) awaitCommit(tapped);
        return;
      }
      clearPendingSend();
      setNote(null);
      // 마지막 렌더가 아니라 초안 저장소를 읽는다: 방금 확정된 글이 아직 렌더되지 않았을 수 있다
      const draft = readTerminalDraft(owner);
      if (draft.length === 0) {
        // 지우거나 취소해 비어 버린 글 보내기는 Enter가 아니다
        if (deferred && current!.textSend) return;
        const entered = onEnter();
        if (typeof entered === "string") setNote(entered);
        else if (!entered) setNote(DISCONNECTED);
        return;
      }
      const sent = onSend(draft);
      if (sent === false) {
        setNote(DISCONNECTED);
        return;
      }
      setTerminalDraftSending(owner, true);
      void sent
        .then((result) => {
          // 가는 동안 친 글은 남는다
          if (result === true) acknowledgeTerminalDraft(owner);
          else setNote(result);
        })
        .catch(() => setNote("확인하지 못했다. 다시 보내기 전에 터미널을 본다"))
        .finally(() => {
          setTerminalDraftSending(owner, false);
        });
    },
    [awaitCommit, clearPendingSend, connected, onEnter, onSend, owner],
  );
  sendRef.current = send;

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    // Enter는 보내기, Shift+Enter는 줄바꿈. IME의 Enter(WebKit이 compositionend 뒤 229로 보내는 확정 Enter도)는 IME 것이다
    if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
    if (event.ctrlKey && !event.altKey && !event.metaKey && onCtrlEnter) {
      event.preventDefault();
      if (!connected || sending || event.repeat) return;
      const draft = readTerminalDraft(owner);
      if (draft.length > 0) {
        const element = event.currentTarget;
        const start = element.selectionStart;
        setText(`${draft.slice(0, start)}\n${draft.slice(element.selectionEnd)}`);
        requestAnimationFrame(() => element.setSelectionRange(start + 1, start + 1));
      } else {
        const result = onCtrlEnter();
        if (typeof result === "string") setNote(result);
      }
      return;
    }
    event.preventDefault();
    send();
  };

  // 접힌 줄도 센다: 받아쓴 문장은 긴 한 줄이고 그 끝이 보여야 한다
  const [wrappedRows, setWrappedRows] = useState(1);
  useLayoutEffect(() => {
    const element = box.current;
    if (!element) return;
    const style = getComputedStyle(element);
    const line = parseFloat(style.lineHeight);
    if (!(line > 0)) return;
    const shown = element.rows;
    element.rows = 1;
    const content = element.scrollHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
    element.rows = shown;
    setWrappedRows(Math.max(1, Math.round(content / line)));
  }, [text]);
  const rows = Math.min(MAX_ROWS, Math.max(wrappedRows, text.split("\n").length));
  const enterOnly = text.length === 0;
  return (
    <div className={`terminal-input${onImage ? " has-image" : ""}`}>
      {onImage && (
        <button
          type="button"
          className="terminal-input-image ghost icon"
          aria-label="그림"
          title="그림을 대상 머신에 올리고 입력창에 경로를 넣는다"
          disabled={!connected}
          onPointerDown={(event) => event.preventDefault()}
          onClick={onImage}
        >
          <Icon name="image" />
        </button>
      )}
      <textarea
        ref={box}
        className="terminal-input-text"
        rows={rows}
        value={text}
        maxLength={TERMINAL_LINE_LIMIT}
        onCompositionStart={() => {
          composing.current = true;
          onComposing?.(true);
        }}
        onCompositionEnd={() => {
          composing.current = false;
          onComposing?.(false);
          const current = attempt.current;
          if (current === null) return;
          if (current.timer !== null) clearTimeout(current.timer);
          current.timer = null;
          // WebKit은 compositionend 뒤 input 사건으로 값을 고친다: 확정된 글을 보낸다
          setTimeout(() => sendRef.current(true));
        }}
        placeholder="터미널에 보낼 글…"
        aria-label="보낼 글"
        enterKeyHint="send"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        onChange={(event) => {
          setText(event.target.value);
          setNote(null);
          // 비운 줄에는 기다리는 보내기가 보낼 것이 없다
          if (event.target.value.length === 0) clearPendingSend();
        }}
        onKeyDown={onKeyDown}
        onFocus={onFocus}
        onPaste={onPaste}
      />
      <button
        type="button"
        className="terminal-input-send"
        aria-label={enterOnly ? "터미널에 Enter 보내기" : "터미널에 보내기"}
        title={enterOnly ? "터미널에 Enter 보내기" : "터미널에 보내기"}
        disabled={!connected || sending}
        // 화면 키보드는 다음 줄을 위해 그대로 둔다
        onPointerDown={(event) => event.preventDefault()}
        onClick={() => send()}
      >
        <Icon name={enterOnly ? "enter" : "send"} />
      </button>
      {note !== null && (
        <p className="terminal-input-note" role="alert">
          {note}
        </p>
      )}
    </div>
  );
}
