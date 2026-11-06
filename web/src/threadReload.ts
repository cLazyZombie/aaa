// 질문 목록과 같은 경계: HTTP 목록을 읽는 동안 받은 thread 사건은 그 목록보다 새롭다.
import type { FrontRequest, FrontResponse, ThreadView } from "./gen/proto";
import { threadKey } from "./sidebarModel.ts";
import { ThreadReplies } from "./threadReplies.ts";

type Read = { sequence: number; updates: Map<string, ThreadView> };

export class ThreadReload {
  private sequence = 0;
  private pending = new Set<Read>();
  private replies = new ThreadReplies();

  beginReply(t: ThreadView) { return this.replies.begin(t); }

  begin(): Read {
    const read = { sequence: ++this.sequence, updates: new Map<string, ThreadView>() };
    this.pending.add(read);
    return read;
  }

  note(t: ThreadView): void {
    this.replies.note(t);
    for (const read of this.pending) read.updates.set(threadKey(t), t);
  }

  /** 열린 목록과 페이지를 먼저 공개해도 읽기 동안의 사건이 항상 이긴다. */
  partial(read: Read, snapshot: ThreadView[]): Map<string, ThreadView> | null {
    if (!this.pending.has(read)) return null;
    const next = new Map(snapshot.map(t => [threadKey(t), t]));
    for (const [key, t] of read.updates) next.set(key, t);
    // 부분 목록을 공개한 뒤의 늦은 답 응답도 이 상태로 되돌리지 않는다.
    for (const t of next.values()) this.replies.note(t);
    // 새 연결의 부분 응답도 옛 연결의 끝 응답보다 새롭다.
    for (const older of this.pending) if (older.sequence < read.sequence)
      for (const [key, t] of next) older.updates.set(key, t);
    return next;
  }

  finish(read: Read, snapshot: ThreadView[]): Map<string, ThreadView> | null {
    // 새 연결이 아직 덜 읽었으면 먼저 끝난 완전 목록을 보여 준다. 더 새 완전 목록 뒤에는 되돌아가지 않는다.
    if (!this.pending.delete(read)) return null;
    for (const older of this.pending) if (older.sequence < read.sequence) this.pending.delete(older);
    const next = new Map(snapshot.map((t) => [threadKey(t), t]));
    for (const [key, t] of read.updates) next.set(key, t);
    for (const t of next.values()) this.replies.note(t);
    return next;
  }

  failed(read: Read): void {
    this.pending.delete(read);
  }
}

// 모든 페이지가 모일 때까지 ThreadReload의 사건 덮어쓰기를 유지한다.
export async function readThreadPages(
  call: (request: FrontRequest) => Promise<FrontResponse>,
  options: { open?: boolean; page?: (threads: ThreadView[]) => void } = {},
): Promise<ThreadView[]> {
  const threads: ThreadView[] = [];
  const seen = new Set<string>();
  let after: string | undefined;
  for (;;) {
    const r = await call({ op: "threads", project: null, role: null, details: true, after, ...(options.open ? { open: true } : {}) });
    if (r.kind !== "threads") throw new Error("thread 목록 응답이 아니다");
    threads.push(...r.threads);
    options.page?.(threads);
    if (!r.next_after) return threads;
    if (seen.has(r.next_after)) throw new Error("thread 목록 페이지 커서가 나아가지 않았다");
    seen.add(r.next_after);
    after = r.next_after;
  }
}

/** 현재 주소·복원 항목은 과거 목록의 차례를 기다리지 않는다. */
export async function readSelectedThread(call: (request: FrontRequest) => Promise<FrontResponse>, selection: string | null): Promise<ThreadView | null> {
  if (!selection?.startsWith("thread:")) return null;
  const key = selection.slice(7), slash = key.indexOf("/");
  const r = await call({ op: "thread_show", project: slash < 0 ? "" : key.slice(0, slash), thread: slash < 0 ? key : key.slice(slash + 1) });
  return r.kind === "thread" ? r : null;
}
