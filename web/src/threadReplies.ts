// 명령의 HTTP 응답을 기다리며 받은 같은 thread의 상태를 보존한다.
// 답을 보낸 뒤 새 보고를 읽었으면 늦은 이전 답 응답으로 그 카드를 되돌리지 않는다.
// 서버 시각 비교 대신 기존 목록 갱신(ThreadReload)과 같은 사건 우선 규칙을 쓴다.
import type { ThreadView } from "./gen/proto";

type Pending = { project: string; id: string; update: ThreadView | null };

export class ThreadReplies {
  private pending = new Set<Pending>();

  note(t: ThreadView): void {
    for (const p of this.pending) {
      if (p.project === t.project && p.id === t.id) p.update = t;
    }
  }

  begin(t: Pick<ThreadView, "project" | "id">) {
    const p: Pending = { project: t.project, id: t.id, update: null };
    this.pending.add(p);
    return {
      finish: (response: ThreadView): ThreadView => {
        this.pending.delete(p);
        return p.update ?? response;
      },
      cancel: () => { this.pending.delete(p); },
    };
  }
}
