// 설정 「머신」의 「SSH로 추가」 규칙(웹 머신 추가 M2 plan §8.384, M3 §8.385). 접속·host key·감지·설치 계획의 정본은 host다
// (machine_setup.rs): 화면은 작업 단계를 사람 말로 옮기고 고를 수 있는 것만 보인다.
import { apiAtLeast } from "./automationModel.ts";
import type { InstallHarness, InstallLogin, MachineDetected, MachineSetupPhase, MachineSetupSettings, MachineSetupView } from "./gen/proto";
import { archLabel, osLabel } from "./machineDetail.ts";

/** 1.93 이상 host만 SSH 추가 요청을 안다 */
export const canSsh = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 93);
/** 1.94 이상 host만 SSH 설치(계획·적용)를 안다 */
export const canInstall = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 94);

export const PHASE: Record<MachineSetupPhase, string> = {
  connecting: "접속 중",
  trust: "지문 확인 필요",
  detecting: "OS 감지 중",
  choose: "OS 고르기 필요",
  ready: "준비됨",
  unsupported: "지원 안 함",
  failed: "실패",
  planning: "설치 계획 만드는 중",
  review: "설치 확인 필요",
  installing: "설치·등록 중",
  needs_user: "그 머신에서 할 일",
  installed: "설치·등록됨",
};

/** 사람이 해야 할 단계(카드를 강조한다) */
export const needsPerson = (p: MachineSetupPhase): boolean => p === "trust" || p === "choose" || p === "review" || p === "needs_user";
export const busy = (p: MachineSetupPhase): boolean =>
  p === "connecting" || p === "detecting" || p === "planning" || p === "installing";
/** 설치 중에는 지우지 않는다(host도 거부한다) */
export const canCancel = (p: MachineSetupPhase): boolean => p !== "installing";

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

// ───── M3 설치 설정 ─────

export const HARNESSES: InstallHarness[] = ["claude", "codex"];
export const LOGIN: Record<InstallLogin, string> = { require: "로그인돼 있어야 함", defer: "나중에 로그인" };

/** host `machine_name_ok`와 같다 */
const NAME = /^[A-Za-z0-9._-]{1,64}$/;
const HOST = /^[A-Za-z0-9._:[\]%-]{1,255}$/;
const remotePath = (p: string): boolean => p.startsWith("/") && p.length <= 4096 && !p.split("/").includes("..") && !/[\u0000-\u001f]/.test(p);

/** host `check_settings`의 앞 검사(실물 경로·mount는 원격 install이 본다) */
export function settingsProblem(s: MachineSetupSettings, taken: string[] = []): string | null {
  if (!NAME.test(s.name)) return "머신 이름은 영문·숫자·.-_ 64자까지다";
  if (taken.includes(s.name)) return `${s.name}은 이미 있는 머신 이름이다`;
  if (!HOST.test(s.host.trim())) return "host 주소는 이름·IP·포트 글자만 쓴다";
  if (!remotePath(s.work_root)) return "작업 root는 그 머신의 절대 경로다(/로 시작)";
  if (s.work_mount && !remotePath(s.work_mount)) return "외장 디스크 mount는 그 머신의 절대 경로다";
  return null;
}

/** 감지 결과에서 고른 기본값: 이름은 대상 별칭을 다듬은 것, 작업 root는 HOME 아래 aaa-work */
export function defaultSettings(target: string, d: MachineDetected | null, host: string): MachineSetupSettings {
  const at = target.replace(/^.*@/, "");
  const bracket = /^\[([^\]]*)\]/.exec(at);
  const bare = bracket ? bracket[1]! : (at.replace(/:\d+$/, "").split(".")[0] ?? "");
  const name = bare.replace(/[^A-Za-z0-9._-]/g, "-").replace(/^-+|-+$/g, "").slice(0, 64);
  const home = d?.home?.replace(/\/+$/, "") ?? "";
  return { name, host, work_root: home ? `${home}/aaa-work` : "", work_mount: null, harness: [], login: "defer" };
}

/** 승인 범위 이름(install operation) */
export const OPERATION: Record<string, string> = {
  write_file: "설정 파일 쓰기",
  stage_binary: "aaa 실행 파일 놓기",
  stage_launcher: "launcher 놓기",
  configure_hook: "harness hook",
  prepare_work_root: "작업 root 준비",
  register_service: "서비스 등록",
  start_service: "서비스 시작",
  publish_web: "웹 공개",
  enroll_agent: "이 host에 등록",
  create_project: "프로젝트 만들기",
  modify_path: "PATH 바꾸기",
};
export const operationLabel = (op: string): string => OPERATION[op] ?? op;

/** 계획 hash를 사람이 맞춰 볼 만큼(앞 12자) */
export const shortHash = (h: string): string => h.slice(0, 12);
