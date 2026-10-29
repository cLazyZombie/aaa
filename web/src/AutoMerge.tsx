// 할 일·thread별 자동 병합(plan §8.530). on이면 thread가 `[병합 준비됨]`으로 보고한 뒤 coordinator가 사용자에게 묻지 않고 착지 창을 준다.
// 값은 할 일 하나에 하나이고 host가 정본이다(thread는 이어진 할 일의 값). 기본 off, 프로젝트 기본값은 없다.
import "./ThreadStageBadge.css";
import "./AutoMerge.css";

import { useEffect, useState } from "react";
import { GitMerge } from "lucide-react";
import { call } from "./api";
import { apiAtLeast } from "./automationModel";
import { Toggle } from "./herdr/SettingsControls";

/** host가 자동 병합을 안다(FRONT_API 1.143) */
export const canAutoMerge = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 143);

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

/** 바로 저장하는 스위치(thread 화면·할 일 상세). 실패하면 원래 값으로 돌리고 까닭을 보인다 */
export function AutoMergeSwitch({ project, target, on, disabled, testid = "auto-merge-switch" }: { project: string; target: string; on: boolean; disabled?: boolean; testid?: string }) {
  const [value, setValue] = useState(on);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // host 사건으로 값이 바뀌면 따른다(다른 기기·CLI에서 바꿈)
  useEffect(() => setValue(on), [on]);
  const change = (next: boolean) => {
    if (busy) return;
    setValue(next);
    setBusy(true);
    setError(null);
    setAutoMerge(project, target, next).then(
      () => setBusy(false),
      (e: unknown) => {
        setValue(!next);
        setBusy(false);
        setError(e instanceof Error ? e.message : String(e));
      },
    );
  };
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
