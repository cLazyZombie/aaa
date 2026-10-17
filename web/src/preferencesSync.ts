// 공유 설정과 고정(preferences.ts)을 이 host의 모든 웹 화면에서 같게 둔다(herdr-web-ui `src/lib/preferencesSync.ts`
// `usePreferencesSync`를 옮김, plan §8.79). 들어온 뒤 한 번 읽어 host 값을 설정마다 이 화면 값 위에 놓는다.
// 아무도 나누지 않은 설정은 이 화면 것으로 남고, 화면을 여는 것만으로는 쓰지 않는다. 여기서 바꾸면 설정이 멈춘 뒤 바뀐 것만 보내고,
// 다른 화면의 변경은 events의 `preferences` 사건으로 온다. host는 events 연결이 열릴 때마다 문서 전체를 보내 다시 붙은 화면도 따라잡는다.
// 설정마다 마지막 쓰기가 이긴다: 여기서 바꾸고 아직 안 보낸 값은 남아, 온 것 위로 나간다.
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { ApiError, loadPreferences, savePreferences } from "./api";
import type { PreferencesChange, SharedPreferences } from "./herdr/preferences.ts";
import { currentSidebarPins, setSidebarPins, subscribeSidebarPins } from "./herdr/sidebarPins.ts";
import { preferencesChange, sanitizeShared, sharedSettings, takePreferences, type Baseline, type Local, type SharedLocal } from "./preferences.ts";
import { updateChatShared, updateUsageSettings, useChatShared, useUsageSettings } from "./settings";
import { updateAppearance, useAppearance } from "./theme";

const SEND_AFTER_MS = 600;
/** 가는 길에 실패한 요청(망, host 재시작)은 이만큼 뒤 다시 보낸다 */
const RETRY_MS = 5_000;

/** 참고 `useSettings()` 자리: aaa의 공유 설정은 모양·사용량·대화 store 셋에 있다 */
function useSharedLocal(): { settings: SharedLocal; update: (patch: Partial<SharedLocal>) => void } {
  const { appearance } = useAppearance();
  const usage = useUsageSettings();
  const chat = useChatShared();
  const settings: SharedLocal = { theme: appearance.theme, palette: appearance.palette, sidebarRows: appearance.sidebarRows, agentOrder: appearance.agentOrder, ...usage, ...chat };
  const update = useCallback((patch: Partial<SharedLocal>) => {
    const { theme, palette, sidebarRows, agentOrder, showThinking, ...rest } = patch;
    const look = Object.fromEntries(Object.entries({ theme, palette, sidebarRows, agentOrder }).filter(([, v]) => v !== undefined));
    if (Object.keys(look).length > 0) updateAppearance(look);
    if (Object.keys(rest).length > 0) updateUsageSettings(rest);
    if (showThinking !== undefined) updateChatShared({ showThinking });
  }, []);
  return { settings, update };
}

export function usePreferencesSync(enabled: boolean): (preferences: SharedPreferences) => void {
  const { settings, update } = useSharedLocal();
  const pins = useSyncExternalStore(subscribeSidebarPins, currentSidebarPins, currentSidebarPins);
  const latest = useRef<Local>({ settings, pins });
  latest.current = { settings, pins };
  // 이 화면이 마지막으로 안 host 값. 문서가 오기 전에는 이 화면 자신의 값
  const base = useRef<Baseline>({ settings: sharedSettings(settings), pins: null });
  const revision = useRef(0);
  // 문서가 왔다(읽기, 또는 events 연결의 첫 사건): 그때부터 보낸다
  const loadedRef = useRef(false);
  const [loaded, setLoaded] = useState(false);
  // 보기 권한 기기: 남이 나눈 것을 받고 보내지 않는다. 제 변경은 다음 공유 변경까지만 간다
  const readOnly = useRef(false);
  const inFlight = useRef<PreferencesChange | null>(null);
  // 변경이 가는 동안 온 가장 새 문서: 그 변경보다 앞인지 뒤인지는 답(revision)이 와야 안다
  const held = useRef<SharedPreferences | null>(null);
  // 보낸 뒤나 실패한 뒤 기다림이 끝나면 보내는 effect를 다시 돌린다
  const [attempt, setAttempt] = useState(0);
  const [loadAttempt, setLoadAttempt] = useState(0);
  // baseline은 ref다: 여기서 바꾸는 것이 없는 문서도 보낼 것을 바꾼다
  const [, setTaken] = useState(0);

  const absorb = useCallback(
    (remote: SharedPreferences) => {
      if (loadedRef.current && remote.revision <= revision.current) return;
      revision.current = Math.max(revision.current, remote.revision);
      const taken = takePreferences(base.current, latest.current, remote, inFlight.current, readOnly.current);
      base.current = taken.base;
      // 같은 tick에 온 둘째 문서는 마지막 그림이 아니라 이 문서가 둔 값과 견준다
      latest.current = {
        settings: sanitizeShared({ ...latest.current.settings, ...taken.patch }),
        pins: taken.pins ?? latest.current.pins,
      };
      if (Object.keys(taken.patch).length > 0) update(taken.patch);
      if (taken.pins !== null) setSidebarPins(taken.pins);
      loadedRef.current = true;
      setLoaded(true);
      setTaken((count) => count + 1);
    },
    [update],
  );

  const receive = useCallback(
    (remote: SharedPreferences) => {
      if (inFlight.current === null) {
        absorb(remote);
        return;
      }
      if (held.current === null || remote.revision > held.current.revision) held.current = remote;
    },
    [absorb],
  );

  // 변경이 답을 받았거나 잃었다: 그동안 온 것을 이제 읽는다(더 옛 문서는 revision으로 버린다)
  const settled = useCallback(() => {
    inFlight.current = null;
    const arrived = held.current;
    held.current = null;
    if (arrived !== null) absorb(arrived);
  }, [absorb]);

  useEffect(() => {
    if (!enabled || loaded) return;
    let cancelled = false;
    let retry: number | undefined;
    loadPreferences().then(
      (remote) => {
        if (!cancelled) receive(remote);
      },
      () => {
        if (!cancelled) retry = window.setTimeout(() => setLoadAttempt((count) => count + 1), RETRY_MS);
      },
    );
    return () => {
      cancelled = true;
      window.clearTimeout(retry);
    };
  }, [enabled, loaded, loadAttempt, receive]);

  const change = loaded ? preferencesChange(base.current, { settings, pins }) : null;
  const pending = change === null ? null : JSON.stringify(change);
  useEffect(() => {
    if (!enabled || !loaded || readOnly.current || pending === null || inFlight.current !== null) return;
    let retry: number | undefined;
    const send = (keepalive: boolean) => {
      window.clearTimeout(timer);
      if (inFlight.current !== null) return;
      const body = JSON.parse(pending) as PreferencesChange;
      inFlight.current = body;
      savePreferences(body, keepalive).then(
        (saved) => {
          // 답은 이 변경이 남긴 문서 전체다: 앞서 다른 곳에서 온 것은 함께 오고, 뒤에 온 것은 붙잡아 두었다가 이제 읽는다
          absorb(saved);
          settled();
          // 가는 동안 바뀐 것을 이제 보낸다
          setAttempt((count) => count + 1);
        },
        (error: unknown) => {
          settled();
          // 참고는 403이면 보기 전용, 404(옛 서버)면 멈춤. aaa 웹은 host binary 안에 있어 옛 host가 없다
          if (error instanceof ApiError && error.body.code === "forbidden") readOnly.current = true;
          else retry = window.setTimeout(() => setAttempt((count) => count + 1), RETRY_MS);
        },
      );
    };
    const timer = window.setTimeout(() => send(false), SEND_AFTER_MS);
    // 바꾸고 곧 탭·앱을 닫거나 다른 앱으로 가면 기다림을 건너뛰고 지금 보낸다(QA t-0337 Q11). 참고는 여기서 잃고,
    // t3code는 떠날 때 미룬 저장을 마저 쓴다(uiStateStore.ts `beforeunload → flush()`). iOS는 beforeunload를 믿을 수 없어
    // pagehide와 가려짐을 듣고, keepalive로 화면이 사라져도 요청이 끝까지 가게 한다.
    const leave = () => send(true);
    const hidden = () => document.visibilityState === "hidden" && send(true);
    window.addEventListener("pagehide", leave);
    document.addEventListener("visibilitychange", hidden);
    return () => {
      window.clearTimeout(timer);
      window.clearTimeout(retry);
      window.removeEventListener("pagehide", leave);
      document.removeEventListener("visibilitychange", hidden);
    };
  }, [enabled, loaded, pending, attempt, absorb, settled]);

  return receive;
}
