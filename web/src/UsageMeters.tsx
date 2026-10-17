// 목록 아래 설정 옆의 구독 사용량 막대. herdr-web-ui `src/components/UsageMeters.tsx`(aa624cb)를 옮겼다(문구 한국어).
// 참고와 다른 곳: 구독 표시는 참고 AgentMark 대신 aaa 아이콘(claude·codex)이고, 나머지 구독은 이름 첫 글자다.
import { useEffect, useRef, useState, type CSSProperties, type FocusEvent, type KeyboardEvent } from "react";
import type { ProviderUsage, UsageProviderId, UsageWindow } from "./gen/proto";
import { Icon } from "./icons";
import { useUsageSettings, type UsageCount, type UsageGlance } from "./settings";
import {
  formatPercent,
  formatResetIn,
  glanceWindow,
  HIGH_PERCENT,
  meterPercent,
  meterText,
  orderProviders,
  PROVIDER_NAME,
  usageName,
  useUsage,
  windowLabel,
} from "./usage";

/** 설정 옆 줄에 두는 칩 수. 넘으면 나머지는 「+N」으로 접는다 */
const MAX_CHIPS = 4;

/** 구독 표시: aaa 아이콘이 있으면 그것, 없으면 이름 첫 글자 */
export function ProviderMark({ id }: { id: UsageProviderId }) {
  if (id === "claude" || id === "codex") return <Icon name={id} />;
  return (
    <span className="usage-mark" aria-hidden="true">
      {PROVIDER_NAME[id][0]}
    </span>
  );
}

function level(window: UsageWindow | null): string {
  return window !== null && window.used_percent >= HIGH_PERCENT ? " is-high" : "";
}

/** 보이는 숫자가 오래됐거나 없는 까닭. 새것이면 null */
function problemText(usage: ProviderUsage): string | null {
  const name = PROVIDER_NAME[usage.id];
  switch (usage.problem) {
    case "expired":
      return `로그인이 끝났다. ${name}을(를) 열어 다시 로그인한다.`;
    case "rate_limited":
      return `${name}이(가) 늦추라고 했다. 마지막 숫자다.`;
    case "failed":
      return `${name}에 닿지 못했다.`;
    case "locked":
      return "이 로그인을 담은 keychain을 host가 열 수 없다.";
    default:
      return null;
  }
}

/** 끝난 로그인·닿지 못함은 오류다. 늦추라고 했거나 잠겼으면 마지막 숫자가 그대로다 */
function isError(usage: ProviderUsage): boolean {
  return usage.problem === "expired" || usage.problem === "failed";
}

function Chip({ usage, count, glance }: { usage: ProviderUsage; count: UsageCount; glance: UsageGlance }) {
  const window = glanceWindow(usage, glance);
  return (
    <span className={`usage-chip${level(window)}${usage.problem ? " has-problem" : ""}`} data-provider={usage.id}>
      <ProviderMark id={usage.id} />
      <span className="usage-chip-value">{window ? formatPercent(meterPercent(window, count)) : "—"}</span>
      <span className="usage-chip-bar" style={{ "--fill": `${window ? meterPercent(window, count) : 0}%` } as CSSProperties} />
    </span>
  );
}

function Provider({ usage, now, count }: { usage: ProviderUsage; now: number; count: UsageCount }) {
  const problem = problemText(usage);
  return (
    <section className="usage-provider" aria-label={usageName(usage)}>
      <header className="usage-provider-head">
        <ProviderMark id={usage.id} />
        <span className="usage-provider-name">{PROVIDER_NAME[usage.id]}</span>
        {usage.plan && <span className="usage-plan">{usage.plan}</span>}
        {usage.account && (
          <span className="usage-account" title={usage.account}>
            {usage.account}
          </span>
        )}
      </header>
      {problem && <p className={`usage-note${isError(usage) ? " is-problem" : ""}`}>{problem}</p>}
      {usage.windows.map((window, index) => {
        const reset = formatResetIn(window.resets_at, now);
        const value = meterPercent(window, count);
        return (
          <div key={index} className={`usage-row${level(window)}`}>
            <span className="usage-row-label">{windowLabel(window)}</span>
            {reset && <span className="usage-row-reset">{reset} 뒤 초기화</span>}
            <span className="usage-row-value">{meterText(window, count)}</span>
            <span
              className="usage-bar"
              role="meter"
              aria-label={windowLabel(window)}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={value}
              aria-valuetext={meterText(window, count)}
            >
              {/* 0보다 조금 큰 값도 점이 아니라 막대로 보인다 */}
              <span style={{ width: value > 0 ? `max(4px, ${value}%)` : 0 }} />
            </span>
          </div>
        );
      })}
      {usage.windows.length === 0 && !problem && <p className="usage-note">알린 한도가 없다</p>}
    </section>
  );
}

/**
 * host 머신에 로그인한 구독의 plan 한도를 설정 옆에: 구독마다 표시와 설정에서 고른 한도(주간·세션).
 * 누르면 모든 한도와 다시 시작하는 때를 연다.
 */
export function UsageMeters() {
  const settings = useUsageSettings();
  const { report, loading, refresh } = useUsage(settings.showUsage);
  const [open, setOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const rootRef = useRef<HTMLDivElement>(null);
  const stripRef = useRef<HTMLButtonElement>(null);

  // 설정에서 끄면 펼친 창도 닫는다(듣기와 시계도 함께)
  useEffect(() => {
    if (!settings.showUsage) setOpen(false);
  }, [settings.showUsage]);
  // 초기화까지 남은 때는 보고 사이에도 흐른다(닫혀 있어도)
  useEffect(() => {
    if (!settings.showUsage) return;
    setNow(Date.now());
    const tick = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(tick);
  }, [settings.showUsage, open]);
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  // Escape와 focus는 focus가 막대 안에 있을 때만 막대 것이다
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (event.key !== "Escape" || !open) return;
    event.stopPropagation();
    setOpen(false);
    stripRef.current?.focus();
  };
  const onBlur = (event: FocusEvent<HTMLDivElement>): void => {
    if (event.relatedTarget instanceof Node && !event.currentTarget.contains(event.relatedTarget)) setOpen(false);
  };

  const count = settings.usageCount;
  // 설정에서 숨긴 계정은 줄과 펼친 창 모두에서 뺀다
  const shown = report
    ? orderProviders(report.providers, settings.usageOrder).filter((usage) => !settings.usageHidden.includes(usage.key))
    : [];
  if (!settings.showUsage || shown.length === 0) return null;
  const folded = shown.length > MAX_CHIPS ? shown.length - (MAX_CHIPS - 1) : 0;
  const chips = folded > 0 ? shown.slice(0, MAX_CHIPS - 1) : shown;
  const summary = shown
    .map((usage) => {
      const window = glanceWindow(usage, settings.usageGlance);
      return `${usageName(usage)} ${window ? meterText(window, count) : "—"}`;
    })
    .join(", ");

  return (
    <div className="usage" ref={rootRef} onKeyDown={onKeyDown} onBlur={onBlur}>
      <button
        ref={stripRef}
        type="button"
        className="usage-strip"
        data-testid="usage-strip"
        aria-expanded={open}
        aria-label={`구독 사용량: ${summary}`}
        title={summary}
        onClick={() => setOpen(!open)}
      >
        {chips.map((usage) => (
          <Chip key={usage.key} usage={usage} count={count} glance={settings.usageGlance} />
        ))}
        {folded > 0 && <span className="usage-more">+{folded}</span>}
      </button>
      {open && (
        <div className="usage-popover" role="dialog" aria-label="구독 사용량">
          <header className="usage-popover-head">
            <span>구독 사용량</span>
            {/* 꺼진 단추는 focus를 놓고 Escape도 함께 놓는다: 바쁨만 알린다 */}
            <button
              type="button"
              className="ghost icon small"
              aria-label="다시 읽기"
              title="다시 읽기"
              aria-busy={loading}
              onClick={() => {
                if (!loading) refresh();
              }}
            >
              <span className={loading ? "is-spinning" : undefined}>
                <Icon name="refresh" />
              </span>
            </button>
          </header>
          {shown.map((usage) => (
            <Provider key={usage.key} usage={usage} now={now} count={count} />
          ))}
        </div>
      )}
    </div>
  );
}
