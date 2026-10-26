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
export async function readThreadPages(call: (request: FrontRequest) => Promise<FrontResponse>): Promise<ThreadView[]> {
  const threads: ThreadView[] = [];
  const seen = new Set<string>();
  let after: string | undefined;
  for (;;) {
    const r = await call({ op: "threads", project: null, role: null, details: true, after });
    if (r.kind !== "threads") throw new Error("thread 목록 응답이 아니다");
    threads.push(...r.threads);
    if (!r.next_after) return threads;
    if (seen.has(r.next_after)) throw new Error("thread 목록 페이지 커서가 나아가지 않았다");
    seen.add(r.next_after);
    after = r.next_after;
  }
}
