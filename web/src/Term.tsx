import { ResponsibilityBadge } from "./ResponsibilityBadge";
// pane 하나의 터미널 보기. WebSocket 하나가 대화형 front 하나다. 조작 권한이 있으면 언제나 입력된다(herdr-web-ui와 같다, §8.33).
// PTY 크기는 「지금 쓰는 화면」(lease)이 정한다. 이 탭이 보이고 focus가 있을 때만 크기를 건다:
// - 이 화면이 지금 쓰는 화면이거나 아무도 쓰지 않으면 칸 크기에 맞춘다(열 때·창 크기가 바뀔 때).
// - focus를 얻거나 다시 보이면, 또는 터미널을 누르거나 입력하면 이 화면이 지금 쓰는 화면이 되어 크기를 다시 건다.
// - 탭이 가려지거나 대화 보기로 바꾸면 바로 놓는다(다른 화면이 크기를 되찾는다). 다른 화면이 쓰는 동안은 PTY 크기 그대로 왼쪽 위에 그린다.
// 그림을 붙여 넣거나(끌어 놓기·「그림」 버튼) 하면 대상 머신 세션 폴더에 저장하고, harness 입력창이면 경로를 넣는다.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type MutableRefObject, type ReactNode, type RefObject } from "react";
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import type { ChatMessage, DeviceRole, LeaseView, PaneView, SubmitStage, InputSubmitMeta, InputReceiptMethod, TermClientMsg, TermServerMsg } from "./gen/proto";
import { b64decode, b64encode, call, REVOKED_CLOSE_CODE, reportSignedOut, wsUrl } from "./api";
import { Icon } from "./icons";
import { ChatPanel } from "./Chat";
import { inputTraceId } from "./inputReceipt";
import { ChatComposer, type PendingSend } from "./ChatComposer";
import { paneAgentStatus } from "./sidebarModel";
import { refusalText, steerAvailability, steerSettled, STEER_IDLE, STEER_SLOW_MS, type SteerState } from "./steerQueued";
import { onThemeChange, token } from "./theme";
import { matchCompositionView } from "./compositionView";
import { adjustTerminalGlyphs } from "./herdr/terminalGlyphs.ts";
import { keyboardHoldsGrid, keyboardUp } from "./herdr/keyboard.ts";
import { attachTrace, trace } from "./inputTrace";
import {
  clipboardKey,
  hasModifiers,
  keyFromData,
  modifyOtherKeysLevel,
  navigationSequence,
  NO_STICKY_MODIFIERS,
  physicalKey,
  terminalChord,
  type StickyModifiers,
} from "./herdr/keys.ts";
import { keyBarInputSequence, type KeyBarKeyItem } from "./herdr/keyBar.ts";
import { chordSequence, IS_MAC } from "./keyChord";
import { KeyBar } from "./KeyBar";
import { TerminalInput } from "./TerminalInput";
import {
  getTerminalWheelSpeed,
  setTerminalFontSize,
  setTerminalInputMode,
  shortcutOverrides,
  terminalToggle,
  useKeyBarItems,
  useDefaultView,
  useMedia,
  useTerminalFontFamily,
  useTerminalFontSize,
  useTerminalInputMode,
} from "./settings";
import { loadFontStack, sanitizeFontFamily } from "./herdr/fontFamily.ts";
import { pinchFontSize } from "./herdr/pinchFontSize.ts";
import { matchHerdrWidths } from "./herdr/terminalWidths.ts";
import { AgentMark } from "./herdr/AgentMark.tsx";
import { finalStep, HERDR_MOUSE_SCROLL_LINES, wheelSteps } from "./herdr/wheelSteps.ts";
import { parseOsc52 } from "./herdr/osc52.ts";
import { applyToDraft, draftIsEmpty, EMPTY_DRAFT, restoreDraft, type InputDraft } from "./herdr/draft.ts";
import { fileUriPath, hyperlinkFilter, isWebLink, terminalFileLinkProvider } from "./herdr/terminalFileLinks.ts";
import { isAppShortcut, modKeyLabel } from "./herdr/shortcuts.ts";
import { disposeAfterPendingFrame } from "./herdr/terminalDispose.ts";
import { pasteText } from "./pasteText.ts";
import { readTerminalDraft } from "./herdr/terminalDraft.ts";
import type { PaneControls } from "./actions";
import { lensesFor, pickedLens, rememberLens } from "./paneLens";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { ArrowDown, RotateCcw, Search, TriangleAlert } from "lucide-react";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";

/** 함께 넣은 터미널 글꼴(style.css @font-face) */
const WEB_FONT = '"Meslotendard Web"';

/**
 * 설정한 터미널 글꼴을 기본 목록 앞에 붙인다(herdr-web-ui fontFamily.ts `terminalFontStack`). 기본 목록은 Meslotendard로 연다(plan §8.31).
 * 이 기기에 없는 글꼴은 그대로 기본 목록으로 넘어간다.
 */
function fontStack(base: string, chosen: string): string {
  const family = sanitizeFontFamily(chosen);
  return family === "" ? base : `${family}, ${base}`;
}

/** 한 번에 보내는 조각(host 상한과 같다) */
const CHUNK = 512 * 1024;
const MAX_IMAGE = 16 << 20;
/** 답이 오지 않는 입력줄 보내기를 그만 기다리는 때(herdr-web-ui ws.ts `SUBMIT_TIMEOUT_MS`): 글은 입력줄에 남는다 */
const SUBMIT_TIMEOUT_MS = 90_000;
/** 멈춤 답을 기다리는 때: 이만큼 오지 않으면 연결이 죽은 것으로 보고 「모름」(plan §8.247) */
const CANCEL_REPLY_MS = 2000;
/** 붙잡은 입력이 있는 동안 다른 입력을 거절하는 말(herdr-web-ui PaneTerminal 1375줄) */
const HELD_FIRST = "보내지 않았다: 붙잡은 입력을 먼저 보내거나 버린다.";
/** 이 탭을 지금 쓰는 중: 보이고 focus가 있다. 아니면 PTY 크기를 걸지 않는다. */
const inUse = () => document.visibilityState === "visible" && document.hasFocus();
/** 「다른 웹 화면과 같이 쓰기」: 다른 창으로 옮기고 이만큼 뒤 보기만 한다(herdr-web-ui PaneTerminal `RELEASE_AFTER_MS`) */
const RELEASE_AFTER_MS = 1000;

/**
 * 붙여 넣기에서 올릴 파일들(herdr-web-ui PaneTerminal 1449~1458줄): 클립보드에 글이 있으면 글이다(xterm이 붙여 넣는다,
 * 복사한 경로도 글로 남는다). 파일만 있을 때 그 파일 전부.
 */
function pastedFiles(data: DataTransfer | null | undefined): File[] {
  if (!data || data.getData("text/plain")) return [];
  return Array.from(data.files);
}

/** stopped: host가 출력을 따라오지 못한 이 화면을 4008로 닫았다. 「다시 연결」까지 다시 붙지 않는다 */
type Link = "connecting" | "open" | "closed" | "stopped";
/** host가 출력을 따라오지 못한 연결을 닫는 code(herdr-web-ui shared/terminal-flow.ts, aaa-host output.rs) */
const OUTPUT_STALLED_CLOSE_CODE = 4008;
/** 다시 보인 뒤 이만큼 안에 온 4008은 가려져 있던 탓으로 친다(herdr-web-ui ws.ts SHOWN_GRACE_MS) */
const SHOWN_GRACE_MS = 5_000;

const ANSI = [
  "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
  "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
] as const;

/** 터미널 색은 style.css token을 그대로 읽는다(밝음·어두움마다 ANSI 16색이 따로 있다) */
function termTheme(): ITheme {
  const t: ITheme = {
    background: token("--term-bg"),
    foreground: token("--term-fg"),
    cursor: token("--term-cursor"),
    cursorAccent: token("--term-bg"),
    selectionBackground: token("--term-selection"),
  };
  ANSI.forEach((k, n) => {
    t[k] = token(`--ansi-${n}`);
  });
  return t;
}

/** 이 화면의 터미널 기본 색(`rrggbb`). session host가 프로그램의 색 질의(OSC 10/11)에 이 색으로 답한다(plan §8.187) */
function termColors(): { fg: string; bg: string } {
  // CSS 빌드가 #ffffff를 #fff로 줄여도 protocol의 rrggbb 계약을 지킨다.
  const hex = (name: string) => token(name).replace(/^#/, "").replace(/^([0-9a-f])([0-9a-f])([0-9a-f])$/i, "$1$1$2$2$3$3");
  return { fg: hex("--term-fg"), bg: hex("--term-bg") };
}

export function TermView({
  pane,
  role,
  title,
  agent,
  crumb,
  status,
  dock,
  controls,
  onOpenFile,
  onOpenLink,
  autoSelected = false,
}: {
  pane: PaneView;
  role: DeviceRole;
  /** 목록과 같은 이름(thread pane이면 thread 제목) */
  title?: string;
  /** pane에서 도는 coding agent(머리 제목 앞 표시, herdr-web-ui App.tsx 1402줄) */
  agent?: string | null;
  crumb?: string;
  status?: ReactNode;
  /** 입력 위에 붙는 질문 카드 */
  dock?: ReactNode;
  /** 단축키가 이 화면에 시키는 일(입력으로 가기·한 화면씩 굴리기·대화 전환, herdr-web-ui PaneTerminal 2675~2692줄) */
  controls?: MutableRefObject<PaneControls | null>;
  /** 파일 경로 링크를 눌렀다: 파일 보기를 연다(결정 20 D7, plan §8.125). 없으면(보기 권한) 링크로 그리지 않는다 */
  onOpenFile?: (path: string) => void;
  /** 대화 글의 링크를 앱 안에서 연다(휴대폰의 보고서·앱 주소, plan §8.300): 열었으면 true */
  onOpenLink?: (href: string) => boolean;
  /** App이 스스로 고른 pane(주소의 pane이 없어 첫 줄로 왔다): 사용자가 고를 때까지 키보드를 가져가지 않는다(참고 autoSelected) */
  autoSelected?: boolean;
}) {
  const box = useRef<HTMLDivElement>(null);
  const openFileRef = useRef(onOpenFile);
  openFileRef.current = onOpenFile;
  const scrollRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  /** 출력이 멈춘 연결을 다시 붙인다(「다시 연결」 단추) */
  const reattachRef = useRef<() => void>(() => {});
  const readonlyRef = useRef(true);
  const [link, setLink] = useState<Link>("connecting");
  // 짧은 연결 안내는 깜빡이지 않게 한다. 재시도 중 closed ↔ connecting은 같은 기다림이다.
  const linking = link === "connecting" || link === "closed";
  const [showLink, setShowLink] = useState(false);
  useEffect(() => {
    setShowLink(false);
    if (!linking) return;
    const timer = window.setTimeout(() => setShowLink(true), 500);
    return () => window.clearTimeout(timer);
  }, [linking, pane.id]);
  const [readonly, setReadonly] = useState(true);
  const [reason, setReason] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [size, setSize] = useState<[number, number]>([pane.cols, pane.rows]);
  // 글꼴이 바뀌어 cell 크기가 달라지면 자동 맞추기를 다시 한다
  const [fontGen, setFontGen] = useState(0);
  const [me, setMe] = useState<string | null>(null);
  const [lease, setLease] = useState<LeaseView | null>(pane.lease);
  const [attach, setAttach] = useState<{ text: string; ok: boolean } | null>(null);
  // Claude·Codex pane은 터미널 대신 대화 기록으로도 본다(읽기 전용)
  const chatable = pane.agent?.harness === "claude" || pane.agent?.harness === "codex";
  // 처음 보기(herdr-web-ui App.tsx `storedView`, paneLens.ts): 손으로 고른 보기, 없으면 설정 「패널 기본 화면」. agent인지 모르는 동안
  // (agent 이름은 있으나 harness 상태가 아직 오지 않음)은 agent로 센다
  const coarse = useMedia("(pointer: coarse)");
  const defaultView = useDefaultView();
  const lensKey = `${pane.machine_id}:${pane.id}`;
  const [picked, setPicked] = useState(() => pickedLens(lensKey));
  // 설정 기본이 바뀌면 고른 보기는 잊혔다(SettingsDialog `forgetPaneViews`)
  useEffect(() => setPicked(pickedLens(lensKey)), [defaultView, lensKey]);
  const chatView = (picked ?? lensesFor(defaultView).of(lensKey, chatable ? true : agent ? null : false, coarse)) === "chat";
  // 대화 입력창이 보낸 수: 대화를 바로 다시 읽는다(Chat `readNow`)
  const [chatSent, setChatSent] = useState(0);
  // 대화 안 찾기를 여는 요청(머리 돋보기·팔레트, plan §8.308). ⌘F는 ChatPanel이 직접 받는다
  const [findRequest, setFindRequest] = useState(0);
  const [chatMessages, setChatMessages] = useState<ChatMessage[] | null>(null);
  // 작업 중에 보내 harness 대기열에 들어간 글(대화 보기 끝의 대기 말). 터미널 보기로 갔다 와도 남게 이 pane 화면이 갖는다
  const [pendingSends, setPendingSends] = useState<PendingSend[]>([]);
  // 대기 메시지 「지금 보내기」(plan §8.271): host가 안다고 알렸나(front.steer_queued, 1.45), 단추 상태, 대화 입력창이 보내는 중인가
  const [steerKnown, setSteerKnown] = useState(false);
  const [steer, setSteer] = useState<SteerState>(STEER_IDLE);
  const [composerBusy, setComposerBusy] = useState(false);
  // 대기 말 수(기록의 Claude 대기열 + 이 화면이 붙든 것, 기록에 없는 것 빼고): 보낸 뒤 줄면 끼워 넣어진 것이다
  const pendingCount = (chatMessages?.filter((m) => m.pending).length ?? 0) + pendingSends.filter((p) => !p.stale).length;
  const pendingCountRef = useRef(pendingCount);
  pendingCountRef.current = pendingCount;
  const agentNameRef = useRef("Claude");
  agentNameRef.current = pane.agent?.harness === "codex" ? "Codex" : "Claude";
  const setChatView = (chat: boolean) => {
    const lens = chat ? "chat" : "terminal";
    rememberLens(lensKey, lens);
    setPicked(lens);
  };
  const fileRef = useRef<HTMLInputElement>(null);
  const canControl = role !== "reader";
  // 다른 front가 지금 쓰는 화면이다(이 연결의 ID를 받기 전에는 판단하지 않는다). 입력은 막지 않는다.
  const heldByOther = me !== null && lease !== null && lease.front !== me;
  // 입력 방식(herdr-web-ui PaneTerminal 358~361줄): 기본은 바로 입력, 입력줄은 이 기기의 선택이다.
  const directTyping = useTerminalInputMode() !== "line";
  // 중단된 pane(아래 `stopped`)은 입력줄 대신 자리 표시의 「다시 열기」다
  const inputLine = !directTyping && !(chatView && chatable) && canControl && pane.state !== "interrupted";
  const inputLineRef = useRef(inputLine);
  // 사용자가 App이 스스로 고른 pane을 골랐다(같은 줄을 다시 눌러 pane은 그대로): 이제 터미널이 키보드를 받는다(PaneTerminal 2209~2216줄)
  const autoSelectedRef = useRef(autoSelected);
  useEffect(() => {
    const wasAuto = autoSelectedRef.current;
    autoSelectedRef.current = autoSelected;
    if (wasAuto && !autoSelected && !window.matchMedia("(pointer: coarse)").matches && !document.querySelector('[aria-modal="true"]')) termRef.current?.focus();
  }, [autoSelected]);
  inputLineRef.current = inputLine;
  // 글자 크기(설정, 이 기기). 핀치 중에는 손가락을 따르는 크기다(herdr-web-ui PaneTerminal `baseFontRef`).
  const terminalFontSize = useTerminalFontSize();
  const baseFontRef = useRef(terminalFontSize);
  const terminalFontFamily = useTerminalFontFamily();
  const fontFamilyRef = useRef(terminalFontFamily);
  fontFamilyRef.current = terminalFontFamily;
  /** 기본 글꼴 목록(--font-term)과, 넣은 WOFF2를 받았는지 */
  const baseFamilyRef = useRef("");
  const webFontReadyRef = useRef(false);
  const pinchBadgeRef = useRef<HTMLSpanElement>(null);
  const chatViewRef = useRef(false);
  chatViewRef.current = chatView && chatable;
  // 입력줄 조합 중: ⌨ 전환 단추만 기다린다(키 막대는 그대로 쓴다)
  const lineComposingRef = useRef(false);
  const [lineComposing, setLineComposingState] = useState(false);
  const setLineComposing = useCallback((active: boolean) => {
    lineComposingRef.current = active;
    setLineComposingState(active);
  }, []);

  // 끊긴 동안 친 글(herdr-web-ui PaneTerminal 363~379·1356~1424줄, lib/draft.ts): 줄을 세워 몰래 다시 보내지 않고 붙잡아 두었다가
  // 다시 붙으면 사람이 「보내기」·「버리기」를 고른다. Enter·줄바꿈은 붙잡지 않는다(보내기는 Enter를 누르지 않는다).
  // 이 기기에 머신·pane마다 24시간 남는다(끊긴 동안 다시 열어도 잃지 않는다).
  const draftKey = `herdr-web-ui:terminal-draft:${pane.machine}:${pane.id}`;
  const [draft, setDraftState] = useState<InputDraft>(() => {
    try {
      return restoreDraft(localStorage.getItem(draftKey), Date.now());
    } catch {
      return EMPTY_DRAFT;
    }
  });
  const draftRef = useRef(draft);
  const setDraft = useCallback((next: (previous: InputDraft) => InputDraft) => {
    draftRef.current = next(draftRef.current);
    setDraftState(draftRef.current);
  }, []);
  useEffect(() => {
    try {
      if (draftIsEmpty(draft)) localStorage.removeItem(draftKey);
      else localStorage.setItem(draftKey, JSON.stringify({ ...draft, at: Date.now() }));
    } catch {
      // private mode: 이 화면 동안만 남는다
    }
  }, [draft, draftKey]);
  /** 클립보드에 쓴 결과를 잠깐 알린다(참고 `noteClipboard`, 2.5초) */
  const [clipNote, setClipNote] = useState<string | null>(null);
  const clipTimerRef = useRef<number | undefined>(undefined);
  const noteClipboard = useCallback((note: string) => {
    window.clearTimeout(clipTimerRef.current);
    setClipNote(note);
    clipTimerRef.current = window.setTimeout(() => setClipNote(null), 2500);
  }, []);
  useEffect(() => () => window.clearTimeout(clipTimerRef.current), []);
  /** 보내지 못한 입력의 까닭(참고 `inputError`). 닫기로 지운다 */
  const [inputError, setInputError] = useState<string | null>(null);
  const ended = pane.state !== "running";
  // 중단된 pane: 자리 표시가 까닭과 「다시 열기」를 보인다. 다시 열면 host가 새 pane으로 바꾸고 화면은 그 pane을 따라간다(App, QA Q19)
  const stopped = pane.state === "interrupted";
  const [resuming, setResuming] = useState(false);
  const [resumeError, setResumeError] = useState<string | null>(null);
  const resume = async () => {
    setResuming(true);
    setResumeError(null);
    try {
      await call({ op: "pane_resume", pane: pane.id });
    } catch (e) {
      setResumeError(e instanceof Error ? e.message : String(e));
    } finally {
      setResuming(false);
    }
  };
  // 다른 창을 쓰는 동안 보기만 한다(설정 「다른 웹 화면과 같이 쓰기」, herdr-web-ui PaneTerminal 288~297·1725~1918줄 `released`).
  // 크기를 놓고 이 화면은 PTY 크기 그대로 그린다. 이 탭을 누르거나 focus가 돌아오면 다시 입력하고 크기를 건다.
  const releasedRef = useRef(false);
  const [released, setReleasedState] = useState(false);
  const setReleased = useCallback((next: boolean) => {
    releasedRef.current = next;
    setReleasedState(next);
  }, []);
  // 위로 굴려 맨 아래가 아니다: 「맨 아래로 이동」 단추(herdr-web-ui TerminalViewportTools `terminal-jump-bottom`)
  const [scrolledUp, setScrolledUp] = useState(false);
  const canControlRef = useRef(canControl);
  canControlRef.current = canControl;
  const endedRef = useRef(ended);
  endedRef.current = ended;

  const send = (m: TermClientMsg) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(m));
  };
  const input = (data: string) => {
    trace("send", { data, readonly: readonlyRef.current });
    if (!readonlyRef.current) send({ t: "input", data });
  };
  /**
   * 답(submitted)을 기다리는 입력줄 보내기(herdr-web-ui ws.ts `submits`). 보낸 연결이 닫히면 확인 못 함으로 끝난다:
   * 그 글은 들어갔을 수도 있으므로 입력줄에 두고 터미널을 보게 한다.
   */
  const submitsRef = useRef(new Map<number, { ws: WebSocket; done: (result: true | string | null) => void }>());
  /** 멈춤 요청의 답(submit id → 단계, null은 답이 오지 않음) */
  const cancelsRef = useRef(new Map<number, (stage: SubmitStage | null) => void>());
  const submitIdRef = useRef(0);
  /** 「지금 보내기」 답을 기다리는 것(id → 보낸 연결). 연결이 닫히면 확인 못 함이다 */
  const steersRef = useRef(new Map<number, WebSocket>());
  const steerIdRef = useRef(0);
  /** 올리는 중인 파일의 결과(attached·error·끊김)를 기다리는 것 */
  const attachWaitRef = useRef<(() => void) | null>(null);
  const attachDone = () => {
    const done = attachWaitRef.current;
    attachWaitRef.current = null;
    done?.();
  };
  /** 그림을 조각으로 보낸다. 결과(attached·error)가 WebSocket으로 올 때까지 기다린다. */
  const attachFile = async (f: File) => {
    if (readonlyRef.current) {
      setAttach({ text: "지금은 입력할 수 없어 그림을 보내지 않았다", ok: false });
      return;
    }
    if (f.size === 0 || f.size > MAX_IMAGE) {
      setAttach({ text: `그림은 ${MAX_IMAGE >> 20}MiB까지 붙여 넣는다`, ok: false });
      return;
    }
    if (!crypto.subtle) {
      setAttach({ text: "이 주소(HTTPS 아님)에서는 그림을 보낼 수 없다", ok: false });
      return;
    }
    const bytes = new Uint8Array(await f.arrayBuffer());
    const sha256 = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)), (b) =>
      b.toString(16).padStart(2, "0"),
    ).join("");
    setError(null);
    setAttach({ text: `그림 보내는 중(${Math.ceil(bytes.length / 1024)}KB)`, ok: true });
    const done = new Promise<void>((resolve) => {
      attachWaitRef.current = resolve;
    });
    for (let off = 0; off < bytes.length; off += CHUNK) {
      send({ t: "attach", offset: off, total: bytes.length, sha256, data: b64encode(bytes.subarray(off, off + CHUNK)) });
    }
    await done;
  };
  /**
   * 터미널로 가는 입력 하나(PaneTerminal onData 1353~1424줄). 붙잡은 입력이 있으면 글·⌫·Enter는 거기에 더하고
   * chord·바뀐 Enter는 거절한다. ^C·Esc·Tab·화살표는 그대로 간다(중단은 붙잡은 입력을 기다리지 않는다).
   * 입력할 수 없으면(host·agent 끊김, 붙는 중) 글은 붙잡고 chord는 「아직 키를 받을 수 없다」고 알린다.
   */
  const deliver = (data: string, how: { chord?: boolean; modifiedEnter?: boolean } = {}) => {
    if (!canControlRef.current) return;
    const pasted = pastingRef.current || data.startsWith("\x1b[200~");
    if (!draftIsEmpty(draftRef.current)) {
      if (how.chord || how.modifiedEnter || (pasted && applyToDraft(EMPTY_DRAFT, data) === EMPTY_DRAFT)) {
        setInputError(HELD_FIRST);
        return;
      }
      if (pasted || /[\r\n\x7f\b]/.test(data) || applyToDraft(EMPTY_DRAFT, data) !== EMPTY_DRAFT) {
        setDraft((previous) => applyToDraft(previous, data));
        return;
      }
    }
    if (!readonlyRef.current) return input(data);
    // 끝난 세션에는 아무것도 붙잡지 않는다
    if (endedRef.current) return;
    if (how.chord) {
      setInputError("보내지 않았다: 터미널이 아직 키를 받을 수 없다.");
      return;
    }
    setDraft((previous) => applyToDraft(previous, data));
  };
  const sendDraft = () => {
    const text = draftRef.current.text;
    if (readonlyRef.current || text.length === 0) return;
    input(text);
    setDraft(() => EMPTY_DRAFT);
    setInputError(null);
  };
  const discardDraft = () => {
    setDraft(() => EMPTY_DRAFT);
    setInputError(null);
  };
  const attachRef = useRef(attachFile);
  attachRef.current = attachFile;
  // 여러 파일은 하나씩 놓은 순서대로 올린다. 겹친 붙여넣기·끌어 놓기도 그 순서다(PaneTerminal 1425~1448줄).
  // 한 묶음에 상한을 넘는 파일이 하나라도 있으면 아무것도 올리지 않는다. 경로는 host가 파일마다 따옴표로 넣는다.
  const fileQueueRef = useRef<Promise<void>>(Promise.resolve());
  const uploadFiles = (files: File[]) => {
    if (files.length === 0) return;
    fileQueueRef.current = fileQueueRef.current.then(async () => {
      const tooBig = files.find((f) => f.size === 0 || f.size > MAX_IMAGE);
      if (tooBig) {
        setAttach({ text: `${tooBig.name || "파일"}: 그림은 ${MAX_IMAGE >> 20}MiB까지 붙여 넣는다(이 묶음은 올리지 않았다)`, ok: false });
        return;
      }
      for (const f of files) await attachRef.current(f);
    });
  };
  const uploadRef = useRef(uploadFiles);
  uploadRef.current = uploadFiles;

  // 키 막대의 유지 키(Ctrl·Alt·Shift, herdr-web-ui PaneTerminal 271~276·2237~2245줄): 다시 누를 때까지 켜져 있다(래치).
  // pane이 바뀌면(다시 mount) 연결이 끊기면 풀린다. 하드웨어 키와 화면 키보드 글자 모두에 chord로 붙는다.
  const [modifiers, setModifiers] = useState<StickyModifiers>(NO_STICKY_MODIFIERS);
  const modifiersRef = useRef(modifiers);
  modifiersRef.current = modifiers;
  // 프로그램이 요청한 modifyOtherKeys 단계(CSI > 4 ; n m). Ctrl+Enter 인코딩에 쓴다.
  const otherKeysRef = useRef(0);
  // 붙여넣기 중: 붙여 넣은 글에는 유지 키를 붙이지 않는다(PaneTerminal `pasting`)
  const pastingRef = useRef(false);
  const chordCtx = () => ({
    applicationCursorKeys: termRef.current?.modes.applicationCursorKeysMode ?? false,
    modifyOtherKeys: otherKeysRef.current,
    isMac: IS_MAC,
  });

  // 터미널 조합 상태(herdr-web-ui PaneTerminal과 같다): 조합 중이거나 방금 끝난 조합의 확정 글에는 유지 키를 붙이지 않는다.
  // 조합 중에는 키 막대를 회색으로 둔다(PaneTerminal 3017줄).
  const composingTermRef = useRef(false);
  const commitPendingRef = useRef(false);
  const [composing, setComposing] = useState(false);

  // 이 터미널이 화면에 있다. 다른 pane으로 바꾸면 화면에서 지우기 전에(layout effect 정리) 내린다: 지워지는 textarea의
  // blur가 조합 중 글자를 확정하는데, 버려진 그 글이 옛 pane으로 가면 안 된다(herdr-web-ui PaneTerminal presentedRef 검사).
  const presentRef = useRef(true);
  useLayoutEffect(() => {
    presentRef.current = true;
    return () => {
      presentRef.current = false;
    };
  }, [pane.id]);

  // 커서 위치 보고(ESC[r;cR)는 session host가 이미 답했다. xterm.js의 자동 답은 보내지 않는다.
  const fromTerminal = (data: string, modifiedEnter = false) => {
    if (!presentRef.current) return;
    if (/^\x1b\[\d+;\d+R$/.test(data)) return;
    // 화면 키보드는 keydown 없이 글을 넣는다: 한 글자 키에는 유지 키 chord를 붙인다(PaneTerminal 1360~1363줄)
    const m = modifiersRef.current;
    if (hasModifiers(m) && !pastingRef.current && !composingTermRef.current && !commitPendingRef.current) {
      const key = keyFromData(data);
      const chord = key !== null ? terminalChord(key, m) : null;
      if (chord === "ctrl+enter") return void pressCtrlEnter();
      const seq = chord !== null ? chordSequence(chord, chordCtx()) : null;
      if (seq !== null) return deliver(seq, { chord: true });
    }
    deliver(data, { modifiedEnter });
  };

  // steering 판정은 host의 detection 입력줄이 한다(plan §8.239 후속: 줄바꿈 없음, 줄바꿈은 Shift+Enter). 웹 입력창(입력줄·대화
  // 입력창)에 글이 있으면 그 입력창이 Ctrl+Enter로 그 글을 steering으로 보낸다(`sendTerminalLine` steer, 사용자 10-19 15:3x).
  // 여기는 빈 입력창·터미널·키 막대의 Ctrl+Enter다. 키 막대가 누른 Ctrl+Enter는 입력줄에 남은 글을 건드리지 않고 아무것도 보내지
  // 않는다. 안내 줄은 없앴다(사용자 「어떻게 해라고 보여주는건 좀 과한것 같아」). `chat`: 대화 입력창이 불렀다(입력줄은 숨었다)
  const blockedCtrlEnter = (method: InputReceiptMethod) => {
    const ws = wsRef.current;
    if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: "input_trace", id: inputTraceId(), event: "ctrl_enter_blocked", method } satisfies TermClientMsg));
  };
  const pressCtrlEnter = (chat = false): boolean | string => {
    const traceId = inputTraceId();
    const blocked = blockedCtrlEnter;
    if (!canControlRef.current || readonlyRef.current) { blocked("readonly"); return false; }
    if (composingTermRef.current || commitPendingRef.current || lineComposingRef.current) { blocked("composing"); return false; }
    if (!draftIsEmpty(draftRef.current)) {
      blocked("held_draft");
      setInputError(HELD_FIRST);
      return HELD_FIRST;
    }
    if (!chat && inputLineRef.current && readTerminalDraft(`${pane.machine}:${pane.id}`).length > 0) { blocked("held_draft"); return false; }
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return false;
    autoFit.current(true);
    ws.send(JSON.stringify({ t: "ctrl_enter", trace: traceId } satisfies TermClientMsg));
    return true;
  };

  // 대기 메시지 「지금 보내기」(plan §8.271): 글은 다시 보내지 않고 host에 키 판정을 청한다. 답(steer_queued)이 결과다
  const pressSteer = () => {
    const ws = wsRef.current;
    if (!canControlRef.current || readonlyRef.current) return;
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      setSteer({ phase: "failed", text: "연결되지 않아 보내지 않았습니다", inspect: false });
      return;
    }
    const id = ++steerIdRef.current;
    steersRef.current.set(id, ws);
    setSteer({ phase: "sending", id });
    ws.send(JSON.stringify({ t: "steer_queued", id, trace: inputTraceId() } satisfies TermClientMsg));
  };
  // 대화 정리 중(plan §8.309, host `AgentStatus.compacting`): 작업 중일 때만 뜻이 있다
  const compacting = paneAgentStatus(pane) === "working" && pane.agent?.compacting === true;
  // 보낸 뒤: 대기 말이 줄면 끝, 10초 줄지 않으면 알린다. 대기 말이 모두 없어지면 지난 실패 줄도 지운다(다음 대기 말에 남지 않게).
  // 정리 중에는 harness가 대기 글을 읽지 않으므로 세지 않고, 정리가 끝난 때부터 다시 잰다
  const wasCompacting = useRef(compacting);
  useEffect(() => {
    const ended = wasCompacting.current && !compacting;
    wasCompacting.current = compacting;
    if (ended && steer.phase === "sent") {
      setSteer({ ...steer, at: Date.now() });
      return;
    }
    if (steer.phase === "sent") {
      const settled = steerSettled(steer, pendingCount, Date.now(), compacting);
      if (settled !== steer) {
        setSteer(settled);
        return;
      }
      if (compacting) return;
      const timer = window.setTimeout(() => setSteer((s) => steerSettled(s, pendingCountRef.current, Date.now())), Math.max(0, steer.at + STEER_SLOW_MS - Date.now()));
      return () => window.clearTimeout(timer);
    }
    if (steer.phase === "failed" && pendingCount === 0) setSteer(STEER_IDLE);
  }, [steer, pendingCount, compacting]);
  const steerOpen = steerKnown && canControl && !readonly ? steerAvailability(paneAgentStatus(pane), composerBusy, compacting) : "hidden";

  useEffect(() => {
    // 모든 기기가 설정한 크기(기본 13px)로 연다. 좁은 화면만 11px로 줄이던 것은 없앴다(herdr-web-ui와 같다)
    const fontSize = baseFontRef.current;
    // xterm은 처음 연 글꼴로 cell 크기를 잰다. 넣은 WOFF2가 아직 안 받아졌으면 그것을 뺀 목록으로 열고,
    // 받아지면 원래 목록으로 바꿔 다시 재게 한다(그 뒤 자동 맞추기가 다시 돈다).
    const family = token("--font-term") || "ui-monospace, Menlo, monospace";
    const webFont = `${fontSize}px ${WEB_FONT}`;
    const ready = !family.includes(WEB_FONT) || document.fonts.check(webFont);
    baseFamilyRef.current = family;
    webFontReadyRef.current = ready;
    const term = new Terminal({
      cols: pane.cols,
      rows: pane.rows,
      fontSize,
      lineHeight: 1.15,
      fontFamily: ready ? family : family.replace(`${WEB_FONT}, `, ""),
      // aaa-vt SCROLLBACK과 같다: 붙을 때는 최근 2000줄(REPLAY_HISTORY)만 오고, 위로 굴리면 앞 덩어리를 받아 붙인다(plan §8.67)
      scrollback: 10_000,
      convertEol: false,
      theme: termTheme(),
      // herdr-web-ui PaneTerminal 539~562줄: 블록 커서 깜빡임(프로그램의 DECSCUSR는 그대로 바꾼다),
      // macOS에서 Option+누름은 마우스 보고 중에도 선택
      cursorStyle: "block",
      cursorBlink: true,
      macOptionClickForcesSelection: true,
      // term.unicode(아래 글자 폭)는 proposed API다
      allowProposedApi: true,
      // OSC 8 링크(herdr-web-ui PaneTerminal 552~560줄): file URI는 파일 보기로, http(s)는 새 탭으로. 누름과 놓음이 다 링크 위면
      // xterm이 연다: 일부를 끌어 고르는 끌기·오른쪽 누름은 「열기」가 아니다(linkPressed)
      linkHandler: {
        activate: (event, uri) => {
          if (!linkPressed(event)) return;
          const path = fileUriPath(uri);
          if (path !== null) openFileRef.current?.(path);
          else if (isWebLink(uri)) window.open(uri, "_blank", "noopener,noreferrer");
        },
        allowNonHttpProtocols: true,
      },
    });
    const linkPressed = (event: MouseEvent): boolean => event.button === 0 && !term.hasSelection();
    const offTheme = onThemeChange(() => {
      term.options.theme = termTheme();
      send({ t: "colors", ...termColors() });
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    // 앞 기록을 받아 붙일 때 지금 버퍼를 다시 쓰는 데 쓴다(xterm은 버퍼 앞에 줄을 끼울 수 없다)
    const serializer = new SerializeAddon();
    term.loadAddon(serializer);
    // 글자 폭(herdr-web-ui terminalWidths.ts 그대로, PaneTerminal 583줄): xterm 기본 Unicode 6 폭이면 이모지 대부분이 한 칸이라
    // 프로그램·session host(vt100, 지금 Unicode 폭)와 줄이 어긋난다. grapheme addon과 참고의 보정으로 이모지는 두 칸,
    // 이모지 묶음(ZWJ·피부색·국기·⚠️)도 두 칸이다. vt100은 묶음을 글자마다 세어 다시 그릴 때만 다를 수 있다(plan §8.53).
    matchHerdrWidths(term);
    term.open(box.current!);
    // FitAddon은 scrollback이 있으면 세로 scrollbar 자리를 빼고 칸을 정한다. overlay scrollbar(휴대폰·트랙패드 Mac)는 폭이 0이라
    // 기본값 15px을 빼서 오른쪽 두 칸쯤이 늘 비었다. 참고는 scrollback 0이라 빼지 않는다(PaneTerminal 542~546줄). aaa xterm은
    // 기록을 가지므로(plan §8.67) scrollback을 두고, 그 자리를 0으로 둔다(scrollbar는 그리지 않는다: style.css, plan §8.124).
    // FitAddon이 읽는 내부 값이고 Viewport가 만들 때 한 번만 정한다. xterm이 바뀌어 없어지면 웹 E2E 「넓은 창」이 실패한다
    const viewportOf = (term as unknown as { _core?: { viewport?: { scrollBarWidth: number } } })._core?.viewport;
    if (viewportOf) viewportOf.scrollBarWidth = 0;
    const el = box.current!;
    // 칸 높이가 바뀌는 사이(넣은 글꼴이 연 뒤에 받아질 때·글자 크기를 바꿀 때) xterm 5.5 Viewport는 화면이 먼저 커져
    // 브라우저가 줄인 scrollTop을 옛 칸 높이로 읽고, 몇 줄 위로 굴린 것(사용자 스크롤)으로 친다. 그러면 맨 아래를 보던
    // 화면이 다섯 줄쯤 위에 멈추고 새 출력도 따라가지 않았다(CI eeee514·52caa0a, plan §8.60). 화면(DOM)이 맨 아래인데
    // 버퍼가 위면 그 사건이다: 맨 아래로 되돌린다. xterm의 처리 뒤에 돌도록 open 뒤에 단다.
    const viewport = el.querySelector<HTMLElement>(".xterm-viewport");
    const onViewportScroll = () => {
      const buf = term.buffer.active;
      if (viewport && viewport.scrollTop >= viewport.scrollHeight - viewport.clientHeight - 1 && buf.viewportY < buf.baseY) term.scrollToBottom();
    };
    viewport?.addEventListener("scroll", onViewportScroll);
    // 맨 아래가 아닌 보통 화면이면 「맨 아래로 이동」을 보인다. 참고는 herdr 기록 위치(offset_from_bottom)를 묻고, aaa는 xterm 기록을 본다.
    const markScrolled = () => {
      const buf = term.buffer.active;
      setScrolledUp(buf.type === "normal" && buf.viewportY < buf.baseY);
    };
    viewport?.addEventListener("scroll", markScrolled, { passive: true });
    const scrolledSub = term.onScroll(markScrolled);
    const bufferSub = term.buffer.onBufferChange(markScrolled);
    // 휠(herdr-web-ui PaneTerminal 665~694줄): 이 화면 것이 아닌 grid는 휠이 브라우저 몫이라 칸을 옮긴다. 휠은 선택을 지운다.
    // 참고는 보고가 꺼졌을 때 xterm을 막는다(herdr가 늘 보고를 켜 그럴 일이 없다). aaa는 herdr 서버가 휠을 나누는 것
    // (server/pane_input.rs 153~183줄: 보고면 프로그램에, alternate screen이면 화살표, 아니면 scrollback)과 같은 xterm 기본에 맡긴다.
    // 끌어 고르는 중 휠은 고르는 끌기 몫이다(dragWheel, 참고 883~893줄): 선택을 지우지 않고 기록을 굴린다(아래 onMouseDown 곁).
    // xterm은 휠 사건 하나에 delta와 상관없이 mouse 보고를 하나 보낸다: 휠 속도가 1보다 크면 진짜 휠을 그만큼 다시 보낸다
    // (참고 681~690줄). 다시 보낸 것과 터치에서 바꾼 휠은 isTrusted가 아니라 되풀이하지 않는다. 트랙패드 핀치(Ctrl 휠)도 한 번만.
    // 참고는 보고가 켜졌을 때만 되풀이하지만 거기서는 휠이 늘 herdr로 가는 보고라, 속도가 herdr의 기록 굴리기·화살표에도 든다.
    // aaa는 기록·화살표를 xterm이 맡으므로 보고가 아니어도 되풀이해 같은 결과를 낸다(plan §8.57).
    term.attachCustomWheelEventHandler((event) => {
      if (drag) {
        dragWheel(event);
        return false;
      }
      if (adoptedRef.current) return false;
      if (term.hasSelection()) term.clearSelection();
      if (event.isTrusted && event.target && !event.ctrlKey) {
        for (let sent = 1; sent < getTerminalWheelSpeed(); sent += 1) {
          event.target.dispatchEvent(
            new WheelEvent("wheel", {
              bubbles: true,
              cancelable: true,
              deltaX: event.deltaX,
              deltaY: event.deltaY,
              deltaMode: event.deltaMode,
              clientX: event.clientX,
              clientY: event.clientY,
              ctrlKey: event.ctrlKey,
              altKey: event.altKey,
              shiftKey: event.shiftKey,
              metaKey: event.metaKey,
            }),
          );
        }
      }
      return true;
    });
    // 데스크톱은 pane을 열면 바로 친다(PaneTerminal 2072줄). 터치 기기는 키보드를 올리지 않게, 모달 중에는 그것을 두고 하지 않는다.
    // App이 스스로 고른 pane도 사용자가 고를 때까지 두고(참고 autoSelected) 아래 effect가 받는다
    if (!autoSelectedRef.current && !window.matchMedia("(pointer: coarse)").matches && !document.querySelector('[aria-modal="true"]')) term.focus();
    // herdr-web-ui PaneTerminal(aa624cb) 588~605줄: xterm textarea의 확정이 이 bubbling 처리보다 먼저 줄을 선다.
    // 그 확정은 글로 두고(유지 키 없음), 새 조합이 시작되면 이 표시는 지운다.
    let compositionEndTimer: number | null = null;
    commitPendingRef.current = false;
    const compositionStart = () => {
      if (compositionEndTimer !== null) window.clearTimeout(compositionEndTimer);
      compositionEndTimer = null;
      commitPendingRef.current = false;
      composingTermRef.current = true;
      setComposing(true);
    };
    const compositionEnd = () => {
      if (compositionEndTimer !== null) window.clearTimeout(compositionEndTimer);
      commitPendingRef.current = true;
      composingTermRef.current = false;
      setComposing(false);
      compositionEndTimer = window.setTimeout(() => {
        compositionEndTimer = null;
        commitPendingRef.current = false;
      }, 0);
    };
    box.current!.addEventListener("compositionstart", compositionStart);
    box.current!.addEventListener("compositionend", compositionEnd);
    // herdr-web-ui terminalGlyphs.ts 그대로(PaneTerminal.tsx 609줄): 칸보다 좁은 글자를 키우고 가운데 두며,
    // 조합 상자의 크기·자간을 그 줄에 맞춘다. 조합 상자 색과 세로 자리는 aaa가 그 칸에 맞춘다(compositionView.ts).
    const stopGlyphs = adjustTerminalGlyphs(term);
    // 화면 키보드가 grid를 붙잡은 동안 새 출력이 오면 커서 줄을 따라간다(herdr-web-ui PaneTerminal 1133줄)
    const parsedSub = term.onWriteParsed(() => {
      if (document.activeElement === term.textarea) trace("write");
      followCursor.current();
      // 받은 출력을 다 읽은 뒤의 버퍼 자리(맨 위 줄·baseY). 올려 둔 동안 아래에 줄이 붙으면 그리기(onRender, data-view-*)는 오지 않아,
      // 시험이 「출력이 와도 올려 둔 자리」를 이 표지 뒤에 본다(웹 E2E 07 「터미널 스크롤」)
      const buf = term.buffer.active;
      el.dataset.parsedTop = String(buf.viewportY);
      el.dataset.parsedBase = String(buf.baseY);
    });
    // 실기 입력 사건 기록(켜져 있을 때만 모은다, inputTrace.ts)
    const stopTrace = attachTrace(term.textarea!, scrollRef.current!);
    // ── 키 처리(herdr-web-ui PaneTerminal 585·620~664·1053~1076·1284~1335줄) ──
    // 웹 링크는 http(s)만 새 탭으로 연다(줄이 링크처럼 보여도 다른 scheme은 열지 않는다)
    term.loadAddon(new WebLinksAddon((_event, uri) => {
      if (isWebLink(uri)) window.open(uri, "_blank", "noopener,noreferrer");
    }));
    // 글 속 파일 경로(herdr-web-ui PaneTerminal 586줄, terminalFileLinks.ts 그대로): 보기 권한 화면은 열 수 없어 그리지 않는다
    const fileLinkProvider = terminalFileLinkProvider(
      () => term.buffer.active,
      (path, event) => {
        if (linkPressed(event)) openFileRef.current?.(path);
      },
    );
    const fileLinks = term.registerLinkProvider({
      provideLinks: (line, callback) => (openFileRef.current ? fileLinkProvider.provideLinks(line, callback) : callback(undefined)),
    });
    const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
    let disposed = false;
    const copySelection = () => {
      const text = term.getSelection();
      if (!text) return;
      // 사용자 동작 안에서는 execCommand가 clipboard 권한이 없어도 된다(HTTP·권한 거절에서도)
      if (document.execCommand("copy")) return noteClipboard("클립보드에 복사했다");
      if (!navigator.clipboard) return noteClipboard("브라우저가 클립보드 쓰기를 막았다");
      void navigator.clipboard.writeText(text).then(
        () => !disposed && noteClipboard("클립보드에 복사했다"),
        () => !disposed && noteClipboard("브라우저가 클립보드 쓰기를 막았다"),
      );
    };
    // 그냥 끌어서 고르기(herdr-web-ui PaneTerminal 791~1046줄): 프로그램이 마우스 보고를 켜면 xterm은 누름을 모두 PTY로 넘기고
    // Shift(macOS는 Option)를 눌러야 고른다. 왼쪽 끌기는 그 키를 누른 것처럼 고르고, 놓으면 복사한다(herdr TUI와 같다).
    // 제자리에서 놓은 누름은 클릭이라 프로그램에 다시 보낸다(forwardClick). 터치는 그대로다.
    // 참고는 xterm에 scrollback이 없어 화면을 넘는 끌기를 herdr 기록 API로 따라간다. aaa xterm은 자기 scrollback이 있어
    // 끌기가 가장자리를 넘으면 xterm이 굴린다. 그래서 그 부분(기록 행 추적·herdr 원문 복사)은 옮기지 않았다.
    type Cell = { row: number; col: number };
    type ClickKeys = { ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean };
    let drag: { pane: string; anchor: Cell; click: ClickKeys | null } | null = null;
    const cellAt = (event: MouseEvent): { cell: Cell; edge: -1 | 0 | 1 } => {
      const rect = (term.element?.querySelector(".xterm-screen") ?? el).getBoundingClientRect();
      const col = Math.floor((event.clientX - rect.left) / (rect.width / term.cols));
      const row = Math.floor((event.clientY - rect.top) / (rect.height / term.rows));
      return {
        cell: { row: Math.max(0, Math.min(term.rows - 1, row)), col: Math.max(0, Math.min(term.cols - 1, col)) },
        edge: row < 0 ? -1 : row >= term.rows ? 1 : 0,
      };
    };
    const onMouseDown = (event: MouseEvent) => {
      // 프로그램에 다시 보내는 클릭(forwardClick)은 새 끌기가 아니다
      if (event.button !== 0 || !event.isTrusted) return;
      if ((event as MouseEvent & { sourceCapabilities?: { firesTouchEvents?: boolean } }).sourceCapabilities?.firesTouchEvents) return;
      if (!term.element?.contains(event.target as Node)) return;
      const reporting = term.modes.mouseTrackingMode !== "none";
      const selecting = isMac ? event.altKey : event.shiftKey;
      // 선택을 지우는 누름, 링크를 여는 누름은 프로그램의 클릭이 아니다
      const onLink = term.element?.querySelector(".xterm-screen")?.classList.contains("xterm-cursor-pointer") === true;
      const click: ClickKeys | null =
        reporting && !selecting && !onLink && !term.hasSelection()
          ? { ctrlKey: event.ctrlKey, metaKey: event.metaKey, altKey: event.altKey, shiftKey: event.shiftKey }
          : null;
      if (reporting) Object.defineProperty(event, isMac ? "altKey" : "shiftKey", { value: true });
      drag = { pane: pane.id, anchor: cellAt(event).cell, click };
      dragWheelPixels = 0;
      dragPointer = { clientX: event.clientX, clientY: event.clientY };
    };
    // window capture: xterm의 document 처리보다 먼저 본다
    const onMouseMove = (event: MouseEvent) => {
      const d = drag;
      if (d) dragPointer = { clientX: event.clientX, clientY: event.clientY };
      if (!d || d.click === null) return;
      const { cell, edge } = cellAt(event);
      // 제 칸을 떠난 누름은 돌아와도 끌기다(옆으로 나가면 cellAt이 끝 열로 붙이므로 따로 본다)
      const grid = term.element?.querySelector(".xterm-screen")?.getBoundingClientRect();
      const beside = grid !== undefined && (event.clientX < grid.left || event.clientX >= grid.right);
      if (beside || edge !== 0 || cell.row !== d.anchor.row || cell.col !== d.anchor.col) d.click = null;
    };
    // 선택이 되지 않은 누름은 클릭이다(Claude Code diff의 닫기 단추, TUI 메뉴). 고르기 키 없이 다시 보내 xterm이
    // 프로그램이 요청한 보고 형식으로 인코딩하게 한다.
    const forwardClick = (event: MouseEvent, keys: ClickKeys) => {
      const target = event.target;
      if (!(target instanceof Element) || !term.element?.contains(target)) return;
      const init = { ...keys, bubbles: true, cancelable: true, view: window, button: 0, detail: 1, clientX: event.clientX, clientY: event.clientY, screenX: event.screenX, screenY: event.screenY };
      target.dispatchEvent(new MouseEvent("mousedown", { ...init, buttons: 1 }));
      target.dispatchEvent(new MouseEvent("mouseup", { ...init, buttons: 0 }));
    };
    const onWindowBlur = () => {
      drag = null;
    };
    // 고르는 끌기 중 휠(herdr-web-ui PaneTerminal 883~893줄 `dragWheel`): 선택은 두고 xterm 기록을 줄 단위로 굴린 뒤, 마지막 마우스
    // 자리로 끌기를 한 번 더 알려 선택 끝이 굴린 줄까지 따라오게 한다. 참고는 herdr 기록을 굴리고 다시 그린다(xterm scrollback 0).
    // 마우스 보고 중에도 프로그램에 휠을 보내지 않는다(끌기는 고르기다). alternate screen은 기록이 없어 굴릴 것이 없다.
    let dragWheelPixels = 0;
    let dragPointer: { clientX: number; clientY: number } | null = null;
    const dragWheel = (event: WheelEvent) => {
      const lineHeight = cellHeight();
      const { steps, rest } = wheelSteps(
        dragWheelPixels + (event.deltaMode === WheelEvent.DOM_DELTA_LINE ? event.deltaY * lineHeight : event.deltaY),
        lineHeight,
      );
      dragWheelPixels = rest;
      if (steps === 0) return;
      term.scrollLines(steps);
      if (dragPointer) document.dispatchEvent(new MouseEvent("mousemove", { ...dragPointer, buttons: 1, bubbles: true }));
    };
    // window bubble: xterm의 document 처리 뒤, 아직 놓는 동작 안이다(타이머를 쓰면 일부 브라우저가 클립보드 권한을 잃는다)
    const onMouseUp = (event: MouseEvent) => {
      const d = drag;
      if (!d || event.button !== 0) return;
      drag = null;
      const released = cellAt(event).cell;
      if (d.click && presentRef.current && !term.hasSelection() && released.row === d.anchor.row && released.col === d.anchor.col) {
        forwardClick(event, d.click);
        return;
      }
      if (term.hasSelection()) copySelection();
    };
    el.addEventListener("mousedown", onMouseDown, { capture: true });
    window.addEventListener("mousemove", onMouseMove, { capture: true });
    window.addEventListener("blur", onWindowBlur);
    window.addEventListener("mouseup", onMouseUp);
    // Ctrl+V는 브라우저 붙여넣기에 맡긴다(herdr-web-ui PaneTerminal 610~660, ^V를 pane에 보내지 않는다). Mac 그림은 Cmd+V의
    // paste 사건이 기존 첨부로 보낸다: Ctrl+V에서 Async Clipboard를 읽던 것은 Safari 붙여넣기 확인 말풍선 때문에 없앴다(plan §8.232 후속).
    // 선택이 있을 때 Ctrl+C는 복사한다(없으면 ^C)
    term.attachCustomKeyEventHandler((event) => {
      // 앱 단축키는 앱 몫이다(PaneTerminal 621줄): 창의 capture 처리기(herdr/shortcuts.ts useShortcuts)는 브라우저 기본만 막으므로
      // xterm이 그 키를 pane에 보내지 않게 먼저 거른다
      if (isAppShortcut(event, shortcutOverrides())) return false;
      // 유지 키 + 하드웨어 키(PaneTerminal 620~651줄): 비라틴 자판 글자는 자판 위치로 읽는다(ㅊ on KeyC + Ctrl = Ctrl+C)
      const held = modifiersRef.current;
      if (
        hasModifiers(held) &&
        !readonlyRef.current &&
        !composingTermRef.current &&
        !commitPendingRef.current &&
        !event.isComposing &&
        event.keyCode !== 229 &&
        !event.metaKey
      ) {
        const combined = {
          ctrl: held.ctrl || event.ctrlKey,
          alt: held.alt || event.altKey,
          shift: held.shift || event.shiftKey,
        };
        const chord = terminalChord(physicalKey(event.key, event.code), combined);
        const navigation = navigationSequence(event.key, combined);
        if (navigation !== null) {
          if (event.type === "keydown") {
            event.preventDefault();
            deliver(navigation);
          }
          return false;
        }
        // 진짜 클립보드 단축키는 브라우저 몫이다. 유지 Ctrl + 친 v는 터미널 chord다.
        const letter = clipboardKey(event.key, event.code);
        const clipboard =
          event.ctrlKey && !event.altKey && /^(c|v)$/.test(letter) && (letter === "v" || term.hasSelection());
        if (chord !== null && !clipboard) {
          if (event.type === "keydown") {
            event.preventDefault();
            if (chord === "ctrl+enter") { if (!event.repeat) pressCtrlEnter(); return false; }
            const seq = chordSequence(chord, chordCtx());
            if (seq !== null) deliver(seq, { chord: true });
          }
          return false;
        }
      }
      if (!event.ctrlKey || event.altKey || event.metaKey) return true;
      const key = clipboardKey(event.key, event.code);
      if (key === "v") return false;
      if (key === "c" && term.hasSelection()) {
        if (event.type === "keydown") {
          event.preventDefault();
          copySelection();
          term.clearSelection();
        }
        return false;
      }
      return true;
    });
    // OSC 52: pane 프로그램(vim·tmux·Claude Code)의 클립보드 쓰기. 질의는 답하지 않는다.
    // 설정 「패널에서 클립보드로 복사」(기본 켬, PaneTerminal 1050~1063줄)를 끄면 받기만 하고 버린다: pane 안 아무 프로세스나
    // (agent의 도구 호출도) 사용자가 다음에 붙여 넣을 글을 심을 수 있다.
    // OSC 8: 이 앱이 열 수 없는 링크(OmO `omo-panel:` 줄)는 링크로 그리지 않는다(herdr-web-ui PaneTerminal 1066줄, #739).
    // 열린 링크 상태는 RIS(replay 맨 앞 `ESC c`, 프로그램의 RIS)와 함께 끝난다(참고는 term.reset()과 함께 reset)
    const hyperlinks = hyperlinkFilter();
    const osc8 = term.parser.registerOscHandler(8, (payload) => hyperlinks.handler(payload));
    const ris = term.parser.registerEscHandler({ final: "c" }, () => {
      hyperlinks.reset();
      return false;
    });
    const osc52 = term.parser.registerOscHandler(52, (payload) => {
      const text = terminalToggle("paneClipboard") ? parseOsc52(payload) : null;
      if (text !== null)
        void navigator.clipboard?.writeText(text).then(
          () => !disposed && noteClipboard("클립보드에 복사했다"),
          () => !disposed && noteClipboard("브라우저가 클립보드 쓰기를 막았다"),
        );
      return true;
    });
    // modifyOtherKeys(CSI > 4 ; n m): xterm.js는 처리하지 않으므로 듣기만 하고 Ctrl+Enter를 그 형식으로 보낸다
    otherKeysRef.current = 0;
    const otherKeys = (final: "m" | "n") =>
      term.parser.registerCsiHandler({ prefix: ">", final }, (params) => {
        otherKeysRef.current = modifyOtherKeysLevel(otherKeysRef.current, final, params);
        return false;
      });
    const otherKeysSet = otherKeys("m");
    const otherKeysOff = otherKeys("n");
    // xterm.js는 Shift+Enter·Ctrl+Enter를 그냥 Enter로 친다: Shift+Enter는 Alt+Enter 줄바꿈으로, Ctrl+Enter는 host가
    // 감지 입력줄로 steering을 정한다(줄바꿈 없음). macOS Cmd+⌫는 줄 지우기(Ctrl+U). onKey는 IME 확정 글 뒤, 그 키의 onData 바로 앞에 온다.
    let enterAs: string | null = null;
    let ctrlEnter = false;
    let ctrlEnterRepeat = false;
    let commandBackspace = false;
    const keySub = term.onKey(({ key, domEvent: event }) => {
      enterAs = null;
      ctrlEnter = false;
      commandBackspace = false;
      if (event.isComposing || event.keyCode === 229) return;
      if (key === "\r" && event.key === "Enter" && !event.altKey && !event.metaKey) {
        if (event.shiftKey && !event.ctrlKey) enterAs = "\x1b\r";
        else if (event.ctrlKey && !event.shiftKey) { ctrlEnter = true; ctrlEnterRepeat = event.repeat; }
      }
      commandBackspace =
        isMac && key === "\x7f" && event.key === "Backspace" && event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
    });
    const remap = (data: string) => {
      if (enterAs !== null && data === "\r") data = enterAs;
      if (commandBackspace && data === "\x7f") data = "\x15";
      enterAs = null;
      commandBackspace = false;
      return data;
    };
    // xterm은 Cmd+화살표를 무시한다: 줄 처음(Ctrl+A)·끝(Ctrl+E). textarea keydown 뒤(bubble)에 IME 글이 먼저 간다.
    const onCommandArrow = (event: KeyboardEvent) => {
      if (!isMac || event.target !== term.textarea || event.defaultPrevented || composingTermRef.current) return;
      if (event.isComposing || event.keyCode === 229 || !event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return;
      const seq = event.key === "ArrowLeft" ? "\x01" : event.key === "ArrowRight" ? "\x05" : null;
      if (seq === null) return;
      event.preventDefault();
      fromTerminal(seq);
    };
    box.current!.addEventListener("keydown", onCommandArrow);
    const stopCompositionView = matchCompositionView(term);
    let alive = true;
    if (!ready) {
      void Promise.all([document.fonts.load(webFont), document.fonts.load(`bold ${webFont}`)])
        .then(() => {
          webFontReadyRef.current = true;
          // 설정한 글꼴이 있으면 그 목록은 아래 글꼴 종류 effect가 그 face를 받은 뒤 넣는다. 여기서 먼저 넣으면
          // 아직 없는 face 대신 글꼴로 cell을 재고, 같은 값이라 그 뒤에도 다시 재지 않는다.
          if (!alive || sanitizeFontFamily(fontFamilyRef.current) !== "" || term.options.fontFamily === family) return;
          term.options.fontFamily = family;
          setFontGen((g) => g + 1);
        })
        .catch(() => undefined);
    }
    // xterm의 글 붙여 넣기보다 먼저 본다: 그림이면 가로채고, 아니면 그대로 둔다
    const onPaste = (e: ClipboardEvent) => {
      // xterm은 이 capture 처리 바로 뒤에 붙여넣기를 처리한다: 그 글에는 유지 키를 붙이지 않는다
      pastingRef.current = true;
      queueMicrotask(() => {
        pastingRef.current = false;
      });
      const files = pastedFiles(e.clipboardData);
      if (files.length === 0) {
        // 글에 ESC가 있으면 뺀 글로 xterm 대신 붙인다(pasteText.ts)
        const text = e.clipboardData?.getData("text/plain") ?? "";
        if (text === pasteText(text)) return;
        e.preventDefault();
        e.stopPropagation();
        term.paste(pasteText(text));
        return;
      }
      e.preventDefault();
      e.stopPropagation();
      uploadRef.current(files);
    };
    // 끌어 놓기(PaneTerminal 1459~1480줄): 파일은 놓은 순서대로 올리고, 글은 붙여 넣는다(Enter 없음). 입력할 수 없으면 놓지 못한다.
    const onDragOver = (e: DragEvent) => {
      if (!e.dataTransfer) return;
      if (!e.dataTransfer.types.some((type) => type === "Files" || type === "text/plain")) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = readonlyRef.current ? "none" : "copy";
    };
    const onDrop = (e: DragEvent) => {
      e.preventDefault();
      if (!e.dataTransfer || readonlyRef.current) return;
      const files = Array.from(e.dataTransfer.files);
      if (files.length > 0) return uploadRef.current(files);
      const text = e.dataTransfer.getData("text/plain");
      if (!text) return;
      pastingRef.current = true;
      try {
        term.paste(pasteText(text));
      } finally {
        pastingRef.current = false;
      }
      term.focus();
    };
    el.addEventListener("paste", onPaste, true);
    el.addEventListener("dragover", onDragOver);
    el.addEventListener("drop", onDrop);
    // 두 손가락으로 글자 크기(herdr-web-ui PaneTerminal 1528~1640줄): 손가락을 따라 글자가 바로 바뀌고 크기를 띄운다.
    // 손을 떼면 설정에 저장하고 PTY 크기를 한 번 다시 건다. aaa 터미널 grid는 늘 PTY 크기를 따르므로(host가 알린다)
    // 참고처럼 핀치 중에 grid를 이 화면에서만 다시 맞추지 않는다: 그 사이 온 출력이 PTY에 없는 grid에 그려진다.
    //
    // 한 손가락 끌기는 스크롤이다(PaneTerminal 1519~1527줄): 터치 화면에는 휠 사건이 없고 xterm에는 쓸 만한 터치 스크롤이 없다.
    // 손가락이 간 만큼 줄 단위 휠 사건을 손가락 자리에 보내, 마우스 휠과 똑같이 xterm이 나눈다(mouse 보고면 프로그램에,
    // alternate screen이면 화살표, 아니면 scrollback). 이 화면 것이 아닌 grid(adopted)는 칸을 손가락대로 옮긴다.
    let touchX = 0;
    let touchY = 0;
    /** 이 동작에서 아직 휠로 보내지 않은 손가락 이동 */
    let touchPixels = 0;
    let tracking = false;
    const cellHeight = (): number => (term.element?.querySelector(".xterm-screen")?.getBoundingClientRect().height ?? term.rows) / term.rows;
    // 휠 하나가 옮기는 줄: 참고는 herdr가 보고 하나에 3줄(HERDR_MOUSE_SCROLL_LINES)을 굴려 손가락 3줄마다 하나를 보낸다.
    // aaa는 보고가 꺼진 normal buffer를 xterm 자기 scrollback이 휠 하나에 한 줄씩 굴리므로 그때는 한 줄마다 보낸다(plan §8.55).
    const stepPixels = (): number =>
      cellHeight() * (term.buffer.active.type === "normal" && term.modes.mouseTrackingMode === "none" ? 1 : HERDR_MOUSE_SCROLL_LINES);
    /** `steps`개의 줄 단위 휠을 손가락 자리에 */
    const sendTouchWheels = (steps: number) => {
      const target = term.element ?? el;
      for (let sent = 0; sent < Math.abs(steps); sent += 1) {
        target.dispatchEvent(
          new WheelEvent("wheel", {
            bubbles: true,
            cancelable: true,
            deltaY: Math.sign(steps),
            deltaMode: WheelEvent.DOM_DELTA_LINE,
            clientX: touchX,
            clientY: touchY,
          }),
        );
      }
    };
    let pinch: { first: number; second: number; startSize: number; startDistance: number; size: number } | null = null;
    // 두 번째 손가락부터 모든 손가락이 뗄 때까지: 이 동작은 핀치이고 스크롤이 아니다
    let pinchStroke = false;
    const spread = (a: Touch, b: Touch): number => Math.hypot(b.clientX - a.clientX, b.clientY - a.clientY);
    const showPinch = (size: number | null) => {
      const badge = pinchBadgeRef.current;
      if (!badge) return;
      badge.hidden = size === null;
      if (size !== null) badge.textContent = `${size}px`;
    };
    const applyLiveFont = (size: number) => {
      if (baseFontRef.current === size) return;
      baseFontRef.current = size;
      term.options.fontSize = size;
    };
    const finishPinch = (save: boolean) => {
      const finished = pinch;
      if (finished && (!save || finished.size === finished.startSize)) applyLiveFont(finished.startSize);
      pinch = null;
      if (finished && save && finished.size !== finished.startSize) {
        setTerminalFontSize(finished.size);
        autoFit.current(true);
      }
      showPinch(null);
    };
    const claimPinch = (event: Event) => {
      event.preventDefault();
      // capture 단계에서 듣는다: 핀치의 첫 손가락으로 xterm이 화면을 굴리면 안 된다
      event.stopPropagation();
    };
    const onTouchStart = (event: TouchEvent) => {
      if (chatViewRef.current) return;
      if (event.touches.length === 1 && pinchStroke) {
        finishPinch(false);
        pinchStroke = false;
      }
      if (pinchStroke) return claimPinch(event);
      if (event.touches.length < 2) {
        tracking = event.touches.length === 1;
        touchPixels = 0;
        const first = event.touches[0];
        if (tracking && first) {
          touchX = first.clientX;
          touchY = first.clientY;
        }
        return;
      }
      tracking = false;
      pinchStroke = true;
      claimPinch(event);
      const a = event.touches[0];
      const b = event.touches[1];
      if (event.touches.length !== 2 || !a || !b || !el.contains(a.target as Node) || !el.contains(b.target as Node) || spread(a, b) <= 0)
        return;
      const startSize = baseFontRef.current;
      pinch = { first: a.identifier, second: b.identifier, startSize, startDistance: spread(a, b), size: startSize };
      showPinch(startSize);
    };
    const onTouchMove = (event: TouchEvent) => {
      if (!pinchStroke && event.touches.length > 1) {
        tracking = false;
        pinchStroke = true;
      }
      if (!pinchStroke) {
        if (!tracking || event.touches.length !== 1) return;
        event.preventDefault();
        // xterm 자기 터치 스크롤(Viewport.handleTouchMove)까지 돌면 scrollback이 두 번 굴러간다: 참고는 xterm scrollback이 0이라 그것이 아무 일도 안 했다
        event.stopPropagation();
        const first = event.touches[0];
        if (!first) return;
        // 손가락이 내려가면(y가 커지면) 오래된 줄이 보인다: 위로 굴리는 휠, deltaY 음수
        const delta = touchY - first.clientY;
        const across = touchX - first.clientX;
        touchX = first.clientX;
        touchY = first.clientY;
        if (adoptedRef.current) {
          kbRef.current.panned = true;
          scrollRef.current?.scrollBy(across, delta);
          return;
        }
        const { steps, rest } = wheelSteps(touchPixels + delta, stepPixels());
        touchPixels = rest;
        sendTouchWheels(steps);
        return;
      }
      claimPinch(event);
      if (!pinch || event.touches.length !== 2) return;
      let first: Touch | undefined;
      let second: Touch | undefined;
      for (let i = 0; i < event.touches.length; i++) {
        const touch = event.touches[i]!;
        if (touch.identifier === pinch.first) first = touch;
        if (touch.identifier === pinch.second) second = touch;
      }
      if (!first || !second) return;
      const size = pinchFontSize(pinch.startSize, pinch.startDistance, spread(first, second));
      if (size === pinch.size) return;
      pinch.size = size;
      applyLiveFont(size);
      showPinch(size);
    };
    const onTouchEnd = (event: TouchEvent) => {
      // 뗄 때 남은 이동이 반 걸음을 넘으면 하나 더: 한 걸음보다 짧게 튕겨도 굴러가고, 길게 끌어도 반 걸음 넘게 잃지 않는다
      if (tracking && event.type === "touchend" && !pinchStroke && !adoptedRef.current) sendTouchWheels(finalStep(touchPixels, stepPixels()));
      touchPixels = 0;
      tracking = false;
      if (!pinchStroke) return;
      claimPinch(event);
      if (event.type === "touchcancel" || event.touches.length < 2) finishPinch(event.type === "touchend");
      if (event.touches.length === 0) pinchStroke = false;
    };
    // Safari는 touch-action이 none이어도 넘치는 영역(xterm viewport) 안에서 화면을 확대한다
    const onGesture = (event: Event) => {
      if (!chatViewRef.current) event.preventDefault();
    };
    const touchOpts = { capture: true, passive: false } as const;
    el.addEventListener("touchstart", onTouchStart, touchOpts);
    el.addEventListener("touchmove", onTouchMove, touchOpts);
    el.addEventListener("touchend", onTouchEnd, touchOpts);
    el.addEventListener("touchcancel", onTouchEnd, touchOpts);
    el.addEventListener("gesturestart", onGesture, { passive: false });
    el.addEventListener("gesturechange", onGesture, { passive: false });
    // 터미널을 누르거나 치면 이 화면이 지금 쓰는 화면이 된다(herdr-web-ui PaneTerminal 1902~1918줄): 보내기 **전에**
    // 크기를 다시 건다. 그래야 첫 글의 답이 이 화면 grid로 그려진다. 왼쪽 누름만, 반복 키·조합 중 키는 아니다.
    const onPointer = (event: PointerEvent) => {
      if (event.button === 0) autoFit.current(true);
    };
    el.addEventListener("pointerdown", onPointer);
    const onReclaimKey = (event: KeyboardEvent) => {
      if (!(event.target instanceof Node) || !term.element?.contains(event.target)) return;
      // IME 첫 키는 compositionstart보다 먼저, isComposing 없이 온다(keyCode 229)
      if (event.repeat || event.isComposing || event.keyCode === 229 || composingTermRef.current || commitPendingRef.current) return;
      autoFit.current(true);
    };
    window.addEventListener("keydown", onReclaimKey, { capture: true });
    termRef.current = term;
    fitRef.current = fit;
    const dataSub = term.onData((data) => {
      if (ctrlEnter && data === "\r") {
        ctrlEnter = false;
        remap(data);
        if (!ctrlEnterRepeat) pressCtrlEnter();
        return;
      }
      // Shift·Ctrl+Enter는 바꾸기 전에 가린다: 바뀐 꼴에는 줄바꿈이 없을 수 있다(PaneTerminal 1345~1347줄)
      const modifiedEnter = enterAs !== null && data === "\r";
      fromTerminal(remap(data), modifiedEnter);
    });
    const binSub = term.onBinary((s) => {
      if (readonlyRef.current) return;
      const ws = wsRef.current;
      if (ws?.readyState === WebSocket.OPEN) ws.send(Uint8Array.from(s, (c) => c.charCodeAt(0)));
    });

    let stopped = false;
    let delay = 500;
    let timer: number | undefined;
    // 출력 흐름 제어(herdr-web-ui pty-ack, plan §8.62): snapshot마다 host가 구독 번호를 주고, xterm이 다 쓴 뒤
    // 그 구독에서 받은 바이트 끝(replay + 출력)을 알린다. 받았을 때가 아니라 쓴 뒤에 알려야 느린 기기가 드러난다.
    let streamId = 0;
    let received = 0;
    const acknowledge = (ws: WebSocket, stream: number, offset: number) => () => {
      if (wsRef.current !== ws || ws.readyState !== WebSocket.OPEN || streamId !== stream) return;
      ws.send(JSON.stringify({ t: "ack", stream, offset } satisfies TermClientMsg));
    };
    // 가려진 탭은 ACK를 보내지 못한다: 그때(또는 다시 보인 바로 뒤) 멈춘 것은 기기가 느린 것이 아니라
    // 치워 둔 것이라, 다시 보일 때 한 번 스스로 붙는다(herdr-web-ui stoppedWhileHidden). 보이는 채로 멈추면 단추를 기다린다.
    let outputStopped = false;
    let stoppedHidden = false;
    let shownAt = 0;
    const reattach = () => {
      if (!outputStopped || stopped) return;
      outputStopped = false;
      stoppedHidden = false;
      delay = 500;
      connect();
    };
    reattachRef.current = reattach;
    const onVisibility = () => {
      if (document.visibilityState !== "visible") return;
      shownAt = Date.now();
      if (outputStopped && stoppedHidden) reattach();
    };
    document.addEventListener("visibilitychange", onVisibility);
    // 다시 보이거나 망이 돌아오면 backoff(최대 10초)를 기다리지 않고 바로 다시 붙는다(QA t-0337 Q5, herdr-web-ui ws.ts
    // `onWake`·`wake()`: online·pageshow·visibilitychange에서 기다리던 재연결을 지금 하고 가장 짧은 지연부터 다시 센다.
    // t3code client-runtime connection/supervisor.ts도 같다). 열린 연결·출력 정지(4008)는 그대로 둔다
    const onWake = () => {
      if (stopped || outputStopped || document.visibilityState !== "visible" || timer === undefined) return;
      window.clearTimeout(timer);
      timer = undefined;
      delay = 500;
      trace("wake");
      connect();
    };
    window.addEventListener("online", onWake);
    window.addEventListener("pageshow", onWake);
    document.addEventListener("visibilitychange", onWake);
    // 앞 기록 받기(plan §8.67, 사용자 요청 10-17 「뒤로 스크롤하다 데이터가 부족해지면 받아오면 어때?」). 붙을 때는 최근 기록만 오고
    // (`history_more`면 host에 더 있다), 위로 굴려 받은 기록 맨 위에서 한 화면 안에 들어오면 그 앞 덩어리를 받는다.
    // xterm은 버퍼 앞에 줄을 끼울 수 없으므로 「받은 덩어리 + 지금 버퍼(serialize)」로 다시 쓰고, 보던 줄 자리를 덩어리 줄 수만큼
    // 내려 맞춘다. 다시 쓰는 동안 온 출력은 붙잡았다가 뒤에 쓰고, 화면은 그 전 모습을 덮어 두어 튀지 않는다.
    let historyMore = false;
    let historyWait: { id: number; stream: number } | null = null;
    let historyId = 0;
    let historyApplied = 0;
    let holding: (() => void)[] | null = null;
    const run = (f: () => void) => (holding ? holding.push(f) : f());
    const markHistory = () => {
      const el = box.current;
      if (!el) return;
      el.dataset.historyApplied = String(historyApplied);
      el.dataset.historyMore = historyMore ? "1" : "0";
      el.dataset.historyWaiting = historyWait ? "1" : "0";
    };
    const wantHistory = () => {
      const ws = wsRef.current;
      const buf = term.buffer.active;
      if (!historyMore || historyWait || holding || !ws || ws.readyState !== WebSocket.OPEN) return;
      // 보통 화면에서 위로 굴려, 받은 기록 맨 위에서 한 화면 안에 들어왔을 때
      if (buf.type !== "normal" || buf.viewportY >= buf.baseY || buf.viewportY > term.rows) return;
      const anchor = [0, 1, 2].map((i) => buf.getLine(i)?.translateToString(true) ?? "");
      historyWait = { id: ++historyId, stream: streamId };
      trace("history-request", { id: historyId, depth: buf.length });
      ws.send(JSON.stringify({ t: "history", id: historyId, anchor, depth: buf.length } satisfies TermClientMsg));
      markHistory();
    };
    // xterm 5.5 onScroll은 새 줄로 버퍼가 밀릴 때만 온다: 사용자가 굴린 것은 viewport의 scroll로 본다(xterm 처리 뒤라 viewportY가 새 자리다)
    const scrollSub = term.onScroll(() => wantHistory());
    // 그린 화면의 맨 위 버퍼 줄과 맨 아래 화면의 맨 위 줄(baseY). 시험이 「이 자리가 그려졌다」를 사건으로 안다: 굴린 뒤 frame 수를 어림하지 않는다
    const renderSub = term.onRender(() => {
      const el = box.current;
      if (!el) return;
      el.dataset.viewTop = String(term.buffer.active.viewportY);
      el.dataset.viewBase = String(term.buffer.active.baseY);
    });
    viewport?.addEventListener("scroll", wantHistory);
    /** 받은 덩어리를 지금 버퍼 앞에 붙인다(위 설명) */
    const prependHistory = (data: Uint8Array, lines: number, more: boolean) => {
      holding = [];
      const done = () => {
        const held = holding ?? [];
        holding = null;
        historyWait = null;
        historyMore = more;
        historyApplied += 1;
        markHistory();
        for (const f of held) f();
      };
      // 앞서 온 출력을 다 읽은 뒤의 버퍼를 다시 쓴다
      term.write("", () => {
        const buf = term.buffer.active;
        if (buf.type !== "normal" || lines === 0) return done();
        const top = buf.viewportY;
        const keys = otherKeysRef.current;
        const body = new TextEncoder().encode(serializer.serialize({ excludeAltBuffer: true }));
        // 다시 쓰는 동안의 화면: 지금 모습을 그대로 덮어 둔다(DOM renderer의 줄 요소 사본)
        const screen = box.current?.querySelector<HTMLElement>(".xterm-screen");
        const cover = screen?.cloneNode(true) as HTMLElement | undefined;
        if (screen && cover) {
          cover.classList.add("term-history-cover");
          cover.style.left = `${screen.offsetLeft}px`;
          cover.style.top = `${screen.offsetTop}px`;
          screen.parentElement?.appendChild(cover);
        }
        term.reset();
        const all = new Uint8Array(data.length + body.length);
        all.set(data);
        all.set(body, data.length);
        term.write(all, () => {
          otherKeysRef.current = keys;
          const want = top + lines;
          term.scrollToLine(want);
          // 덮개는 새 버퍼를 그린 뒤에 뗀다. write 콜백은 읽기가 끝난 때이고, 줄은 다음 frame에 그린다(xterm RenderService
          // RenderDebouncer → _renderRows → onRender). 그 전에 떼면 reset한 빈 화면이 한 frame 보인다(t-0327 측정: 25번 중 10번).
          // 참고도 그린 뒤 시점을 onRender로 잡는다(herdr-web-ui TerminalViewportTools.tsx 216줄 `terminal.onRender(paintHits)`).
          // 그 그림이 보던 줄 자리여야 한다: viewport 요소의 scroll 사건(xterm Viewport._handleScroll)이 그 사이 자리를 옮겼으면
          // 다시 맞추고 다음 그림을 기다린다(버퍼 끝을 넘는 자리는 끝으로 본다). 붙잡은 출력은 그 뒤에 쓴다.
          const drawn = term.onRender(() => {
            const b = term.buffer.active;
            const at = Math.min(want, b.baseY);
            // 보던 줄이 숫자가 아니면(xterm이 줄 높이를 재기 전 scroll로 NaN, patches/xterm-5.5.0-viewport.patch가 막는다)
            // 다시 맞출 수 없다: 기다리지 않고 붙잡은 출력을 푼다(QA t-0337 Q1, plan §8.105)
            if (Number.isFinite(b.viewportY) && b.viewportY !== at) {
              term.scrollToLine(at);
              return;
            }
            drawn.dispose();
            cover?.remove();
            trace("history-applied", { lines, more });
            done();
          });
          term.refresh(0, term.rows - 1);
        });
      });
    };
    const connect = () => {
      setLink("connecting");
      // flow=ack: 이 화면은 출력 ACK를 보낸다(herdr-web-ui attach flow_control "ack"). 없으면 host는 보내기 줄로만 잰다
      // fg·bg: 이 화면의 색. host는 그 pane의 agent가 받은 뒤에 연다(그 뒤 프로그램이 묻는 색에 이 색으로 답한다)
      const colors = new URLSearchParams(termColors()).toString();
      const ws = new WebSocket(wsUrl(`/api/v1/panes/${encodeURIComponent(pane.id)}/ws?flow=ack&${colors}`));
      ws.binaryType = "arraybuffer";
      wsRef.current = ws;
      ws.onopen = () => {
        delay = 500;
        setLink("open");
      };
      ws.onmessage = (m) => {
        if (m.data instanceof ArrayBuffer) {
          received += m.data.byteLength;
          const data = new Uint8Array(m.data);
          const ack = acknowledge(ws, streamId, received);
          run(() => term.write(data, ack));
          return;
        }
        const msg = JSON.parse(m.data as string) as TermServerMsg;
        switch (msg.t) {
          case "snapshot": {
            trace("snapshot");
            // 새 snapshot은 버퍼를 새로 쓴다: 기다리던 앞 기록 답은 버린다(버퍼가 바뀌었다)
            historyWait = null;
            // 지우기와 크기는 흐름 순서대로(herdr-web-ui PaneTerminal 1755~1760줄 「Reset in stream order」): xterm write는
            // 줄을 서 나중에 읽히는데 reset·resize는 바로 일어난다. 바로 하면 아직 읽지 않은 앞 출력이 새 화면 위에 그려지고,
            // 곧 온 state의 크기가 replay보다 먼저 걸려 replay가 다른 크기로 읽혔다(CI에서 빈 줄로 보인 실패, plan §8.59).
            const { cols, rows } = msg;
            setSize([cols, rows]);
            const replay = b64decode(msg.replay);
            streamId = msg.stream;
            received = replay.length;
            const ack = acknowledge(ws, msg.stream, received);
            // 앞 기록을 붙이는 중이면 그 뒤에(붙잡은 출력과 같은 줄)
            run(() => {
              historyMore = msg.history_more;
              markHistory();
              term.write("\x1bc", () => {
                // replay가 키보드 모드를 처음부터 다시 말한다(herdr-web-ui도 재접속 때 0으로 되돌린다)
                otherKeysRef.current = 0;
                term.resize(cols, rows);
              });
              term.write(replay, ack);
            });
            readonlyRef.current = msg.readonly;
            setReadonly(msg.readonly);
            setError(null);
            break;
          }
          case "history": {
            const wait = historyWait;
            if (!wait || wait.id !== msg.id) break;
            // 그 사이 snapshot이 버퍼를 새로 썼으면 맞지 않는다
            if (wait.stream !== streamId) {
              historyWait = null;
              markHistory();
              break;
            }
            trace("history", { lines: msg.lines, more: msg.more, found: msg.found });
            if (!msg.found) {
              // host 기록에서 받은 기록 맨 위를 찾지 못했다(크기가 바뀌어 줄이 달라졌거나 기록에서 빠짐): 더 받지 않는다
              historyWait = null;
              historyMore = false;
              markHistory();
              break;
            }
            if (msg.lines === 0) {
              // 붙일 줄이 없다(host mirror 기록 끝에서 session host까지 물었는데 그 앞도 없다, plan §8.118): 버퍼를 다시 쓰지 않는다
              historyWait = null;
              historyMore = msg.more;
              markHistory();
              break;
            }
            prependHistory(b64decode(msg.data), msg.lines, msg.more);
            break;
          }
          case "readonly":
            readonlyRef.current = msg.readonly;
            setReadonly(msg.readonly);
            setReason(msg.reason);
            break;
          case "front":
            setMe(msg.id);
            // 옛 host(1.44 이하)는 이 칸이 없다: 단추를 숨긴다
            setSteerKnown(msg.steer_queued === true);
            break;
          case "state":
            setLease(msg.pane.lease);
            {
              // 크기도 흐름 순서대로: 앞서 온 출력(snapshot replay 포함)을 다 읽은 뒤 바꾼다
              const { cols, rows } = msg.pane;
              setSize((prev) => (prev[0] === cols && prev[1] === rows ? prev : [cols, rows]));
              run(() =>
                term.write("", () => {
                  if (cols !== term.cols || rows !== term.rows) term.resize(cols, rows);
                }),
              );
            }
            break;
          case "attached":
            setAttach({
              text: msg.inserted
                ? `그림 경로를 넣었다(Enter는 보내지 않았다): ${msg.path}`
                : `그림을 저장했다: ${msg.path}${msg.note ? ` · ${msg.note}` : ""}`,
              ok: true,
            });
            if (!inputLineRef.current) termRef.current?.focus();
            attachDone();
            break;
          case "submitted": {
            const waiting = submitsRef.current.get(msg.id);
            submitsRef.current.delete(msg.id);
            waiting?.done(msg.ok ? true : `보내지 못했다: ${msg.message ?? "pane이 받지 않았다"}`);
            break;
          }
          case "steer_queued": {
            if (!steersRef.current.delete(msg.id)) break;
            const next: SteerState = msg.sent
              ? { phase: "sent", at: Date.now(), before: pendingCountRef.current }
              : { phase: "failed", ...refusalText(msg.reason, msg.message, agentNameRef.current) };
            setSteer((s) => (s.phase === "sending" && s.id === msg.id ? next : s));
            // 키가 들어갔다: 대화를 바로 다시 읽어 대기 말이 빠졌는지 본다
            if (msg.sent) setChatSent((n) => n + 1);
            break;
          }
          case "submit_cancelled": {
            const answer = cancelsRef.current.get(msg.id);
            cancelsRef.current.delete(msg.id);
            answer?.(msg.stage);
            break;
          }
          case "error":
            setError(msg.message);
            setAttach((a) => (a?.text.startsWith("그림 보내는 중") ? null : a));
            attachDone();
            break;
        }
      };
      ws.onclose = (event) => {
        if (wsRef.current === ws) wsRef.current = null;
        // 올리던 파일은 끝나지 않는다: 묶음의 나머지를 기다리게 두지 않는다
        attachDone();
        // 기다리던 앞 기록 답은 오지 않는다: 다시 붙으면 새 snapshot부터 다시 받는다
        historyWait = null;
        for (const [id, waiting] of submitsRef.current)
          if (waiting.ws === ws) {
            submitsRef.current.delete(id);
            waiting.done(null);
          }
        // 멈춤 답도 오지 않는다: 모름
        for (const [id, answer] of cancelsRef.current) {
          cancelsRef.current.delete(id);
          answer(null);
        }
        // 「지금 보내기」 답도 오지 않는다: 보냈는지 모른다
        for (const [id, sent] of steersRef.current)
          if (sent === ws) {
            steersRef.current.delete(id);
            setSteer((s) => (s.phase === "sending" && s.id === id ? { phase: "failed", ...refusalText(null, null, agentNameRef.current) } : s));
          }
        readonlyRef.current = true;
        setReadonly(true);
        setLink("closed");
        // 끊기면 유지 키를 푼다(PaneTerminal 1261줄)
        setModifiers(NO_STICKY_MODIFIERS);
        // 이 기기가 취소됐다: 다시 붙지 않고 pairing 화면으로(App이 onSignedOut으로 듣는다)
        if (event.code === REVOKED_CLOSE_CODE) {
          stopped = true;
          reportSignedOut();
          return;
        }
        // 출력을 따라오지 못해 host가 닫았다: 자동으로 다시 붙지 않는다(과부하를 되풀이하지 않는다, herdr-web-ui ws.ts)
        if (event.code === OUTPUT_STALLED_CLOSE_CODE && !stopped) {
          outputStopped = true;
          stoppedHidden = document.visibilityState === "hidden" || (shownAt > 0 && Date.now() - shownAt <= SHOWN_GRACE_MS);
          setLink("stopped");
          if (stoppedHidden && document.visibilityState === "visible") queueMicrotask(reattach);
          return;
        }
        if (!stopped) {
          timer = window.setTimeout(() => {
            timer = undefined;
            connect();
          }, delay);
          // 표지: 어느 close 뒤에 몇 ms 뒤 다시 붙기를 걸었다. 시험이 「4008 뒤 스스로 다시 붙지 않는다」를 그 close 처리 뒤에 sleep 없이 본다(웹 E2E 07 흐름 제어)
          el.dataset.retry = `${event.code} ${delay}`;
        }
        delay = Math.min(delay * 2, 10_000);
      };
    };
    connect();
    return () => {
      alive = false;
      stopped = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("online", onWake);
      window.removeEventListener("pageshow", onWake);
      document.removeEventListener("visibilitychange", onWake);
      wsRef.current?.close();
      el.removeEventListener("paste", onPaste, true);
      viewport?.removeEventListener("scroll", onViewportScroll);
      viewport?.removeEventListener("scroll", markScrolled);
      scrolledSub.dispose();
      bufferSub.dispose();
      el.removeEventListener("dragover", onDragOver);
      el.removeEventListener("drop", onDrop);
      el.removeEventListener("pointerdown", onPointer);
      window.removeEventListener("keydown", onReclaimKey, { capture: true });
      el.removeEventListener("mousedown", onMouseDown, { capture: true });
      window.removeEventListener("mousemove", onMouseMove, { capture: true });
      window.removeEventListener("blur", onWindowBlur);
      window.removeEventListener("mouseup", onMouseUp);
      disposed = true;
      el.removeEventListener("touchstart", onTouchStart, true);
      el.removeEventListener("touchmove", onTouchMove, true);
      el.removeEventListener("touchend", onTouchEnd, true);
      el.removeEventListener("touchcancel", onTouchEnd, true);
      el.removeEventListener("gesturestart", onGesture);
      el.removeEventListener("gesturechange", onGesture);
      el.removeEventListener("compositionstart", compositionStart);
      el.removeEventListener("compositionend", compositionEnd);
      el.removeEventListener("keydown", onCommandArrow);
      if (compositionEndTimer !== null) window.clearTimeout(compositionEndTimer);
      stopGlyphs();
      stopCompositionView();
      dataSub.dispose();
      parsedSub.dispose();
      stopTrace();
      keySub.dispose();
      osc52.dispose();
      osc8.dispose();
      ris.dispose();
      fileLinks.dispose();
      otherKeysSet.dispose();
      otherKeysOff.dispose();
      binSub.dispose();
      scrollSub.dispose();
      renderSub.dispose();
      viewport?.removeEventListener("scroll", wantHistory);
      offTheme();
      termRef.current = null;
      // 앞 기록을 붙이는 중에 붙잡은 출력은 버린다: 이 터미널에 더 쓰지 않는다
      holding = null;
      // xterm이 걸어 둔 frame·task가 돈 뒤에 치운다(PaneTerminal 1982줄 `disposeAfterPendingFrame`): snapshot의 RIS로 Viewport.reset이
      // 다음 frame에 renderer 크기를 읽는데, 단축키로 pane을 잇달아 넘기면 그 frame 전에 내려가 TypeError가 났다.
      // 받은 출력을 xterm이 다 읽은 뒤에 기다리기 시작한다: 참고는 같은 탭 안 pane 전환에서 터미널을 내리지 않지만(terminalDispose.ts
      // 머리 설명) aaa는 pane을 넘길 때마다 내린다. 막 온 snapshot을 아직 읽지 않은 채(WriteBuffer task가 남음) 내려가면, 그 task가 기다림
      // frame 뒤에 RIS를 읽어 reset frame이 치운 뒤에 돌았다(CI 6번 중 1번, t-0332 재현).
      term.write("", () => disposeAfterPendingFrame(term));
    };
    // pane.id가 바뀔 때만 새로 붙는다. cols/rows는 snapshot이 알려 준다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pane.id]);

  // 자동 맞추기: 조작할 수 있고, 연결돼 있고, 터미널이 보이고, 이 탭을 쓰는 중(보이고 focus)일 때만.
  // claim이 아니면 이 화면이 지금 쓰는 화면이거나 아무도 쓰지 않을 때만 맞춘다. claim이면(focus를 얻음·터미널 누름)
  // 크기가 같아도 보내 이 화면이 지금 쓰는 화면이 된다. 크기를 보내면 host가 이 연결을 지금 쓰는 화면으로 둔다.
  //
  // 화면 키보드(herdr-web-ui PaneTerminal 705~752줄, lib/keyboard.ts `keyboardHoldsGrid`): 키보드는 화면 아래를 가릴 뿐
  // pane을 줄이지 않는다. 남은 띠에 맞추면 PTY와 그 pane을 보는 다른 화면이 키보드마다 두 번 줄바꿈한다.
  // 그래서 높이만 줄면 grid를 그대로 두고 커서 줄이 보이게 터미널을 옮긴다. 폭이 바뀌면(회전) 키보드 중에도 맞춘다.
  // 붙잡아 둔 맞추기는 키보드가 내려가 터미널이 커질 때(ResizeObserver) 한다.
  const kbRef = useRef({ held: false, fittedWidth: null as number | null, panned: false });
  // 이 화면 것이 아닌 grid(다른 화면이 크기를 정하는 중, 보기 권한)는 칸보다 클 수 있다. 칸이 굴러가고, 손으로 옮기기 전까지
  // 커서 줄을 보이게 따라간다(herdr-web-ui PaneTerminal 699~727줄 `adopted`·`followCursor`).
  const adopted = heldByOther || !canControl;
  const adoptedRef = useRef(adopted);
  useEffect(() => {
    if (adopted && !adoptedRef.current) kbRef.current.panned = false;
    adoptedRef.current = adopted;
    if (adopted) followCursor.current();
  }, [adopted]);
  const followCursor = useRef(() => {});
  followCursor.current = () => {
    const host = scrollRef.current;
    const term = termRef.current;
    const k = kbRef.current;
    const screen = term?.element?.querySelector<HTMLElement>(".xterm-screen");
    if (!host || !term || !screen || !(k.held || adoptedRef.current) || k.panned) return;
    const row = screen.offsetHeight / term.rows;
    const top = screen.getBoundingClientRect().top - host.getBoundingClientRect().top + host.scrollTop;
    const cursorBottom = top + (term.buffer.active.cursorY + 1) * row;
    const max = host.scrollHeight - host.clientHeight;
    host.scrollTop = cursorBottom <= host.clientHeight ? 0 : cursorBottom - row >= max ? max : cursorBottom - host.clientHeight;
  };
  /** 맞추기 전: 키보드가 grid를 붙잡으면 true(맞추지 않고 커서 줄로 옮긴다) */
  const keyboardHolds = () => {
    const host = scrollRef.current;
    if (!host || !keyboardHoldsGrid(keyboardUp(), host.clientWidth, kbRef.current.fittedWidth)) return false;
    kbRef.current.held = true;
    kbRef.current.panned = false;
    followCursor.current();
    return true;
  };
  /**
   * 맞추기 전: 입력줄이 여러 줄로 커져 터미널 칸을 줄였으면 true(plan §8.262). 키보드처럼 grid(PTY 크기)는 그대로 두고 칸 안에서
   * 커서 줄로 옮긴다: 줄 하나 칠 때마다 그 pane을 보는 모든 기기의 크기를 바꾸지 않는다. 폭이 바뀌었으면 맞춘다
   */
  const inputSlotRef = useRef<HTMLDivElement | null>(null);
  const inputHolds = () => {
    const host = scrollRef.current;
    const slot = inputSlotRef.current;
    if (!host || !slot) return false;
    const grown = slot.getBoundingClientRect().height - parseFloat(getComputedStyle(slot).minHeight) > 1;
    if (!keyboardHoldsGrid(grown, host.clientWidth, kbRef.current.fittedWidth)) return false;
    kbRef.current.held = true;
    followCursor.current();
    return true;
  };
  /** 맞춘 뒤: grid가 다시 이 화면 것이고 옮길 것이 없다 */
  const fitted = () => {
    const host = scrollRef.current;
    if (!host) return;
    const wasHeld = kbRef.current.held;
    kbRef.current = { held: false, fittedWidth: host.clientWidth, panned: false };
    if (wasHeld) host.scrollTop = 0;
  };
  const autoFit = useRef((_claim?: boolean) => {});
  autoFit.current = (claim = false) => {
    // 표지 `data-fit`: 마지막 맞추기 판단(off·released·other·keyboard·input·unsized·sent·same). 크기는 그 판단 안에서 바로 보내므로,
    // 시험이 「열기만 해서는 크기를 걸지 않는다」를 그 판단 뒤에 sleep 없이 본다(웹 E2E 02 「두 controller」)
    const mark = (why: string): void => {
      if (box.current) box.current.dataset.fit = why;
    };
    if (!canControl || readonly || link !== "open" || me === null || (chatView && chatable) || !inUse()) return mark("off");
    // 보기만 하는 동안은 크기를 걸지 않는다(돌아올 때 back이 풀고 건다)
    if (releasedRef.current) return mark("released");
    if (heldByOther && !claim) return mark("other");
    if (keyboardHolds()) return mark("keyboard");
    if (inputHolds()) return mark("input");
    const dims = fitRef.current?.proposeDimensions();
    if (!dims || !Number.isFinite(dims.cols) || dims.cols < 10 || dims.rows < 3) return mark("unsized");
    const change = dims.cols !== size[0] || dims.rows !== size[1] || (claim && heldByOther);
    if (change) send({ t: "resize", cols: dims.cols, rows: dims.rows });
    fitted();
    mark(change ? "sent" : "same");
  };
  useEffect(() => autoFit.current(), [link, me, lease, readonly, chatView, size, fontGen]);
  // 대화 보기는 grid를 가린다: 이 화면이 건 크기를 놓아 터미널을 보는 다른 화면이 되찾게 한다(PaneTerminal 2049~2056줄)
  useEffect(() => {
    if (chatView && chatable && leaseMineRef.current) send({ t: "release" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatView, chatable]);
  // 설정에서 글자 크기를 바꾸면 따라간다(PaneTerminal 1996~2025줄). 같은 크기면(핀치가 이미 바꿨다) 아무 일도 없다.
  // 쓰는 중이면 이 화면 크기로 PTY를 다시 건다.
  useEffect(() => {
    const term = termRef.current;
    baseFontRef.current = terminalFontSize;
    if (!term || term.options.fontSize === terminalFontSize) return;
    term.options.fontSize = terminalFontSize;
    autoFit.current(true);
  }, [terminalFontSize]);
  // 설정한 글꼴 종류도 따라간다(PaneTerminal 1996~2028줄). xterm은 fontFamily가 바뀔 때 cell을 재고 같은 값이면 다시 재지 않는다.
  // 그래서 고른 글꼴은 그 face를 받은 뒤에 넣는다. 아니면 대신 글꼴로 잰 cell이 남는다. 바뀌면 grid가 바뀌므로 크기를 다시 건다.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    const base = baseFamilyRef.current;
    const chosen = sanitizeFontFamily(terminalFontFamily);
    // 기본 목록으로 돌아갈 때 넣은 WOFF2가 아직이면 그것을 뺀 목록이다(받으면 위 open의 처리가 바꾼다)
    const stack = chosen === "" && !webFontReadyRef.current ? base.replace(`${WEB_FONT}, `, "") : fontStack(base, chosen);
    let superseded = false;
    const apply = () => {
      // 더 새 설정이나 새로 연 터미널이 그 사이 넘겨받았다
      if (superseded || termRef.current !== term || term.options.fontFamily === stack) return;
      term.options.fontFamily = stack;
      autoFit.current(true);
    };
    if (chosen === "") apply();
    else void loadFontStack(stack, baseFontRef.current).then(apply);
    return () => {
      superseded = true;
    };
  }, [terminalFontFamily, pane.id]);
  // focus를 얻거나 다시 보이면 이 화면 크기를 다시 건다. 가려지면 바로 크기를 놓는다(herdr-web-ui PaneTerminal 1831~1842줄
  // `keepSize(hidden)`: 숨은 문서는 서버 grid를 놓고 연결은 둔다). 보이는 채 focus만 잃으면 그대로 둔다.
  // 설정 「다른 웹 화면과 같이 쓰기」(`releasePaneAway`, 기본 꺼짐)를 켜면 다른 창으로 옮기고 1초 뒤 크기를 놓고 보기만 한다
  // (참고 `leave`·`release`). 참고는 attach를 떼고 보기 전용 구독(watch)으로 바꾼다. aaa는 한 연결이 출력을 계속 받고 크기만 놓으면
  // 같은 일이 된다(크기는 host lease, 보기는 그 연결). 그래서 host에 새 구독을 두지 않았다.
  // 돌아오면(focus, 이 화면 누르기·치기) 보기를 풀고 크기를 건다(참고 `back`·`reclaim`).
  const leaseMine = me !== null && lease?.front === me;
  const leaseMineRef = useRef(leaseMine);
  leaseMineRef.current = leaseMine;
  const sectionRef = useRef<HTMLElement>(null);
  const stackRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let releaseTimer: number | undefined;
    const cancelLeave = () => {
      window.clearTimeout(releaseTimer);
      releaseTimer = undefined;
    };
    const release = () => {
      cancelLeave();
      if (releasedRef.current || inUse() || endedRef.current || !canControlRef.current) return;
      if (leaseMineRef.current) send({ t: "release" });
      setReleased(true);
    };
    const leave = () => {
      if (inUse()) return;
      if (document.visibilityState === "hidden" && leaseMineRef.current) send({ t: "release" });
      if (terminalToggle("releasePaneAway") && releaseTimer === undefined && !releasedRef.current)
        releaseTimer = window.setTimeout(release, RELEASE_AFTER_MS);
    };
    const back = () => {
      cancelLeave();
      if (releasedRef.current) setReleased(false);
      autoFit.current(true);
    };
    const vis = () => (inUse() ? back() : leave());
    // 누름은 창이 focus를 말하지 않아도 사용자가 여기 있다는 것이다(참고 onPointer): 보기만 하던 화면을 먼저 되찾는다
    const onPointer = (event: PointerEvent) => {
      if (releasedRef.current && sectionRef.current?.contains(event.target as Node)) back();
    };
    window.addEventListener("focus", back);
    window.addEventListener("blur", leave);
    window.addEventListener("pointerdown", onPointer, { capture: true });
    document.addEventListener("visibilitychange", vis);
    return () => {
      cancelLeave();
      setReleased(false);
      window.removeEventListener("focus", back);
      window.removeEventListener("blur", leave);
      window.removeEventListener("pointerdown", onPointer, { capture: true });
      document.removeEventListener("visibilitychange", vis);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pane.id]);
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    let t: number | undefined;
    // 표지 `data-fit-pending`: 크기 바뀜 뒤 맞추기를 기다리는 동안(그 판단은 data-fit에 남는다)
    const ro = new ResizeObserver(() => {
      window.clearTimeout(t);
      el.dataset.fitPending = "1";
      t = window.setTimeout(() => {
        delete el.dataset.fitPending;
        // 키보드가 화면 아래를 가렸다: grid는 그대로, 터미널만 커서 줄로 옮긴다(맞출 수 없는 화면도)
        if (keyboardHolds()) {
          el.dataset.fit = "keyboard";
          return;
        }
        if (inputHolds()) {
          el.dataset.fit = "input";
          return;
        }
        autoFit.current();
      }, 120);
    });
    ro.observe(el);
    // 손으로 옮기면 키보드가 있는 동안 커서를 따라가지 않는다(herdr-web-ui `panned`)
    const host = scrollRef.current;
    const pan = () => {
      if (kbRef.current.held || adoptedRef.current) kbRef.current.panned = true;
    };
    host?.addEventListener("touchmove", pan, { passive: true });
    host?.addEventListener("wheel", pan, { passive: true });
    return () => {
      ro.disconnect();
      window.clearTimeout(t);
      host?.removeEventListener("touchmove", pan);
      host?.removeEventListener("wheel", pan);
    };
  }, [pane.id]);

  // 키 막대(herdr-web-ui PaneTerminal 2218~2245줄). 항목은 기기별 저장값(설정 「키 바 편집」이 바꾼다), 없으면 참고 기본 배열.
  const barItems = useKeyBarItems();
  const pressKey = (item: KeyBarKeyItem) => {
    const term = termRef.current;
    if (!term || composingTermRef.current || commitPendingRef.current) return;
    // 저장된 조합은 그대로, 보통 키는 유지 키를 이어받는다. 내장 ^C 키는 늘 ^C다.
    const active = item.modifiers ?? modifiersRef.current;
    const modified = (item.key !== "ctrl-c" || item.modifiers !== undefined) && (hasModifiers(active) || item.modifiers !== undefined);
    const chord = modified ? terminalChord(item.key, active) : null;
    if (chord === "ctrl+enter") { pressCtrlEnter(); return; }
    const seq =
      (modified ? navigationSequence(item.key, active) : null) ??
      (chord !== null ? chordSequence(chord, chordCtx()) : null) ??
      keyBarInputSequence(item.key, term.modes.applicationCursorKeysMode);
    if (seq === null) return;
    // 키 막대는 grid 밖이다: 보내기 전에 이 화면 크기를 다시 건다(PaneTerminal 2231줄)
    autoFit.current(true);
    deliver(seq, { chord: chord !== null });
    // 입력줄이면 키보드는 입력줄 것이다: 키를 눌러도 터미널로 옮기지 않는다
    if (!inputLineRef.current) term.focus();
  };
  // 대화 입력창의 중단 단추(t3code ComposerPrimaryActions.tsx:131·264, plan §8.266): 키 막대의 Esc와 같은 길이다. 다만 유지 키를
  // 잇지 않고(눌러 둔 Ctrl이 Esc를 다른 키로 바꾸지 않게) 입력창에 있는 focus를 터미널로 옮기지 않는다
  const interruptAgent = () => {
    const term = termRef.current;
    if (!term || composingTermRef.current || commitPendingRef.current) return;
    const seq = keyBarInputSequence("Escape", term.modes.applicationCursorKeysMode);
    if (seq === null) return;
    autoFit.current(true);
    deliver(seq);
  };
  const toggleModifier = (modifier: keyof StickyModifiers) => {
    if (composingTermRef.current) return;
    setModifiers((m) => ({ ...m, [modifier]: !m[modifier] }));
    if (!inputLineRef.current) termRef.current?.focus();
  };

  // 입력줄(herdr-web-ui PaneTerminal 2318~2373줄 `sendTerminalLine`·`pressEnter`·`toggleDirect`).
  // 한 줄을 `submit` 하나로 보내고 host 답을 기다린다(참고 `socket.submit` → `submit-result`). host가 글을 쓰고 조금 뒤 Enter를 쓴다:
  // 한 덩어리로 보내면 Codex는 붙여넣기로 보고 Enter를 줄바꿈으로 넣는다(실측). 여러 줄은 터미널 붙여넣기 모양이다:
  // 프로그램이 bracketed paste를 켰으면 감싸고, 줄바꿈은 CR로 바꾼다(xterm.js paste, 참고 `composerPayload`).
  // 그래야 Claude·Codex 입력창이 줄마다 제출하지 않고 한 덩어리로 받는다. 입력줄 글은 유지 키를 붙이지 않는다.
  // `steer`: 입력창의 Ctrl+Enter다. host가 글 뒤 Enter 대신 steering 키를 고른다(작업 중인 Claude는 sendNow, 그 밖은 Enter).
  // 잠금·전달 확인·멈춤은 보통 보내기와 같다(plan §8.247)
  const sendTerminalLine = (value: string, steer = false, traceId?: string, meta?: InputSubmitMeta): false | Promise<true | string> => {
    const term = termRef.current;
    const ws = wsRef.current;
    if (!term || readonlyRef.current || !ws || ws.readyState !== WebSocket.OPEN) return false;
    // 그 글과 Enter가 붙잡은 입력 옆에 들어가면 붙잡은 앞부분 없이 줄이 돈다(PaneTerminal 2338줄)
    if (!draftIsEmpty(draftRef.current)) return Promise.resolve(HELD_FIRST);
    autoFit.current(true);
    // 붙여 넣은 글처럼 ESC를 뺀다(pasteText.ts)
    // 참고 server/index.ts closeMention: 끝의 @파일 자동완성을 공백으로 닫는다.
    // Enter/Escape를 더 보내면 다른 입력까지 실행하거나 작업을 끊으므로 글 안에서만 닫는다.
    let data = pasteText(value);
    if (/(?:^|\s)@\S+$/.test(data)) data += " ";
    if (data.includes("\n")) {
      data = data.replace(/\r?\n/g, "\r");
      if (term.modes.bracketedPasteMode) data = `\x1b[200~${data}\x1b[201~`;
    }
    const id = ++submitIdRef.current;
    trace("submit", { id, data, steer });
    ws.send(JSON.stringify({ t: "submit", id, data, steer, trace: traceId ?? null, meta: meta ?? { pasted: false, attachments: 0, ctrl_enter: steer } } satisfies TermClientMsg));
    term.scrollToBottom();
    // 확인되면 입력줄이 보낸 앞부분만 지운다(그 사이 친 글은 남는다). 거절이면 이유, 끊기거나 답이 없으면 확인 못 함
    return new Promise((resolve, reject) => {
      const timer = window.setTimeout(() => {
        submitsRef.current.delete(id);
        reject(new Error("submit timeout"));
      }, SUBMIT_TIMEOUT_MS);
      submitsRef.current.set(id, {
        ws,
        done: (result) => {
          window.clearTimeout(timer);
          if (result === null) reject(new Error("closed"));
          else resolve(result);
        },
      });
    });
  };
  // 전달 전에 막힌 submit을 멈춘다(plan §8.247): 답을 기다리는 submit 중 마지막 것. 멈췄으면(보내지 않음·Enter 안 침)
  // 그 submit을 끝내고 단계를 돌려준다. 기다리는 submit이 없으면 이미 답이 왔다(늦음), 답이 없으면 null(모름)
  const cancelSubmit = (): Promise<SubmitStage | null> => {
    const ws = wsRef.current;
    const ids = [...submitsRef.current.keys()];
    const id = ids.at(-1);
    if (id === undefined) return Promise.resolve("too_late");
    if (!ws || ws.readyState !== WebSocket.OPEN || submitsRef.current.get(id)?.ws !== ws) return Promise.resolve(null);
    trace("submit_cancel", { id });
    ws.send(JSON.stringify({ t: "submit_cancel", id } satisfies TermClientMsg));
    return new Promise((resolve) => {
      const timer = window.setTimeout(() => {
        cancelsRef.current.delete(id);
        resolve(null);
      }, CANCEL_REPLY_MS);
      cancelsRef.current.set(id, (stage) => {
        window.clearTimeout(timer);
        if (stage === "not_sent" || stage === "no_enter") {
          const waiting = submitsRef.current.get(id);
          submitsRef.current.delete(id);
          waiting?.done(stage === "not_sent" ? "멈춤: 보내지 않았다" : "멈춤: Enter를 보내지 않았다");
        }
        resolve(stage);
      });
    });
  };
  const pressEnter = (): boolean | string => {
    if (readonlyRef.current) return false;
    if (!draftIsEmpty(draftRef.current)) return HELD_FIRST;
    autoFit.current(true);
    const m = modifiersRef.current;
    const chord = hasModifiers(m) ? terminalChord("Enter", m) : null;
    if (chord === "ctrl+enter") return pressCtrlEnter();
    input((chord !== null ? chordSequence(chord, chordCtx()) : null) ?? "\r");
    termRef.current?.scrollToBottom();
    return true;
  };
  const toggleDirect = () => {
    if (composingTermRef.current || lineComposingRef.current) return;
    setTerminalInputMode(directTyping ? "line" : "direct");
  };
  // 입력줄은 터미널을 눌러도 키보드를 올리지 않게 한다. 바로 입력으로 돌리면 키보드를 돌려준다.
  // 바로 입력을 켤 때만 올린다: 켜진 채 pane을 고른 것으로는 올리지 않는다.
  const directTypingRef = useRef(directTyping);
  useEffect(() => {
    const turnedOn = directTyping && !directTypingRef.current;
    directTypingRef.current = directTyping;
    const textarea = termRef.current?.textarea;
    if (!textarea) return;
    if (inputLine) {
      textarea.setAttribute("inputmode", "none");
      if (document.activeElement === textarea) textarea.blur();
    } else {
      textarea.removeAttribute("inputmode");
      if (coarse && turnedOn) termRef.current?.focus();
    }
  }, [inputLine, coarse, directTyping, pane.id]);

  // 입력줄은 흐름 안에서 자라 터미널 칸을 줄인다. 터미널 위끝까지만 자란다(--terminal-input-room, style.css). 커지면 그 자리에서
  // 바로 커서 줄로 옮긴다(맞추기 판단은 크기 바뀜 뒤 120ms에 온다: 그 사이 마지막 줄이 가려지지 않게)
  const [inputSlot, setInputSlot] = useState<HTMLDivElement | null>(null);
  inputSlotRef.current = inputSlot;
  useLayoutEffect(() => {
    const surface = scrollRef.current;
    if (!surface || !inputSlot) return;
    const measure = () => {
      const room = inputSlot.getBoundingClientRect().bottom - surface.getBoundingClientRect().top;
      inputSlot.style.setProperty("--terminal-input-room", `${Math.max(0, Math.floor(room))}px`);
      if (kbRef.current.held) followCursor.current();
      else inputHolds();
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(surface);
    observer.observe(inputSlot);
    return () => observer.disconnect();
  }, [inputSlot]);

  let banner: string | null = null;
  // 출력이 멈춘 것은 아래 띠가 단추와 함께 말한다. 중단된 pane(재부팅 뒤 다시 뜨지 않음)은 터미널 칸의 자리 표시가 까닭과
  // 「다시 열기」를 말한다(아래 `restore-error`, 참고는 그 pane에 붙지 않아 연결 띠도 없다)
  if (link === "stopped" || stopped) banner = null;
  else if (link !== "open") banner = showLink ? (link === "closed" ? "host와 연결이 끊겼다. 다시 연결하는 중" : "연결하는 중") : null;
  else if (readonly) banner = reason ?? (pane.connection === "disconnected" ? "agent와 연결이 끊겼다(읽기 전용)" : "읽기 전용");
  // 끊긴 동안 붙잡은 글은 끊김 띠에 보이고, 다시 붙으면 따로 띠가 되어 보내기·버리기를 고른다(PaneTerminal 2777~2800줄)
  const held = canControl && !draftIsEmpty(draft);
  if (banner && held && ended) banner += " · 붙잡은 입력은 버렸다";
  else if (banner && held && draft.text.length > 0) banner += ` · 붙잡은 입력: “${draft.text}”`;

  // 단축키가 시키는 일(PaneTerminal 2665~2692줄 `focusInput`·`scrollPage`·`sendApplicationPage`). 열린 창(설정 등)이 있으면 하지 않는다
  useLayoutEffect(() => {
    if (!controls) return;
    const modalOpen = () => document.querySelector("[aria-modal='true'], dialog[open]") !== null;
    const current: PaneControls = {
      focusInput: () => {
        if (modalOpen() || !canControl) return;
        // 대화 보기에는 입력창이 없다(읽기 전용)
        if (chatView && chatable) return;
        const line = sectionRef.current?.querySelector<HTMLTextAreaElement>(".terminal-input-text");
        if (line) line.focus({ preventScroll: true });
        else if (!ended) termRef.current?.focus();
      },
      scrollPage: (direction) => {
        if (modalOpen()) return;
        if (chatView && chatable) {
          // 한 화면에서 한 줄을 남기고(참고 pageScrollDistance, PAGE_OVERLAP_ROWS 1)
          const chat = sectionRef.current?.querySelector<HTMLElement>(".chat");
          if (!chat) return;
          const line = parseFloat(getComputedStyle(chat).lineHeight) || 20;
          chat.scrollBy({ top: direction * Math.max(1, chat.clientHeight - line) });
          return;
        }
        const term = termRef.current;
        if (!term) return;
        const buf = term.buffer.active;
        // 기록이 있으면 한 화면에서 한 줄을 남기고 굴린다
        if (buf.type === "normal" && buf.baseY > 0) {
          term.scrollLines(direction * Math.max(1, term.rows - 1));
          return;
        }
        // 기록이 없으면(alternate screen 프로그램) 그 프로그램의 PageUp·PageDown. 지금 받을 수 있을 때만 보내고 미뤄 두지 않는다
        if (!canControl || readonlyRef.current || endedRef.current || releasedRef.current || !draftIsEmpty(draftRef.current)) return;
        if (composingTermRef.current || commitPendingRef.current) return;
        input(direction < 0 ? "\x1b[5~" : "\x1b[6~");
      },
      toggleView: () => {
        if (chatable) setChatView(!chatViewRef.current);
      },
      view: () => (chatable ? (chatViewRef.current ? "chat" : "terminal") : null),
      openFind: () => {
        if (chatViewRef.current) setFindRequest((n) => n + 1);
      },
    };
    controls.current = current;
    return () => {
      if (controls.current === current) controls.current = null;
    };
  });

  const locked = readonly;
  // 대화 보기는 grid가 없고 크기도 놓았다: 거기서는 띠가 흐름에 있다. 터미널에서는 터미널 위 오른쪽에 뜬다(참고 PaneTerminal.css
  // .terminal-banners): 흐름에 넣으면 띠가 나고 들 때마다 칸이 줄었다 늘어 PTY 크기가 바뀐다(차이 목록 1장 G·§8.48 남은 것).
  const flowNotices = chatView && chatable;
  const notices = (
    <>
      {error && (
        <div className="banner err" role="alert">
          {error}
        </div>
      )}
      {banner && (
        <div className={`banner ${readonly ? "warn" : ""}`} role="status" data-testid="readonly-banner">
          {banner}
        </div>
      )}
      {attach && (
        <div className={`banner ${attach.ok ? "" : "warn"}`} role="status" data-testid="attach-banner">
          {attach.text}
          <button type="button" className="ghost icon small" onClick={() => setAttach(null)} aria-label="닫기">
            <Icon name="close" />
          </button>
        </div>
      )}
    </>
  );
  return (
    <section className="term" ref={sectionRef}>
      <header className="term-head">
        {/* 좁은 창의 목록 drawer(drawer.tsx). 전에는 휴대폰에서 「목록으로」 화면을 바꿨다 */}
        <DrawerToggle />
        <HeadNote />
        <div className="term-title">
          <span className="term-title-line">
            {agent && <AgentMark agent={agent} size={18} />}
            <strong>{title ?? (pane.name || pane.id)}</strong>
          </span>
          <span className="muted">
            {crumb ?? pane.machine} · {size[0]}×{size[1]} <ResponsibilityBadge value={pane.responsibility} />
          </span>
        </div>
        {status}
        {chatView && chatable && (
          <button type="button" className="ghost icon chat-find-open" data-testid="chat-find-open" aria-label="대화에서 찾기" title={`대화에서 찾기 (${modKeyLabel() === "⌘" ? "⌘F" : "Ctrl+F"})`}
            onPointerDown={(e) => e.preventDefault()} onClick={() => setFindRequest((n) => n + 1)}>
            <Search aria-hidden="true" />
          </button>
        )}
        {chatable && (
          <button
            className="ghost chat-btn"
            aria-label={chatView ? "터미널" : "대화"}
            aria-pressed={chatView}
            data-testid="chat-toggle"
            onClick={() => setChatView(!chatView)}
            title={chatView ? "터미널로 본다" : "대화 기록으로 본다(읽기 전용)"}
          >
            <Icon name={chatView ? "shell" : "chat"} />
            <span className="head-button-label">{chatView ? "터미널" : "대화"}</span>
          </button>
        )}
        <HeadMore />
      </header>
      {flowNotices && notices}
      {chatView && chatable && <ChatPanel pane={pane.id} status={paneAgentStatus(pane)} compacting={compacting} compaction={pane.agent?.context?.compaction ?? null} readNow={chatSent} onOpenFile={onOpenFile} onOpenLink={onOpenLink} onMessages={setChatMessages} findRequest={findRequest}
        pending={pendingSends} harness={pane.agent?.harness === "codex" ? "codex" : "claude"} onDismissPending={(id) => setPendingSends((list) => list.filter((p) => p.id !== id))}
        steer={steerOpen === "hidden" ? undefined : { state: steer, ...steerOpen, onSteer: pressSteer, onInspect: () => setChatView(false) }} />}
      {/* data-direct-typing: 바로 입력이면 xterm 칸도 화면 키보드를 올린다(herdr/viewport.ts가 본다) */}
      {/* 띠(끊김·권한·오류·그림·보기만·붙잡은 입력·보내지 못함·복사)는 터미널 위 오른쪽에 뜬다: 흐름에 넣으면 칸이 줄어 PTY 크기가 바뀐다(PaneTerminal.css .terminal-banners) */}
      <div className="term-stack" ref={stackRef} hidden={chatView && chatable}>
        <div className="term-scroll" ref={scrollRef} data-direct-typing={inputLine ? undefined : ""} data-adopted-grid={adopted ? "" : undefined}>
          {/* 시험이 「이 pane이 입력을 받는다」를 기다리는 표지: 그 전에 친 글은 붙잡힌다(draft 띠) */}
          <div className="term-box" ref={box} data-testid="terminal" data-pane-name={pane.name} data-readonly={readonly ? "true" : "false"} />
        </div>
        {/* 중단된 pane(재부팅 뒤 다시 띄우지 못했거나 일부러 두었다): 옛 화면 대신 까닭을 보인다(참고 PaneTerminal 2727~2734줄
            `is-restore-error`, plan §8.111). 다시 여는 길도 여기 둔다(QA Q22: 데스크톱은 줄 메뉴가 오른쪽 클릭에만 있다).
            thread pane은 `thread restart`가 띄운다(까닭 글에 명령이 있다) */}
        {stopped && (
          <div className="terminal-placeholder is-restore-error" role="status" data-testid="restore-error">
            <div className="terminal-placeholder-inner">
              <TriangleAlert aria-hidden="true" />
              <span>이 pane은 다시 뜨지 않았다</span>
              <span className="terminal-placeholder-detail">{pane.restore_error ?? "agent backend가 다시 떴을 때 이 세션이 없었다"}</span>
              {canControl && (!pane.role || pane.role === "coordinator") && (
                <button type="button" className="btn btn-primary" data-testid="pane-resume" disabled={resuming} onClick={() => void resume()}>
                  <RotateCcw aria-hidden="true" />
                  다시 열기
                </button>
              )}
              {resumeError && <span className="terminal-placeholder-detail is-error">{resumeError}</span>}
            </div>
          </div>
        )}
        <div className="terminal-banners">
          {link === "stopped" && (
            <div className="banner warn" role="status" data-testid="output-stalled">
              <span>이 기기가 출력을 따라가지 못해 터미널 출력을 멈췄다.</span>
              <button type="button" data-testid="output-reconnect" onClick={() => reattachRef.current()}>
                다시 연결
              </button>
            </div>
          )}
          {!flowNotices && notices}
          {/* 참고 i18n.ko 「View only while you use another window · click to type」 */}
          {released && (
            <div className="banner observe" role="status" data-testid="released-banner">
              다른 창을 쓰는 동안 보기 전용 · 클릭하면 입력
            </div>
          )}
          {!banner && held && !ended && (
            <div className="banner draft" role="status" data-testid="draft-banner">
              <span className="draft-label">터미널이 준비될 때까지 붙잡은 입력:</span>
              {draft.text.length > 0 && <code className="draft-text">{draft.text}</code>}
              {/* 앞부분만 보이면 끝을 잃은 입력도 온전해 보인다 */}
              {draft.truncated && <span className="draft-note">붙잡지 못한 입력이 있어 뺐다.</span>}
              {/* Enter는 붙잡지 않는다: 이것이 없으면 두 명령이 한 줄로, 붙여 넣은 글이 온전하게 보인다 */}
              {draft.leftOut && <span className="draft-note">Enter·줄바꿈은 뺐다: 보내기는 Enter를 누르지 않는다.</span>}
              <span className="draft-actions">
                <button type="button" data-testid="draft-send" disabled={draft.text.length === 0 || readonly} onClick={sendDraft}>
                  보내기
                </button>
                <button type="button" className="ghost" data-testid="draft-discard" onClick={discardDraft}>
                  버리기
                </button>
              </span>
            </div>
          )}
          {inputError && (
            <div className="banner warn" role="status" data-testid="input-error">
              {inputError}
              <button type="button" className="ghost icon small" onClick={() => setInputError(null)} aria-label="닫기">
                <Icon name="close" />
              </button>
            </div>
          )}
          {clipNote && (
            <div className="banner" role="status" data-testid="clipboard-note">
              {clipNote}
            </div>
          )}
        </div>
        {/* 위로 굴린 동안 grid 오른쪽 아래(칸이 보이는 끝)에 뜬다. 누름이 focus를 가져가지 않는다(참고 TerminalViewportTools 244~247줄) */}
        {scrolledUp && !chatView && (
          <JumpToBottom stack={stackRef} onJump={() => termRef.current?.scrollToBottom()} />
        )}
      </div>
      <span ref={pinchBadgeRef} className="pinch-badge" data-testid="pinch-badge" aria-live="polite" hidden />
      {dock}
      {/* 입력줄은 그리드 마지막 줄 위에 뜬다. 흐름에는 한 줄 높이만 두어 자라도 PTY 크기를 바꾸지 않는다(PaneTerminal 3008~3016줄) */}
      {inputLine && (
        <div ref={setInputSlot} className="terminal-input-slot">
          <TerminalInput
            key={pane.id}
            owner={`${pane.machine}:${pane.id}`}
            connected={!locked}
            onComposing={setLineComposing}
            onSend={sendTerminalLine}
            onEnter={pressEnter}
            onCtrlEnter={() => pressCtrlEnter()}
            onCtrlEnterBlocked={blockedCtrlEnter}
            onFocus={() => autoFit.current(true)}
            onImage={() => fileRef.current?.click()}
            onPaste={(e) => {
              const files = pastedFiles(e.clipboardData);
              if (files.length > 0) {
                e.preventDefault();
                uploadFiles(files);
              }
            }}
          />
        </div>
      )}
      {canControl && (
        <input
          ref={fileRef}
          type="file"
          accept="image/png,image/jpeg,image/gif,image/webp"
          multiple
          hidden
          data-testid="attach-file"
          onChange={(e) => {
            const files = Array.from(e.target.files ?? []);
            e.target.value = "";
            uploadFiles(files);
          }}
        />
      )}
      {/* 대화 보기의 입력창(차이 목록 C2): 터미널 입력줄과 같은 길로 보낸다. 키 막대는 터미널 보기 것이다 */}
      {chatView && chatable && canControl && !stopped && (
        <ChatComposer
          key={pane.id}
          owner={`${pane.machine_id}:${pane.id}`}
          agentLabel={pane.agent?.harness === "codex" ? "Codex" : "Claude"}
          harness={pane.agent?.harness === "codex" ? "codex" : "claude"}
          pending={pendingSends}
          setPending={setPendingSends}
          connected={!locked}
          pane={pane.id}
          touch={coarse}
          working={paneAgentStatus(pane) === "working"}
          compacting={compacting}
          asking={paneAgentStatus(pane) === "blocked"}
          coordinator={pane.role === "coordinator"}
          context={pane.agent?.context ?? null}
          onInterrupt={interruptAgent}
          messages={chatMessages}
          onInspect={() => setChatView(false)}
          onSend={sendTerminalLine}
          onInputTrace={(id, event, method) => {
            const ws = wsRef.current;
            if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: "input_trace", id, event, method: method ?? null } satisfies TermClientMsg));
          }}
          onCtrlEnter={() => pressCtrlEnter(true)}
          onCancelSend={cancelSubmit}
          onSent={() => setChatSent((n) => n + 1)}
          onBusy={setComposerBusy}
        />
      )}
      {canControl && !stopped && !(chatView && chatable) && (
        <KeyBar
          items={barItems}
          modifiers={modifiers}
          onKey={pressKey}
          onToggleModifier={toggleModifier}
          disabled={locked || composing}
          holdScope={pane.id}
          {...(coarse ? { directTyping, onToggleDirect: toggleDirect, directDisabled: lineComposing } : {})}
        />
      )}
    </section>
  );
}

/**
 * 「맨 아래로 이동」 단추(herdr-web-ui TerminalViewportTools 64~82·244~247줄 `paintBox`·`terminal-jump-bottom`). grid 오른쪽 아래에 두되
 * 칸이 보이는 끝까지만이다: 이 화면 것이 아닌 grid(보기 권한, 다른 화면이 정한 크기)는 칸보다 커서 칸 안에서 굴러간다.
 * 자리는 칸 크기가 바뀌거나 칸이 굴러갈 때만 다시 잰다(그린 frame마다가 아니다).
 */
function JumpToBottom({ stack, onJump }: { stack: RefObject<HTMLDivElement | null>; onJump: () => void }) {
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  useLayoutEffect(() => {
    const outer = stack.current;
    const mount = outer?.querySelector<HTMLElement>(".term-scroll");
    const screen = outer?.querySelector<HTMLElement>(".xterm-screen");
    if (!outer || !mount || !screen) return;
    const paint = () => {
      const rect = screen.getBoundingClientRect();
      const box = outer.getBoundingClientRect();
      const view = mount.getBoundingClientRect();
      const right = Math.min(rect.right, box.right, view.left + mount.clientLeft + mount.clientWidth);
      const bottom = Math.min(rect.bottom, box.bottom, view.top + mount.clientTop + mount.clientHeight);
      const next = { x: right - box.left, y: bottom - box.top };
      setAnchor((old) => (old && old.x === next.x && old.y === next.y ? old : next));
    };
    paint();
    const observer = new ResizeObserver(paint);
    observer.observe(outer);
    observer.observe(screen);
    mount.addEventListener("scroll", paint, { passive: true });
    return () => {
      observer.disconnect();
      mount.removeEventListener("scroll", paint);
    };
  }, [stack]);
  if (!anchor) return null;
  return (
    <button
      type="button"
      className="terminal-jump-bottom"
      data-testid="jump-bottom"
      aria-label="맨 아래로 이동"
      title="맨 아래로 이동"
      style={{ left: anchor.x, top: anchor.y }}
      onPointerDown={(event) => event.preventDefault()}
      onClick={onJump}
    >
      <ArrowDown aria-hidden="true" />
    </button>
  );
}
