// 설정 창. 목록 아래 톱니 「설정」이 연다(herdr-web-ui MachineSidebar 바닥 줄 → SettingsDialog).
// 참고 SettingsDialog(aa624cb)의 쪽 가운데 aaa가 뒷받침하는 묶음만 옮겼다: 터미널 「글자 크기」·「휠 스크롤 속도」·「입력 방식」(Terminal 쪽 395~409줄)과
// 「구독 사용량」(Usage 쪽 UsagePage·UsageAccounts 570~631줄). 행·묶음·스위치·여럿 중 하나는 참고 SettingsControls.tsx와 같다.
// 「터미널 글꼴」(395~400줄, FontFamilyInput 72~205줄)은 적은 목록을 기본 글꼴 앞에 붙인다(plan §8.64).
// 「키 바」 줄의 「키 바 편집」은 참고처럼 같은 창 안에서 편집 화면(herdr/KeyBarSettings)으로 바꾸고 ← 로 돌아온다(참고 984~1116줄).
// 나머지 쪽(모양·chat·알림·기기 등)은 그 기능을 옮길 때 넣는다(plan §8.45).
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { DeviceRole, ProviderUsage, PublishState } from "./gen/proto";
import { call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { DevicesPanel } from "./DevicesPanel";
import { PhonePanel, phonePlan } from "./PhonePanel";
import { isLoopbackHost } from "./herdr/phone";
import { Icon } from "./icons";
import { AgentMark } from "./herdr/AgentMark.tsx";
import { KeyBarSettings } from "./herdr/KeyBarSettings.tsx";
import { AppearanceSettings } from "./AppearanceSettings";
import { FONT_FAMILY_MAX_CHARS, sanitizeFontFamily } from "./herdr/fontFamily.ts";
import { canListInstalledFonts, filterFamilies, isMonospaceFamily, queryInstalledFonts } from "./herdr/installedFonts.ts";
import { useInstallPrompt } from "./herdr/install.ts";
import { previewAlertSound, unlockAlertSound } from "./herdr/alertSound.ts";
import { enableAlerts } from "./alerts";
import { PushTestControls } from "./PushTestControls";
import {
  setKeepScreenOn,
  setTerminalFontFamily,
  setTerminalFontSize,
  setTerminalInputMode,
  setTerminalToggle,
  setTerminalWheelSpeed,
  TERMINAL_FONT_MAX,
  TERMINAL_FONT_MIN,
  TERMINAL_WHEEL_SPEED_MAX,
  TERMINAL_WHEEL_SPEED_MIN,
  updateUsageSettings,
  useKeepScreenOn,
  setAlertSetting,
  useAlertSetting,
  useTerminalFontFamily,
  useTerminalFontSize,
  useTerminalInputMode,
  useTerminalToggle,
  useTerminalWheelSpeed,
  useUsageSettings,
} from "./settings";
import { moveInOrder, orderProviders, PROVIDER_MARK, PROVIDER_NAME, usageName, useUsage } from "./usage";

/** 묶음: 제목 아래 카드 하나, 카드 안에 줄들 */
export function SettingsGroup({ title, note, className, children }: { title?: string; note?: string; className?: string; children: ReactNode }) {
  return (
    <section className={className ? `settings-section ${className}` : "settings-section"}>
      {title !== undefined && <h3>{title}</h3>}
      {note !== undefined && <p className="settings-note">{note}</p>}
      <div className="settings-card">{children}</div>
    </section>
  );
}

/** 설정 하나: 이름(과 설명)과 그 끝의 조작 하나. `wide`는 휴대폰에서 줄 전체가 필요한 조작이다 */
export function SettingsRow({ label, description, htmlFor, wide = false, children }: { label: string; description?: ReactNode; htmlFor?: string; wide?: boolean; children: ReactNode }) {
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
export function Segmented<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: readonly { value: T; label: string }[]; onChange: (value: T) => void }) {
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
              <AgentMark agent={PROVIDER_MARK[usage.id]} size={16} />
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

const FONT_LIST_LIMIT = 150;
const FONT_FAMILY_PLACEHOLDER = 'D2Coding, "Cascadia Mono"';

/**
 * 글꼴 목록 칸(참고 SettingsDialog FontFamilyInput). 칸을 떠날 때·Enter·창을 닫을 때 저장한다. 누를 때마다 저장하면
 * 치는 중인 쉼표·빈칸이 걸러지고, 글꼴이 바뀔 때마다 터미널이 다시 맞춰 pane 크기가 바뀐다.
 * 「찾아보기」는 깔린 글꼴을 물을 수 있는 브라우저(Chromium)에만 있고, 고정폭이 아닌 글꼴은 고르지 않는다.
 */
function FontFamilyInput({ value, label, onCommit }: { value: string; label: string; onCommit: (family: string) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = (): void => {
    const next = sanitizeFontFamily(draft);
    setDraft(next);
    if (next !== value) onCommit(next);
  };
  // Esc로 창을 닫으면 blur 없이 칸이 사라진다
  const commitRef = useRef(commit);
  commitRef.current = commit;
  useEffect(() => () => commitRef.current(), []);
  const [families, setFamilies] = useState<string[] | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [query, setQuery] = useState("");
  const [refused, setRefused] = useState("");
  const [unavailable, setUnavailable] = useState(false);
  // 허락 창은 누름(gesture)에서만 뜬다. API가 없는 브라우저에는 단추가 없다
  const browse = async (): Promise<void> => {
    if (browsing) {
      setBrowsing(false);
      return;
    }
    setRefused("");
    setQuery("");
    if (families === null) {
      const result = await queryInstalledFonts();
      if (result.status !== "granted") {
        setUnavailable(true);
        return;
      }
      setUnavailable(false);
      setFamilies(result.families);
    }
    setBrowsing(true);
  };
  const matches = useMemo(() => filterFamilies(families ?? [], query), [families, query]);
  const pick = (family: string): void => {
    if (!isMonospaceFamily(family)) {
      setRefused(family);
      return;
    }
    const next = sanitizeFontFamily(family);
    setDraft(next);
    setBrowsing(false);
    if (next !== value) onCommit(next);
  };
  return (
    <div className="settings-font-field">
      <div className="settings-font-line">
        <input
          className="settings-font-input"
          value={draft}
          placeholder={FONT_FAMILY_PLACEHOLDER}
          maxLength={FONT_FAMILY_MAX_CHARS}
          aria-label={label}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            // IME의 Enter는 둔다(WebKit이 compositionend 뒤에 보내는 key code 229 포함)
            if (event.key !== "Enter" || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
            event.preventDefault();
            commit();
          }}
        />
        {canListInstalledFonts() && (
          <button type="button" className="settings-font-browse" aria-expanded={browsing} aria-label={`${label}에 설치된 글꼴 찾아보기`} onClick={() => void browse()}>
            <Icon name="search" />
            찾아보기
          </button>
        )}
      </div>
      {unavailable && (
        <p className="settings-font-note" role="status">
          설치된 글꼴을 불러오지 못했습니다. 이름을 직접 입력하세요.
        </p>
      )}
      {browsing && families !== null && (
        <div className="settings-font-picker">
          <input
            className="settings-font-search"
            type="search"
            value={query}
            placeholder="글꼴 검색"
            aria-label="글꼴 검색"
            spellCheck={false}
            autoFocus
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape" && !event.nativeEvent.isComposing) {
                // 창이 아니라 목록을 닫는다
                event.stopPropagation();
                setBrowsing(false);
              }
            }}
          />
          {refused !== "" && (
            <p className="settings-font-note" role="status">
              {refused}은(는) 고정폭 글꼴이 아닙니다. 터미널에는 고정폭 글꼴이 필요하여 현재 글꼴을 유지했습니다.
            </p>
          )}
          <ul className="settings-font-list" aria-label="설치된 글꼴">
            {matches.slice(0, FONT_LIST_LIMIT).map((family) => (
              <li key={family}>
                <button type="button" className="settings-font-option" style={{ fontFamily: `"${family}", var(--font-ui)` }} onClick={() => pick(family)}>
                  {family}
                </button>
              </li>
            ))}
            {matches.length === 0 && <li className="settings-font-empty">글꼴을 찾을 수 없습니다</li>}
          </ul>
          {matches.length > FONT_LIST_LIMIT && <p className="settings-font-note">처음 {FONT_LIST_LIMIT}개의 글꼴을 표시합니다. 검색해서 목록을 좁히세요.</p>}
        </div>
      )}
    </div>
  );
}

/** 참고 SettingsDialog `DevicesPage`(820~862줄): 폰(열 주소·화면 켜짐·설치)과 기기(목록·페어링·로그아웃) */
function DevicesPage({ role, device, onSignOut }: { role: DeviceRole; device: string; onSignOut: () => Promise<void> }) {
  const keepScreenOn = useKeepScreenOn();
  const installPrompt = useInstallPrompt();
  // host가 tailscale serve로 이미 공개하는 주소(`web_status`)
  const [publish, setPublish] = useState<PublishState | null | undefined>(undefined);
  const loadPublish = useCallback(() => {
    setPublish(undefined);
    call({ op: "web_status" }).then(
      (r) => setPublish(r.kind === "publish" ? r : null),
      () => setPublish(null),
    );
  }, []);
  useEffect(loadPublish, [loadPublish]);
  const plan = phonePlan({ protocol: location.protocol, hostname: location.hostname, origin: location.origin, secure: window.isSecureContext }, publish ?? null);
  // 폰이 지금 열 수 있는 곳(pairing QR): 공개 주소, 아니면 loopback이 아닌 이 주소
  const pairUrl = plan.kind === "here" || plan.kind === "served" ? plan.url : isLoopbackHost(location.hostname) ? null : location.origin;
  const [confirmSignOut, setConfirmSignOut] = useState(false);
  return (
    <>
      <SettingsGroup title="폰">
        <div className="settings-item">
          <PhonePanel plan={plan} loading={publish === undefined} onRefresh={loadPublish} />
        </div>
        <SettingsRow label="화면 켜짐 유지" description="터미널이나 채팅 패널을 보는 동안 화면을 켜 둡니다. HTTPS 또는 localhost와 지원 브라우저가 필요합니다.">
          <Toggle label="화면 켜짐 유지" checked={keepScreenOn} onChange={setKeepScreenOn} />
        </SettingsRow>
        <SettingsRow label="설치" description={installPrompt.installed ? "설치됨" : installPrompt.canInstall ? undefined : installPrompt.help}>
          {!installPrompt.installed && installPrompt.canInstall && (
            <button type="button" className="primary" onClick={() => void installPrompt.install()}>
              앱 설치
            </button>
          )}
        </SettingsRow>
      </SettingsGroup>
      <SettingsGroup title="기기">
        <div className="settings-item">
          <DevicesPanel pairUrl={pairUrl} role={role} device={device} />
        </div>
        <SettingsRow label="로그아웃" description="이 브라우저의 로그인을 지웁니다. 다시 들어오려면 페어링 코드가 필요합니다.">
          <button type="button" className="btn" onClick={() => setConfirmSignOut(true)}>
            로그아웃
          </button>
        </SettingsRow>
        {confirmSignOut && (
          <ConfirmDialog
            title="이 브라우저에서 로그아웃할까요?"
            body="다시 들어오려면 페어링 코드가 필요합니다. 코드는 페어링된 기기의 설정 → 폰 및 기기나 host 머신 터미널의 aaa web pair에서 받습니다."
            confirmLabel="로그아웃"
            onClose={() => setConfirmSignOut(false)}
            onConfirm={onSignOut}
          />
        )}
      </SettingsGroup>
    </>
  );
}

export function SettingsDialog({ onClose, role, device, onSignOut }: { onClose: () => void; role: DeviceRole; device: string; onSignOut: () => Promise<void> }) {
  const mode = useTerminalInputMode();
  const fontSize = useTerminalFontSize();
  const fontFamily = useTerminalFontFamily();
  const alertInApp = useAlertSetting("alertInApp");
  const alertSound = useAlertSetting("alertSound");
  // the Sound switch as last set: the preview waits for the audio, and must not play once it is off
  const alertSoundWanted = useRef(alertSound);
  const wheelSpeed = useTerminalWheelSpeed();
  const paneClipboard = useTerminalToggle("paneClipboard");
  const releasePaneAway = useTerminalToggle("releasePaneAway");
  const surface = useRef<HTMLElement>(null);
  const [keyBarOpen, setKeyBarOpen] = useState(false);
  const keyBarButton = useRef<HTMLButtonElement>(null);
  const backButton = useRef<HTMLButtonElement>(null);
  const wasKeyBar = useRef(false);
  useEffect(() => {
    surface.current?.focus();
  }, []);
  // 편집 화면에 들어가면 ← 에, 돌아오면 「키 바 편집」 단추에 focus(참고 1017~1023줄)
  useEffect(() => {
    if (keyBarOpen) backButton.current?.focus();
    else if (wasKeyBar.current) keyBarButton.current?.focus();
    wasKeyBar.current = keyBarOpen;
  }, [keyBarOpen]);
  useEffect(() => {
    // Esc는 편집 화면이면 설정으로 돌아가고, 아니면 닫는다(참고 1052~1061줄)
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (keyBarOpen) setKeyBarOpen(false);
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, keyBarOpen]);
  // 목록 바닥 줄의 단추 모양을 물려받지 않게 body에 둔다
  return createPortal(
    <div
      className="modal-backdrop"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <section ref={surface} className="modal settings-dialog" role="dialog" aria-modal="true" aria-label={keyBarOpen ? "키 바" : "설정"} tabIndex={-1}>
        <header className="settings-head">
          {keyBarOpen && (
            <button type="button" ref={backButton} className="ghost icon" aria-label="설정으로 돌아가기" onClick={() => setKeyBarOpen(false)}>
              <Icon name="back" />
            </button>
          )}
          <h2>{keyBarOpen ? "키 바" : "설정"}</h2>
          <button type="button" className="ghost icon" aria-label="닫기" onClick={onClose}>
            <Icon name="close" />
          </button>
        </header>
        {keyBarOpen && (
          <div className="settings-body key-bar-body">
            <KeyBarSettings />
          </div>
        )}
        <div className="settings-body" hidden={keyBarOpen}>
          <AppearanceSettings />
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
            {/* 참고 SettingsDialog 398~400줄(문구는 참고 i18n.ko.ts) */}
            <SettingsRow label="터미널 글꼴" description="쉼표로 구분하며 앞에서부터 차례로 사용합니다. 이 기기에 없는 글꼴은 기본 글꼴로 대체합니다." wide>
              <FontFamilyInput value={fontFamily} label="터미널 글꼴" onCommit={setTerminalFontFamily} />
            </SettingsRow>
            {/* 참고 SettingsDialog 401~403줄(문구는 참고 i18n.ko.ts) */}
            <SettingsRow label="휠 스크롤 속도" description="휠을 한 번 굴릴 때 터미널이 움직이는 정도">
              <Stepper
                label="휠 스크롤 속도"
                value={wheelSpeed}
                text={`${wheelSpeed}×`}
                min={TERMINAL_WHEEL_SPEED_MIN}
                max={TERMINAL_WHEEL_SPEED_MAX}
                decreaseLabel="휠 스크롤 느리게"
                increaseLabel="휠 스크롤 빠르게"
                onChange={setTerminalWheelSpeed}
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
            {/* 참고 SettingsDialog 410~412줄(문구는 참고 i18n.ko.ts) */}
            <SettingsRow label="키 바" description="터미널 키와 순서, 사용자 조합을 설정합니다.">
              <button type="button" ref={keyBarButton} onClick={() => setKeyBarOpen(true)}>
                키 바 편집
              </button>
            </SettingsRow>
            {/* 참고 SettingsDialog 413~418줄(문구는 참고 i18n.ko.ts) */}
            <SettingsRow
              label="패널에서 클립보드로 복사"
              description="패널에서 복사하는 프로그램(vim, tmux, Claude Code)은 직접 복사한 것과 같이 이 기기의 클립보드에 텍스트를 넣습니다. 믿을 수 없는 출력을 실행하는 패널이 있다면 끄세요. 다음에 붙여 넣을 내용이 바뀔 수 있습니다."
            >
              <Toggle label="패널에서 클립보드로 복사" checked={paneClipboard} onChange={(on) => setTerminalToggle("paneClipboard", on)} />
            </SettingsRow>
            {/* 참고 「터미널의 herdr와 같이 쓰기」: herdr 창 자리에 aaa TUI(`aaa attach`)를 넣은 문구 */}
            <SettingsRow
              label="터미널의 aaa와 같이 쓰기"
              description="이 컴퓨터의 터미널 창에서도 aaa를 띄워 쓴다면 켜세요. 다른 창으로 옮기고 1초가 지나면 이 탭은 패널을 보기만 해서, 터미널 창이 패널을 이 창 크기가 아닌 자기 크기로 유지합니다. 다시 입력하려면 탭을 클릭하세요. 보는 동안 패널은 터미널 창 크기로 그려져 이 창을 다 채우지 않을 수 있습니다."
            >
              <Toggle label="터미널의 aaa와 같이 쓰기" checked={releasePaneAway} onChange={(on) => setTerminalToggle("releasePaneAway", on)} />
            </SettingsRow>
          </SettingsGroup>
          {/* 참고 SettingsDialog AlertsPage(문구는 참고 i18n.ko.ts). 무엇을 알릴지(입력 필요·작업 완료)는 기기별 종류(차이 목록 N3)와 함께 */}
          <SettingsGroup title="알림" note="이 기기에 적용됩니다. 알림은 잠시 모았다가 보내며, 그사이 질문에 답하거나 보고를 확인하면 보내지 않습니다.">
            <PushTestControls onEnable={enableAlerts} />
            <SettingsRow label="앱 안에서" description="앱을 열어 둔 동안에는 화면 위에서 바로 내려옵니다. 누르면 그 패널을 엽니다.">
              <Toggle label="앱 안에서" checked={alertInApp} onChange={(on) => setAlertSetting("alertInApp", on)} />
            </SettingsRow>
            <SettingsRow label="소리" description="앱 탭을 열어 둔 동안에는 이 알림마다 소리를 냅니다. 집중 모드나 방해 금지 모드로 알림이 꺼져 있어도 들립니다.">
              <Toggle
                label="소리"
                checked={alertSound}
                onChange={(on) => {
                  setAlertSetting("alertSound", on);
                  alertSoundWanted.current = on;
                  // this tap is the gesture the page needs to play audio; the chime is the preview,
                  // unless the switch went off again while the audio was getting ready
                  if (on)
                    void unlockAlertSound().then((ready) => {
                      if (ready && alertSoundWanted.current) previewAlertSound();
                    });
                }}
              />
            </SettingsRow>
          </SettingsGroup>
          <DevicesPage role={role} device={device} onSignOut={onSignOut} />
          <UsageGroups />
        </div>
      </section>
    </div>,
    document.body,
  );
}
