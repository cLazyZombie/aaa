// pane 하나의 터미널 보기. WebSocket 하나가 대화형 front 하나다. 조작 권한이 있으면 언제나 입력된다(herdr-web-ui와 같다, §8.33).
// PTY 크기는 「지금 쓰는 화면」(lease)이 정한다. 이 탭이 보이고 focus가 있을 때만 크기를 건다:
// - 이 화면이 지금 쓰는 화면이거나 아무도 쓰지 않으면 칸 크기에 맞춘다(열 때·창 크기가 바뀔 때).
// - focus를 얻거나 다시 보이면, 또는 터미널을 누르거나 입력하면 이 화면이 지금 쓰는 화면이 되어 크기를 다시 건다.
// - 탭이 가려지거나 대화 보기로 바꾸면 바로 놓는다(다른 화면이 크기를 되찾는다). 다른 화면이 쓰는 동안은 PTY 크기 그대로 왼쪽 위에 그린다.
// 그림을 붙여 넣거나(끌어 놓기·「그림」 버튼) 하면 대상 머신 세션 폴더에 저장하고, harness 입력창이면 경로를 넣는다.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { DeviceRole, LeaseView, PaneView, TermClientMsg, TermServerMsg } from "./gen/proto";
import { b64decode, b64encode, wsUrl } from "./api";
import { Icon } from "./icons";
import { ChatPanel } from "./Chat";
import { onThemeChange, token } from "./theme";
import { matchCompositionView } from "./compositionView";
import { adjustTerminalGlyphs } from "./herdr/terminalGlyphs.ts";
import { keyboardHoldsGrid, keyboardUp } from "./herdr/keyboard.ts";
import { attachTrace, trace } from "./inputTrace";
import {
  clipboardKey,
  ctrlEnterSequence,
  hasModifiers,
  keyFromData,
  modifyOtherKeysLevel,
  navigationSequence,
  NO_STICKY_MODIFIERS,
  physicalKey,
  terminalChord,
  type StickyModifiers,
} from "./herdr/keys.ts";
import { DEFAULT_KEY_BAR_ITEMS, keyBarInputSequence, sanitizeKeyBarItems, type KeyBarKeyItem } from "./herdr/keyBar.ts";
import { chordSequence, IS_MAC } from "./keyChord";
import { KeyBar } from "./KeyBar";
import { TerminalInput } from "./TerminalInput";
import { setTerminalFontSize, setTerminalInputMode, useMedia, useTerminalFontSize, useTerminalInputMode } from "./settings";
import { pinchFontSize } from "./herdr/pinchFontSize.ts";
import { parseOsc52 } from "./herdr/osc52.ts";
import { applyToDraft, draftIsEmpty, EMPTY_DRAFT, restoreDraft, type InputDraft } from "./herdr/draft.ts";
import { isWebLink } from "./herdr/terminalFileLinks.ts";
import { WebLinksAddon } from "@xterm/addon-web-links";

/** 함께 넣은 터미널 글꼴(style.css @font-face) */
const WEB_FONT = '"Meslotendard Web"';

/** 한 번에 보내는 조각(host 상한과 같다) */
const CHUNK = 512 * 1024;
const MAX_IMAGE = 16 << 20;
/** 글 뒤 Enter까지 기다리는 시간(host의 brief·메시지 전달과 같다) */
const ENTER_GAP_MS = 300;
/** 붙잡은 입력이 있는 동안 다른 입력을 거절하는 말(herdr-web-ui PaneTerminal 1375줄) */
const HELD_FIRST = "보내지 않았다: 붙잡은 입력을 먼저 보내거나 버린다.";
/** 이 탭을 지금 쓰는 중: 보이고 focus가 있다. 아니면 PTY 크기를 걸지 않는다. */
const inUse = () => document.visibilityState === "visible" && document.hasFocus();

/**
 * 붙여 넣기에서 올릴 파일들(herdr-web-ui PaneTerminal 1449~1458줄): 클립보드에 글이 있으면 글이다(xterm이 붙여 넣는다,
 * 복사한 경로도 글로 남는다). 파일만 있을 때 그 파일 전부.
 */
function pastedFiles(data: DataTransfer | null | undefined): File[] {
  if (!data || data.getData("text/plain")) return [];
  return Array.from(data.files);
}

type Link = "connecting" | "open" | "closed";

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

export function TermView({
  pane,
  role,
  onBack,
  title,
  crumb,
  status,
  dock,
}: {
  pane: PaneView;
  role: DeviceRole;
  onBack: () => void;
  /** 목록과 같은 이름(thread pane이면 thread 제목) */
  title?: string;
  crumb?: string;
  status?: ReactNode;
  /** 입력 위에 붙는 질문 카드 */
  dock?: ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const readonlyRef = useRef(true);
  const [link, setLink] = useState<Link>("connecting");
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
  const [chatView, setChatView] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);
  const canControl = role !== "reader";
  // 다른 front가 지금 쓰는 화면이다(이 연결의 ID를 받기 전에는 판단하지 않는다). 입력은 막지 않는다.
  const heldByOther = me !== null && lease !== null && lease.front !== me;
  // 입력 방식(herdr-web-ui PaneTerminal 358~361줄): 기본은 바로 입력, 입력줄은 이 기기의 선택이다.
  const directTyping = useTerminalInputMode() !== "line";
  const inputLine = !directTyping && !(chatView && chatable) && canControl;
  const inputLineRef = useRef(inputLine);
  inputLineRef.current = inputLine;
  const coarse = useMedia("(pointer: coarse)");
  // 글자 크기(설정, 이 기기). 핀치 중에는 손가락을 따르는 크기다(herdr-web-ui PaneTerminal `baseFontRef`).
  const terminalFontSize = useTerminalFontSize();
  const baseFontRef = useRef(terminalFontSize);
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
      const seq = chord !== null ? chordSequence(chord, chordCtx()) : null;
      if (seq !== null) return deliver(seq, { chord: true });
    }
    deliver(data, { modifiedEnter });
  };

  useEffect(() => {
    // 모든 기기가 설정한 크기(기본 13px)로 연다. 좁은 화면만 11px로 줄이던 것은 없앴다(herdr-web-ui와 같다)
    const fontSize = baseFontRef.current;
    // xterm은 처음 연 글꼴로 cell 크기를 잰다. 넣은 WOFF2가 아직 안 받아졌으면 그것을 뺀 목록으로 열고,
    // 받아지면 원래 목록으로 바꿔 다시 재게 한다(그 뒤 자동 맞추기가 다시 돈다).
    const family = token("--font-term") || "ui-monospace, Menlo, monospace";
    const webFont = `${fontSize}px ${WEB_FONT}`;
    const ready = !family.includes(WEB_FONT) || document.fonts.check(webFont);
    const term = new Terminal({
      cols: pane.cols,
      rows: pane.rows,
      fontSize,
      lineHeight: 1.15,
      fontFamily: ready ? family : family.replace(`${WEB_FONT}, `, ""),
      scrollback: 2000,
      convertEol: false,
      theme: termTheme(),
      // herdr-web-ui PaneTerminal 539~562줄: 블록 커서 깜빡임(프로그램의 DECSCUSR는 그대로 바꾼다),
      // macOS에서 Option+누름은 마우스 보고 중에도 선택
      cursorStyle: "block",
      cursorBlink: true,
      macOptionClickForcesSelection: true,
    });
    const offTheme = onThemeChange(() => {
      term.options.theme = termTheme();
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(box.current!);
    const el = box.current!;
    // 데스크톱은 pane을 열면 바로 친다(PaneTerminal 2072줄). 터치 기기는 키보드를 올리지 않게, 모달 중에는 그것을 두고 하지 않는다.
    if (!window.matchMedia("(pointer: coarse)").matches && !document.querySelector('[aria-modal="true"]')) term.focus();
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
    });
    // 실기 입력 사건 기록(켜져 있을 때만 모은다, inputTrace.ts)
    const stopTrace = attachTrace(term.textarea!, scrollRef.current!);
    // ── 키 처리(herdr-web-ui PaneTerminal 585·620~664·1053~1076·1284~1335줄) ──
    // 웹 링크는 http(s)만 새 탭으로 연다(줄이 링크처럼 보여도 다른 scheme은 열지 않는다)
    term.loadAddon(new WebLinksAddon((_event, uri) => {
      if (isWebLink(uri)) window.open(uri, "_blank", "noopener,noreferrer");
    }));
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
    };
    // window capture: xterm의 document 처리보다 먼저 본다
    const onMouseMove = (event: MouseEvent) => {
      const d = drag;
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
    // Windows·Linux: Ctrl+V는 브라우저 붙여넣기에 맡기고, 선택이 있을 때 Ctrl+C는 복사한다(없으면 ^C)
    term.attachCustomKeyEventHandler((event) => {
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
    const osc52 = term.parser.registerOscHandler(52, (payload) => {
      const text = parseOsc52(payload);
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
    // xterm.js는 Shift+Enter·Ctrl+Enter를 그냥 Enter로 친다: Shift+Enter는 Alt+Enter 줄바꿈으로, Ctrl+Enter는 프로그램이
    // 요청한 형식으로 바꾼다. macOS Cmd+⌫는 줄 지우기(Ctrl+U). onKey는 IME 확정 글 뒤, 그 키의 onData 바로 앞에 온다.
    let enterAs: string | null = null;
    let commandBackspace = false;
    const keySub = term.onKey(({ key, domEvent: event }) => {
      enterAs = null;
      commandBackspace = false;
      if (event.isComposing || event.keyCode === 229) return;
      if (key === "\r" && event.key === "Enter" && !event.altKey && !event.metaKey) {
        if (event.shiftKey && !event.ctrlKey) enterAs = "\x1b\r";
        else if (event.ctrlKey && !event.shiftKey) enterAs = ctrlEnterSequence(otherKeysRef.current);
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
          if (!alive || term.options.fontFamily === family) return;
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
      if (files.length === 0) return;
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
        term.paste(text);
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
    // 한 손가락 끌기는 xterm 그대로 둔다(차이 목록 J, 스크롤 묶음과 함께).
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
      if (event.touches.length < 2) return;
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
      if (!pinchStroke && event.touches.length > 1) pinchStroke = true;
      if (!pinchStroke) return;
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
    const connect = () => {
      setLink("connecting");
      const ws = new WebSocket(wsUrl(`/api/v1/panes/${encodeURIComponent(pane.id)}/ws`));
      ws.binaryType = "arraybuffer";
      wsRef.current = ws;
      ws.onopen = () => {
        delay = 500;
        setLink("open");
      };
      ws.onmessage = (m) => {
        if (m.data instanceof ArrayBuffer) {
          term.write(new Uint8Array(m.data));
          return;
        }
        const msg = JSON.parse(m.data as string) as TermServerMsg;
        switch (msg.t) {
          case "snapshot":
            trace("snapshot");
            term.reset();
            // replay가 키보드 모드를 처음부터 다시 말한다(herdr-web-ui도 재접속 때 0으로 되돌린다)
            otherKeysRef.current = 0;
            term.resize(msg.cols, msg.rows);
            setSize([msg.cols, msg.rows]);
            term.write(b64decode(msg.replay));
            readonlyRef.current = msg.readonly;
            setReadonly(msg.readonly);
            setError(null);
            break;
          case "readonly":
            readonlyRef.current = msg.readonly;
            setReadonly(msg.readonly);
            setReason(msg.reason);
            break;
          case "front":
            setMe(msg.id);
            break;
          case "state":
            setLease(msg.pane.lease);
            if (msg.pane.cols !== term.cols || msg.pane.rows !== term.rows) {
              term.resize(msg.pane.cols, msg.pane.rows);
              setSize([msg.pane.cols, msg.pane.rows]);
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
          case "error":
            setError(msg.message);
            setAttach((a) => (a?.text.startsWith("그림 보내는 중") ? null : a));
            attachDone();
            break;
        }
      };
      ws.onclose = () => {
        if (wsRef.current === ws) wsRef.current = null;
        // 올리던 파일은 끝나지 않는다: 묶음의 나머지를 기다리게 두지 않는다
        attachDone();
        readonlyRef.current = true;
        setReadonly(true);
        setLink("closed");
        // 끊기면 유지 키를 푼다(PaneTerminal 1261줄)
        setModifiers(NO_STICKY_MODIFIERS);
        if (!stopped) timer = window.setTimeout(connect, delay);
        delay = Math.min(delay * 2, 10_000);
      };
    };
    connect();
    return () => {
      alive = false;
      stopped = true;
      window.clearTimeout(timer);
      wsRef.current?.close();
      el.removeEventListener("paste", onPaste, true);
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
      otherKeysSet.dispose();
      otherKeysOff.dispose();
      binSub.dispose();
      offTheme();
      term.dispose();
      termRef.current = null;
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
  const followCursor = useRef(() => {});
  followCursor.current = () => {
    const host = scrollRef.current;
    const term = termRef.current;
    const k = kbRef.current;
    const screen = term?.element?.querySelector<HTMLElement>(".xterm-screen");
    if (!host || !term || !screen || !k.held || k.panned) return;
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
    if (!canControl || readonly || link !== "open" || me === null || (chatView && chatable) || !inUse()) return;
    if (heldByOther && !claim) return;
    if (keyboardHolds()) return;
    const dims = fitRef.current?.proposeDimensions();
    if (!dims || !Number.isFinite(dims.cols) || dims.cols < 10 || dims.rows < 3) return;
    if (dims.cols !== size[0] || dims.rows !== size[1] || (claim && heldByOther))
      send({ t: "resize", cols: dims.cols, rows: dims.rows });
    fitted();
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
  // focus를 얻거나 다시 보이면 이 화면 크기를 다시 건다. 가려지면 바로 크기를 놓는다(herdr-web-ui PaneTerminal 1831~1842줄
  // `keepSize(hidden)`: 숨은 문서는 서버 grid를 놓고 연결은 둔다). 보이는 채 focus만 잃으면 그대로 둔다.
  // 참고의 「가려지면 1초 뒤 보기 전용으로」(`releasePaneAway`)는 기본 꺼짐 설정이라 옮기지 않았다(host에 watch가 없다).
  const leaseMine = me !== null && lease?.front === me;
  const leaseMineRef = useRef(leaseMine);
  leaseMineRef.current = leaseMine;
  useEffect(() => {
    const back = () => autoFit.current(true);
    const vis = () => {
      if (document.visibilityState === "visible") return back();
      if (leaseMineRef.current) send({ t: "release" });
    };
    window.addEventListener("focus", back);
    document.addEventListener("visibilitychange", vis);
    return () => {
      window.removeEventListener("focus", back);
      document.removeEventListener("visibilitychange", vis);
    };
  }, [pane.id]);
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    let t: number | undefined;
    const ro = new ResizeObserver(() => {
      window.clearTimeout(t);
      t = window.setTimeout(() => {
        // 키보드가 화면 아래를 가렸다: grid는 그대로, 터미널만 커서 줄로 옮긴다(맞출 수 없는 화면도)
        if (keyboardHolds()) return;
        autoFit.current();
      }, 120);
    });
    ro.observe(el);
    // 손으로 옮기면 키보드가 있는 동안 커서를 따라가지 않는다(herdr-web-ui `panned`)
    const host = scrollRef.current;
    const pan = () => {
      if (kbRef.current.held) kbRef.current.panned = true;
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

  // 키 막대(herdr-web-ui PaneTerminal 2218~2245줄). 항목은 기기별 저장값(설정 화면은 아직 없다), 없으면 참고 기본 배열.
  const [barItems] = useState(() => {
    try {
      const raw = localStorage.getItem("aaa:keybar-items");
      return sanitizeKeyBarItems(raw === null ? undefined : JSON.parse(raw));
    } catch {
      return DEFAULT_KEY_BAR_ITEMS;
    }
  });
  const pressKey = (item: KeyBarKeyItem) => {
    const term = termRef.current;
    if (!term || composingTermRef.current || commitPendingRef.current) return;
    // 저장된 조합은 그대로, 보통 키는 유지 키를 이어받는다. 내장 ^C 키는 늘 ^C다.
    const active = item.modifiers ?? modifiersRef.current;
    const modified = (item.key !== "ctrl-c" || item.modifiers !== undefined) && (hasModifiers(active) || item.modifiers !== undefined);
    const chord = modified ? terminalChord(item.key, active) : null;
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
  const toggleModifier = (modifier: keyof StickyModifiers) => {
    if (composingTermRef.current) return;
    setModifiers((m) => ({ ...m, [modifier]: !m[modifier] }));
    if (!inputLineRef.current) termRef.current?.focus();
  };

  // 입력줄(herdr-web-ui PaneTerminal 2318~2373줄 `sendTerminalLine`·`pressEnter`·`toggleDirect`).
  // 글과 Enter를 한 덩어리로 보내면 Codex는 붙여넣기로 보고 Enter를 줄바꿈으로 넣는다(실측). Enter는 따로, 조금 뒤에 보낸다
  // (참고는 server가 이 간격을 둔다). 여러 줄은 터미널 붙여넣기로 보낸다: 프로그램이 bracketed paste를 켰으면 감싸고,
  // 줄바꿈은 CR로 바꾼다(xterm.js). 그래야 Claude·Codex 입력창이 줄마다 제출하지 않고 한 덩어리로 받는다.
  // 입력줄 글은 유지 키를 붙이지 않고 바로 간다.
  const sendTerminalLine = (value: string): false | Promise<true | string> => {
    const term = termRef.current;
    if (!term || readonlyRef.current) return false;
    // 그 글과 Enter가 붙잡은 입력 옆에 들어가면 붙잡은 앞부분 없이 줄이 돈다(PaneTerminal 2338줄)
    if (!draftIsEmpty(draftRef.current)) return Promise.resolve(HELD_FIRST);
    autoFit.current(true);
    if (value.includes("\n")) {
      pastingRef.current = true;
      try {
        term.paste(value);
      } finally {
        pastingRef.current = false;
      }
    } else input(value);
    term.scrollToBottom();
    // aaa 입력에는 host 확인 답이 없다(참고는 server의 submit 답을 기다린다). Enter까지 보낸 때를 「보냄」으로 친다:
    // 그 사이 친 글은 줄에 남는다(terminalDraft가 보낸 앞부분만 지운다).
    return new Promise((resolve) =>
      window.setTimeout(() => {
        if (readonlyRef.current) return resolve("Enter를 보내지 못했다: 터미널 연결이 끊겼다");
        input("\r");
        resolve(true);
      }, ENTER_GAP_MS),
    );
  };
  const pressEnter = (): boolean | string => {
    if (readonlyRef.current) return false;
    if (!draftIsEmpty(draftRef.current)) return HELD_FIRST;
    autoFit.current(true);
    const m = modifiersRef.current;
    const chord = hasModifiers(m) ? terminalChord("Enter", m) : null;
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

  // 입력줄은 터미널 마지막 줄 위에 떠서 자란다. 터미널 위끝까지만 자란다(--terminal-input-room, TerminalInput.css).
  const [inputSlot, setInputSlot] = useState<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    const surface = scrollRef.current;
    if (!surface || !inputSlot) return;
    const measure = () => {
      const room = inputSlot.getBoundingClientRect().bottom - surface.getBoundingClientRect().top;
      inputSlot.style.setProperty("--terminal-input-room", `${Math.max(0, Math.floor(room))}px`);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(surface);
    observer.observe(inputSlot);
    return () => observer.disconnect();
  }, [inputSlot]);

  let banner: string | null = null;
  if (link !== "open") banner = link === "closed" ? "host와 연결이 끊겼다. 다시 연결하는 중" : "연결하는 중";
  else if (readonly) banner = reason ?? (pane.connection === "disconnected" ? "agent와 연결이 끊겼다(읽기 전용)" : "읽기 전용");
  // 끊긴 동안 붙잡은 글은 끊김 띠에 보이고, 다시 붙으면 따로 띠가 되어 보내기·버리기를 고른다(PaneTerminal 2777~2800줄)
  const held = canControl && !draftIsEmpty(draft);
  if (banner && held && ended) banner += " · 붙잡은 입력은 버렸다";
  else if (banner && held && draft.text.length > 0) banner += ` · 붙잡은 입력: “${draft.text}”`;

  const locked = readonly;
  return (
    <section className="term">
      <header className="term-head">
        <button className="back ghost icon" onClick={onBack} aria-label="목록으로">
          <Icon name="back" />
        </button>
        <div className="term-title">
          <strong>{title ?? (pane.name || pane.id)}</strong>
          <span className="muted">
            {crumb ?? pane.machine} · {size[0]}×{size[1]}
          </span>
        </div>
        {status}
        {chatable && (
          <button
            className="ghost chat-btn"
            aria-pressed={chatView}
            data-testid="chat-toggle"
            onClick={() => setChatView((v) => !v)}
            title={chatView ? "터미널로 본다" : "대화 기록으로 본다(읽기 전용)"}
          >
            <Icon name={chatView ? "shell" : "chat"} />
            {chatView ? "터미널" : "대화"}
          </button>
        )}
      </header>
      {banner && (
        <div className={`banner ${readonly ? "warn" : ""}`} data-testid="readonly-banner">
          {banner}
        </div>
      )}
      {error && <div className="banner err">{error}</div>}
      {attach && (
        <div className={`banner ${attach.ok ? "" : "warn"}`} data-testid="attach-banner">
          {attach.text}
          <button type="button" className="ghost icon small" onClick={() => setAttach(null)} aria-label="닫기">
            <Icon name="close" />
          </button>
        </div>
      )}
      {chatView && chatable && <ChatPanel pane={pane.id} />}
      {/* data-direct-typing: 바로 입력이면 xterm 칸도 화면 키보드를 올린다(herdr/viewport.ts가 본다) */}
      {/* 띠(붙잡은 입력·보내지 못함·복사)는 터미널 위 오른쪽에 뜬다: 흐름에 넣으면 칸이 줄어 PTY 크기가 바뀐다(PaneTerminal.css .terminal-banners) */}
      <div className="term-stack" hidden={chatView && chatable}>
        <div className="term-scroll" ref={scrollRef} data-direct-typing={inputLine ? undefined : ""}>
          <div className="term-box" ref={box} data-testid="terminal" />
        </div>
        <div className="terminal-banners">
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
      {canControl && (
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
