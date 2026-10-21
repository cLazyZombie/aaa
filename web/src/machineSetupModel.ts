// 설정 「머신」의 「SSH로 추가」 규칙(웹 머신 추가 M2, plan §8.384). 접속·host key·감지의 정본은 host다(machine_setup.rs): 화면은
// 작업 단계를 사람 말로 옮기고 고를 수 있는 것만 보인다. 원격에 쓰는 설치(M3)는 아직 없다.
import { apiAtLeast } from "./automationModel.ts";
import type { MachineDetected, MachineSetupPhase, MachineSetupView } from "./gen/proto";
import { archLabel, osLabel } from "./machineDetail.ts";

/** 1.93 이상 host만 SSH 추가 요청을 안다 */
export const canSsh = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 93);

export const PHASE: Record<MachineSetupPhase, string> = {
  connecting: "접속 중",
  trust: "지문 확인 필요",
  detecting: "OS 감지 중",
  choose: "OS 고르기 필요",
  ready: "준비됨",
  unsupported: "지원 안 함",
  failed: "실패",
};

/** 사람이 해야 할 단계(카드를 강조한다) */
export const needsPerson = (p: MachineSetupPhase): boolean => p === "trust" || p === "choose";
export const busy = (p: MachineSetupPhase): boolean => p === "connecting" || p === "detecting";

/** host `parse_target`의 가벼운 앞 검사: 공백·따옴표·옵션 꼴·비밀번호(`user:pw@`)는 보내기 전에 막는다 */
export function targetProblem(raw: string): string | null {
  const t = raw.trim();
  if (t === "") return "SSH 대상을 넣는다";
  if (t.startsWith("-")) return "-로 시작할 수 없다";
  if (!/^[A-Za-z0-9._@:[\]%-]+$/.test(t)) return "공백·따옴표 같은 글자는 쓸 수 없다";
  const at = t.lastIndexOf("@");
  if (at >= 0 && t.slice(0, at).includes(":")) return "비밀번호는 넣지 않는다(host의 SSH 키·설정만 쓴다)";
  return null;
}

/** release 대상(host `TARGETS`와 같다) */
export const TARGETS: { os: string; arch: string }[] = [
  { os: "macos", arch: "aarch64" },
  { os: "macos", arch: "x86_64" },
  { os: "linux", arch: "x86_64" },
  { os: "linux", arch: "aarch64" },
  { os: "windows", arch: "x86_64" },
];
export const targetLabel = (t: { os: string; arch: string }): string => `${osLabel(t.os)} · ${archLabel(t.arch)}`;

/** 감지 결과 한 줄 */
export function detectedLine(d: MachineDetected): string {
  return [
    targetLabel(d),
    d.chosen ? "직접 고름" : null,
    d.home,
    d.shell ? d.shell.split(/[\\/]/).pop() : null,
    d.aaa ? `${d.aaa} 있음` : "aaa 없음",
  ]
    .filter(Boolean)
    .join(" · ");
}

/** `ssh -G`로 푼 접속 */
export function sshLine(v: MachineSetupView): string | null {
  const s = v.ssh;
  if (!s) return null;
  const host = s.host.includes(":") ? `[${s.host}]` : s.host;
  return `${s.user}@${host}:${s.port}${s.proxy_jump ? " (ProxyJump 거침)" : ""}`;
}
