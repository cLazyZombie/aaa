// 이 host의 모든 웹 화면이 함께 쓰는 설정과 고정(plan §8.79). herdr-web-ui `src/lib/preferencesSync.ts`의
// `preferencesChange`·`takePreferences`를 그대로 옮기고, 참고 `settings.ts` `sharedSettings`·`sanitizeSettings` 자리에
// aaa가 가진 공유 설정(모양 theme·palette·sidebarRows·agentOrder, 사용량 막대 다섯, 대화 showThinking·autoSendQueued·quickReplies·showQuickReplies)만 둔다. React·브라우저를 쓰지 않아 node 시험이 바로 부른다.
// 참고 SHARED_SETTING_KEYS 가운데 aaa에 아직 없는 이름(language·agentMarks 등)은 host가 그대로 들고 있고 여기서는 읽지 않는다.
import { SHARED_SETTING_KEYS, type PreferencesChange, type SharedPreferences, type SharedSettingKey } from "./herdr/preferences.ts";
import { sanitizeAppearance, type Appearance } from "./theme.ts";

/** 사용량 막대 설정(참고 settings.ts `showUsage`·`usageCount`·`usageGlance`·`usageOrder`·`usageHidden`) */
export type UsageCount = "used" | "left";
export type UsageGlance = "week" | "session";
export interface UsageSettings {
  showUsage: boolean;
  /** 막대가 보이는 것: 쓴 몫, 남은 몫 */
  usageCount: UsageCount;
  /** 칩이 보이는 한도: 주간, 짧은 세션 */
  usageGlance: UsageGlance;
  /** 계정 key 순서(사용자가 정함) */
  usageOrder: string[];
  /** 숨긴 계정 key */
  usageHidden: string[];
}

/** 참고 DEFAULT_SETTINGS와 같다. 다만 showUsage는 켬: 사용자가 이 막대를 보이라고 했다(10-17, 참고는 끔). */
export const DEFAULT_USAGE: UsageSettings = { showUsage: true, usageCount: "used", usageGlance: "week", usageOrder: [], usageHidden: [] };

const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

/** 잘못된 값은 기본값으로(참고 settings.ts `sanitizeSettings`의 해당 줄) */
export function sanitizeUsage(raw: unknown): UsageSettings {
  const r = raw && typeof raw === "object" && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {};
  return {
    showUsage: typeof r.showUsage === "boolean" ? r.showUsage : DEFAULT_USAGE.showUsage,
    usageCount: r.usageCount === "used" || r.usageCount === "left" ? r.usageCount : DEFAULT_USAGE.usageCount,
    usageGlance: r.usageGlance === "week" || r.usageGlance === "session" ? r.usageGlance : DEFAULT_USAGE.usageGlance,
    usageOrder: strings(r.usageOrder),
    usageHidden: strings(r.usageHidden),
  };
}

/** 대화 보기 공유 설정(참고 settings.ts `showThinking`·`autoSendQueued`·`quickReplies`·`showQuickReplies`: 모두 공유) */
export interface ChatShared {
  /** 대화에 agent의 생각(접힌 「생각」 줄)을 함께 보인다 */
  showThinking: boolean;
  /** 켬: 작업 중에 쓴 메시지도 곧 보낸다(harness가 다음 턴으로 줄 세운다, plan §8.100). 끔: 「지금 보내기」까지 이 기기에 붙잡는다 */
  autoSendQueued: boolean;
  /** 입력창 위 한 번 누름 답(차례대로, 빈 것은 쓰는 동안만 남고 보이지 않는다) */
  quickReplies: string[];
  showQuickReplies: boolean;
}
/** 참고 `QUICK_REPLIES_MAX`·`QUICK_REPLY_MAX_CHARS` */
export const QUICK_REPLIES_MAX = 12;
export const QUICK_REPLY_MAX_CHARS = 200;
/** 참고 DEFAULT_SETTINGS와 같다 */
export const DEFAULT_CHAT_SHARED: ChatShared = {
  showThinking: false,
  autoSendQueued: true,
  quickReplies: ["continue", "yes", "no", "commit and push", "retry"],
  showQuickReplies: false,
};

export function sanitizeChatShared(raw: unknown): ChatShared {
  const r = raw && typeof raw === "object" && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {};
  return {
    showThinking: typeof r.showThinking === "boolean" ? r.showThinking : DEFAULT_CHAT_SHARED.showThinking,
    autoSendQueued: typeof r.autoSendQueued === "boolean" ? r.autoSendQueued : DEFAULT_CHAT_SHARED.autoSendQueued,
    quickReplies: Array.isArray(r.quickReplies)
      ? r.quickReplies.filter((reply): reply is string => typeof reply === "string").slice(0, QUICK_REPLIES_MAX).map((reply) => reply.slice(0, QUICK_REPLY_MAX_CHARS))
      : [...DEFAULT_CHAT_SHARED.quickReplies],
    showQuickReplies: typeof r.showQuickReplies === "boolean" ? r.showQuickReplies : DEFAULT_CHAT_SHARED.showQuickReplies,
  };
}

/**
 * 목록 pane 줄 순서(차이 목록 L9, plan §8.195): 사람이 옮긴 차례의 `machine:pane` 열쇠(고정 열쇠와 같은 꼴). 참고는 herdr workspace
 * 순서라 서버에 있고 모든 화면이 같이 본다: aaa는 그 자리를 host 공유 설정에 둔다. 열쇠가 없는 줄은 host 차례로 그 뒤에 선다
 */
export interface SidebarShared {
  paneOrder: string[];
}
export const PANE_ORDER_MAX = 500;
const PANE_KEY_MAX = 300;

export function sanitizeSidebarShared(raw: unknown): SidebarShared {
  const r = raw && typeof raw === "object" && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {};
  const keys = [...new Set(strings(r.paneOrder).filter((key) => key.length > 0 && key.length <= PANE_KEY_MAX))];
  return { paneOrder: keys.slice(-PANE_ORDER_MAX) };
}

/** 보일 빠른 답(참고 `quickReplyButtons`): 빈 것은 뺀다 */
export function quickReplyButtons(settings: Pick<ChatShared, "quickReplies">): string[] {
  return settings.quickReplies.filter((reply) => reply.trim() !== "");
}

/** aaa가 가진 공유 설정. 나머지(다크·라이트 짝 팔레트·상태 표시 등)는 참고처럼 기기 것이다(SETTING_SCOPE "device") */
export type SharedLocal = Pick<Appearance, "theme" | "palette" | "sidebarRows" | "agentOrder"> & UsageSettings & ChatShared & SidebarShared;

const LOCAL_KEYS = ["theme", "palette", "sidebarRows", "agentOrder", "showUsage", "usageCount", "usageGlance", "usageOrder", "usageHidden", "showThinking", "autoSendQueued", "quickReplies", "showQuickReplies", "paneOrder"] as const satisfies readonly (SharedSettingKey & keyof SharedLocal)[];

/** 참고 `sanitizeSettings`: 읽을 수 있는 값만 남긴다. 모르는 이름은 결과에 없다 */
export function sanitizeShared(raw: Record<string, unknown>): SharedLocal {
  const a = sanitizeAppearance(raw);
  return { theme: a.theme, palette: a.palette, sidebarRows: a.sidebarRows, agentOrder: a.agentOrder, ...sanitizeUsage(raw), ...sanitizeChatShared(raw), ...sanitizeSidebarShared(raw) };
}

/** 참고 `sharedSettings`: 공유 이름의 값만 */
export function sharedSettings(settings: SharedLocal): Partial<Record<SharedSettingKey, unknown>> {
  return Object.fromEntries(LOCAL_KEYS.map((key) => [key, settings[key]]));
}

type Shared = Partial<Record<SharedSettingKey, unknown>>;
/** 이 화면이 마지막으로 안 host의 값. host가 고정을 아직 갖지 않았으면 `pins`는 null */
export interface Baseline {
  settings: Shared;
  pins: readonly string[] | null;
}
/** 이 화면이 가진 값 */
export interface Local {
  settings: SharedLocal;
  pins: readonly string[];
}

const same = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b);

/**
 * `base` 뒤로 여기서 바뀐 것: 달라진 공유 설정, 달라졌으면 고정. 없으면 null.
 * host가 고정을 아직 갖지 않았으면 이 화면의 고정이 첫 변경에 함께 간다(먼저 나누는 화면의 고정을 잃지 않는다).
 */
export function preferencesChange(base: Baseline, local: Local): PreferencesChange | null {
  const now = sharedSettings(local.settings);
  const settings = Object.fromEntries(Object.entries(now).filter(([key, value]) => !same(value, base.settings[key as SharedSettingKey])));
  const pinsChanged = base.pins !== null && !same(local.pins, base.pins);
  if (Object.keys(settings).length === 0 && !pinsChanged) return null;
  return pinsChanged || base.pins === null ? { settings, pins: [...local.pins] } : { settings };
}

export interface Taken {
  base: Baseline;
  /** 여기에 둘 설정 */
  patch: Partial<SharedLocal>;
  /** 여기에 둘 고정. null이면 그대로 */
  pins: string[] | null;
}

/**
 * host 문서가 여기서 하는 일. 문서의 설정 하나하나가 이 화면 값을 바꾼다. 다만 `base` 뒤로 여기서 바꾸고 아직 안 보낸 값
 * (다음에 나가 이긴다)과 이 화면이 읽지 못하는 값(새 판의 값은 다른 화면들 것으로 둔다)은 두지 않는다.
 * `inFlight`는 이 문서가 답하는 변경이다: host가 받은 값은 더는 이 화면만의 것이 아니므로 문서 값이 선다.
 * `force`(보기 권한 기기, 보내지 않는다)면 모두 받는다. baseline은 어느 쪽이든 host 값을 받는다.
 * host가 처음 갖는 고정은 이 화면 고정과 합친다(나누기 시작할 때 아무도 고정을 잃지 않는다). 그 뒤로는 마지막 쓰기가 이긴다.
 */
export function takePreferences(base: Baseline, local: Local, remote: SharedPreferences, inFlight: PreferencesChange | null, force = false): Taken {
  const settings: Shared = { ...base.settings };
  const patch: Record<string, unknown> = {};
  const held = local.settings as unknown as Record<string, unknown>;
  for (const key of SHARED_SETTING_KEYS) {
    if (!(key in remote.settings)) continue;
    const value = remote.settings[key];
    if (!same((sanitizeShared({ ...held, [key]: value }) as unknown as Record<string, unknown>)[key], value)) continue;
    const sent = inFlight !== null && key in inFlight.settings && same(inFlight.settings[key], held[key]);
    const edited = !same(held[key], base.settings[key]) && !sent;
    settings[key] = value;
    if (force || !edited) patch[key] = value;
  }
  let pins: string[] | null = null;
  if (remote.pins !== undefined) {
    if (base.pins === null && !force) pins = [...remote.pins, ...local.pins.filter((pin) => !remote.pins!.includes(pin))];
    else {
      const sent = inFlight?.pins !== undefined && same(inFlight.pins, local.pins);
      const edited = !same(local.pins, base.pins) && !sent;
      if (force || !edited) pins = [...remote.pins];
    }
  }
  return { base: { settings, pins: remote.pins === undefined ? base.pins : [...remote.pins] }, patch: patch as Partial<SharedLocal>, pins };
}
