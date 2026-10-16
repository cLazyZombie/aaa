// pane 하나의 터미널 보기. 크기는 PTY(cols·rows)를 따르고, 「이 화면에 맞추기」만 PTY 크기를 바꾼다.
// WebSocket 하나가 대화형 front 하나다. 다른 front가 조작(lease) 중이면 입력을 막고 「가져오기」를 보인다.
// 그림을 붙여 넣거나(끌어 놓기·「그림」 버튼) 하면 대상 머신 세션 폴더에 저장하고, harness 입력창이면 경로를 넣는다.
import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { DeviceRole, LeaseView, PaneView, TermClientMsg, TermServerMsg } from "./gen/proto";
import { b64decode, b64encode, wsUrl } from "./api";

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

export function TermView({ pane, role, onBack }: { pane: PaneView; role: DeviceRole; onBack: () => void }) {
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
  const [text, setText] = useState("");
  const [me, setMe] = useState<string | null>(null);
  const [lease, setLease] = useState<LeaseView | null>(pane.lease);
  const [attach, setAttach] = useState<{ text: string; ok: boolean } | null>(null);
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

  // 커서 위치 보고(ESC[r;cR)는 session host가 이미 답했다. xterm.js의 자동 답은 보내지 않는다.
  const fromTerminal = (data: string) => {
    if (/^\x1b\[\d+;\d+R$/.test(data)) return;
    input(data);
  };

  useEffect(() => {
    const narrow = window.matchMedia("(max-width: 700px)").matches;
    const term = new Terminal({
      cols: pane.cols,
      rows: pane.rows,
      fontSize: narrow ? 11 : 13,
      fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
      scrollback: 2000,
      convertEol: false,
      theme: { background: "#0d1014" },
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
  const sendText = (enter: boolean) => {
    if (text) input(text);
    if (enter) {
      if (text) window.setTimeout(() => input("\r"), ENTER_GAP_MS);
      else input("\r");
    }
    setText("");
    termRef.current?.focus();
  };

  let banner: string | null = null;
  if (link !== "open") banner = link === "closed" ? "host와 연결이 끊겼다. 다시 연결하는 중(읽기 전용)" : "연결하는 중";
  else if (readonly) banner = reason ?? (pane.connection === "disconnected" ? "agent와 연결이 끊겼다(읽기 전용)" : "읽기 전용");

  return (
    <section className="term">
      <header className="term-head">
        <button className="back" onClick={onBack} aria-label="목록으로">
          ←
        </button>
        <div className="term-title">
          <strong>{pane.name || pane.id}</strong>
          <span className="muted">
            {pane.machine} · {size[0]}×{size[1]}
          </span>
        </div>
        {canControl && (
          <button onClick={fitToScreen} disabled={readonly || heldByOther} title="PTY 크기를 이 화면에 맞춘다">
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
          <button type="button" onClick={() => setAttach(null)} aria-label="닫기">
            ×
          </button>
        </div>
      )}
      <div className="term-scroll">
        <div className="term-box" ref={box} data-testid="terminal" />
      </div>
      {canControl && (
        <form
          className="input-bar"
          onSubmit={(e) => {
            e.preventDefault();
            sendText(true);
          }}
        >
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            onPaste={(e) => {
              const f = imageOf(e.clipboardData.files);
              if (f) {
                e.preventDefault();
                void attachFile(f);
              }
            }}
            placeholder={readonly || heldByOther ? "입력할 수 없다" : "입력(Enter로 보냄)"}
            disabled={readonly || heldByOther}
            aria-label="보낼 글"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
          />
          <button type="submit" disabled={readonly || heldByOther}>
            보내기
          </button>
          <button
            type="button"
            disabled={readonly || heldByOther}
            onClick={() => fileRef.current?.click()}
            title="그림을 대상 머신에 올리고 입력창에 경로를 넣는다"
          >
            그림
          </button>
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
                ["^C", "\x03"],
                ["↑", "\x1b[A"],
                ["↓", "\x1b[B"],
                ["⏎", "\r"],
              ] as const
            ).map(([label, seq]) => (
              <button type="button" key={label} disabled={readonly || heldByOther} onClick={() => input(seq)}>
                {label}
              </button>
            ))}
          </div>
        </form>
      )}
    </section>
  );
}
