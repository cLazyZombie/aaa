// pane 하나의 터미널 보기. 크기는 PTY(cols·rows)를 따르고, 「이 화면에 맞추기」만 PTY 크기를 바꾼다.
// WebSocket 하나가 대화형 front 하나다. 다른 front가 조작(lease) 중이면 입력을 막고 「가져오기」를 보인다.
// 그림을 붙여 넣거나(끌어 놓기·「그림」 버튼) 하면 대상 머신 세션 폴더에 저장하고, harness 입력창이면 경로를 넣는다.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { DeviceRole, LeaseView, PaneView, TermClientMsg, TermServerMsg } from "./gen/proto";
import { b64decode, b64encode, wsUrl } from "./api";
import { Icon } from "./icons";
import { ChatPanel } from "./Chat";
import { onThemeChange, token } from "./theme";

/** 한 번에 보내는 조각(host 상한과 같다) */
const CHUNK = 512 * 1024;
const MAX_IMAGE = 16 << 20;
/** 글 뒤 Enter까지 기다리는 시간(host의 brief·메시지 전달과 같다) */
const ENTER_GAP_MS = 300;

/** 붙여 넣기·끌어 놓기에서 그림 파일 하나 */
function imageOf(list: FileList | undefined | null): File | null {
  for (const f of Array.from(list ?? [])) if (f.type.startsWith("image/")) return f;
  return null;
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
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const readonlyRef = useRef(true);
  const blockedRef = useRef(false);
  const [link, setLink] = useState<Link>("connecting");
  const [readonly, setReadonly] = useState(true);
  const [reason, setReason] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [size, setSize] = useState<[number, number]>([pane.cols, pane.rows]);
  // 쓰다 만 글은 pane마다 이 탭에 남긴다(휴대폰에서 앱을 내렸다 올려도, 다른 pane을 봤다 와도 그대로)
  const draftKey = `aaa:draft:${pane.id}`;
  const [text, setTextState] = useState(() => {
    try {
      return sessionStorage.getItem(draftKey) ?? "";
    } catch {
      return "";
    }
  });
  const setText = (v: string) => {
    setTextState(v);
    try {
      if (v) sessionStorage.setItem(draftKey, v);
      else sessionStorage.removeItem(draftKey);
    } catch {
      // 저장소를 못 쓰는 브라우저면 남기지 않는다
    }
  };
  const textRef = useRef<HTMLTextAreaElement>(null);
  const [me, setMe] = useState<string | null>(null);
  const [lease, setLease] = useState<LeaseView | null>(pane.lease);
  const [attach, setAttach] = useState<{ text: string; ok: boolean } | null>(null);
  // Claude·Codex pane은 터미널 대신 대화 기록으로도 본다(읽기 전용)
  const chatable = pane.agent?.harness === "claude" || pane.agent?.harness === "codex";
  const [chatView, setChatView] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);
  const canControl = role !== "reader";
  // 다른 front가 조작 중이다(이 연결의 ID를 받기 전에는 판단하지 않는다)
  const heldByOther = me !== null && lease !== null && lease.front !== me;
  blockedRef.current = heldByOther;

  const send = (m: TermClientMsg) => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(m));
  };
  const input = (data: string) => {
    if (!readonlyRef.current && !blockedRef.current) send({ t: "input", data });
  };
  /** 그림을 조각으로 보낸다. 결과(attached·error)는 WebSocket으로 온다. */
  const attachFile = async (f: File) => {
    if (readonlyRef.current || blockedRef.current) {
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
    for (let off = 0; off < bytes.length; off += CHUNK) {
      send({ t: "attach", offset: off, total: bytes.length, sha256, data: b64encode(bytes.subarray(off, off + CHUNK)) });
    }
  };
  const attachRef = useRef(attachFile);
  attachRef.current = attachFile;

  // 키 줄의 유지 키(Ctrl·Alt): 누르면 켜지고, 다음 입력 한 번에 적용한 뒤 풀린다(휴대폰 키보드에는 Ctrl이 없다)
  const [mods, setMods] = useState({ ctrl: false, alt: false });
  const modsRef = useRef(mods);
  modsRef.current = mods;
  const withMods = (data: string): string => {
    const m = modsRef.current;
    if (!m.ctrl && !m.alt) return data;
    let out = data;
    if (m.ctrl && data.length === 1) {
      const c = data.toLowerCase().charCodeAt(0);
      if (c >= 97 && c <= 122) out = String.fromCharCode(c - 96);
      else if ("@[\\]^_".includes(data)) out = String.fromCharCode(data.charCodeAt(0) - 64);
      else if (data === " ") out = "\x00";
    }
    if (m.alt) out = "\x1b" + out;
    modsRef.current = { ctrl: false, alt: false };
    setMods(modsRef.current);
    return out;
  };

  // 커서 위치 보고(ESC[r;cR)는 session host가 이미 답했다. xterm.js의 자동 답은 보내지 않는다.
  const fromTerminal = (data: string) => {
    if (/^\x1b\[\d+;\d+R$/.test(data)) return;
    input(withMods(data));
  };

  useEffect(() => {
    const narrow = window.matchMedia("(max-width: 700px)").matches;
    const term = new Terminal({
      cols: pane.cols,
      rows: pane.rows,
      fontSize: narrow ? 11 : 13,
      lineHeight: 1.15,
      fontFamily: token("--font-mono") || "ui-monospace, Menlo, monospace",
      scrollback: 2000,
      convertEol: false,
      theme: termTheme(),
    });
    const offTheme = onThemeChange(() => {
      term.options.theme = termTheme();
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(box.current!);
    // xterm의 글 붙여 넣기보다 먼저 본다: 그림이면 가로채고, 아니면 그대로 둔다
    const onPaste = (e: ClipboardEvent) => {
      const f = imageOf(e.clipboardData?.files);
      if (!f) return;
      e.preventDefault();
      e.stopPropagation();
      void attachRef.current(f);
    };
    const onDragOver = (e: DragEvent) => {
      if (e.dataTransfer?.types.includes("Files")) e.preventDefault();
    };
    const onDrop = (e: DragEvent) => {
      const f = imageOf(e.dataTransfer?.files);
      if (!f) return;
      e.preventDefault();
      void attachRef.current(f);
    };
    const el = box.current!;
    el.addEventListener("paste", onPaste, true);
    el.addEventListener("dragover", onDragOver);
    el.addEventListener("drop", onDrop);
    termRef.current = term;
    fitRef.current = fit;
    const dataSub = term.onData(fromTerminal);
    const binSub = term.onBinary((s) => {
      if (readonlyRef.current || blockedRef.current) return;
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
            term.reset();
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
                ? `그림을 입력창에 넣었다(Enter로 보낸다): ${msg.path}`
                : `그림을 저장했다: ${msg.path}${msg.note ? ` · ${msg.note}` : ""}`,
              ok: true,
            });
            termRef.current?.focus();
            break;
          case "error":
            setError(msg.message);
            setAttach((a) => (a?.text.startsWith("그림 보내는 중") ? null : a));
            break;
        }
      };
      ws.onclose = () => {
        if (wsRef.current === ws) wsRef.current = null;
        readonlyRef.current = true;
        setReadonly(true);
        setLink("closed");
        if (!stopped) timer = window.setTimeout(connect, delay);
        delay = Math.min(delay * 2, 10_000);
      };
    };
    connect();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
      wsRef.current?.close();
      el.removeEventListener("paste", onPaste, true);
      el.removeEventListener("dragover", onDragOver);
      el.removeEventListener("drop", onDrop);
      dataSub.dispose();
      binSub.dispose();
      offTheme();
      term.dispose();
      termRef.current = null;
    };
    // pane.id가 바뀔 때만 새로 붙는다. cols/rows는 snapshot이 알려 준다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pane.id]);

  const fitToScreen = () => {
    const dims = fitRef.current?.proposeDimensions();
    if (dims && dims.cols > 0 && dims.rows > 0) send({ t: "resize", cols: dims.cols, rows: dims.rows });
  };

  const take = () => {
    setError(null);
    send({ t: "take" });
  };

  // 글과 Enter를 한 덩어리로 보내면 Codex는 붙여넣기로 보고 Enter를 줄바꿈으로 넣는다(실측). Enter는 따로, 조금 뒤에 보낸다.
  // 여러 줄은 터미널 붙여넣기로 보낸다: 프로그램이 bracketed paste를 켰으면 감싸고, 줄바꿈은 CR로 바꾼다(xterm.js).
  // 그래야 Claude·Codex 입력창이 줄마다 제출하지 않고 한 덩어리로 받는다.
  const sendText = (enter: boolean) => {
    if (text.includes("\n")) termRef.current?.paste(text);
    else if (text) input(text);
    if (enter) {
      if (text) window.setTimeout(() => input("\r"), ENTER_GAP_MS);
      else input("\r");
    }
    setText("");
    termRef.current?.focus();
  };

  // 입력칸은 글 줄 수만큼(최대 6줄) 늘어난다
  useEffect(() => {
    const el = textRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 6 * 22 + 20)}px`;
  }, [text]);

  let banner: string | null = null;
  if (link !== "open") banner = link === "closed" ? "host와 연결이 끊겼다. 다시 연결하는 중(읽기 전용)" : "연결하는 중";
  else if (readonly) banner = reason ?? (pane.connection === "disconnected" ? "agent와 연결이 끊겼다(읽기 전용)" : "읽기 전용");

  const locked = readonly || heldByOther;
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
        {canControl && (
          <button className="ghost fit-btn" onClick={fitToScreen} disabled={locked} title="PTY 크기를 이 화면에 맞춘다">
            <Icon name="fit" />
            화면에 맞추기
          </button>
        )}
      </header>
      {banner && (
        <div className={`banner ${readonly ? "warn" : ""}`} data-testid="readonly-banner">
          {banner}
        </div>
      )}
      {!banner && heldByOther && lease && (
        <div className="banner warn" data-testid="lease-banner">
          {lease.label}이 조작 중이다(보기만 된다)
          {canControl && (
            <button type="button" onClick={take} data-testid="take">
              가져오기
            </button>
          )}
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
      <div className="term-scroll" hidden={chatView && chatable}>
        <div className="term-box" ref={box} data-testid="terminal" />
      </div>
      {dock}
      {canControl && (
        <form
          className="input-bar"
          onSubmit={(e) => {
            e.preventDefault();
            sendText(true);
          }}
        >
          <div className="composer">
            <button
              type="button"
              className="ghost icon"
              disabled={locked}
              onClick={() => fileRef.current?.click()}
              aria-label="그림"
              title="그림을 대상 머신에 올리고 입력창에 경로를 넣는다"
            >
              <Icon name="image" />
            </button>
            <textarea
              ref={textRef}
              rows={1}
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key !== "Enter") return;
                // 한글 등 조합 중의 Enter는 글자를 확정할 뿐이다
                if (e.nativeEvent.isComposing || e.keyCode === 229) return;
                // 터치 기기(화면 키보드)는 Enter가 줄바꿈이고 보내기는 단추로 한다. 데스크톱은 Shift+Enter가 줄바꿈이다.
                if (e.shiftKey || e.altKey || window.matchMedia("(hover: none)").matches) return;
                e.preventDefault();
                sendText(true);
              }}
              onPaste={(e) => {
                const f = imageOf(e.clipboardData.files);
                if (f) {
                  e.preventDefault();
                  void attachFile(f);
                }
              }}
              placeholder={
                locked
                  ? "입력할 수 없다"
                  : window.matchMedia("(hover: none)").matches
                    ? "글을 쓰고 보내기 단추로 보낸다"
                    : "글을 쓰고 Enter로 보낸다(Shift+Enter 줄바꿈)"
              }
              enterKeyHint="enter"
              disabled={locked}
              aria-label="보낼 글"
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
            />
            <button type="submit" className="primary send" disabled={locked} aria-label="보내기" title="보내기(Enter)">
              <Icon name="send" />
            </button>
          </div>
          <input
            ref={fileRef}
            type="file"
            accept="image/png,image/jpeg,image/gif,image/webp"
            hidden
            data-testid="attach-file"
            onChange={(e) => {
              const f = e.target.files?.[0];
              e.target.value = "";
              if (f) void attachFile(f);
            }}
          />
          <div className="keys">
            {(
              [
                ["Esc", "\x1b"],
                ["Tab", "\t"],
                ["Ctrl", "ctrl"],
                ["Alt", "alt"],
                ["^C", "\x03"],
                ["←", "\x1b[D"],
                ["↑", "\x1b[A"],
                ["↓", "\x1b[B"],
                ["→", "\x1b[C"],
                ["⏎", "\r"],
              ] as const
            ).map(([label, seq]) =>
              seq === "ctrl" || seq === "alt" ? (
                <button
                  type="button"
                  key={label}
                  className={`mod ${mods[seq] ? "on" : ""}`}
                  aria-pressed={mods[seq]}
                  disabled={locked}
                  onClick={() => setMods((m) => ({ ...m, [seq]: !m[seq] }))}
                >
                  {label}
                </button>
              ) : (
                <button type="button" key={label} disabled={locked} onClick={() => input(withMods(seq))}>
                  {label}
                </button>
              ),
            )}
            <span className="hint">터미널을 누르면 키보드로 바로 입력한다 · 그림은 붙여 넣거나 끌어 놓는다</span>
          </div>
        </form>
      )}
    </section>
  );
}
