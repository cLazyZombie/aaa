// 할 일·thread별 자동 병합(plan §8.530). on이면 thread가 `[병합 준비됨]`으로 보고한 뒤 coordinator가 사용자에게 묻지 않고 착지 창을 준다.
// 값은 할 일 하나에 하나이고 host가 정본이다(thread는 이어진 할 일의 값). 기본 off, 프로젝트 기본값은 없다.
import "./ThreadStageBadge.css";
import "./AutoMerge.css";

import { useEffect, useState } from "react";
import { GitMerge } from "lucide-react";
import { call, me } from "./api";
import { apiAtLeast } from "./automationModel";
import { ignoredFailure } from "./errorDiagnostic.ts";
import { Toggle } from "./herdr/SettingsControls";
import { useMediaQuery } from "./herdr/useMediaQuery";
import { Icon } from "./icons";
import type { DeviceRole, ThreadView } from "./gen/proto";

/** host가 자동 병합을 안다(FRONT_API 1.143) */
export const canAutoMerge = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 143);

/** 이 thread 머리줄에서 자동 병합을 켜고 끌 수 있다: 사람이 맡긴 열린 Worker, 조작 기기, 그 값을 아는 host */
export function useAutoMergeSwitchable(t: ThreadView, role: DeviceRole): boolean {
  const [frontApi, setFrontApi] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    me().then((m) => live && setFrontApi(m?.front_api ?? null), ignoredFailure("AutoMerge.tsx:me", () => undefined));
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

/**
 * thread pane 머리줄(t-1968): 제목 줄 「작업 중 N%」 왼쪽. 켜고 끌 수 없는 기기·thread는 켜졌을 때만 표지.
 * 실패 까닭은 머리 아래 오른쪽에 뜨는 띠다(머리 아래로 흐름에 넣으면 터미널 칸이 줄어 PTY 크기가 바뀐다)
 */
export function PaneAutoMerge({ t, role }: { t: ThreadView; role: DeviceRole }) {
  const switchable = useAutoMergeSwitchable(t, role);
  // 휴대폰 머리줄(style.css 700px 규칙)은 아이콘 단추로 줄인다(t-1495 D안과 같다)
  const compact = useMediaQuery("(max-width: 700px)");
  const [error, setError] = useState<string | null>(null);
  if (!switchable) return <AutoMergeBadge on={t.auto_merge} />;
  return (
    <span className="pane-auto-merge">
      <AutoMergeHeadSwitch project={t.project} target={t.id} on={!!t.auto_merge} compact={compact} onError={setError} />
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
