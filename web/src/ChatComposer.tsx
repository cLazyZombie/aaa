// 대화 보기의 입력창(herdr-web-ui src/components/Composer.tsx의 글 칸·보내기·빠른 답과 PaneTerminal.tsx 2885~2950줄
// `composer-queue`(이 기기에 붙잡은 메시지), 모양 herdr/Composer.css 그대로, 문구는 참고 i18n.ko.ts). 차이 목록 C2, plan §8.130.
// 보내는 길은 터미널 입력줄과 같다(Term `sendTerminalLine`: 글 + Enter, 여러 줄은 bracketed paste). aaa는 host 대기열이 없어
// 작업 중이어도 바로 입력하고 harness가 다음 턴으로 줄 세운다(plan §8.100) — 참고의 「턴이 끝나면 저절로 전송」 자리다.
// 「대기 메시지 자동 전송」을 끄면 작업 중에 쓴 메시지는 이 기기에 붙잡혀(참고 messageQueue.ts) 「지금 보내기」를 누를 때 간다.
// iPad에서 입력창을 끌면 화면이 통째로 따라가던 것은 pageHold.ts가 막는다(참고에 없음).
// 글 칸 높이는 참고 `sizeTextarea`(글만큼, CSS 10줄에서 멈춤)다. 한 줄이면 보내기 단추를 같은 줄에 두는 것은 t3code `resting`
// 배치(ChatComposer.tsx `isComposerResting`, useComposerMultilinePrompt.ts)이고, 데스크톱·긴 대화로 묶은 조건 없이 글만 본다(plan §8.215).
// 다른 곳: 참고의 슬래시 명령·파일 멘션·받아쓰기·제안 칩·크기 손잡이·멈춤 단추·상태 줄은 옮기지 않았다.
// 보내기 상태(plan §8.247, 사용자 10-19): 보내기는 바로 가고, 보낸 글이 「보내짐」이 될 때까지 입력창에 남아 잠긴다(readOnly:
// disabled는 focus를 잃어 휴대폰 자판이 닫힌다, 참고 Composer.tsx 1272~1274줄). 앞 글이 보내져야 다음 글을 쓴다(웹 대기열 없음).
// 보내짐은 agent가 받은 기록 증거다: 대화 기록에 같은 새 사람 글(Claude는 작업 중이면 대기열 줄이 그것이다), 기록에 대기를 남기지
// 않는 Codex가 작업 중이면 host의 전달(글+Enter)이다. 그때는 대화 끝에 대기 말로 남긴다. 둥근 단추는 도는 표시이고(t3code
// `isSendBusy`), host가 3초 동안 전달을 확인하지 않을 때만 ×(탈출구)가 된다. 누르면 host가 멈춘 단계를 답한다: 보내지 않음·
// Enter 안 침·늦음. 답이 없으면 모름이다. 전달 뒤 10초 동안 증거가 없으면 경고판에서 사람이 잠금을 푼다.
// Ctrl+Enter(사용자 10-19 15:3x, plan §8.239 후속)는 안내 없이 글을 steering으로 보낸다: 같은 보내기 길에 `steer`를 붙인다(`ctrlEnter`).
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
} from "react";
import { ArrowUp, Clock, ImagePlus, Info, LoaderCircle, TriangleAlert, X } from "lucide-react";
import { uploadChatImage } from "./api";
import type { ChatMessage, SubmitStage, InputTraceEvent, InputReceiptMethod, InputSubmitMeta } from "./gen/proto";
import { imageMention, insertLeadingMention } from "./herdr/composerImages";
import { normalizeComposerImageFile } from "./herdr/composerImageMime";
import "./herdr/Composer.css";
import { composerDrafts } from "./herdr/composerDraft.ts";
import { messageQueues } from "./herdr/messageQueue.ts";
import { holdPageRef } from "./pageHold";
import { quickReplyButtons } from "./preferences";
import { useChatShared, useTerminalToggle } from "./settings";
import { useAppearance } from "./theme";
import { receiptMethod, receiptHash, inputTraceId } from "./inputReceipt";

/** 참고 `MAX_COMPOSER_CHARS` */
const MAX_COMPOSER_CHARS = 100_000;
/** host가 이만큼 전달(글+Enter)을 확인하지 않으면 막힌 것으로 보고 탈출구(×)를 보인다. 평소 답은 0.3~0.5초다 */
export const STUCK_MS = 3000;
/** 전달된 글이 이만큼 기록에 보이지 않으면 오래 걸림(t-0387의 미확인과 같은 10초) */
const SLOW_MS = 10_000;
/** 작업 중에 보내 harness 대기열에 들어간 글. 기록에 같은 새 사람 글이 보이면 지운다.
 * `stale`: 그 harness가 이미 적었어야 할 때가 지났는데 기록에 없다(Enter가 먹혔을 수 있다) */
export interface PendingSend {
  id: number;
  trace?: string;
  hash?: string;
  text: string;
  /** 보낼 때 기록에 있던 같은 글 수(기록을 아직 못 읽었으면 null: 확인하지 못한다) */
  before: number | null;
  at: number;
  stale: boolean;
}
/** 보내는 중(host 답 전) → 막힘(3초, 탈출구) → 멈추는 중 / 전달됨(보내짐 증거 기다림) → 오래 걸림 */
type Phase = "sending" | "stuck" | "stopping" | "sent" | "slow";
interface Flight {
  id: number;
  trace: string;
  hash?: string;
  text: string;
  /** 입력창 글·빠른 답장·이 기기 대기 메시지 */
  source: "box" | "reply" | "queue";
  /** Ctrl+Enter로 작업 중에 보냈다: host가 Enter 대신 steering 키를 고른다(작업 중인 Claude는 sendNow) */
  steer?: boolean;
  ctrlEnter?: boolean;
  queueId?: string;
  attachments: Attachment[];
  phase: Phase;
  /** 이 단계가 시작된 때(타이머는 이 때부터 잰다: 늦게 걸린 타이머도 지난 만큼 빨리 끝난다) */
  at: number;
  /** 탈출구를 눌렀는데 늦었다(Enter까지 갔다) */
  escaped?: boolean;
  before: number | null;
}
interface Note {
  text: string;
  /** 실패가 아닌 알림(취소함): 빨간 띠가 아니다 */
  quiet?: boolean;
  retry?: () => void;
  /** 터미널을 보라(갔는지 모를 때) */
  inspect?: boolean;
}
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
  /** Claude는 대기열에 넣자마자 기록에 적고, Codex는 실제로 읽을 때 적는다 */
  harness: "claude" | "codex";
  /** 작업 중에 보낸 글(대화 보기가 끝에 그린다). pane마다 Term이 갖는다 */
  pending: PendingSend[];
  setPending: (update: (list: PendingSend[]) => PendingSend[]) => void;
  /** 글 + Enter를 보낸다(`steer`면 Enter 대신 host가 고른 steering 키). false는 보내지 못함(끊김), 약속은 true(받음)·글(거절 이유) */
  onSend: (text: string, steer?: boolean, trace?: string, meta?: InputSubmitMeta) => false | Promise<true | string>;
  /** 빈 입력창의 Ctrl+Enter: host의 공통 판정(작업 중인 Claude는 대기 메시지를 지금 보내기, Codex 대기 steering은 Esc) */
  onCtrlEnter: () => boolean | string;
  onInputTrace: (id: string, event: InputTraceEvent, method?: InputReceiptMethod) => void;
  /** 가장 최근 submit을 멈춘다(탈출구). 단계, null은 답이 없음(모름) */
  onCancelSend: () => Promise<SubmitStage | null>;
  /** 보낸 뒤: 대화를 바로 다시 읽는다(참고 「every send」 읽기) */
  onSent: () => void;
  messages: ChatMessage[] | null;
  /** 터미널 보기로(입력창에 글이 남았는지 본다) */
  onInspect: () => void;
  /** 보내는 중(잠김)인지 알린다: 그동안 대기 말 「지금 보내기」를 끈다(plan §8.270) */
  onBusy?: (busy: boolean) => void;
}

export function ChatComposer({
  owner,
  pane,
  touch,
  agentLabel,
  connected,
  working,
  harness,
  pending,
  setPending,
  onSend,
  onCtrlEnter,
  onInputTrace,
  onCancelSend,
  onSent,
  messages,
  onInspect,
  onBusy,
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
  const [flight, setFlight] = useState<Flight | null>(null);
  const flightRef = useRef(flight);
  flightRef.current = flight;
  const locked = flight !== null;
  useEffect(() => onBusy?.(locked), [locked, onBusy]);
  // 가는 글은 초안에서 뺐어도 기록에 오를 때까지 입력창에 보인다
  const shown = flight?.source === "box" ? flight.text : text;
  const [note, setNote] = useState<Note | null>(null);
  const messagesRef = useRef(messages);
  messagesRef.current = messages;
  const workingRef = useRef(working);
  workingRef.current = working;
  const pasted = useRef(false);
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
    if (!connected || locked || files.length === 0) return;
    if (files.some((f) => f.size === 0 || f.size > 16 * 1024 * 1024)) {
      setNote({ text: "그림은 1바이트부터 16MiB까지 붙여 넣습니다. 이 묶음은 올리지 않았습니다." });
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
        setNote({ text: error instanceof Error ? error.message : "그림을 올리지 못했습니다." });
      }
    });
  };
  const clearAttachments = (sent: Attachment[]) => {
    for (const a of sent) { removed.current.add(a.id); URL.revokeObjectURL(a.preview); }
    setAttachments((current) => current.filter((a) => !removed.current.has(a.id)));
  };
  const flightSequence = useRef(0);
  /** 멈춤 답을 기다리는 동안 온 전달 답 */
  const answered = useRef<{ id: number; result: true | string | null } | null>(null);
  const occurrences = (text: string, list = messagesRef.current, hash?: string) =>
    (list ?? []).filter((m) => receiptMethod(text, m, hash) !== null).length;
  /** 기록에 이 글의 새 사람 글이 보인다(같은 글의 출현 횟수가 늘었다, t-0387) */
  const seen = (item: { text: string; hash?: string; before: number | null }, list = messagesRef.current) =>
    item.before !== null && occurrences(item.text, list, item.hash) > item.before;
  const queued = useSyncExternalStore(messageQueues.subscribe, () =>
    messageQueues.read(owner),
  );
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
    if (shown === "") next = "one";
    else if (shown.includes("\n")) next = "more";
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
  }, [shown, lines, boxWidth, appearance.chatFontSize, appearance.uiFontSize, appearance.density]);

  // 끝난 보내기를 지운다: 이 보내기가 아직 지금 것일 때만(늦은 답이 다음 보내기를 지우지 않는다)
  const finish = (f: Flight): boolean => {
    if (flightRef.current?.id !== f.id) return false;
    flightRef.current = null;
    setFlight(null);
    return true;
  };
  /** 기록에 올랐거나 harness 대기열에 들어갔다: 입력창은 이미 비었고 첨부를 놓는다 */
  const settle = (f: Flight, fallback?: InputReceiptMethod): void => {
    if (!finish(f)) return;
    const method = seen(f) ? [...(messagesRef.current ?? [])].reverse().map((m) => receiptMethod(f.text, m, f.hash)).find((m) => m !== null) : fallback;
    if (method) onInputTrace(f.trace, "receipt", method);
    onInputTrace(f.trace, "unlocked");
    if (f.source === "box") { clearAttachments(f.attachments); pasted.current = false; }
  };
  /** 가지 않았거나 갔는지 모른다: 글을 입력창(대기 메시지는 그 칸)에 돌려놓고 까닭을 말한다 */
  const fail = (f: Flight, note: Note, method: InputReceiptMethod = "unknown"): void => {
    if (!finish(f)) return;
    onInputTrace(f.trace, "failed", method);
    if (f.source === "queue" && f.queueId !== undefined) {
      messageQueues.endSend(owner, f.queueId);
      setQueueError({ id: f.queueId, text: note.text });
      return;
    }
    // 보내기 시작에 비운 초안: 그 사이 다른 탭이 쓴 글이 있으면 그것을 둔다
    if (f.source === "box") composerDrafts.set(draftKey, (current) => current === "" ? f.text : current);
    setNote(note);
  };
  const step = (f: Flight, phase: Phase, more: Partial<Flight> = {}): void => {
    const current = flightRef.current;
    if (current?.id !== f.id) return;
    const next: Flight = { ...current, ...more, phase, at: Date.now() };
    flightRef.current = next;
    setFlight(next);
    if (phase === "slow" || phase === "stuck") onInputTrace(f.trace, phase);
  };
  /** 바로 보낸다. 보내짐까지 입력창은 잠긴다 */
  const deliver = (given: Omit<Flight, "id" | "trace" | "phase" | "at" | "before">): void => {
    setNote(null);
    const f: Flight = { ...given, id: ++flightSequence.current, trace: inputTraceId(), phase: "sending", at: Date.now(), before: messagesRef.current === null ? null : occurrences(given.text) };
    flightRef.current = f;
    setFlight(f);
    // 가는 글은 초안에 두지 않는다: 잠긴 채 새로 읽어도 같은 글이 보통 초안으로 돌아와 두 번 가지 않는다
    if (f.source === "box") setText("");
    onInputTrace(f.trace, "preparing");
    const again = f.source === "queue" ? undefined : () => send(f.source === "reply" ? f.text : undefined, f.steer);
    void (async () => {
      f.hash = await receiptHash(f.text);
      if (flightRef.current?.id !== f.id) return;
      f.before = messagesRef.current === null ? null : occurrences(f.text, messagesRef.current, f.hash);
      let sent: false | Promise<true | string>;
      try {
        sent = onSend(f.text, f.steer, f.trace, { pasted: pasted.current, attachments: f.attachments.length, ctrl_enter: f.ctrlEnter ?? f.steer ?? false });
      } catch {
        sent = false;
      }
      if (sent === false) {
        fail(f, { text: "보내지 못했습니다: 연결이 끊겼습니다. 아무것도 보내지 않았고 글은 그대로 있습니다.", retry: again });
        return;
      }
      sent.then((result) => {
        if (flightRef.current?.id !== f.id) return;
        // 멈춤 답을 기다리는 동안 온 전달 답: 멈춤이 늦었으면 그때 이어 받는다
        if (flightRef.current.phase === "stopping") { answered.current = { id: f.id, result }; return; }
        answer(f, result, again);
      }, () => {
        if (flightRef.current?.id !== f.id) return;
        if (flightRef.current.phase === "stopping") { answered.current = { id: f.id, result: null }; return; }
        answer(f, null, again);
      });
    })().catch(() => fail(f, { text: "수신 확인을 준비하지 못했습니다. 글은 보내지 않았습니다." }, "unknown"));
  };
  /** 전달 답: true는 글과 Enter가 pane에 들어갔다, 글은 거절 이유, null은 끊기거나 답이 없어 모른다 */
  const answer = (f: Flight, result: true | string | null, again?: () => void): void => {
    if (result === null) {
      fail(f, { text: "전송 여부를 확인하지 못했습니다. 다시 보내기 전에 터미널을 확인하세요.", inspect: true }, "connection");
      return;
    }
    if (result !== true) {
      fail(f, { text: result, retry: again }, "rejected");
      return;
    }
    onSent();
      if (f.source === "queue" && f.queueId !== undefined) {
        messageQueues.remove(owner, f.queueId);
        messageQueues.endSend(owner, f.queueId);
      }
      // 보낼 때 대화 기록을 읽지 못했으면(Claude 첫 hook 전 등) 기록 증거가 올 수 없다: host의 전달이 보내짐이다
      if (seen(f) || f.before === null) settle(f, f.before === null ? "no_transcript" : undefined);
      else if (harness === "codex" && workingRef.current) {
        // Codex는 작업 중에 받은 글을 다음 도구 호출 뒤 읽을 때에야 기록한다: host의 전달이 보내짐이고 대화 끝 대기 말로 기다린다
        setPending((list) => [...list, { id: f.id, trace: f.trace, hash: f.hash, text: f.text, before: f.before, at: Date.now(), stale: false }]);
        onInputTrace(f.trace, "pending", "codex_delivery");
        settle(f, "codex_delivery");
    } else step(f, "sent");
  };
  /** 탈출구: 전달 전에 막힌 보내기를 멈추고 글을 돌려받는다. host가 확실히 말할 때만 「보내지 않음」 */
  const escape = (): void => {
    const f = flightRef.current;
    if (f?.phase !== "stuck") return;
    onInputTrace(f.trace, "cancel");
    step(f, "stopping");
    void onCancelSend().then((stage) => {
      const late = answered.current?.id === f.id ? answered.current : null;
      answered.current = null;
      if (stage === "too_late") {
        // Enter까지 갔다: 이미 온 전달 답을 잇거나 곧 올 답을 기다린다(탈출구는 다시 보이지 않는다)
        step(f, "sending", { escaped: true });
        if (late) answer(f, late.result);
        return;
      }
      fail(f, stage === "not_sent"
        ? { text: "멈췄습니다. 에이전트에 보내지 않았고 글은 그대로 있습니다.", quiet: true }
        : stage === "no_enter"
          ? { text: "멈췄습니다. 제출하지 않았습니다(Enter를 보내지 않음). 에이전트 입력창에 글이 남았을 수 있으니 터미널을 확인하세요.", inspect: true }
          : { text: "멈췄습니다. 전송 여부를 확인하지 못했습니다. 다시 보내기 전에 터미널을 확인하세요.", inspect: true });
      textareaRef.current?.focus();
    });
  };
  /** 오래 걸림에서 「잠금 풀기」: 갔을 수 있는 글을 고칠 수 있게 돌려놓는다 */
  const unlock = (): void => {
    const f = flightRef.current;
    if (f === null || !finish(f)) return;
    onInputTrace(f.trace, "unlock");
    if (f.source === "box") composerDrafts.set(draftKey, (current) => current === "" ? f.text : current);
    textareaRef.current?.focus();
  };

  // 3초 동안 전달 답이 없으면 탈출구, 전달됨에서 10초 동안 보내짐 증거가 없으면 오래 걸림. 늦게 걸린 타이머도 시작 때부터 잰다
  useEffect(() => {
    if (flight?.phase !== "sending" && flight?.phase !== "sent") return;
    // 멈춤이 늦어 돌아온 보내기는 다시 탈출구를 보이지 않는다
    if (flight.phase === "sending" && flight.escaped) return;
    const [wait, phase]: [number, Phase] = flight.phase === "sending" ? [STUCK_MS, "stuck"] : [SLOW_MS, "slow"];
    const timer = window.setTimeout(() => step(flight, phase), Math.max(0, flight.at + wait - Date.now()));
    return () => window.clearTimeout(timer);
  }, [flight]);
  // 기록에 같은 새 사람 글이 보이면(늦게라도) 비우고 푼다. 대기 말도 기록에 오른 것은 지운다
  useEffect(() => {
    if (flight && (flight.phase === "sent" || flight.phase === "slow") && seen(flight, messages)) settle(flight);
    for (const p of pending.filter((p) => p.trace && seen(p, messages))) {
      const method = [...(messages ?? [])].reverse().map((m) => receiptMethod(p.text, m, p.hash)).find((m) => m !== null);
      onInputTrace(p.trace!, "receipt", method ?? undefined);
    }
    if (pending.some((p) => seen(p, messages))) setPending((list) => list.filter((p) => !seen(p, messages)));
  }, [messages, flight, pending]);
  // 대기 말(Codex 작업 중)이 기록에 없을 때: 작업이 끝나고 10초 뒤에도 없으면 Enter가 먹혔을 수 있다
  const idleSince = useRef<number | null>(working ? null : Date.now());
  if (working) idleSince.current = null;
  else idleSince.current ??= Date.now();
  useEffect(() => {
    const due = (p: PendingSend) => idleSince.current === null ? Infinity : Math.max(p.at, idleSince.current) + SLOW_MS;
    const next = Math.min(...pending.filter((p) => !p.stale).map(due));
    if (!Number.isFinite(next)) return;
    const timer = window.setTimeout(() => {
      for (const p of pending.filter((p) => !p.stale && due(p) <= Date.now()))
        if (p.trace) onInputTrace(p.trace, "slow", "unknown");
      setPending((list) => list.map((p) => !p.stale && due(p) <= Date.now() ? { ...p, stale: true } : p));
    }, Math.max(0, next - Date.now()));
    return () => window.clearTimeout(timer);
  }, [pending, working]);

  /** `steer`: Ctrl+Enter로 작업 중에 보낸다(지금 턴에 끼워 넣는다). 붙잡지 않는다 */
  const send = (given?: string, steer = false, ctrlEnter = false): void => {
    const message = (given ?? textRef.current).trim();
    if (message === "" || flightRef.current !== null || uploading || noRoom || composing || !connected) return;
    const sentAttachments = attachments;
    setNote(null);
    // 참고 composerSend: 작업 중이고 자동 전송이 꺼져 있으면 붙잡는다(hold, 설정으로 켜는 이 기기의 보관함)
    if (working && !autoSendQueued && !steer) {
      messageQueues.add(owner, message);
      if (given === undefined) { setText(""); clearAttachments(sentAttachments); }
      return;
    }
    deliver({ text: message, source: given === undefined ? "box" : "reply", attachments: given === undefined ? sentAttachments : [], ctrlEnter, ...(steer ? { steer } : {}) });
  };
  // Ctrl+Enter(사용자 10-19 15:3x, plan §8.239 후속): 안내 줄 없이 글을 steering으로 보낸다. 작업 중이면 지금 턴에 끼워 넣고
  // (host가 Claude는 sendNow, Codex는 Enter = steer 대기열을 고른다), 쉬는 중이면 보통 보내기(Enter)와 같다. 잠금·전달 확인·멈춤은
  // 보내기와 같은 길이다. 빈 입력창: 작업 중이면 대기 메시지를 지금 보낸다. 이 기기에 붙잡은 것이 있으면 그 맨 앞을 steering으로,
  // 없으면 harness 대기열을 host 판정으로(Claude sendNow, Codex Esc) 보낸다. 쉬는 중이면 아무 일 없다
  const ctrlEnter = (): void => {
    const blocked = (reason: InputReceiptMethod) => onInputTrace(inputTraceId(), "ctrl_enter_blocked", reason);
    if (flightRef.current !== null) { blocked("locked"); return; }
    if (!connected) { blocked("connection"); return; }
    if (textRef.current.trim() !== "") {
      if (composing) { blocked("composing"); return; }
      if (uploading) { blocked("uploading"); return; }
      if (noRoom) { blocked("no_room"); return; }
      send(undefined, workingRef.current, true);
      return;
    }
    if (!workingRef.current) { onCtrlEnter(); return; }
    const first = messageQueues.read(owner).find((m) => m.text.trim() !== "" && !messageQueues.isSending(m.id));
    if (first) sendQueued(first.id, first.text, true);
    else {
      const result = onCtrlEnter();
      if (typeof result === "string") setNote({ text: result });
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    if (event.key === "Escape" && flightRef.current?.phase === "stuck") {
      event.preventDefault();
      escape();
      return;
    }
    // IME는 제 키를 갖는다. WebKit은 확정 Enter를 compositionend 뒤 229로 보낼 수 있다
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) {
      if (event.key === "Enter" && event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey)
        onInputTrace(inputTraceId(), "ctrl_enter_blocked", "composing");
      return;
    }
    if (event.key !== "Enter") return;
    // Ctrl+Enter는 보내기 설정·터치와 상관없는 steering이다(macOS Cmd+Enter 보내기는 그대로). 줄을 바꾸지 않는다
    if (event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey) {
      event.preventDefault();
      if (!event.repeat) ctrlEnter();
      return;
    }
    // 터치 화면은 참고의 coarse 판별을 쓰되 사용자 요청대로 Enter는 항상 줄바꿈이다.
    if (touch) return;
    const shouldSend = enterSends
      ? !event.shiftKey && !event.metaKey && !event.ctrlKey
      : (event.metaKey || event.ctrlKey) && !event.shiftKey;
    if (!shouldSend) return;
    event.preventDefault();
    send();
  };

  const sendQueued = (id: string, message: string, steer = false): void => {
    if (flightRef.current !== null || !connected || !messageQueues.beginSend(owner, id)) return;
    setQueueError(null);
    deliver({ text: message, source: "queue", queueId: id, attachments: [], ...(steer ? { steer } : {}) });
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
                      flight !== null ||
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
                disabled={!connected || locked}
                onClick={() => send(reply)}
              >
                {reply}
              </button>
            ))}
          </div>
        )}
        {/* 메시지가 가지 못한 까닭(취소는 조용한 알림): 상자 위끝에, 상자와 함께 읽힌다 */}
        {note !== null && flight?.phase !== "slow" && (
          // 확실히 보내지 않은 멈춤은 조용히, 갔는지 모르면 경고판, 가지 못했으면 빨간 띠
          <div className={`composer-note${note.quiet ? " is-quiet" : note.inspect ? " is-slow" : ""}`} role={note.quiet ? "status" : "alert"} data-testid="chat-note">
            {note.quiet ? <Info className="composer-note-icon" aria-hidden="true" /> : <TriangleAlert className="composer-note-icon" aria-hidden="true" />}
            <span className="composer-note-text">{note.text}</span>
            {(note.retry || note.inspect) && <span className="composer-note-actions">
              {note.inspect && <button type="button" className="composer-note-action" onClick={onInspect}>터미널 보기</button>}
              {note.retry && <button type="button" className="composer-note-action" disabled={!connected} onClick={note.retry}>다시 보내기</button>}
            </span>}
          </div>
        )}
        {flight?.phase === "slow" && (
          <div className="composer-note is-slow" role="alert" data-testid="chat-note">
            <Clock className="composer-note-icon" aria-hidden="true" />
            <span className="composer-note-text">전달했지만 10초가 지나도 대화 기록에 보이지 않습니다. 에이전트 입력창에 글이 남아 있는지 확인하세요. 잠금을 풀어도 이미 간 글은 되돌리지 않습니다.</span>
            <span className="composer-note-actions">
              <button type="button" className="composer-note-action" onClick={onInspect}>터미널 보기</button>
              <button type="button" className="composer-note-action" onClick={unlock}>잠금 풀기</button>
            </span>
          </div>
        )}
        {/* 보내는 동안은 상태 문장이 단추 옆 줄에 서야 해서 늘 두 줄 배치다 */}
        <div ref={surfaceRef} data-lines={flight ? "more" : lines} className={`composer-surface${dragging ? " is-dragging" : ""}${locked ? " is-locked" : ""}`}
          onDragOver={(event) => {
            if (!event.dataTransfer.types.includes("Files")) return;
            event.preventDefault();
            event.dataTransfer.dropEffect = connected && !locked ? "copy" : "none";
            setDragging(connected && !locked);
          }}
          onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDragging(false); }}
          onDrop={(event) => { event.preventDefault(); setDragging(false); uploadImages(Array.from(event.dataTransfer.files)); }}
        >
          {attachments.length > 0 && <div className="composer-attachments" aria-label="첨부한 그림">
            {attachments.map((a) => <div className={`composer-attachment is-${a.state}`} key={a.id}>
              <img src={a.preview} alt={a.file.name} />
              <span className="composer-attachment-state">{a.state === "ready" ? "첨부됨" : a.state === "error" ? "실패" : a.state === "no-room" ? "글을 줄여 주세요" : "올리는 중"}</span>
              <button type="button" aria-label={`${a.file.name} 첨부 삭제`} disabled={locked} onClick={() => {
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
            value={shown}
            readOnly={locked}
            aria-readonly={locked || undefined}
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
              pasted.current = true;
              const images = Array.from(event.clipboardData.items).filter((item) => item.kind === "file").map((item) => item.getAsFile()).filter((file): file is File => file !== null);
              if (images.length === 0) return;
              event.preventDefault();
              if (!locked) uploadImages(images);
            }}
            onChange={(event) => {
              setText(event.target.value);
              setNote(null);
            }}
          />
          <input ref={picker} type="file" accept="image/png,image/jpeg,image/gif,image/webp" multiple hidden
            onChange={(event) => { const files = Array.from(event.target.files ?? []); event.target.value = ""; uploadImages(files); }} />
          <div className="composer-controls composer-controls-left">
            <button type="button" className="icon-button" aria-label="그림 첨부" title="그림 첨부" disabled={!connected || uploading || locked}
              onPointerDown={(event) => event.preventDefault()} onClick={() => picker.current?.click()}><ImagePlus aria-hidden="true" /></button>
          </div>
          {flight && (
            <div className="composer-status is-send-state" data-hint="" role="status" data-testid="chat-send-state" data-state={flight.phase}>
              {flight.phase === "sending" ? <span><span className="is-strong">보내는 중</span> · 전달을 확인하는 중</span>
                : flight.phase === "stuck" ? <span><span className="is-strong">전달 확인이 늦습니다</span> · ×로 멈추고 글 돌려받기</span>
                : flight.phase === "stopping" ? <span><span className="is-strong">멈추는 중…</span></span>
                : flight.phase === "sent" ? <span><span className="is-strong">전달됨</span> · 대화에 오르면 풀립니다 · 취소할 수 없음</span>
                : <span><span className="is-strong">전달됨 · 10초 넘음</span> · 취소할 수 없음</span>}
            </div>
          )}
          <div className="composer-controls composer-controls-right">
            {flight?.phase === "stuck" ? (
              // 탈출구: 전달 전에 막혔을 때만 보내기 자리에 나온다(취향 취소가 아니다)
              <button
                type="button"
                className="composer-action composer-cancel"
                aria-label="멈추고 글 돌려받기"
                title="전달 전에 막혔습니다. 멈추고 글을 돌려받습니다"
                onPointerDown={(event) => event.preventDefault()}
                onClick={escape}
              >
                <X aria-hidden="true" />
              </button>
            ) : (
              <button
                type="button"
                className="composer-action composer-send"
                aria-label={locked ? "보내는 중 · 취소할 수 없음" : "메시지 보내기"}
                title={locked ? "보내는 중입니다" : "메시지 보내기"}
                aria-busy={locked || undefined}
                disabled={!connected || locked || uploading || noRoom || composing || text.trim().length === 0}
                // focus는 글 칸에 남는다: 떠나면 휴대폰 자판이 닫혀 누르는 사이 단추가 옮겨 간다
                onPointerDown={(event) => event.preventDefault()}
                onClick={() => send()}
              >
                {locked ? <LoaderCircle className="composer-send-progress" aria-hidden="true" /> : <ArrowUp aria-hidden="true" />}
              </button>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
