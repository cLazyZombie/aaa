// 처음/다시 붙을 때의 질문 목록보다 그 요청 뒤 받은 사건이 우선한다.
// HTTP 응답은 events WebSocket보다 늦게 올 수 있어, 통째로 교체하면 열린 카드를 잃거나 닫힌 카드를 되살린다.
import type { QuestionView } from "./gen/proto";

type Read = { updates: Map<string, QuestionView> };

export class QuestionReload {
  private latest: Read | null = null;

  begin(): Read {
    const read = { updates: new Map<string, QuestionView>() };
    this.latest = read;
    return read;
  }

  note(q: QuestionView): void {
    this.latest?.updates.set(q.id, q);
  }

  finish(read: Read, snapshot: QuestionView[]): Map<string, QuestionView> | null {
    if (this.latest !== read) return null;
    this.latest = null;
    const next = new Map(snapshot.filter((q) => q.state === "open").map((q) => [q.id, q]));
    for (const q of read.updates.values()) {
      if (q.state === "open") next.set(q.id, q);
      else next.delete(q.id);
    }
    return next;
  }

  failed(read: Read): void {
    if (this.latest === read) this.latest = null;
  }
}
