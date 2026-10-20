// 입력칸 컨텍스트 게이지(plan §8.321): 보내기 앞의 작은 링과 숫자, 누르면 위로 펼침(사용·창·주의·정리 단추).
// herdr-web-ui `ContextRing`(Composer.tsx 180~225줄)·t3code `ContextWindowMeter`를 따른다. 다른 곳(사용자 10-20): 남은 양이 아니라
// 사용량 %이고 넘거나 창을 모르면 k, 숫자는 늘 보이고, 휴대폰도 펼친다. 글을 치는 동안은 자리를 둔 채 투명해진다(입력칸 폭이 그대로).
import { useEffect, useRef, useState } from "react";
import { Minimize2 } from "lucide-react";
import type { ContextUsage } from "./gen/proto";
import { compactionText, contextView, formatK, hiddenWhileTyping } from "./contextUsage";

export interface ContextGaugeProps {
  context: ContextUsage;
  /** 「Claude」·「Codex」 */
  agentLabel: string;
  /** 입력칸에 글이 있다 */
  typing: boolean;
  /** agent가 대화를 정리하는 중(plan §8.309): 마지막 값을 흐리게, 링이 돈다 */
  compacting: boolean;
  /** 지금 정리를 보낼 수 없는 까닭. null이면 보낼 수 있다 */
  compactBlocked: string | null;
  onCompact: () => void;
}

const RADIUS = 7;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

export function ContextGauge({ context, agentLabel, typing, compacting, compactBlocked, onCompact }: ContextGaugeProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLSpanElement>(null);
  const view = contextView(context);
  const hidden = hiddenWhileTyping(view.level, typing);

  // 치기 시작하면 펼침도 닫는다
  useEffect(() => {
    if (hidden) setOpen(false);
  }, [hidden]);

  // 펼침 밖을 누르거나 Esc면 닫는다(입력창 Esc는 그대로 간다: 펼침이 열렸을 때만 먹는다)
  useEffect(() => {
    if (!open) return;
    const down = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
    };
    document.addEventListener("pointerdown", down);
    document.addEventListener("keydown", key, true);
    return () => {
      document.removeEventListener("pointerdown", down);
      document.removeEventListener("keydown", key, true);
    };
  }, [open]);

  const title = compacting ? `${view.title} · 대화 정리 중: 끝나면 새 값` : view.title;
  const note =
    view.level === "over"
      ? `기록의 토큰이 이 모델의 창(${formatK(context.window ?? 0)})보다 크다. 창 크기 추정이 틀렸을 수 있어 %가 아니라 실제 수로 보인다.`
      : view.level === "unknown"
        ? "이 모델의 창 크기를 기록으로 알 수 없어 %가 아니라 실제 수로 보인다."
        : view.level === "high"
          ? "80%를 넘었다. harness가 곧 스스로 정리할 수 있다."
          : null;
  const amount =
    view.level === "over" ? `사용 · 창 ${formatK(context.window ?? 0)} 넘음`
    : view.level === "unknown" ? "사용 · 창 크기 모름"
    : `${formatK(context.used)} / ${formatK(context.window ?? 0)} 사용`;

  return (
    <span
      ref={rootRef}
      className="context-gauge"
      data-level={view.level}
      data-hidden={hidden || undefined}
      data-compacting={compacting || undefined}
      data-testid="context-gauge"
    >
      <button
        type="button"
        className="context-gauge-button"
        aria-label={title}
        title={title}
        aria-expanded={open}
        aria-hidden={hidden || undefined}
        tabIndex={hidden ? -1 : undefined}
        // focus는 글 칸에 남는다(휴대폰 자판이 닫히지 않게, 다른 입력칸 단추와 같다)
        onPointerDown={(event) => event.preventDefault()}
        onClick={() => setOpen((was) => !was)}
      >
        <svg className="context-gauge-ring" viewBox="0 0 18 18" aria-hidden="true">
          <circle className="context-gauge-track" cx="9" cy="9" r={RADIUS} />
          {view.fill > 0 && (
            <circle className="context-gauge-fill" cx="9" cy="9" r={RADIUS} strokeDasharray={`${(CIRCUMFERENCE * view.fill) / 100} ${CIRCUMFERENCE}`} />
          )}
        </svg>
        <span className="context-gauge-value" data-testid="context-gauge-value">{view.text}</span>
      </button>
      {open && (
        <div className="context-pop" role="dialog" aria-label="컨텍스트" data-testid="context-pop">
          <div className="context-pop-head">
            <span>컨텍스트</span>
            <span>{agentLabel}{context.window !== null ? ` · 창 ${formatK(context.window)}` : ""}</span>
          </div>
          <div className="context-pop-big">
            <span className="context-gauge-value">{view.text}</span>
            <span>{amount}</span>
          </div>
          {context.window !== null && (
            <span className="usage-bar" role="meter" aria-label="컨텍스트 사용" aria-valuemin={0} aria-valuemax={100} aria-valuenow={view.fill} aria-valuetext={view.title}>
              <span style={{ width: view.fill > 0 ? `max(4px, ${view.fill}%)` : 0 }} />
            </span>
          )}
          {compacting ? <p className="context-pop-note">대화 정리 중이다. 끝나면 새 값이 된다.</p> : note && <p className={`context-pop-note${view.level === "unknown" ? "" : " is-warn"}`}>{note}</p>}
          {context.compaction && <p className="context-pop-note">마지막 정리 {compactionText(context.compaction.before, context.compaction.after)}</p>}
          <div className="context-pop-foot">
            <span>마지막 응답 기준</span>
            <button
              type="button"
              className={`btn${view.level === "high" || view.level === "over" ? " btn-primary" : ""}`}
              disabled={compactBlocked !== null}
              title={compactBlocked ?? "/compact를 보낸다: 대화를 요약해 컨텍스트를 비운다"}
              data-testid="context-compact"
              onPointerDown={(event) => event.preventDefault()}
              onClick={() => {
                setOpen(false);
                onCompact();
              }}
            >
              <Minimize2 aria-hidden="true" />
              <code>/compact</code> 정리
            </button>
          </div>
          {compactBlocked && <p className="context-pop-note" data-testid="context-compact-blocked">{compactBlocked}</p>}
        </div>
      )}
    </span>
  );
}
