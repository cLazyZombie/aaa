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
  refresh(): Promise<void> {
    if (this.closed) return Promise.resolve();
    this.dirty = true;
    this.invalidate();
    if (this.active) return this.active;
    this.active = this.run().finally(() => { this.active = null; if (this.dirty && !this.closed) void this.refresh(); });
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
