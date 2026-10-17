// 설정 창. 목록 아래 톱니 「설정」이 연다(herdr-web-ui MachineSidebar 바닥 줄 → SettingsDialog).
// 참고 SettingsDialog(aa624cb)의 쪽 가운데 aaa가 뒷받침하는 묶음만 옮겼다: 터미널 「글자 크기」·「입력 방식」(Terminal 쪽 395~409줄)과
// 「구독 사용량」(Usage 쪽 UsagePage·UsageAccounts 570~631줄). 행·묶음·스위치·여럿 중 하나는 참고 SettingsControls.tsx와 같다.
// 나머지 쪽(모양·chat·알림·기기·키 막대 편집 등)은 그 기능을 옮길 때 넣는다(plan §8.45).
import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { ProviderUsage } from "./gen/proto";
import { Icon } from "./icons";
import { ProviderMark } from "./UsageMeters";
import {
  setTerminalFontSize,
  setTerminalInputMode,
  TERMINAL_FONT_MAX,
  TERMINAL_FONT_MIN,
  updateUsageSettings,
  useTerminalFontSize,
  useTerminalInputMode,
  useUsageSettings,
} from "./settings";
import { moveInOrder, orderProviders, PROVIDER_NAME, usageName, useUsage } from "./usage";

/** 묶음: 제목 아래 카드 하나, 카드 안에 줄들 */
function SettingsGroup({ title, className, children }: { title?: string; className?: string; children: ReactNode }) {
  return (
    <section className={className ? `settings-section ${className}` : "settings-section"}>
      {title !== undefined && <h3>{title}</h3>}
      <div className="settings-card">{children}</div>
    </section>
  );
}

/** 설정 하나: 이름(과 설명)과 그 끝의 조작 하나. `wide`는 휴대폰에서 줄 전체가 필요한 조작이다 */
function SettingsRow({ label, description, htmlFor, wide = false, children }: { label: string; description?: ReactNode; htmlFor?: string; wide?: boolean; children: ReactNode }) {
  return (
    <div className={wide ? "settings-row is-wide" : "settings-row"}>
      <div className="settings-row-text">
        {htmlFor !== undefined ? (
          <label className="settings-label" htmlFor={htmlFor}>
            {label}
          </label>
        ) : (
          <span className="settings-label">{label}</span>
        )}
        {description !== undefined && <span className="settings-description">{description}</span>}
      </div>
      {children}
    </div>
  );
}

function Toggle({ checked, label, onChange }: { checked: boolean; label: string; onChange: (checked: boolean) => void }) {
  return (
    <button type="button" className="settings-toggle" role="switch" aria-checked={checked} aria-label={label} onClick={() => onChange(!checked)}>
      <span className="settings-toggle-thumb" />
    </button>
  );
}

/** 숫자 하나를 한 칸씩(참고 SettingsControls `Stepper`) */
function Stepper({ label, value, text, min, max, decreaseLabel, increaseLabel, onChange }: { label: string; value: number; text: string; min: number; max: number; decreaseLabel: string; increaseLabel: string; onChange: (value: number) => void }) {
  return (
    <div className="settings-stepper" role="group" aria-label={label}>
      <button type="button" className="ghost icon small" aria-label={decreaseLabel} disabled={value <= min} onClick={() => onChange(value - 1)}>
        <Icon name="minus" />
      </button>
      <output aria-live="polite">{text}</output>
      <button type="button" className="ghost icon small" aria-label={increaseLabel} disabled={value >= max} onClick={() => onChange(value + 1)}>
        <Icon name="plus" />
      </button>
    </div>
  );
}

/** 짧은 선택지 몇 개를 다 보이게 */
function Segmented<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: readonly { value: T; label: string }[]; onChange: (value: T) => void }) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {options.map((option) => (
        <button key={option.value} type="button" aria-pressed={value === option.value} onClick={() => onChange(option.value)}>
          {option.label}
        </button>
      ))}
    </div>
  );
}

/** 막대가 아는 계정을 막대 순서로: 줄마다 위·아래로 옮기기와 보이기·숨기기 */
function UsageAccounts({ providers }: { providers: readonly ProviderUsage[] }) {
  const settings = useUsageSettings();
  const ordered = orderProviders(providers, settings.usageOrder);
  const keys = ordered.map((usage) => usage.key);
  const move = (key: string, by: -1 | 1) => updateUsageSettings({ usageOrder: moveInOrder(keys, settings.usageOrder, key, by) });
  return (
    <SettingsGroup title="계정" className="usage-accounts">
      <ol aria-label="계정">
        {ordered.map((usage, index) => {
          const name = usageName(usage);
          const hidden = settings.usageHidden.includes(usage.key);
          return (
            <li key={usage.key} className={`usage-accounts-row${hidden ? " is-hidden" : ""}`}>
              <ProviderMark id={usage.id} />
              <span className="usage-accounts-name">{PROVIDER_NAME[usage.id]}</span>
              <span className="usage-accounts-account" title={usage.account ?? undefined}>
                {usage.account}
              </span>
              <span className="usage-accounts-actions">
                <button type="button" className="ghost icon small" aria-label={`${name} 위로`} title={`${name} 위로`} disabled={index === 0} onClick={() => move(usage.key, -1)}>
                  <Icon name="up" />
                </button>
                <button
                  type="button"
                  className="ghost icon small"
                  aria-label={`${name} 아래로`}
                  title={`${name} 아래로`}
                  disabled={index === ordered.length - 1}
                  onClick={() => move(usage.key, 1)}
                >
                  <Icon name="down" />
                </button>
                <button
                  type="button"
                  className="ghost icon small"
                  role="switch"
                  aria-checked={!hidden}
                  aria-label={`${name} 보이기`}
                  title={`${name} 보이기`}
                  onClick={() =>
                    updateUsageSettings({ usageHidden: hidden ? settings.usageHidden.filter((key) => key !== usage.key) : [...settings.usageHidden, usage.key] })
                  }
                >
                  <Icon name={hidden ? "eye-off" : "eye"} />
                </button>
              </span>
            </li>
          );
        })}
      </ol>
    </SettingsGroup>
  );
}

function UsageGroups() {
  const settings = useUsageSettings();
  // 순서·숨기기를 정할 계정: 막대가 보이는 것과 같은 보고(host 캐시)
  const usage = useUsage(settings.showUsage);
  return (
    <>
      <SettingsGroup title="구독 사용량">
        <SettingsRow
          label="plan 한도 보이기"
          description="설정 옆에 host 머신의 AI 도구가 각 plan을 얼마나 썼는지 보인다. 켜면 그 로그인을 각 구독의 사용량 주소로 보낸다. 로그인을 새로 받지는 않는다."
        >
          <Toggle label="plan 한도 보이기" checked={settings.showUsage} onChange={(showUsage) => updateUsageSettings({ showUsage })} />
        </SettingsRow>
        {settings.showUsage && (
          <>
            <SettingsRow label="막대가 보이는 것">
              <Segmented
                label="막대가 보이는 것"
                value={settings.usageCount}
                onChange={(usageCount) => updateUsageSettings({ usageCount })}
                options={[
                  { value: "used", label: "쓴 양" },
                  { value: "left", label: "남은 양" },
                ]}
              />
            </SettingsRow>
            <SettingsRow
              label="보이는 한도"
              description="칩마다 보이는 한도. 세션은 짧은 것(Claude·Codex는 5시간)이다. 고른 한도가 없는 plan은 바닥에 가장 가까운 것을 보인다."
              wide
            >
              <Segmented
                label="보이는 한도"
                value={settings.usageGlance}
                onChange={(usageGlance) => updateUsageSettings({ usageGlance })}
                options={[
                  { value: "week", label: "주간" },
                  { value: "session", label: "세션" },
                ]}
              />
            </SettingsRow>
          </>
        )}
      </SettingsGroup>
      {settings.showUsage && usage.report && usage.report.providers.length > 0 && <UsageAccounts providers={usage.report.providers} />}
    </>
  );
}

export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const mode = useTerminalInputMode();
  const fontSize = useTerminalFontSize();
  const surface = useRef<HTMLElement>(null);
  useEffect(() => {
    surface.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  // 목록 바닥 줄의 단추 모양을 물려받지 않게 body에 둔다
  return createPortal(
    <div
      className="modal-backdrop"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <section ref={surface} className="modal settings-dialog" role="dialog" aria-modal="true" aria-label="설정" tabIndex={-1}>
        <header className="settings-head">
          <h2>설정</h2>
          <button type="button" className="ghost icon" aria-label="닫기" onClick={onClose}>
            <Icon name="close" />
          </button>
        </header>
        <div className="settings-body">
          <SettingsGroup title="터미널">
            <SettingsRow label="글자 크기" description="이 기기의 모든 터미널. 터미널을 두 손가락으로 벌리거나 오므려도 바뀐다.">
              <Stepper
                label="글자 크기"
                value={fontSize}
                text={`${fontSize}px`}
                min={TERMINAL_FONT_MIN}
                max={TERMINAL_FONT_MAX}
                decreaseLabel="글자 작게"
                increaseLabel="글자 크게"
                onChange={setTerminalFontSize}
              />
            </SettingsRow>
            <SettingsRow label="입력 방식" htmlFor="terminal-input-mode">
              {/* 저장 기본값 auto는 모든 기기에서 그리드에 바로 친다: 바로 입력으로 읽는다 */}
              <select
                id="terminal-input-mode"
                className="settings-select"
                value={mode === "line" ? "line" : "direct"}
                onChange={(event) => setTerminalInputMode(event.target.value as "line" | "direct")}
              >
                <option value="direct">바로 입력</option>
                <option value="line">입력줄</option>
              </select>
            </SettingsRow>
          </SettingsGroup>
          <UsageGroups />
        </div>
      </section>
    </div>,
    document.body,
  );
}
