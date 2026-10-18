// 화면 선택 창 카드(차이 목록 2장 Q2, plan §8.164). herdr-web-ui `src/components/PromptCard.tsx`(aa624cb)의 그리기를 옮겼다:
// 선택지 번호 keycap(보이기만, 키는 터미널 그대로), 선택지 설명, 「(Recommended)」는 떼고 「추천」 표시, 여러 개 고르기는 체크와
// 「보내기 (n)」, 직접 쓰기 칸(IME의 Enter는 조합 확정). 문구는 참고 i18n.ko.ts.
// aaa가 다른 곳: 답은 host가 화면을 다시 읽어 같은 창일 때만 보낸다(참고와 같다). 바뀌었으면 host가 질문의 창을 새로 붙이고
// 카드는 그 창(`menu.id`)으로 새로 그린다. 입력창에 번호·이름을 쳐서 답하기(참고 `answerFromText`)·계획 파일 보기·여러 질문
// form(omo) 단계는 옮기지 않았다.
import { useEffect, useId, useRef, useState } from "react";
import { Send } from "lucide-react";

import { ApiError } from "./api";
import type { MenuPick, QuestionMenu } from "./gen/proto";
import "./herdr/PromptCard.css";

/** Claude는 메뉴 스스로 추천을 「Redis (Recommended)」로 그린다: 꼬리말보다 표시가 읽기 좋다(참고 labelOf) */
export function labelOf(label: string): { text: string; recommended: boolean } {
  const text = label.replace(/\s*\(recommended\)$/i, "");
  return { text, recommended: text !== label };
}

export function PromptCard({
  menu,
  where,
  canControl,
  onAnswer,
  testid,
}: {
  menu: QuestionMenu;
  where: string;
  canControl: boolean;
  onAnswer: (pick: MenuPick) => Promise<void>;
  testid: string;
}) {
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [custom, setCustom] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 카드가 사라진 뒤(다른 창으로 바뀜·pane을 떠남) 돌아온 답은 아무것도 바꾸지 않는다
  const shown = useRef(false);
  useEffect(() => {
    shown.current = true;
    return () => {
      shown.current = false;
    };
  }, []);
  const customLabelId = useId();

  const answer = async (pick: MenuPick) => {
    setPending(true);
    setError(null);
    try {
      await onAnswer(pick);
    } catch (e) {
      if (shown.current) setError(e instanceof ApiError ? e.message : String(e));
    }
    if (shown.current) setPending(false);
  };
  const toggle = (index: number) =>
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  const off = pending || !canControl;
  const hasChoices = menu.options.some((_, index) => index !== menu.custom_option_index);
  const text = custom.trim();

  return (
    <section className="prompt-card" role="region" aria-label="에이전트의 질문" aria-busy={pending} data-testid={testid} data-menu={menu.id}>
      <header className="prompt-card-header">
        <span className="visually-hidden">입력 필요</span>
        <h2>{menu.title}</h2>
        <span className="prompt-card-where">{where}</span>
      </header>
      {/* Claude 승인은 머리가 곧 질문이다: 한 번만 */}
      {menu.question !== menu.title && <p className="prompt-card-question">{menu.question}</p>}
      {menu.body !== null && menu.body.length > 0 && <pre className={`prompt-card-body${menu.body.includes("\n") ? "" : " is-line"}`}>{menu.body}</pre>}
      {hasChoices && (
        <div className="prompt-card-options" role={menu.multi_select ? "group" : undefined} aria-label={menu.multi_select ? menu.question : undefined}>
          {menu.options.map((option, index) => {
            if (index === menu.custom_option_index) return null;
            const { text: label, recommended } = labelOf(option.label);
            const content = (
              <span className="prompt-card-option-text">
                <span className="prompt-card-option-label">
                  {label}
                  {recommended && (
                    <>
                      {" "}
                      <span className="prompt-card-tag">추천</span>
                    </>
                  )}
                </span>
                {option.description !== null && <span className="prompt-card-option-description">{option.description}</span>}
              </span>
            );
            // 터미널에서 칠 수 있는 번호를 keycap으로. 선택지 이름은 메뉴의 「1.」을 그대로 읽힌다
            const number = (
              <span className="prompt-card-number">
                <span aria-hidden="true">{index + 1}</span>
                <span className="visually-hidden">{index + 1}.</span>
              </span>
            );
            const described = option.description !== null ? " has-description" : "";
            if (menu.multi_select) {
              const checked = selected.has(index);
              return (
                <label className={`prompt-card-option${described}${checked ? " is-checked" : ""}`} key={index}>
                  <input type="checkbox" checked={checked} disabled={off} onChange={() => toggle(index)} />
                  {number} {content}
                </label>
              );
            }
            return (
              <button key={index} type="button" className={`prompt-card-option${described}`} disabled={off} onClick={() => void answer({ by: "option", index })}>
                {number} {content}
              </button>
            );
          })}
        </div>
      )}
      {menu.multi_select && (
        <button
          type="button"
          className={`btn${selected.size > 0 ? " btn-primary" : ""} prompt-card-submit`}
          disabled={off || selected.size === 0}
          onClick={() => void answer({ by: "options", indices: [...selected].sort((a, b) => a - b) })}
        >
          {selected.size > 0 ? `제출 (${selected.size})` : "제출"}
        </button>
      )}
      {menu.custom_option_index !== null && (
        <div className="prompt-card-custom">
          {hasChoices && (
            <span className="prompt-card-custom-label" id={customLabelId}>
              또는 직접 답을 입력
            </span>
          )}
          <div className="prompt-card-custom-row">
            <input
              className="input"
              value={custom}
              disabled={off}
              placeholder={menu.options[menu.custom_option_index]?.label ?? "답을 입력"}
              aria-label={hasChoices ? undefined : "직접 입력한 답"}
              aria-labelledby={hasChoices ? customLabelId : undefined}
              onChange={(event) => setCustom(event.currentTarget.value)}
              onKeyDown={(event) => {
                // IME의 Enter는 후보를 확정한다. WebKit은 compositionend 뒤에 key code 229로 보낼 수 있다
                if (event.key === "Enter" && !event.nativeEvent.isComposing && event.nativeEvent.keyCode !== 229 && text.length > 0) void answer({ by: "custom", text });
              }}
            />
            <button type="button" className={`btn${text.length > 0 ? " btn-primary" : ""}`} disabled={off || text.length === 0} onClick={() => void answer({ by: "custom", text })}>
              <Send aria-hidden="true" /> 보내기
            </button>
          </div>
        </div>
      )}
      {!canControl && <p className="prompt-card-hint">보기 권한이라 답할 수 없다</p>}
      {error !== null && (
        <p className="prompt-card-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
