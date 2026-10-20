// 설정 창. 목록 아래 톱니 「설정」이 연다(herdr-web-ui aa624cb MachineSidebar 바닥 줄 → SettingsDialog, plan §8.97).
// 참고 OpenSettingsDialog(954~1110줄)대로 쪽 목록과 쪽이다: 641px부터는 왼쪽 목록 옆에 쪽, 휴대폰은 목록 → 쪽(← 로 돌아감).
// 쪽마다 history 한 칸(herdr/settingsHistory.ts): 시스템 「뒤로」가 한 칸씩 나온다. Tab은 창 안에 머물고(useFocusTrap) 닫으면 연 곳으로
// focus가 돌아간다. 휴대폰은 아래로 밀어 닫는다(useSheetSwipe). 묶음·줄·스위치·여럿 중 하나·한 칸씩은 참고 SettingsControls 그대로다.
// 쪽은 aaa가 뒷받침하는 것만 둔다: 모양·터미널·알림·구독 사용량·단축키·폰 및 기기. 참고의 채팅·음성 입력·원격 PC·에이전트 연동·정보는
// 그 기능이 aaa에 없어 뺐다. 「터미널 글꼴」(FontFamilyInput 72~205줄)은 적은 목록을 기본 글꼴 앞에 붙인다(plan §8.64).
// 「키 바」 줄의 「키 바 편집」은 같은 창 안에서 편집 화면(herdr/KeyBarSettings)으로 바꾸고 ← 로 돌아온다(참고 984~1116줄).
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type RefObject } from "react";
import { createPortal } from "react-dom";
import { ArrowLeft, Bell, ChevronDown, ChevronRight, ChevronUp, Eye, EyeOff, FolderCog, FolderGit2, Gauge, Keyboard, KeyRound, MessageSquare, Palette, Plus, Search, Server, ShieldCheck, SlidersHorizontal, Smartphone, SquareTerminal, X, type LucideIcon } from "lucide-react";
import "./herdr/SettingsDialog.css";
import type { DeviceRole, MachineView, ProviderUsage, PublishState } from "./gen/proto";
import { call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { DevicesPanel } from "./DevicesPanel";
import { ReposPage, ThisAaaPage } from "./ProjectSettingsPages";
import { AccountsPage } from "./AccountsPage";
import { MachinesPage, PublishControls } from "./AdminPages";
import { ProfilesPage, SafetyPage } from "./SettingsAdminPages";
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
  setDefaultView,
  updateChatShared,
  useChatShared,
  useDefaultView,
  MARKDOWN_WIDTHS,
  setMarkdownWidth,
  useMarkdownWidth,
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
  alertSetting,
  useAlertSetting,
  useTerminalFontFamily,
  useTerminalFontSize,
  useTerminalInputMode,
  useTerminalToggle,
  useMedia,
  useTerminalWheelSpeed,
  useUsageSettings,
  AAA_SHORTCUTS,
  setShortcutOverrides,
  useShortcutOverrides,
  type AaaShortcutId,
} from "./settings";
import { forgetPaneViews } from "./paneLens";
import { CHAT_FONT_MAX, CHAT_FONT_MIN, chatFontSize, updateAppearance, useAppearance } from "./theme";
import { DEFAULT_CHAT_SHARED, QUICK_REPLIES_MAX, QUICK_REPLY_MAX_CHARS } from "./preferences";
import { requestedSettingsPage, requestSettingsPage } from "./palette";
import { bindingDisplayKeys, formatKeys, isMacPlatform, setShortcutRecording, shortcutConflict, shortcutDisplayKeys, shortcutKeys } from "./herdr/shortcuts.ts";
import { MENU_SHORTCUT_KEYS, recordShortcut, shortcutMatchesQuery } from "./herdr/shortcutRecorder.ts";
import { exactChordRisk, isReservedShortcutKey, parseExactChord } from "./herdr/shortcutBindings.ts";
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
          {role !== "reader" && <PublishControls publish={publish} onChanged={setPublish} />}
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

/**
 * 참고 ChatPage(300~370줄) 가운데 aaa에 있는 것: 「패널 기본 화면」(이 기기, 바꾸면 pane마다 고른 보기를 잊는다, 참고 그대로),
 * 「생각 표시」(공유), 「입력창」(Enter로 보내기: 이 기기, 터치 화면은 「Return으로 줄바꿈」으로 뒤집어 묻는다. 대기 메시지 자동
 * 전송: 공유), 「빠른 답장」(공유), 「파일 뷰어」(긴 줄 바꿈·Markdown 너비: 이 기기). 「비교 보기 방식」은 aaa에 저장소 비교 보기
 * (참고 RepoDiffViewer)가 없어 뺐다. 채팅 글꼴·폭·제안 칩은 그 기능을 옮길 때 더한다.
 */
function ChatPage() {
  const view = useDefaultView();
  const { showThinking, autoSendQueued, quickReplies, showQuickReplies } = useChatShared();
  const enterSends = useTerminalToggle("enterSends");
  const wrapCode = useTerminalToggle("wrapCode");
  const markdownWidth = useMarkdownWidth();
  const touch = useMedia("(pointer: coarse)");
  const { appearance } = useAppearance();
  return (
    <>
      <SettingsGroup>
        <SettingsRow
          label="패널 기본 화면"
          description="이 기기의 모든 패널에 적용됩니다. 패널에서 화면을 바꾸면 이 설정을 다시 바꿀 때까지 그 화면으로 열립니다. 자동: 터치 화면의 에이전트는 채팅, 그 밖에는 터미널."
          wide
        >
          <Segmented
            label="패널 기본 화면"
            value={view}
            options={[
              { value: "auto", label: "자동" },
              { value: "chat", label: "채팅" },
              { value: "terminal", label: "터미널" },
            ]}
            onChange={(next) => {
              if (next === view) return;
              // 모든 pane에 한 선택: pane마다 기억한 보기는 이것에 자리를 내준다
              forgetPaneViews();
              setDefaultView(next);
            }}
          />
        </SettingsRow>
        <SettingsRow label="생각 표시" description="에이전트의 추론 블록을 함께 보여줍니다">
          <Toggle label="생각 표시" checked={showThinking} onChange={(on) => updateChatShared({ showThinking: on })} />
        </SettingsRow>
        {/* 참고 「Chat font size」(이 기기, 차이 목록 T4): 고르지 않으면 인터페이스 크기를 따른다 */}
        <SettingsRow label="채팅 글자 크기" description="채팅 화면의 메시지, 코드, 프롬프트 카드, 입력창">
          <Stepper
            label="채팅 글자 크기"
            value={chatFontSize(appearance)}
            text={`${chatFontSize(appearance)}px`}
            min={CHAT_FONT_MIN}
            max={CHAT_FONT_MAX}
            decreaseLabel="채팅 글자 작게"
            increaseLabel="채팅 글자 크게"
            onChange={(size) => updateAppearance({ chatFontSize: size })}
          />
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup title="입력창">
        <SettingsRow label={touch ? "Return으로 줄바꿈" : "Enter로 보내기"} description={touch ? "켜면 전송 버튼이나 하드웨어 키보드의 Mod+Enter로 전송합니다." : "끄면 Mod+Enter로 보냅니다"}>
          <Toggle label={touch ? "Return으로 줄바꿈" : "Enter로 보내기"} checked={touch ? !enterSends : enterSends} onChange={(checked) => setTerminalToggle("enterSends", touch ? !checked : checked)} />
        </SettingsRow>
        <SettingsRow label="대기 메시지 자동 전송" description="에이전트가 작업 중일 때 보낸 메시지는 턴이 끝나면 저절로 전송됩니다. 끄면 지금 보내기를 누를 때까지 기다립니다.">
          <Toggle label="대기 메시지 자동 전송" checked={autoSendQueued} onChange={(on) => updateChatShared({ autoSendQueued: on })} />
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup title="빠른 답장">
        <SettingsRow label="입력창 위에 표시" description="메시지 입력창 위에 놓이는 한 번 탭 메시지로, 이 기기에 적용됩니다. 직접 입력한 것처럼 보내므로 에이전트가 작업 중이면 대기열에 들어가고, 질문이 열려 있으면 답으로 쓰입니다.">
          <Toggle label="입력창 위에 표시" checked={showQuickReplies} onChange={(on) => updateChatShared({ showQuickReplies: on })} />
        </SettingsRow>
        <div className="settings-item">
          <ol className="quick-replies-list">
            {quickReplies.map((reply, index) => (
              <li key={index}>
                <input
                  className="input"
                  value={reply}
                  maxLength={QUICK_REPLY_MAX_CHARS}
                  aria-label={`빠른 답장 ${index + 1}`}
                  spellCheck={false}
                  autoCapitalize="off"
                  autoCorrect="off"
                  onChange={(event) => updateChatShared({ quickReplies: quickReplies.map((current, at) => (at === index ? event.target.value : current)) })}
                />
                <button type="button" className="icon-button" aria-label={`빠른 답장 ${index + 1} 삭제`} onClick={() => updateChatShared({ quickReplies: quickReplies.filter((_, at) => at !== index) })}>
                  <X aria-hidden="true" />
                </button>
              </li>
            ))}
          </ol>
          <div className="settings-actions">
            <button type="button" className="btn" disabled={quickReplies.length >= QUICK_REPLIES_MAX} onClick={() => updateChatShared({ quickReplies: [...quickReplies, ""] })}>
              <Plus aria-hidden="true" />
              답장 추가
            </button>
            <button type="button" className="btn btn-ghost" onClick={() => updateChatShared({ quickReplies: [...DEFAULT_CHAT_SHARED.quickReplies] })}>
              기본값으로 되돌리기
            </button>
          </div>
        </div>
      </SettingsGroup>

      <SettingsGroup title="파일 뷰어">
        <SettingsRow label="긴 줄 바꿈">
          <Toggle label="긴 줄 바꿈" checked={wrapCode} onChange={(on) => setTerminalToggle("wrapCode", on)} />
        </SettingsRow>
        <SettingsRow label="Markdown 너비">
          <Segmented label="Markdown 너비" value={markdownWidth} onChange={setMarkdownWidth} options={MARKDOWN_WIDTHS.map((width) => ({ value: width, label: width === "full" ? "전체 너비" : "기본값" }))} />
        </SettingsRow>
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
  const paneClipboard = useTerminalToggle("paneClipboard");
  const releasePaneAway = useTerminalToggle("releasePaneAway");
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
        {/* 참고 SettingsDialog 413~418줄(문구는 참고 i18n.ko.ts) */}
        <SettingsRow
          label="패널에서 클립보드로 복사"
          description="패널에서 복사하는 프로그램(vim, tmux, Claude Code)은 직접 복사한 것과 같이 이 기기의 클립보드에 텍스트를 넣습니다. 믿을 수 없는 출력을 실행하는 패널이 있다면 끄세요. 다음에 붙여 넣을 내용이 바뀔 수 있습니다."
        >
          <Toggle label="패널에서 클립보드로 복사" checked={paneClipboard} onChange={(on) => setTerminalToggle("paneClipboard", on)} />
        </SettingsRow>
        {/* herdr-web-ui releasePaneAway: 다른 웹 화면이 패널 크기를 유지하게 한다. */}
        <SettingsRow
          label="다른 웹 화면과 같이 쓰기"
          description="같은 패널을 다른 기기나 브라우저 창에서도 쓴다면 켜세요. 다른 창으로 옮기고 1초가 지나면 이 탭은 크기 조절을 놓고 보기만 합니다. 다른 웹 화면이 자기 크기를 유지할 수 있습니다. 다시 조작하려면 패널을 클릭하세요. 보는 동안 패널이 이 창을 다 채우지 않을 수 있습니다."
        >
          <Toggle label="다른 웹 화면과 같이 쓰기" checked={releasePaneAway} onChange={(on) => setTerminalToggle("releasePaneAway", on)} />
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

/** 참고 AlertsPage(423~450줄, 문구는 참고 i18n.ko.ts). 무엇을 알릴지(입력 필요·작업 완료)는 기기마다(차이 목록 N3) */
function AlertsPage() {
  const alertInput = useAlertSetting("alertInput");
  const alertDone = useAlertSetting("alertDone");
  const alertInApp = useAlertSetting("alertInApp");
  const alertSound = useAlertSetting("alertSound");
  const alertSilent = useAlertSetting("alertSilent");
  // the Sound switch as last set: the preview waits for the audio, and must not play once it is off
  const alertSoundWanted = useRef(alertSound);
  return (
    <SettingsGroup note="이 기기에 적용됩니다. 알림은 잠시 모았다가 보내며, 그사이 질문에 답하거나 보고를 확인하면 보내지 않습니다.">
      <PushTestControls onEnable={enableAlerts} />
      <SettingsRow label="입력 필요" description="에이전트가 답이나 권한을 기다릴 때">
        <Toggle label="입력 필요" checked={alertInput} onChange={(on) => setAlertSetting("alertInput", on)} />
      </SettingsRow>
      {/* 참고는 끄기·긴 작업·매번이다. aaa의 작업 완료는 thread가 낸 새 보고(일 한 단위)라 길이로 거르지 않는다(plan §8.119) */}
      <SettingsRow label="작업 완료" description="thread가 새 보고를 내고 검토를 기다릴 때">
        <Toggle label="작업 완료" checked={alertDone} onChange={(on) => setAlertSetting("alertDone", on)} />
      </SettingsRow>
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

/** 단축키 이름(참고 SHORTCUTS label의 i18n.ko.ts 문구) */
const SHORTCUT_LABEL: Record<AaaShortcutId, string> = {
  palette: "명령 팔레트",
  // 참고 「새 워크스페이스」: aaa에서 그 자리는 pane이다(새 pane 창)
  "new-session": "새 pane",
  "toggle-view": "채팅 / 터미널 전환",
  "focus-input": "패널 입력창으로 이동",
  "scroll-up": "패널 위로 스크롤",
  "scroll-down": "패널 아래로 스크롤",
  // 참고 i18n.ko.ts 「Toggle sidebar」(활동 막대가 생겨 aaa에도 있다, plan §8.240)
  "toggle-sidebar": "사이드바 토글",
  "previous-pane": "이전 패널",
  "next-pane": "다음 패널",
  settings: "설정",
};

/** 참고 SettingsDialog `compactKeys`: ⌘+⇧+K처럼 한 줄로 */
function compactKeys(keys: readonly string[]): string {
  return formatKeys(keys)
    .map((key) => ({ Shift: "⇧", ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→" })[key] ?? (key.length === 1 ? key.toUpperCase() : key))
    .join("+");
}

/**
 * 설정 → 단축키 쪽(참고 SettingsDialog `ShortcutsPage` 701~843줄 배치 그대로: 찾기 칸, 묶음, 초기화. 문구는 i18n.ko.ts). 줄마다 「기록」으로 누른 조합을 받고, 목록에서
 * Mod+Shift 키나 「끄기」(터미널로 보냄)를 고른다. 터미널·브라우저도 쓰는 조합은 「그래도 저장」을 물은 뒤 저장한다. aaa에 그 화면이 있는
 * 동작만 보인다(settings.ts `AAA_SHORTCUTS`).
 */
function ShortcutsPage() {
  const overrides = useShortcutOverrides();
  const platformIsMac = isMacPlatform();
  const [query, setQuery] = useState("");
  const [recording, setRecording] = useState<{ id: AaaShortcutId; problem: string | null } | null>(null);
  // 터미널이나 브라우저도 쓰는 조합은 「그래도 저장」·「취소」를 기다린다
  const [pending, setPending] = useState<{ id: AaaShortcutId; value: string; risk: "terminal" | "browser" } | null>(null);
  // 「그래도 저장」·「취소」 뒤 focus는 그 줄의 「기록」 단추로
  const recordButtons = useRef(new Map<AaaShortcutId, HTMLButtonElement>());
  const settlePending = (id: AaaShortcutId) => {
    setPending(null);
    recordButtons.current.get(id)?.focus();
  };
  const recordingId = recording?.id ?? null;
  const riskText = (risk: "terminal" | "browser", keys: string) =>
    risk === "terminal"
      ? `${keys} 키는 터미널 키입니다. 셸, 편집기, 에이전트가 사용하며, 저장하면 더 이상 패널로 전달되지 않습니다.`
      : `브라우저나 운영체제가 ${keys} 입력을 가로챌 수 있습니다.`;
  // 줄이 기록하는 동안 누른 조합은 여기서 읽고 앱 단축키는 쉰다
  useEffect(() => {
    if (recordingId === null) return;
    setShortcutRecording(true);
    const onKeyDown = (event: KeyboardEvent) => {
      const result = recordShortcut(event, platformIsMac);
      if (result.kind === "ignore") return;
      event.preventDefault();
      event.stopPropagation();
      if (result.kind === "cancel") return setRecording(null);
      if (result.kind === "invalid") {
        const problem =
          result.reason === "chord"
            ? platformIsMac
              ? "⌘, Ctrl 또는 ⌥를 누른 채 키를 누르세요. Shift는 함께 눌러도 되고 안 눌러도 됩니다."
              : "Ctrl 또는 Alt를 누른 채 키를 누르세요. Shift는 함께 눌러도 되고 안 눌러도 됩니다."
            : result.reason === "key"
              ? "이 키는 단축키로 쓸 수 없습니다."
              : "브라우저나 운영 체제가 이 단축키를 사용합니다.";
        return setRecording({ id: recordingId, problem });
      }
      const value = result.kind === "key" ? result.key : result.value;
      if (shortcutConflict(recordingId, [value], overrides, platformIsMac)) return setRecording({ id: recordingId, problem: "이미 지정됨" });
      setRecording(null);
      if (result.kind === "exact" && result.risk !== null) return setPending({ id: recordingId, value, risk: result.risk });
      setShortcutOverrides({ ...overrides, [recordingId]: value });
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      setShortcutRecording(false);
    };
  }, [recordingId, platformIsMac, overrides]);
  const visible = AAA_SHORTCUTS.filter((id) => {
    const keys = shortcutDisplayKeys(id, overrides);
    return shortcutMatchesQuery(SHORTCUT_LABEL[id], `${formatKeys(keys).join(" ")} ${compactKeys(keys)}`, query);
  });
  return (
    <>
      <input
        type="search"
        className="input settings-shortcut-search"
        value={query}
        placeholder="단축키 검색"
        aria-label="단축키 검색"
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => setQuery(event.target.value)}
      />
      {visible.length === 0 && (
        <p className="settings-hint" role="status">
          일치하는 단축키가 없습니다.
        </p>
      )}
      <SettingsGroup className="settings-shortcuts" section="shortcuts" note="단축키는 이 브라우저와 기기에 적용됩니다. Mod+Shift 조합은 터미널 키를 그대로 남겨 두며, 기록 기능으로는 Ctrl, Alt 또는 (Mac에서는) ⌘를 쓰는 다른 조합도 누른 그대로 저장할 수 있습니다. 포커스된 입력란의 텍스트 선택은 기본 동작을 따릅니다. Tab/목록 키는 UI 안에서만 작동하며 전역 단축키가 아닙니다. 모바일 키 바는 앱 동작이 아닌 터미널 키를 보냅니다.">
        {visible.map((id) => {
          const label = SHORTCUT_LABEL[id];
          const displayedKeys = shortcutDisplayKeys(id, overrides);
          const selectedKey = displayedKeys[displayedKeys.length - 1];
          const override = Object.hasOwn(overrides, id) ? overrides[id] : null;
          const exact = override ? parseExactChord(override) : null;
          const selectedRisk = exact ? exactChordRisk(exact, platformIsMac) : selectedKey !== undefined && isReservedShortcutKey(selectedKey, platformIsMac) ? "browser" : null;
          const confirming = pending?.id === id ? pending : null;
          const defaultKeys = shortcutDisplayKeys(id, {});
          const defaultKey = defaultKeys[defaultKeys.length - 1];
          const defaultReserved = defaultKey !== undefined && isReservedShortcutKey(defaultKey, platformIsMac);
          const rowText =
            recording?.id === id
              ? (recording.problem ?? "새 단축키를 누르거나 Escape로 취소하세요.")
              : confirming
                ? riskText(confirming.risk, compactKeys(bindingDisplayKeys(confirming.value)))
                : selectedRisk
                  ? riskText(selectedRisk, compactKeys(displayedKeys))
                  : undefined;
          return (
            <SettingsRow key={id} label={label} wide description={rowText === undefined ? undefined : <span role="status">{rowText}</span>}>
              <span className="settings-shortcut-control">
                <button
                  type="button"
                  className="btn settings-shortcut-record"
                  ref={(button) => {
                    if (button) recordButtons.current.set(id, button);
                    else recordButtons.current.delete(id);
                  }}
                  aria-pressed={recording?.id === id}
                  aria-label={`${label}의 단축키 기록`}
                  onClick={() => {
                    setPending(null);
                    setRecording(recording?.id === id ? null : { id, problem: null });
                  }}
                >
                  {recording?.id === id ? "키를 누르세요…" : "기록"}
                </button>
                {confirming ? (
                  <>
                    <button
                      type="button"
                      className="btn settings-shortcut-confirm"
                      onClick={() => {
                        settlePending(confirming.id);
                        setShortcutOverrides({ ...overrides, [confirming.id]: confirming.value });
                      }}
                    >
                      그래도 저장
                    </button>
                    <button type="button" className="btn" onClick={() => settlePending(confirming.id)}>
                      취소
                    </button>
                  </>
                ) : (
                  <select
                    className="select settings-select settings-shortcut-select"
                    aria-label={label}
                    value={Object.hasOwn(overrides, id) ? (overrides[id] ?? "off") : "default"}
                    onChange={(event) => {
                      const next = { ...overrides };
                      if (event.target.value === "default") delete next[id];
                      else next[id] = event.target.value === "off" ? null : event.target.value;
                      setShortcutOverrides(next);
                    }}
                  >
                    <option value="default" title="기본값" disabled={shortcutConflict(id, shortcutKeys(id, {}), overrides)}>
                      {compactKeys(defaultKeys)}
                      {defaultReserved ? " — 예약됨" : ""}
                    </option>
                    <option value="off" title="키를 터미널로 전달">
                      끄기
                    </option>
                    {/* 목록에 없는 기록 조합(정확한 조합, Home·End·문장 부호)은 그 자체로 둔다 */}
                    {override && (exact || !MENU_SHORTCUT_KEYS.includes(override)) && <option value={override}>{compactKeys(displayedKeys)}</option>}
                    {MENU_SHORTCUT_KEYS.map((key) => {
                      const conflict = shortcutConflict(id, [key], overrides);
                      const reserved = isReservedShortcutKey(key, platformIsMac);
                      return (
                        <option key={key} value={key} disabled={conflict}>
                          {compactKeys(["Mod", "Shift", key])}
                          {reserved ? " — 예약됨" : ""}
                          {conflict ? " — 이미 지정됨" : ""}
                        </option>
                      );
                    })}
                  </select>
                )}
              </span>
            </SettingsRow>
          );
        })}
      </SettingsGroup>
      <div className="settings-actions">
        <button
          type="button"
          className="btn"
          onClick={() => {
            setPending(null);
            setRecording(null);
            setShortcutOverrides({});
          }}
        >
          단축키 초기화
        </button>
      </div>
    </>
  );
}

type SettingsPage = "appearance" | "chat" | "terminal" | "alerts" | "usage" | "accounts" | "safety" | "profiles" | "shortcuts" | "devices" | "machines" | "project" | "repos";

/** 목록에 보이는 차례(참고 PAGES 198~210줄 가운데 aaa에 있는 쪽) */
export const PAGES: readonly { id: SettingsPage; icon: LucideIcon; label: string }[] = [
  { id: "appearance", icon: Palette, label: "모양" },
  { id: "chat", icon: MessageSquare, label: "채팅" },
  { id: "terminal", icon: SquareTerminal, label: "터미널" },
  { id: "alerts", icon: Bell, label: "알림" },
  { id: "usage", icon: Gauge, label: "구독 사용량" },
  { id: "accounts", icon: KeyRound, label: "계정" },
  { id: "safety", icon: ShieldCheck, label: "안전" },
  { id: "profiles", icon: SlidersHorizontal, label: "profile" },
  { id: "shortcuts", icon: Keyboard, label: "단축키" },
  { id: "devices", icon: Smartphone, label: "폰 및 기기" },
  { id: "machines", icon: Server, label: "머신" },
  { id: "project", icon: FolderCog, label: "이 aaa" },
  { id: "repos", icon: FolderGit2, label: "저장소" },
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

export function SettingsDialog({
  onClose,
  role,
  device,
  machines,
  onSignOut,
}: {
  onClose: () => void;
  role: DeviceRole;
  device: string;
  /** host `state` 사건의 머신(연결·폐기가 바로 보인다) */
  machines: MachineView[];
  onSignOut: () => Promise<void>;
}) {
  const narrow = useNarrow();
  // 앞으로 가기로 열렸으면 그 칸이 보이던 것을 보인다
  const [restored] = useState(() => shownBy(settingsEntry(window.history.state)));
  // 휴대폰은 쪽 목록으로 열고, 넓은 창은 목록 옆에 첫 쪽을 보인다
  // 명령 팔레트가 고른 쪽(참고 paletteSettings `requestedSettingsPage`: 열릴 때 한 번 읽고 지운다)
  const [chosen, setChosen] = useState<SettingsPage | null>(() => {
    const requested = requestedSettingsPage();
    requestSettingsPage(null);
    return restored ? restored.page : requested;
  });
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
      case "chat":
        return <ChatPage />;
      case "terminal":
        return <TerminalPage keyBarButtonRef={keyBarButtonRef} onEditKeyBar={openKeyBar} />;
      case "alerts":
        return <AlertsPage />;
      case "usage":
        return <UsageGroups />;
      case "accounts":
        return <AccountsPage role={role} />;
      case "safety":
        return <SafetyPage role={role} />;
      case "profiles":
        return <ProfilesPage role={role} />;
      case "shortcuts":
        return <ShortcutsPage />;
      case "devices":
        return <DevicesPage role={role} device={device} onSignOut={onSignOut} />;
      case "machines":
        return <MachinesPage role={role} machines={machines} />;
      case "project":
        return <ThisAaaPage role={role} />;
      case "repos":
        return <ReposPage role={role} />;
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
