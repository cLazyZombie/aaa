// 웹 「배포」의 순수 규칙(관리 GUI W6a, plan §8.361). 판정의 정본은 host다(조작 기기만, 빌드 머신에 이미 있는 후보만, 웹은 서비스 재시작만
// 개별로 연다, 진행 중 rollout이 있으면 거부). 화면은 점검을 통과한 그 후보·대상일 때만 시작을 연다.
import { apiAtLeast } from "./automationModel.ts";
import type { DeployAction, DeployJobCard, DeployState, DeployWork, ErrorCode, RolloutPlanView, RolloutSpec } from "./gen/proto";

/** 웹 배포·재시작은 1.82 이상 host만 안다 */
export const canDeploy = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 82);

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/** rollout·배포 작업 ID: host `valid_id`(영숫자 26자, CLI의 ULID와 같은 꼴). 보내기 전에 만들어 응답이 끊겨도 같은 ID로 이어 본다 */
export function newDeployId(): string {
  const b = new Uint8Array(26);
  crypto.getRandomValues(b);
  return Array.from(b, (x) => CROCKFORD[x % 32]).join("");
}

/** 후보 경로: 빌드 머신의 `<절대 경로>/aaa` 또는 `~/<폴더>/aaa`(host·agent가 provenance·symlink를 다시 본다) */
export function candidatePathOk(path: string): boolean {
  const p = path.trim();
  return (p.startsWith("/") || p.startsWith("~/")) && /\/aaa$/.test(p) && !p.split("/").includes("..");
}

export type PlanInput = { source: string; path: string; machines: string[] };
const sameInput = (a: PlanInput, b: PlanInput) => a.source === b.source && a.path.trim() === b.path.trim() && a.machines.join("\n") === b.machines.join("\n");

/** 시작은 같은 화면에서 점검한 그 후보·대상이고, 모든 대상이 준비됐고, 진행 중 rollout이 없을 때만 */
export function canStart(plan: { input: PlanInput; view: RolloutPlanView } | null, now: PlanInput): boolean {
  return plan !== null && sameInput(plan.input, now) && !plan.view.busy && plan.view.targets.length > 0 && plan.view.targets.every((t) => t.problem === null);
}

/** 점검한 후보로 보낼 rollout. SHA·커밋은 점검 결과 그대로다(사람이 치지 않는다). 대상은 host가 다시 host 먼저로 맞춘다 */
export function rolloutSpec(plan: RolloutPlanView, machines: string[]): RolloutSpec {
  const c = plan.candidate;
  return { from: c.path, source_machine: c.machine, sha256: c.sha256, commit: c.commit, machines };
}

export const restartAction = (service: "host" | "agent"): DeployAction => ({ action: "restart", service });

export function jobWhat(work: DeployWork): string {
  return work.kind === "restart" ? `${work.service} 재시작` : work.kind === "rollback" ? "되돌리기" : "실행 파일 교체";
}

/**
 * 보낼 대상: host 머신을 늘 맨 앞에 넣는다. 빈 목록은 host가 「등록된 모든 머신」으로 읽으므로, 다른 머신을 모두 풀었을 때
 * host만 가게 한다
 */
export function rolloutTargets(host: string | undefined, picked: string[]): string[] {
  return host ? [host, ...picked.filter((m) => m !== host)] : picked;
}

/** host가 확정으로 거절한 답(시작하지 않았다). 연결·내부 오류는 닿았는지 모르므로 보인 ID를 그대로 둔다 */
export function refused(code: ErrorCode): boolean {
  return ["bad_request", "forbidden", "not_found", "unsupported_version", "agent_disconnected", "conflict"].includes(code);
}

const JOB_STATES: Record<DeployState, string> = { queued: "대기", running: "진행 중", completed: "완료", failed: "실패", interrupted: "중단" };
export function jobState(job: DeployJobCard): string {
  return job.state === null ? "상태 모름(머신 연결 끊김)" : JOB_STATES[job.state];
}

/** 바이트를 읽기 좋게(후보 크기) */
export function sizeText(bytes: number): string {
  return bytes >= 1 << 20 ? `${(bytes / (1 << 20)).toFixed(1)} MiB` : `${Math.ceil(bytes / 1024)} KiB`;
}
