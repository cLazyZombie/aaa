// 앱 안 참조 칩(plan §8.329, 사용자 결정 10-20 1A): 대화 글의 `aaa:question/<thread>`·`aaa:report/<이름>`을 누르면 앱 안에서
// 여는 칩으로 그린다. http 주소가 아니라 새 창 규칙(넓은 창은 새 탭)을 따르지 않고 늘 앱 안이다: 질문은 보고서 화면 「답할 것」의
// 그 카드를 펼치고(2A), 보고서는 보고서 화면의 그 보고서를 연다. 칩의 상태(답할 것·답함)는 App이 thread 기록으로 정한다.
// 열린 질문 하나를 가리키면 칩 뒤에 선택지 단추를 붙인다: 누르면 카드 답과 같은 host 경로로 답하고 칩은 「답함」이 된다(t-1287).
import { createContext, useContext, useState, type ReactNode } from "react";
import { CircleCheck, CircleQuestionMark, FileText, LoaderCircle } from "lucide-react";
import type { AppRef } from "./appRef";
import { appIdHash, type AppId } from "./appIds";
import { browserClick } from "./reportLinks";

export type AppRefLook = {
  label: string;
  /** ask 답할 것, done 답함, report 보고서, plain 지난 질문·모르는 대상 */
  tone: "ask" | "done" | "report" | "plain";
  /** 이름 뒤의 짧은 상태(「답할 것」·「답함 · 1A 2B」) */
  state: string | null;
  /** 칩에서 바로 답할 열린 질문 하나. 보기 권한·여러 질문 카드는 없다 */
  ask?: ChipAsk;
};

export type ChipAsk = {
  id: string;
  project?: string;
  text: string;
  /** card: 글이 필요한 선택지(검수 B)라 누르면 카드를 연다 */
  choices: { letter: string; label: string; recommended: boolean; card: boolean }[];
};

export type AppRefs = {
  exists: (id: AppId) => boolean; openId: (id: AppId) => void; look: (ref: AppRef) => AppRefLook; open: (ref: AppRef) => void;
  answer?: (ask: ChipAsk, letter: string) => Promise<void>;
};

/** 대화 글이 참조를 칩으로 그리는 곳(App). 없으면 글 그대로다 */
export const AppRefContext = createContext<AppRefs | null>(null);

/** choices: 열린 질문이면 선택지 단추를 붙인다. 문장 속 맨 ID는 끈다(링크·`aaa:` 참조만 단추) */
export function AppRefChip({ at, refs, choices = true }: { at: AppRef; refs?: AppRefs; choices?: boolean }) {
  const context = useContext(AppRefContext);
  const use = refs ?? context;
  if (use === null) return <>{`aaa:${at.kind}/${at.target}`}</>;
  const look = use.look(at);
  const Icon = at.kind === "report" ? FileText : look.tone === "done" ? CircleCheck : CircleQuestionMark;
  const chip = (
    <button
      type="button"
      className={`app-ref is-${look.tone}`}
      data-testid="app-ref"
      data-kind={at.kind}
      data-target={at.target}
      data-tone={look.tone}
      title={`aaa:${at.kind}/${at.target}`}
      onClick={(e) => {
        e.stopPropagation();
        use.open(at);
      }}
    >
      <Icon aria-hidden="true" />
      <span className="app-ref-label">{look.label}</span>
      {look.state && <span className="app-ref-state">{look.state}</span>}
    </button>
  );
  const answer = use.answer;
  if (!choices || !look.ask || !answer) return chip;
  return <ChipChoices at={at} ask={look.ask} chip={chip} answer={answer} open={() => use.open(at)} />;
}

function ChipChoices({ at, ask, chip, answer, open }: { at: AppRef; ask: ChipAsk; chip: ReactNode; answer: NonNullable<AppRefs["answer"]>; open: () => void }) {
  // 보낸 뒤에는 host 사건으로 칩이 「답함」이 될 때까지 단추를 다시 열지 않는다(두 번 답하지 않게).
  const [sending, setSending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pick = async (letter: string) => {
    setSending(letter);
    setError(null);
    try {
      await answer(ask, letter);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSending(null);
    }
  };
  return (
    <span className="app-ref-ask" data-testid="app-ref-ask" data-target={at.target} aria-busy={sending !== null}>
      {chip}
      <span className="app-ref-text">{ask.text}</span>
      <span className="app-ref-picks" role="group" aria-label={`${ask.id} ${ask.text}`}>
        {ask.choices.map((c) => (
          <button key={c.letter} type="button" className={`app-ref-pick${c.recommended ? " is-recommended" : ""}${sending === c.letter ? " is-sending" : ""}`}
            data-testid="app-ref-pick" data-question={ask.id} data-letter={c.letter} title={`${c.letter} ${c.label}`} disabled={sending !== null}
            onClick={(e) => { e.stopPropagation(); if (c.card) open(); else void pick(c.letter); }}>
            {sending === c.letter ? <LoaderCircle className="app-ref-spin" aria-hidden="true" /> : <span className="app-ref-pick-letter">{c.letter}</span>}
            <span className="app-ref-pick-label">{c.label}</span>
            {c.recommended && <span className="app-ref-pick-tag">추천</span>}
          </button>
        ))}
      </span>
      {error !== null && <span className="app-ref-error" role="alert">{error}</span>}
    </span>
  );
}

/** 질문 ID는 명시 참조와 같은 상태 칩, 나머지는 보조 키·가운데 누름도 되는 실제 링크다. */
export function AppIdLink({ at, refs }: { at: AppId; refs: AppRefs }) {
  if (at.kind === "question") return <AppRefChip at={{ kind: "question", target: at.target }} refs={refs} choices={false} />;
  return <a className="app-id" data-testid="app-id" data-kind={at.kind} data-target={at.target} href={appIdHash(at)}
    onClick={(e) => { if (!browserClick(e)) { e.preventDefault(); e.stopPropagation(); refs.openId(at); } }}>{at.label ?? at.target}</a>;
}
