// events 연결의 첫 asks도 일반 변경 사건이다. 연결 열림에서 따로 읽지 않는다.
// 읽는 동안의 변경은 한 번 더 읽고, 변경 전 응답은 적용하지 않는다.
export class AsksReload<T> {
  private revision = 0;
  private active: Promise<void> | null = null;
  private dirty = false;
  private closed = false;
  private readonly read: () => Promise<T>;
  private readonly apply: (value: T) => void;
  private readonly failed: (error: unknown) => void;
  constructor(read: () => Promise<T>, apply: (value: T) => void, failed: (error: unknown) => void) {
    this.read = read; this.apply = apply; this.failed = failed;
  }

  invalidate(): void { ++this.revision; }
  close(): void { this.closed = true; this.invalidate(); }
  /** 바뀜 사건: 지금 읽는 것은 바뀌기 전 것일 수 있어 적용하지 않고 다시 읽는다. */
  refresh(): Promise<void> {
    this.invalidate();
    return this.follow();
  }
  /**
   * 조작 응답 뒤(t-1767): 지금 읽는 것은 그대로 적용하고 끝나면 한 번 더 읽는다. host는 조작을 마치고 `asks` 사건을 먼저 보내므로
   * 그 사건이 시작한 읽기는 이미 조작 뒤의 상태다. 그 읽기를 버리면 같은 목록을 한 번 더 기다린다. 사건 연결이 끊겨 지금 읽는 것이
   * 조작보다 앞선 것이어도 뒤 읽기가 바로잡는다. 읽는 것이 없으면 지금 읽는다.
   */
  follow(): Promise<void> {
    if (this.closed) return Promise.resolve();
    this.dirty = true;
    if (this.active) return this.active;
    this.active = this.run().finally(() => { this.active = null; if (this.dirty && !this.closed) void this.follow(); });
    return this.active;
  }
  private async run(): Promise<void> {
    while (this.dirty && !this.closed) {
      this.dirty = false;
      const revision = this.revision;
      try {
        const value = await this.read();
        if (!this.closed && revision === this.revision) this.apply(value);
      } catch (error) { if (!this.closed) this.failed(error); }
    }
  }
}
