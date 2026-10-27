// herdr-web-ui aa624cb `src/lib/conversationRefresh.ts`: 최신/이전 쪽 요청의 lane.
// ES2022에서는 Promise.withResolvers 대신 Promise 생성자를 쓴다.

/** With pushes, a lost one still gets a full conversation read. */
const BACKSTOP_MS = 10_000;

/** Pushes never start newest-page reads faster than the former 2s polling cadence, and a chat
 * without them (an older bridge, a tab that let go of its pane) reads at that cadence. */
const INVALIDATION_MS = 2000;

/** One serial lane for newest pages and their gap fills; an older page starts in it, after the
 * newest read before it, but never holds the next newest read back. */
export class ConversationRefresh {
  private read: (() => Promise<void>) | null = null;
  private pending = false;
  private invalidated = false;
  private running = false;
  private timer: number | undefined;
  private newestAt = -Infinity;
  private pushes = false;
  private source: string | undefined;
  private signature: string | undefined;
  private readonly pages: (() => Promise<void>)[] = [];

  /** Replace a cancelled reader without letting its still-pending REST request overlap this one. */
  setRead(read: () => Promise<void>, source?: string): void {
    if (this.source !== source) this.signature = undefined;
    this.source = source;
    this.clearTimer();
    this.pending = false;
    this.invalidated = false;
    this.newestAt = -Infinity;
    this.read = read;
  }

  /** Whether the bridge pushes this pane's transcript changes; without, the backstop is the poll. */
  setPushes(available: boolean): void {
    if (this.pushes === available) return;
    this.pushes = available;
    // a waiting backstop takes the new cadence; a read in flight schedules it when it ends
    if (this.read !== null && !this.running && !this.invalidated && this.timer !== undefined) {
      this.clearTimer();
      this.timer = window.setTimeout(() => this.refresh(), this.backstop());
    }
  }

  /** A newest-page read is in flight: one requested now would start only after it settles. */
  get reading(): boolean {
    return this.running;
  }

  refresh(): void {
    if (this.read === null) return;
    this.clearTimer();
    this.pending = true;
    this.invalidated = false;
    void this.drain();
  }

  /** The first push after idle reads immediately; further pushes share a fixed trailing deadline. */
  invalidate(signature?: string): void {
    if (this.read === null) return;
    // 재구독은 현재 signature를 다시 보낸다. 같은 pane의 복귀 read에 같은 변경을 덧예약하지 않는다.
    if (signature !== undefined) {
      if (this.signature === signature) return;
      this.signature = signature;
    }
    if (this.invalidated) return;
    this.invalidated = true;
    this.clearTimer();
    void this.drain();
  }

  /** 최신 읽기 뒤에 이전 쪽을 시작한다. 다음 최신 읽기는 이전 쪽 전송을 기다리지 않는다. */
  page<T>(read: () => Promise<T>): Promise<T> {
    this.clearTimer();
    return new Promise<T>((resolve, reject) => {
      this.pages.push(async () => { try { resolve(await read()); } catch (error) { reject(error); } });
      void this.drain();
    });
  }

  /** Hidden/unmounted readers leave no timer or pending refresh; an in-flight read finishes first. */
  stop(): void {
    this.read = null;
    this.pending = false;
    this.invalidated = false;
    this.clearTimer();
  }

  private backstop(): number {
    return this.pushes ? BACKSTOP_MS : INVALIDATION_MS;
  }

  private clearTimer(): void {
    window.clearTimeout(this.timer);
    this.timer = undefined;
  }

  private async drain(): Promise<void> {
    if (this.running) return;
    this.running = true;
    try {
      while (true) {
        const page = this.pages.shift();
        if (page !== undefined) {
          void page();
          continue;
        }
        if (this.read === null || (!this.pending && !this.invalidated)) break;
        if (!this.pending && performance.now() < this.newestAt + INVALIDATION_MS) break;
        this.pending = false;
        this.invalidated = false;
        // Monotonic elapsed time keeps wall-clock corrections out of the rate bound.
        this.newestAt = performance.now();
        await this.read();
      }
    } finally {
      this.running = false;
      if (this.read !== null) {
        this.timer = this.invalidated
          ? window.setTimeout(() => {
            this.timer = undefined;
            void this.drain();
          }, Math.max(0, this.newestAt + INVALIDATION_MS - performance.now()))
          : window.setTimeout(() => this.refresh(), this.backstop());
      }
    }
  }
}
