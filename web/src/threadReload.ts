// 질문 목록과 같은 경계: HTTP 목록을 읽는 동안 받은 thread 사건은 그 목록보다 새롭다.
import type { ThreadView } from "./gen/proto";
import { threadKey } from "./sidebarModel";
import { ThreadReplies } from "./threadReplies";

type Read = { updates: Map<string, ThreadView> };

export class ThreadReload {
  private latest: Read | null = null;
  private replies = new ThreadReplies();

  beginReply(t: ThreadView) { return this.replies.begin(t); }

  begin(): Read {
    const read = { updates: new Map<string, ThreadView>() };
    this.latest = read;
    return read;
  }

  note(t: ThreadView): void {
    this.replies.note(t);
    this.latest?.updates.set(threadKey(t), t);
  }

  finish(read: Read, snapshot: ThreadView[]): Map<string, ThreadView> | null {
    if (this.latest !== read) return null;
    this.latest = null;
    const next = new Map(snapshot.map((t) => [threadKey(t), t]));
    for (const [key, t] of read.updates) next.set(key, t);
    for (const t of next.values()) this.replies.note(t);
    return next;
  }

  failed(read: Read): void {
    if (this.latest === read) this.latest = null;
  }
}
