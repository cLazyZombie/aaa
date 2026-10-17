// 설정 창. 목록 아래 톱니 「설정」이 연다(herdr-web-ui aa624cb MachineSidebar 바닥 줄 → SettingsDialog, plan §8.91).
// 참고 OpenSettingsDialog(954~1110줄)대로 쪽 목록과 쪽이다: 641px부터는 왼쪽 목록 옆에 쪽, 휴대폰은 목록 → 쪽(← 로 돌아감).
// 쪽마다 history 한 칸(herdr/settingsHistory.ts): 시스템 「뒤로」가 한 칸씩 나온다. Tab은 창 안에 머물고(useFocusTrap) 닫으면 연 곳으로
// focus가 돌아간다. 휴대폰은 아래로 밀어 닫는다(useSheetSwipe). 묶음·줄·스위치·여럿 중 하나·한 칸씩은 참고 SettingsControls 그대로다.
// 쪽은 aaa가 뒷받침하는 것만 둔다: 모양·터미널·알림·구독 사용량·폰 및 기기. 참고의 채팅·음성 입력·단축키·원격 PC·에이전트 연동·정보는
// 그 기능이 aaa에 없어 뺐다. 「터미널 글꼴」(FontFamilyInput 72~205줄)은 적은 목록을 기본 글꼴 앞에 붙인다(plan §8.64).
// 「키 바」 줄의 「키 바 편집」은 같은 창 안에서 편집 화면(herdr/KeyBarSettings)으로 바꾸고 ← 로 돌아온다(참고 984~1116줄).
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type RefObject } from "react";
import { createPortal } from "react-dom";
import { ArrowLeft, Bell, ChevronDown, ChevronRight, ChevronUp, Eye, EyeOff, Gauge, Palette, Search, Smartphone, SquareTerminal, X, type LucideIcon } from "lucide-react";
import "./herdr/SettingsDialog.css";
import type { DeviceRole, ProviderUsage, PublishState } from "./gen/proto";
import { call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { DevicesPanel } from "./DevicesPanel";
import { PhonePanel, phonePlan } from "./PhonePanel";
import { isLoopbackHost } from "./herdr/phone";
import { AgentMark } from "./herdr/AgentMark.tsx";
import { KeyBarSettings } from "./herdr/KeyBarSettings.tsx";
import { AppearanceSettings } from "./AppearanceSettings";
import { FONT_FAMILY_MAX_CHARS, sanitizeFontFamily } from "./herdr/fontFamily.ts";
import { canListInstalledFonts, filterFamilies, isMonospaceFamily, queryInstalledFonts } from "./herdr/installedFonts.ts";
import { useInstallPrompt } from "./herdr/install.ts";
import { previewAlertSound, unlockAlertSound } from "./herdr/alertSound.ts";
import { useFocusTrap } from "./herdr/useFocusTrap.ts";
import { useSheetSwipe } from "./herdr/sheets.ts";
import { onSettingsHistory, recordSettings, settingsEntry, settingsLevels, type SettingsLevel } from "./herdr/settingsHistory.ts";
import { Segmented, SettingsGroup, SettingsRow, Stepper, Toggle } from "./herdr/SettingsControls.tsx";
import { enableAlerts } from "./alerts";
import { PushTestControls } from "./PushTestControls";
import { beforeAlertSound, canPlayInSilentMode } from "./silentMode";
import { onTraceChange, sendTrace, setTrace, traceCount, traceOn } from "./inputTrace";
import {
  setKeepScreenOn,
  setTerminalFontFamily,
  setTerminalFontSize,
  setTerminalInputMode,
  setTerminalWheelSpeed,
  TERMINAL_FONT_MAX,
  TERMINAL_FONT_MIN,
  TERMINAL_WHEEL_SPEED_MAX,
  TERMINAL_WHEEL_SPEED_MIN,
  updateUsageSettings,
  useKeepScreenOn,
  setAlertSetting,
  alertSetting,
  useAlertSetting,
  useTerminalFontFamily,
  useTerminalFontSize,
  useTerminalInputMode,
  useTerminalWheelSpeed,
  useUsageSettings,
} from "./settings";
import { moveInOrder, orderProviders, PROVIDER_MARK, PROVIDER_NAME, usageName, useUsage } from "./usage";

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
                <button type="button" className="icon-button" aria-label={`${name} 위로`} title={`${name} 위로`} disabled={index === 0} onClick={() => move(usage.key, -1)}>
                  <ChevronUp aria-hidden="true" />
                </button>
                <button
                  type="button"
                  className="icon-button"
                  aria-label={`${name} 아래로`}
                  title={`${name} 아래로`}
                  disabled={index === ordered.length - 1}
                  onClick={() => move(usage.key, 1)}
                >
                  <ChevronDown aria-hidden="true" />
                </button>
                <button
                  type="button"
                  className="icon-button usage-accounts-visibility"
                  role="switch"
                  aria-checked={!hidden}
                  aria-label={`${name} 보이기`}
                  title={`${name} 보이기`}
                  onClick={() =>
                    updateUsageSettings({ usageHidden: hidden ? settings.usageHidden.filter((key) => key !== usage.key) : [...settings.usageHidden, usage.key] })
                  }
                >
                  {hidden ? <EyeOff aria-hidden="true" /> : <Eye aria-hidden="true" />}
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
      <SettingsGroup>
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
          className="input settings-font-input"
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
          <button type="button" className="btn settings-font-browse" aria-expanded={browsing} aria-label={`${label}에 설치된 글꼴 찾아보기`} onClick={() => void browse()}>
            <Search aria-hidden="true" />
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
            className="input settings-font-search"
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
            <button type="button" className="btn btn-primary" onClick={() => void installPrompt.install()}>
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

/** 참고 TerminalPage(390~421줄). 참고의 「pane에서 클립보드」·「herdr 창과 함께 쓰기」는 aaa에 그 기능이 없어 뺐다 */
/** 실기 입력 사건 기록(inputTrace.ts): 켜기·끄기, 모은 수, host에 보내기 */
function useInputTrace() {
  const [on, setOn] = useState(traceOn());
  const [count, setCount] = useState(traceCount());
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(
    () =>
      onTraceChange(() => {
        setOn(traceOn());
        setCount(traceCount());
      }),
    [],
  );
  const send = async () => {
    try {
      setMsg(`보냈다: ${await sendTrace()}`);
    } catch (e) {
      setMsg(`보내지 못했다: ${(e as Error).message}`);
    }
  };
  return { on, count, msg, send };
}

function TerminalPage({ keyBarButtonRef, onEditKeyBar }: { keyBarButtonRef: RefObject<HTMLButtonElement | null>; onEditKeyBar: () => void }) {
  const mode = useTerminalInputMode();
  const fontSize = useTerminalFontSize();
  const fontFamily = useTerminalFontFamily();
  const wheelSpeed = useTerminalWheelSpeed();
  const trace = useInputTrace();
  return (
    <>
      <SettingsGroup>
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
            className="select settings-select"
            value={mode === "line" ? "line" : "direct"}
            onChange={(event) => setTerminalInputMode(event.target.value as "line" | "direct")}
          >
            <option value="direct">바로 입력</option>
            <option value="line">입력줄</option>
          </select>
        </SettingsRow>
        {/* 참고 SettingsDialog 410~412줄(문구는 참고 i18n.ko.ts) */}
        <SettingsRow label="키 바" description="터미널 키와 순서, 사용자 조합을 설정합니다.">
          <button type="button" ref={keyBarButtonRef} className="btn" onClick={onEditKeyBar}>
            키 바 편집
          </button>
        </SettingsRow>
      </SettingsGroup>
      {/* aaa: 실기 입력 사건 기록(inputTrace.ts, plan §8.82). 참고에는 이런 진단이 없어 그 기록이 보는 터미널 쪽 맨 끝에 둔다 */}
      <SettingsGroup title="진단">
        <SettingsRow
          label="입력 기록"
          description={
            <>
              {trace.on
                ? `${trace.count}개 모았습니다. 문제를 재현한 뒤 보내면 host 상태 폴더 diag/에 남습니다.`
                : "실기 키보드·한글 입력 문제를 볼 때 켭니다. 터미널 입력 사건과 입력칸 상태를 모읍니다."}
              {trace.msg && (
                <span className="settings-status" role="status" data-testid="trace-msg">
                  {trace.msg}
                </span>
              )}
            </>
          }
        >
          <span className="settings-actions">
            {trace.on && (
              <button type="button" className="btn" data-testid="trace-send" disabled={trace.count === 0} onClick={() => void trace.send()}>
                보내기
              </button>
            )}
            <Toggle label="입력 기록" checked={trace.on} onChange={setTrace} />
          </span>
        </SettingsRow>
      </SettingsGroup>
    </>
  );
}

/** 참고 AlertsPage(423~450줄, 문구는 참고 i18n.ko.ts). 무엇을 알릴지(입력 필요·작업 완료)는 기기별 종류(차이 목록 N3)와 함께 */
function AlertsPage() {
  const alertInApp = useAlertSetting("alertInApp");
  const alertSound = useAlertSetting("alertSound");
  const alertSilent = useAlertSetting("alertSilent");
  // the Sound switch as last set: the preview waits for the audio, and must not play once it is off
  const alertSoundWanted = useRef(alertSound);
  return (
    <SettingsGroup note="이 기기에 적용됩니다. 알림은 잠시 모았다가 보내며, 그사이 질문에 답하거나 보고를 확인하면 보내지 않습니다.">
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
                if (ready && alertSoundWanted.current) {
                  beforeAlertSound();
                  previewAlertSound();
                }
              });
          }}
        />
      </SettingsRow>
      {/* aaa(plan §8.83): 참고에 없다. 소리 범주를 바꿀 수 있는 브라우저(iPhone·iPad Safari 16.4+)에서만 */}
      {alertSound && canPlayInSilentMode() && (
        <SettingsRow label="무음 모드에서도" description="iPhone·iPad의 무음 모드에서도 알림 소리를 냅니다. 소리가 나는 동안 음악 같은 다른 앱의 소리가 멈추고, 저절로 다시 재생되지 않습니다.">
          <Toggle
            label="무음 모드에서도"
            checked={alertSilent}
            onChange={(on) => {
              setAlertSetting("alertSilent", on);
              // 켜는 누름이 미리 듣기: 무음 스위치를 켠 채로 들리는지 바로 안다
              if (on)
                void unlockAlertSound().then((ready) => {
                  if (ready && alertSetting("alertSilent")) {
                    beforeAlertSound();
                    previewAlertSound();
                  }
                });
            }}
          />
        </SettingsRow>
      )}
    </SettingsGroup>
  );
}

type SettingsPage = "appearance" | "terminal" | "alerts" | "usage" | "devices";

/** 목록에 보이는 차례(참고 PAGES 198~210줄 가운데 aaa에 있는 쪽) */
export const PAGES: readonly { id: SettingsPage; icon: LucideIcon; label: string }[] = [
  { id: "appearance", icon: Palette, label: "모양" },
  { id: "terminal", icon: SquareTerminal, label: "터미널" },
  { id: "alerts", icon: Bell, label: "알림" },
  { id: "usage", icon: Gauge, label: "구독 사용량" },
  { id: "devices", icon: Smartphone, label: "폰 및 기기" },
];
const label = (id: SettingsPage): string => PAGES.find((p) => p.id === id)!.label;

/** 창 자신의 경계(참고 `.modal` 641px): 그 아래는 목록과 쪽이 번갈아 보인다 */
const NARROW_QUERY = "(max-width: 640px)";

function useNarrow(): boolean {
  const [narrow, setNarrow] = useState(() => window.matchMedia?.(NARROW_QUERY).matches === true);
  useEffect(() => {
    const media = window.matchMedia?.(NARROW_QUERY);
    if (!media) return;
    const refresh = () => setNarrow(media.matches);
    refresh();
    media.addEventListener("change", refresh);
    return () => media.removeEventListener("change", refresh);
  }, []);
  return narrow;
}

/** history의 설정 칸이 보이는 것(이 판에 있는 쪽일 때만) */
function shownBy(entry: SettingsLevel | null): { page: SettingsPage | null; keyBar: boolean } | null {
  if (entry === null) return null;
  const page = PAGES.find(({ id }) => id === entry.page)?.id ?? null;
  return page === null && entry.page !== null ? null : { page, keyBar: entry.keyBar && page === "terminal" };
}

export function SettingsDialog({ onClose, role, device, onSignOut }: { onClose: () => void; role: DeviceRole; device: string; onSignOut: () => Promise<void> }) {
  const narrow = useNarrow();
  // 앞으로 가기로 열렸으면 그 칸이 보이던 것을 보인다
  const [restored] = useState(() => shownBy(settingsEntry(window.history.state)));
  // 휴대폰은 쪽 목록으로 열고, 넓은 창은 목록 옆에 첫 쪽을 보인다
  const [chosen, setChosen] = useState<SettingsPage | null>(restored ? restored.page : null);
  const page = chosen ?? (narrow ? null : PAGES[0]!.id);
  const [keyBarOpen, setKeyBarOpen] = useState(restored?.keyBar ?? false);
  // 한 걸음 들어갈 때마다 history 한 칸: 시스템 「뒤로」가 한 걸음 나온다(herdr/settingsHistory.ts). ←·X·Esc도 같은 칸을 지운다
  useEffect(() => {
    recordSettings(settingsLevels(narrow, page, keyBarOpen));
  }, [narrow, page, keyBarOpen]);
  useEffect(
    () =>
      onSettingsHistory((entry, own) => {
        const view = own ? null : shownBy(entry);
        if (view === null) return;
        setChosen(view.page);
        setKeyBarOpen(view.keyBar);
      }),
    [],
  );
  const keyBarButtonRef = useRef<HTMLButtonElement>(null);
  const backRef = useRef<HTMLButtonElement>(null);
  const tabsRef = useRef<HTMLDivElement>(null);
  const settingsBodyRef = useRef<HTMLDivElement>(null);
  const settingsScrollRef = useRef(0);
  // Tab은 창 안에 머물고, 닫으면 연 것으로 focus가 돌아간다
  const surface = useFocusTrap<HTMLElement>(true, { initialFocus: backRef });
  useSheetSwipe(surface, onClose);
  const shown = useRef<{ page: SettingsPage | null; keyBar: boolean } | null>(null);
  const openPage = (id: SettingsPage): void => {
    setKeyBarOpen(false);
    setChosen(id);
  };
  const openKeyBar = (): void => {
    settingsScrollRef.current = settingsBodyRef.current?.scrollTop ?? 0;
    setKeyBarOpen(true);
  };
  const goBack = (): void => {
    if (keyBarOpen) setKeyBarOpen(false);
    else setChosen(null);
  };
  const tab = (id: SettingsPage | null): HTMLElement | null =>
    tabsRef.current?.querySelector<HTMLElement>(id === null ? '[role="tab"]' : `[data-settings-page="${id}"]`) ?? null;
  useLayoutEffect(() => {
    const before = shown.current;
    shown.current = { page, keyBar: keyBarOpen };
    if (keyBarOpen) backRef.current?.focus();
    else if (before?.keyBar && before.page === page) {
      // 키 바 편집에서 돌아오면 보던 자리와 「키 바 편집」 단추로(참고 1017~1023줄)
      if (settingsBodyRef.current) settingsBodyRef.current.scrollTop = settingsScrollRef.current;
      keyBarButtonRef.current?.focus({ preventScroll: true });
    } else if (before === null) tab(page)?.focus();
    else if (before.page !== page && narrow) {
      // 휴대폰은 둘 중 하나만 보인다: focus가 쪽으로 따라 들어가고, 나오면 그 줄로
      if (page !== null) backRef.current?.focus();
      else tab(before.page)?.focus();
    }
  }, [page, keyBarOpen, narrow]);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent): void => {
      // IME 조합을 끄는 Esc(키 바 편집의 글자 칸)는 IME의 것이다
      if (event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
      event.preventDefault();
      // 키 바 편집이면 먼저 거기서 나오고, 어느 쪽에서든(휴대폰 포함) 창을 닫는다(참고 1052~1061줄)
      if (keyBarOpen) setKeyBarOpen(false);
      else onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose, keyBarOpen]);

  /** 목록은 Tab 한 번에 하나: 화살표가 목록을 걷고, 쪽이 옆에 열려 있으면 쪽도 넘긴다 */
  const onTabKey = (event: ReactKeyboardEvent<HTMLButtonElement>, index: number): void => {
    const to =
      event.key === "ArrowDown" ? index + 1 : event.key === "ArrowUp" ? index - 1 : event.key === "Home" ? 0 : event.key === "End" ? PAGES.length - 1 : null;
    if (to === null) return;
    event.preventDefault();
    const next = PAGES[(to + PAGES.length) % PAGES.length]!.id;
    tab(next)?.focus();
    if (!narrow) openPage(next);
  };

  const body = () => {
    switch (page) {
      case "appearance":
        return <AppearanceSettings />;
      case "terminal":
        return <TerminalPage keyBarButtonRef={keyBarButtonRef} onEditKeyBar={openKeyBar} />;
      case "alerts":
        return <AlertsPage />;
      case "usage":
        return <UsageGroups />;
      case "devices":
        return <DevicesPage role={role} device={device} onSignOut={onSignOut} />;
      default:
        return null;
    }
  };
  const listShown = !narrow || (page === null && !keyBarOpen);
  const focusable = page ?? PAGES[0]!.id;

  // 목록 바닥 줄의 단추 모양을 물려받지 않게 body에 둔다
  return createPortal(
    <div className="modal-scrim" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      {/* 어느 쪽에서든 이름은 「설정」: 쪽 이름은 보이는 제목이다 */}
      <section ref={surface} className="modal settings-dialog" role="dialog" aria-modal="true" aria-label={keyBarOpen ? "키 바" : "설정"} tabIndex={-1}>
        <header className="modal-header settings-header">
          {(keyBarOpen || (narrow && page !== null)) && (
            <button type="button" ref={backRef} className="icon-button" aria-label="설정으로 돌아가기" onClick={goBack}>
              <ArrowLeft aria-hidden="true" />
            </button>
          )}
          <h2 className="modal-title">{keyBarOpen ? "키 바" : page === null ? "설정" : label(page)}</h2>
          <button type="button" className="icon-button" aria-label="설정 닫기" onClick={onClose}>
            <X aria-hidden="true" />
          </button>
        </header>
        {listShown && (
          <nav className="settings-nav" aria-label="설정">
            <p className="settings-nav-title" aria-hidden="true">
              설정
            </p>
            <div ref={tabsRef} className="settings-tabs" role="tablist" aria-orientation="vertical">
              {PAGES.map(({ id, icon: PageIcon, label: name }, index) => (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  className="settings-tab"
                  data-settings-page={id}
                  aria-selected={page === id}
                  aria-controls={page === id ? "settings-panel" : undefined}
                  tabIndex={focusable === id ? 0 : -1}
                  onClick={() => openPage(id)}
                  onKeyDown={(event) => onTabKey(event, index)}
                >
                  <PageIcon aria-hidden="true" />
                  <span>{name}</span>
                  <ChevronRight className="settings-tab-chevron" aria-hidden="true" />
                </button>
              ))}
            </div>
          </nav>
        )}
        {page !== null && (
          <div key={page} ref={settingsBodyRef} id="settings-panel" className="modal-body settings-body" role="tabpanel" aria-label={label(page)} hidden={keyBarOpen}>
            {body()}
          </div>
        )}
        {keyBarOpen && (
          <div className="modal-body settings-key-bar-body">
            <KeyBarSettings />
          </div>
        )}
      </section>
    </div>,
    document.body,
  );
}
