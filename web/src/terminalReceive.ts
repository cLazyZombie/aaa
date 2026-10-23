import { logIgnoredFailure } from "./errorDiagnostic.ts";
import type { TermServerMsg } from "./gen/proto";
import { decodeBase64, rawLength, WIRE_MAX } from "./terminalInflate.ts";

export type ReceivedTerminal = { message: TermServerMsg; payload?: Uint8Array<ArrayBuffer> } | { binary: Uint8Array<ArrayBuffer> };
type Pending = { wire: string | ArrayBuffer; message?: TermServerMsg; bytes: number };

/** 연결마다 한 큐·한 Worker. 입력 송신은 이 큐와 무관하다. close는 대기 결과를 모두 버린다. */
export class TerminalReceiver {
  readonly ready: Promise<boolean>;
  private worker: Worker | null = null;
  private queue: Pending[] = [];
  private pendingBytes = 0;
  private running = false;
  private closed = false;
  private gzip = false;
  private sequence = 0;
  private settleReady: ((supported: boolean) => void) | null = null;
  private result: { id: number; resolve: (bytes: Uint8Array<ArrayBuffer>) => void; reject: () => void } | null = null;
  private consume: (item: ReceivedTerminal) => void;
  private failed: () => void;

  constructor(consume: (item: ReceivedTerminal) => void, failed: () => void,
    makeWorker = () => new Worker(new URL("./terminalInflate.worker.ts", import.meta.url), { type: "module" }), verifiedGzip = false) {
    this.consume = consume;
    this.failed = failed;
    this.gzip = verifiedGzip;
    this.ready = new Promise((resolve) => { this.settleReady = resolve; });
    try {
      const worker = makeWorker();
      this.worker = worker;
      worker.onmessage = (event) => {
        if (this.closed) return;
        const reply = event.data;
        if (typeof reply.ready === "boolean" && this.settleReady) {
          this.gzip = reply.ready;
          this.settleReady(reply.ready);
          this.settleReady = null;
          if (!reply.ready) { worker.terminate(); this.worker = null; }
          return;
        }
        const result = this.result;
        if (!result || reply.id !== result.id) { this.fail(); return; }
        this.result = null;
        if (reply.error || !(reply.bytes instanceof Uint8Array)) result.reject();
        else result.resolve(reply.bytes);
      };
      worker.onerror = () => {
        if (this.settleReady) {
          this.settleReady(false);
          this.settleReady = null;
          worker.terminate(); this.worker = null;
        } else this.fail();
      };
    } catch (ignoredError) { logIgnoredFailure(ignoredError, "terminalReceive.ts:55");
      this.settleReady?.(false); this.settleReady = null;
    }
  }

  push(wire: string | ArrayBuffer): void {
    if (this.closed) return;
    try {
      let message: TermServerMsg | undefined;
      let bytes: number;
      let wireBytes: number;
      if (typeof wire === "string") {
        if (wire.length > WIRE_MAX) throw new Error();
        wireBytes = new TextEncoder().encode(wire).length;
        if (wireBytes > WIRE_MAX) throw new Error();
        message = JSON.parse(wire) as TermServerMsg;
        bytes = wire.length;
        if (message.t === "snapshot" || message.t === "history") {
          const encoded = message.t === "snapshot" ? message.replay : message.data;
          if (message.compression !== undefined) {
            if (!this.gzip) throw new Error();
            bytes = rawLength(message.compression);
            if (typeof encoded !== "string" || encoded.length > WIRE_MAX) throw new Error();
          } else {
            bytes = decodeBase64(encoded).length;
          }
          // 압축 전 JSON도 기존 1 MiB 한도에 들어야 한다.
          const metadataBytes = message.compression === undefined ? 0 : JSON.stringify({ compression: message.compression }).length - 1;
          if (wire.length - encoded.length - metadataBytes + 4 * Math.ceil(bytes / 3) > WIRE_MAX) throw new Error();
        }
      } else {
        bytes = wire.byteLength;
        wireBytes = bytes;
        if (bytes > WIRE_MAX) throw new Error();
      }
      // raw_len이 작게 위조되어도 큰 압축/base64 문자열을 줄에 쌓을 수 없다.
      const budget = Math.max(bytes, wireBytes, 1);
      if (this.pendingBytes + budget > WIRE_MAX) throw new Error();
      this.pendingBytes += budget;
      this.queue.push({ wire, message, bytes: budget });
      void this.drain();
    } catch (ignoredError) { logIgnoredFailure(ignoredError, "terminalReceive.ts:96"); this.fail(); }
  }

  private async drain(): Promise<void> {
    if (this.running) return;
    this.running = true;
    try {
      while (!this.closed && this.queue.length) {
        const item = this.queue.shift()!;
        if (!item.message) {
          this.consume({ binary: new Uint8Array(item.wire as ArrayBuffer) });
        } else {
          const message = item.message;
          let payload: Uint8Array<ArrayBuffer> | undefined;
          if (message.t === "snapshot" || message.t === "history") {
            const data = message.t === "snapshot" ? message.replay : message.data;
            if (message.compression === undefined) payload = decodeBase64(data);
            else {
              // 재연결은 앞 Worker에서 확인한 지원으로 WS를 바로 연다. 새 Worker에는 준비 사건 뒤에만 보낸다.
              if ((this.settleReady && !await this.ready) || this.closed || !this.gzip) throw new Error();
              const rawLen = rawLength(message.compression);
              payload = await new Promise<Uint8Array<ArrayBuffer>>((resolve, reject) => {
                const id = ++this.sequence;
                this.result = { id, resolve, reject: () => reject(new Error("터미널 해제 실패")) };
                this.worker!.postMessage({ id, data, rawLen });
              });
              if (payload.length !== rawLen) throw new Error();
            }
          }
          if (!this.closed) this.consume({ message, payload });
        }
        if (!this.closed) this.pendingBytes -= item.bytes;
      }
    } catch (ignoredError) { logIgnoredFailure(ignoredError, "terminalReceive.ts:129"); if (!this.closed) this.fail(); }
    finally { this.running = false; }
  }

  private fail(): void {
    if (this.closed) return;
    this.close();
    this.failed();
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.worker?.terminate(); this.worker = null;
    this.queue = []; this.pendingBytes = 0;
    this.result?.reject(); this.result = null;
    this.settleReady?.(false); this.settleReady = null;
  }
}
