// 앱 안 참조 칩(plan §8.329, 사용자 결정 10-20 1A): 대화 글의 `aaa:question/<thread>`·`aaa:report/<이름>`을 누르면 앱 안에서
// 여는 칩으로 그린다. http 주소가 아니라 새 창 규칙(넓은 창은 새 탭)을 따르지 않고 늘 앱 안이다: 질문은 보고서 화면 「답할 것」의
// 그 카드를 펼치고(2A), 보고서는 보고서 화면의 그 보고서를 연다. 칩의 상태(답할 것·답함)는 App이 thread 기록으로 정한다.
import { createContext, useContext } from "react";
import { CircleCheck, CircleQuestionMark, FileText } from "lucide-react";
import type { AppRef } from "./appRef";
import { appIdHash, type AppId } from "./appIds";
import { browserClick } from "./reportLinks";

export type AppRefLook = {
  label: string;
  /** ask 답할 것, done 답함, report 보고서, plain 지난 질문·모르는 대상 */
  tone: "ask" | "done" | "report" | "plain";
  /** 이름 뒤의 짧은 상태(「답할 것」·「답함 · 1A 2B」) */
  state: string | null;
};

export type AppRefs = { exists: (id: AppId) => boolean; openId: (id: AppId) => void; look: (ref: AppRef) => AppRefLook; open: (ref: AppRef) => void };

/** 대화 글이 참조를 칩으로 그리는 곳(App). 없으면 글 그대로다 */
export const AppRefContext = createContext<AppRefs | null>(null);

export function AppRefChip({ at, refs }: { at: AppRef; refs?: AppRefs }) {
  const context = useContext(AppRefContext);
  const use = refs ?? context;
  if (use === null) return <>{`aaa:${at.kind}/${at.target}`}</>;
  const look = use.look(at);
  const Icon = at.kind === "report" ? FileText : look.tone === "done" ? CircleCheck : CircleQuestionMark;
  return (
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
}

/** 질문 ID는 명시 참조와 같은 상태 칩, 나머지는 보조 키·가운데 누름도 되는 실제 링크다. */
export function AppIdLink({ at, refs }: { at: AppId; refs: AppRefs }) {
  if (at.kind === "question") return <AppRefChip at={{ kind: "question", target: at.target }} refs={refs} />;
  return <a className="app-id" data-testid="app-id" data-kind={at.kind} data-target={at.target} href={appIdHash(at)}
    onClick={(e) => { if (!browserClick(e)) { e.preventDefault(); e.stopPropagation(); refs.openId(at); } }}>{at.label ?? at.target}</a>;
}
