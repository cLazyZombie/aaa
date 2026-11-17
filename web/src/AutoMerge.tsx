// 할 일·thread별 자동 병합(plan §8.530). on이면 thread가 `[병합 준비됨]`으로 보고한 뒤 coordinator가 사용자에게 묻지 않고 착지 창을 준다.
// 값은 할 일 하나에 하나이고 host가 정본이다(thread는 이어진 할 일의 값). 기본 off, 프로젝트 기본값은 없다.
import "./ThreadStageBadge.css";
import "./AutoMerge.css";

import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { GitMerge } from "lucide-react";
import { call, me } from "./api";
import { apiAtLeast } from "./automationModel";
import { ignoredFailure } from "./errorDiagnostic.ts";
import { Toggle } from "./herdr/SettingsControls";
import { Icon } from "./icons";
import type { DeviceRole, ThreadView } from "./gen/proto";

/** host가 자동 병합을 안다(FRONT_API 1.143) */
export const canAutoMerge = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 143);

let knownFrontApi: string | null = null;
let frontApiOnce: Promise<string | null> | null = null;

/** 이 thread 머리줄에서 자동 병합을 켜고 끌 수 있다: 사람이 맡긴 열린 Worker, 조작 기기, 그 값을 아는 host */
export function useAutoMergeSwitchable(t: ThreadView, role: DeviceRole): boolean {
  // host 판은 탭 하나에 한 번 묻는다: pane을 바꿀 때마다 다시 물으면 답 전까지 표지로 그렸다가 스위치로 바뀌며 머리줄이 밀린다
  const [frontApi, setFrontApi] = useState<string | null>(knownFrontApi);
  useEffect(() => {
    let live = true;
    frontApiOnce ??= me().then((m) => (knownFrontApi = m?.front_api ?? null));
    frontApiOnce.then((v) => live && setFrontApi(v), ignoredFailure("AutoMerge.tsx:me", () => {
      frontApiOnce = null;
    }));
    return () => {
      live = false;
    };
  }, []);
  return role !== "reader" && canAutoMerge(frontApi) && t.role === "worker" && !t.automation && t.status !== "resolved";
}

/** 할 일(`d-0001`·제목) 또는 thread(`t-0001`)의 자동 병합을 정한다. 값이 같으면 host가 그대로 둔다 */
export async function setAutoMerge(project: string, target: string, on: boolean): Promise<void> {
  const r = await call({ op: "auto_merge_set", project, target, on });
  if (r.kind !== "auto_merge") throw new Error(`예상하지 못한 응답: ${r.kind}`);
}

/** 목록·제목 앞 표지: on일 때만 */
export function AutoMergeBadge({ on }: { on?: boolean | null }) {
  if (!on) return null;
  return <span className="thread-stage is-auto-merge" data-testid="auto-merge-badge" title="병합 준비되면 사용자에게 묻지 않고 착지 창">자동 병합</span>;
}

/** 「작업」 목록 줄의 아이콘 표지(t-1293 사용자 답): 단계 표지 옆, 상태 말 앞. on일 때만 */
export function AutoMergeMark({ on }: { on?: boolean | null }) {
  if (!on) return null;
  return (
    <span className="auto-merge-mark" role="img" aria-label="자동 병합" data-testid="auto-merge-mark" title="자동 병합: 병합 준비되면 사용자에게 묻지 않고 착지 창">
      <GitMerge aria-hidden="true" />
    </span>
  );
}

/** 바로 저장하는 값. 실패하면 원래 값으로 돌리고 까닭을 onError로 알린다 */
function useAutoMergeValue(project: string, target: string, on: boolean, onError: (message: string | null) => void) {
  const [value, setValue] = useState(on);
  const [busy, setBusy] = useState(false);
  // host 사건으로 값이 바뀌면 따른다(다른 기기·CLI에서 바꿈)
  useEffect(() => setValue(on), [on]);
  const change = (next: boolean) => {
    if (busy) return;
    setValue(next);
    setBusy(true);
    onError(null);
    setAutoMerge(project, target, next).then(
      () => setBusy(false),
      (e: unknown) => {
        setValue(!next);
        setBusy(false);
        onError(e instanceof Error ? e.message : String(e));
      },
    );
  };
  return { value, busy, change };
}

/** thread 화면 맨 윗줄(t-1495 D안, t-1968부터 pane 머리줄): 넓은 화면은 글자와 스위치, 휴대폰은 아이콘 단추(켜면 청록 바탕). 실패 까닭은 머리 아래 띠(onError)에 */
export function AutoMergeHeadSwitch({ project, target, on, compact, onError }: { project: string; target: string; on: boolean; compact: boolean; onError: (message: string | null) => void }) {
  const { value, busy, change } = useAutoMergeValue(project, target, on, onError);
  if (compact) {
    return (
      <button
        type="button"
        className="ghost small auto-merge-head-btn"
        role="switch"
        aria-checked={value}
        aria-label="자동 병합"
        title="자동 병합"
        disabled={busy}
        data-testid="auto-merge-switch"
        data-on={value ? "true" : "false"}
        onClick={() => change(!value)}
      >
        <GitMerge aria-hidden="true" />
      </button>
    );
  }
  return (
    <span className="auto-merge-switch auto-merge-head" data-testid="auto-merge-switch" data-on={value ? "true" : "false"} data-busy={busy ? "true" : "false"}>
      <span className="auto-merge-label">자동 병합</span>
      <Toggle checked={value} label="자동 병합" disabled={busy} onChange={change} />
    </span>
  );
}

/** 머리줄 폭이 700px 이하다. 화면 폭이 아니라 머리줄 폭으로 본다: 목록이 옆에 붙은 769~1000px 창도 머리줄은 400~630px다 */
function useNarrowHead(ref: RefObject<HTMLElement | null>, on: boolean): boolean {
  const [narrow, setNarrow] = useState(false);
  useLayoutEffect(() => {
    const head = on ? ref.current?.closest<HTMLElement>(".term-head") : null;
    if (!head) return;
    const check = () => setNarrow(head.getBoundingClientRect().width <= 700);
    check();
    const ro = new ResizeObserver(check);
    ro.observe(head);
    return () => ro.disconnect();
  }, [ref, on]);
  return narrow;
}

/**
 * thread pane 머리줄(t-1968): 제목 줄 상태 표시 왼쪽. 켜고 끌 수 없는 기기·thread는 켜졌을 때만 표지.
 * 머리줄이 좁으면(700px 이하) 아이콘으로 줄이고 상태 표시는 진행률 숫자만 둔다(data-narrow, AutoMerge.css, q-4124 1A).
 * 실패 까닭은 머리 아래 오른쪽 끝에 뜨는 띠다(흐름에 넣으면 터미널 칸이 줄어 PTY 크기가 바뀐다)
 */
export function PaneAutoMerge({ t, role }: { t: ThreadView; role: DeviceRole }) {
  const switchable = useAutoMergeSwitchable(t, role);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLSpanElement>(null);
  const shown = switchable || !!t.auto_merge;
  const narrow = useNarrowHead(ref, shown);
  if (!shown) return null;
  return (
    <span className="pane-auto-merge" ref={ref} data-narrow={narrow ? "" : undefined}>
      {!switchable ? (
        narrow ? <AutoMergeMark on /> : <AutoMergeBadge on />
      ) : (
        <AutoMergeHeadSwitch project={t.project} target={t.id} on={!!t.auto_merge} compact={narrow} onError={setError} />
      )}
      {error && (
        <span className="banner err pane-auto-merge-error" role="alert">
          {error}
          <button type="button" className="ghost icon small" onClick={() => setError(null)} aria-label="닫기">
            <Icon name="close" />
          </button>
        </span>
      )}
    </span>
  );
}

/** 바로 저장하는 스위치(할 일 상세). 실패하면 원래 값으로 돌리고 까닭을 보인다 */
export function AutoMergeSwitch({ project, target, on, disabled, testid = "auto-merge-switch" }: { project: string; target: string; on: boolean; disabled?: boolean; testid?: string }) {
  const [error, setError] = useState<string | null>(null);
  const { value, busy, change } = useAutoMergeValue(project, target, on, setError);
  return (
    <span className="auto-merge-switch" data-testid={testid} data-on={value ? "true" : "false"} data-busy={busy ? "true" : "false"}>
      <span className="auto-merge-label">자동 병합</span>
      <Toggle checked={value} label="자동 병합" disabled={disabled || busy} onChange={change} />
      {error && <span className="auto-merge-error" role="alert">{error}</span>}
    </span>
  );
}

/** 확인창의 체크 칸(바로 시작·예약): 고른 값은 확인할 때 저장한다 */
export function AutoMergeCheck({ checked, disabled, onChange, testid }: { checked: boolean; disabled?: boolean; onChange: (on: boolean) => void; testid: string }) {
  return (
    <label className="auto-merge-check">
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} data-testid={testid} />
      <span>자동 병합</span>
    </label>
  );
}
