import type { ThreadQuestions } from "./gen/proto";
import { logIgnoredFailure } from "./errorDiagnostic.ts";

export type QuestionDraft = { picks: (number | null)[]; custom: string; comments: (string | null)[] };
type DraftStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;
type SavedDraft = { identity: string; revision: string; server: QuestionDraft; value: QuestionDraft };
const PREFIX = "aaa:question-draft:";

/** host가 유지한 q-ID와 질문·선택지·검수 규칙이 같을 때만 초안을 잇는다. 자료 교체는 새 q-ID로 구별한다. */
export function questionIdentity(q: ThreadQuestions): string {
  return JSON.stringify([q.review ?? false, q.items.map((item, n) => [
    item.id ?? null, item.source_number ?? n + 1, item.text, item.body ?? "", item.after ?? "",
    item.choices.map((c, i) => [c.letter ?? String.fromCharCode(65 + i), c.label, c.description ?? "", c.recommended, c.body ?? ""]),
  ])]);
}

/** 새로 확인한 host 일부 답만 덮어쓴다. 이미 받은 일부 답을 사용자가 다시 고친 초안도 보존한다. */
export function mergeQuestionDraft(q: ThreadQuestions, local?: QuestionDraft, before?: QuestionDraft): QuestionDraft {
  const multiple = q.items.length > 1;
  const server: QuestionDraft = {
    picks: q.items.map((_, n) => q.draft?.picks[n] ?? null),
    custom: q.draft?.text ?? (!multiple ? q.draft?.comments?.[0] : null) ?? "",
    comments: q.items.map((_, n) => q.draft?.comments?.[n] ?? null),
  };
  if (!local) return server;
  return {
    picks: server.picks.map((p, n) => p !== null && p !== before?.picks[n] ? p : local.picks[n] ?? null),
    custom: server.custom && server.custom !== before?.custom ? server.custom : local.custom,
    comments: server.comments.map((c, n) => c !== null && c !== before?.comments[n] ? c : local.comments[n] ?? null),
  };
}

function validDraft(v: QuestionDraft | undefined): v is QuestionDraft {
  return !!v && typeof v.custom === "string" && Array.isArray(v.picks) && Array.isArray(v.comments)
    && v.picks.every(p => p === null || Number.isInteger(p) && p >= 0)
    && v.comments.every(c => c === null || typeof c === "string");
}

export class QuestionDraftStore {
  private memory = new Map<string, string | null>();
  private unsaved = new Set<string>();
  private storage: () => DraftStorage;
  constructor(storage: () => DraftStorage = () => window.localStorage) { this.storage = storage; }

  private raw(owner: string): string | null {
    if (this.unsaved.has(owner)) return this.memory.get(owner) ?? null;
    try {
      const raw = this.storage().getItem(PREFIX + owner);
      this.memory.set(owner, raw);
      return raw;
    }
    catch (error) { logIgnoredFailure(error, "질문 초안 읽기"); return this.memory.get(owner) ?? null; }
  }

  private saved(owner: string): SavedDraft | null {
    const raw = this.raw(owner);
    if (raw === null) return null;
    try {
      const s = JSON.parse(raw) as SavedDraft;
      if (typeof s?.identity !== "string" || typeof s.revision !== "string" || !validDraft(s.value) || !validDraft(s.server)) return null;
      return s;
    } catch (error) { logIgnoredFailure(error, "질문 초안 해석"); return null; }
  }

  read(owner: string, q: ThreadQuestions): QuestionDraft {
    const s = this.saved(owner);
    const local = s?.identity === questionIdentity(q) ? s.value : undefined;
    // 손상된 저장값의 범위 밖 선택은 보내지 않는다.
    const draft = mergeQuestionDraft(q, local, s?.server);
    draft.picks = draft.picks.map((p, n) => p !== null && p < q.items[n]!.choices.length ? p : null);
    return draft;
  }

  write(owner: string, q: ThreadQuestions, value: QuestionDraft): void {
    const raw = JSON.stringify({ identity: questionIdentity(q), revision: q.report_hash, server: mergeQuestionDraft(q), value } satisfies SavedDraft);
    this.memory.set(owner, raw);
    try { this.storage().setItem(PREFIX + owner, raw); this.unsaved.delete(owner); }
    catch (error) { this.unsaved.add(owner); logIgnoredFailure(error, "질문 초안 저장"); }
  }

  snapshot(owner: string): string | null { return this.raw(owner); }

  /** 성공한 요청이 실제로 보낸 초안만 지운다. 늦은 응답이 새 판·새 입력을 지우지 않는다. */
  settle(owner: string, sent: string | null): void {
    if (sent !== null && this.raw(owner) === sent) this.remove(owner);
  }

  private remove(owner: string): void {
    this.memory.set(owner, null);
    try { this.storage().removeItem(PREFIX + owner); this.unsaved.delete(owner); }
    catch (error) { this.unsaved.add(owner); logIgnoredFailure(error, "질문 초안 지우기"); }
  }
}

export const questionDrafts = new QuestionDraftStore();
