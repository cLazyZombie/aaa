// 구독 사용량(plan 한도) 화면 쪽. herdr-web-ui `src/lib/usage.ts`(aa624cb)를 옮겼다.
// 그대로 복사하지 못한 까닭: 참고는 api.ts·settings.ts·i18n·visibility를 부른다. aaa에 맞춰 그 import만 바꾸고
// 문구를 한국어로 옮겼다(함수와 규칙은 같다). chat 입력칸의 `composerUsage`는 aaa에 chat 입력칸이 없어 뺐다.
// 구독은 Claude·Codex 둘이다(사용자 요청 10-17, plan §8.49): PROVIDER_NAME·PROVIDER_MARK도 그 둘만 남았다.
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { Outcome, ProviderUsage, UsageProviderId, UsageReport, UsageWindow, UsageWindowKind } from "./gen/proto";
import type { UsageCount, UsageGlance } from "./settings";

/** host는 구독에 5분에 한 번까지 묻는다. 여기 1분은 그것을 조금 일찍 집을 뿐이다. */
const POLL_MS = 60_000;
/** 이만큼 썼으면 막힘 색으로 보일 만큼 가깝다 */
export const HIGH_PERCENT = 80;

export const PROVIDER_NAME: Readonly<Record<UsageProviderId, string>> = {
  claude: "Claude",
  codex: "Codex",
};

/** AgentMark's name for each provider's logo */
export const PROVIDER_MARK: Readonly<Record<UsageProviderId, string>> = {
  claude: "claude", codex: "codex",
};

export const WINDOW_LABEL: Readonly<Record<UsageWindowKind, string>> = {
  session: "세션",
  day: "일간",
  week: "주간",
  month: "월간",
};

/** host가 구독 이름 그대로 보내는 몫 이름 가운데 옮길 것 */
const SCOPE_LABEL: Readonly<Record<string, string>> = { "Other models": "다른 모델" };

/** 5시간 세션 다음 주간, plan 전체 한도만. 모델 몫은 그 둘을 대신하지 않는다. */
export function statusWindows(usage: ProviderUsage): UsageWindow[] {
  return (["session", "week"] as const)
    .map((kind) => usage.windows.find((window) => window.kind === kind && window.scope === null))
    .filter((window): window is UsageWindow => window !== undefined);
}

/** 가장 바닥에 가까운 한도: plan에 고른 종류의 한도가 없을 때 칩이 보인다. */
export function tightestWindow(usage: ProviderUsage): UsageWindow | null {
  return usage.windows.reduce<UsageWindow | null>(
    (tightest, window) => (tightest === null || window.used_percent > tightest.used_percent ? window : tightest),
    null,
  );
}

/** 칩이 보이는 한도: 설정에서 고른 종류(주간·세션)의 plan 전체 한도. 없으면 바닥에 가장 가까운 것. */
export function glanceWindow(usage: ProviderUsage, glance: UsageGlance): UsageWindow | null {
  return usage.windows.find((window) => window.kind === glance && window.scope === null) ?? tightestWindow(usage);
}

export function windowLabel(window: UsageWindow): string {
  const kind = WINDOW_LABEL[window.kind];
  if (window.scope === null) return kind;
  return `${kind} · ${SCOPE_LABEL[window.scope] ?? window.scope}`;
}

/** 구독과, 알면 누구 계정인지: 「Codex · me@example.com」 */
export function usageName(usage: ProviderUsage): string {
  return usage.account ? `${PROVIDER_NAME[usage.id]} · ${usage.account}` : PROVIDER_NAME[usage.id];
}

/** 막대가 보이는 몫: 쓴 것, 또는 남은 것 */
export function meterPercent(window: UsageWindow, count: UsageCount): number {
  return count === "left" ? Math.round((100 - window.used_percent) * 10) / 10 : window.used_percent;
}

/** 막대 값 글: 「77%」(쓴 몫), 「23% 남음」 */
export function meterText(window: UsageWindow, count: UsageCount): string {
  const value = formatPercent(meterPercent(window, count));
  return count === "left" ? `${value} 남음` : value;
}

/** 다시 시작까지: 「2일 4시간」, 「3시간 12분」, 「12분」. 모르거나 지났으면 null. */
export function formatResetIn(resetsAt: string | null, now: number): string | null {
  if (resetsAt === null) return null;
  const minutes = Math.ceil((Date.parse(resetsAt) - now) / 60_000);
  if (!Number.isFinite(minutes) || minutes <= 0) return null;
  if (minutes < 60) return `${minutes}분`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}시간 ${minutes % 60}분`;
  return `${Math.floor(hours / 24)}일 ${hours % 24}시간`;
}

/** 막대의 퍼센트 글: 정수. 0보다 조금 큰 값만 소수 한 자리(0으로 보이지 않게) */
export function formatPercent(value: number): string {
  return value > 0 && value < 1 ? `${value.toFixed(1)}%` : `${Math.round(value)}%`;
}

/** 사용자가 정한 순서(`order`, key)의 계정, 그다음 나머지는 host가 늘어놓은 대로 */
export function orderProviders(providers: readonly ProviderUsage[], order: readonly string[] = []): ProviderUsage[] {
  const rank = new Map(order.map((key, index) => [key, index]));
  return [...providers].sort((a, b) => {
    const ranked = [rank.get(a.key), rank.get(b.key)];
    if (ranked[0] !== undefined && ranked[1] !== undefined) return ranked[0] - ranked[1];
    if (ranked[0] !== undefined || ranked[1] !== undefined) return ranked[0] !== undefined ? -1 : 1;
    return 0;
  });
}

/**
 * 보이는 계정들(`shown`, 지금 순서) 가운데 `key`를 한 칸 위나 아래로 옮긴 순서. 기억하지만 지금 알리지 않는
 * 계정은 그 뒤에 자리를 지킨다.
 */
export function moveInOrder(shown: readonly string[], saved: readonly string[], key: string, by: -1 | 1): string[] {
  const next = [...shown];
  const from = next.indexOf(key);
  const to = from + by;
  if (from < 0 || to < 0 || to >= next.length) return [...shown, ...saved.filter((known) => !shown.includes(known))];
  [next[from], next[to]] = [next[to]!, next[from]!];
  return [...next, ...saved.filter((known) => !shown.includes(known))];
}

async function fetchUsage(refresh: boolean): Promise<UsageReport> {
  const res = await fetch(`/api/v1/usage${refresh ? "?refresh=1" : ""}`, { credentials: "same-origin", cache: "no-store" });
  const out = (await res.json()) as Outcome<UsageReport>;
  if ("err" in out) throw new Error(out.err.message);
  return out.ok;
}

/** 화면이 보이는지(참고 lib/visibility.ts usePageVisible) */
function usePageVisible(): boolean {
  return useSyncExternalStore(
    useCallback((f: () => void) => {
      document.addEventListener("visibilitychange", f);
      return () => document.removeEventListener("visibilitychange", f);
    }, []),
    () => document.visibilityState === "visible",
  );
}

export function useUsage(enabled: boolean) {
  const [report, setReport] = useState<UsageReport | null>(null);
  const [loading, setLoading] = useState(false);
  const visible = usePageVisible();
  const generation = useRef(0);

  const load = useCallback(async (refresh: boolean) => {
    const current = ++generation.current;
    setLoading(true);
    try {
      const next = await fetchUsage(refresh);
      if (current === generation.current)
        setReport((previous) => (JSON.stringify(previous) === JSON.stringify(next) ? previous : next));
    } catch {
      // 끊겼거나 다시 뜨는 중: 마지막 보고를 둔다
    } finally {
      if (current === generation.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) {
      generation.current++;
      setReport(null);
      setLoading(false);
      return;
    }
    if (!visible) return;
    void load(false);
    const timer = setInterval(() => void load(false), POLL_MS);
    return () => clearInterval(timer);
  }, [enabled, visible, load]);

  const refresh = useCallback(() => void load(true), [load]);
  return { report, loading, refresh };
}
