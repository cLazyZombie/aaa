// 화면 사이에 넘기는 인용(t-1222). thread 화면의 의견 모음은 그 thread pane 화면을 열고 그 화면이 보이는 입력 초안에 붙이게
// 맡긴다(붙일 곳·넘침·focus는 t-1204 인용과 같이 pane 화면이 정한다). 보낸 말의 「돌아가기」는 보고·diff 화면을 열고 그 화면이
// 그린 뒤 자리를 고르게 맡긴다. 둘 다 이 탭 안에서만 산다(새로고침하면 없다).
import type { DiffScope } from "./gen/proto";
import type { DiffQuoteRange } from "./selectionQuote.ts";
import type { TextAnchor } from "./textAnchor.ts";

type Listener = () => void;
const listeners = new Set<Listener>();
const notify = () => { for (const listener of listeners) listener(); };
export function subscribeHandoff(listener: Listener): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

// ── pane 입력 초안에 붙일 글 ──

interface Draft { owner: string; block: string; done: (ok: boolean) => void }
let drafts: Draft[] = [];

/** `owner`(머신:pane) 화면이 보이면 붙인다. done(true)면 붙였다, false면 붙이지 못했다(넘침·입력 없음) */
export function handDraft(owner: string, block: string, done: (ok: boolean) => void): void {
  drafts = [...drafts.filter((d) => d.owner !== owner), { owner, block, done }];
  notify();
}

export function takeDraft(owner: string): Draft | null {
  const found = drafts.find((d) => d.owner === owner) ?? null;
  if (found) drafts = drafts.filter((d) => d !== found);
  return found;
}

// ── 인용 위치로 돌아가기 ──

export type QuoteReveal =
  | { kind: "report"; project: string; report: string; anchor: Pick<TextAnchor, "text" | "start" | "end" | "prefix" | "suffix"> }
  | { kind: "diff"; project: string; thread: string; scope: DiffScope; path: string; range: DiffQuoteRange; text: string };

let reveal: { target: QuoteReveal; seq: number } | null = null;
let seq = 0;

export function requestReveal(target: QuoteReveal): void {
  reveal = { target, seq: ++seq };
  notify();
}

/** 이 화면이 맡을 자리면 꺼낸다 */
export function takeReveal(fits: (target: QuoteReveal) => boolean): QuoteReveal | null {
  if (reveal === null || !fits(reveal.target)) return null;
  const { target } = reveal;
  reveal = null;
  return target;
}
