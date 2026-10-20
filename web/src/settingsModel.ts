// 설정 「안전」·「profile」·머신 작업 폴더·hook의 순수 규칙(관리 GUI W5c, plan §8.353). 판정의 정본은 host다(웹이 바꾸는 safety
// key 넷, custom profile 거부, 판 대조, 절대 경로). 화면은 고를 수 있는 것만 보인다.
import { apiAtLeast } from "./automationModel.ts";
import type { ProfileAdminRow, ProfileChangeReq } from "./gen/proto";

/** 웹 profile·safety·작업 root·hook 관리는 1.81 이상 host만 안다 */
export const canSettings = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 81);

/** 웹이 고르는 safety 값(host `value_item`과 같다). 「기본」은 이 표의 값을 지워 아래 층을 따른다 */
export const SAFETY_CHOICES: Record<string, readonly string[]> = {
  yolo: ["on", "off"],
  start_threads: ["propose", "auto"],
  trust_screens: ["coordinator", "user"],
  coordinator_deploy: ["on", "off"],
};

/** safety key의 뜻(화면 글) */
export const SAFETY_WORDS: Record<string, string> = {
  yolo: "thread를 묻지 않고 시작하고 thread·coordinator를 권한 건너뛰기 인자로 띄운다",
  start_threads: "propose: coordinator가 제안하고 사용자가 시작 · auto: 바로 시작",
  trust_screens: "thread pane의 폴더·hook 신뢰 창에 누가 답하나",
  coordinator_deploy: "사용자가 채팅으로 지시한 배포를 coordinator가 할 수 있나",
  coordinator_agent_args: "기본 coordinator profile에 붙는 인자",
  thread_agent_args: "기본 thread profile에 붙는 인자",
  routine_commands: "routine 명령을 쓸 수 있나(명령마다 사람이 승인한다)",
  thread_profiles: "thread가 쓸 수 있는 profile",
  coordinator_profiles: "coordinator가 쓸 수 있는 profile",
};

/** yolo를 켜는 바꾸기만 한 번 더 확인한다(사용자 답 2A) */
export const needsYoloConfirm = (key: string, value: string | null): boolean => key === "yolo" && value === "on";

/** 웹에서 만드는 harness(답 1A: custom은 host CLI) */
export const WEB_HARNESSES = ["claude", "codex", "shell"] as const;

/** harness가 받는 effort(aaa-config `effort_values`). shell은 없다 */
export function effortValues(harness: string): readonly string[] {
  if (harness === "claude") return ["low", "medium", "high", "xhigh", "max"];
  if (harness === "codex") return ["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"];
  return [];
}

/** model 칸이 있는 harness */
export const hasModel = (harness: string): boolean => harness === "claude" || harness === "codex";

/** 인자 글: 줄마다 인자 하나(빈칸이 든 인자도 그대로). 빈 줄은 뺀다 */
export function argsFromText(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l !== "");
}

export type ProfileDraft = {
  name: string;
  harness: string;
  model: string;
  effort: string;
  description: string;
  args: string;
};

export function draftOf(p: ProfileAdminRow | null): ProfileDraft {
  if (!p)
    return {
      name: "",
      harness: "claude",
      model: "",
      effort: "",
      description: "",
      args: "",
    };
  return {
    name: p.name,
    harness: p.harness,
    model: p.model,
    effort: p.effort,
    description: p.description,
    args: p.args.join("\n"),
  };
}

/**
 * 고치기 창의 바꾸기. 기본 profile(host.toml에 표가 없다)을 고치면 같은 이름으로 더해 대신한다(CLI `aaa profile add`와 같다).
 * harness를 바꾸면 그 harness가 받지 않는 model·effort는 비운다
 */
export function profileChange(before: ProfileAdminRow | null, d: ProfileDraft): ProfileChangeReq {
  const model = hasModel(d.harness) ? d.model.trim() : "";
  const effort = effortValues(d.harness).includes(d.effort) ? d.effort : "";
  const args = argsFromText(d.args);
  if (!before || before.builtin) {
    return {
      change: "add",
      name: d.name.trim(),
      harness: d.harness,
      model,
      effort,
      description: d.description.trim(),
      args,
    };
  }
  return {
    change: "edit",
    name: before.name,
    harness: d.harness,
    model,
    effort,
    description: d.description.trim(),
    args,
  };
}

/** host `validate_name`과 같다: 영문·숫자·`.`·`_`·`-` 40자까지, `-`·`.`로 시작하지 않는다 */
export const profileNameOk = (name: string): boolean => /^[A-Za-z0-9._-]{1,40}$/.test(name) && !/^[-.]/.test(name);

/** 허용 목록 글: 없으면 모든 profile */
export function allowText(names: string[] | null | undefined): string {
  if (names === null || names === undefined) return "모든 profile";
  return names.length === 0 ? "없음" : names.join(", ");
}

/** 작업 root는 절대 경로만(host `absolute`와 같다): Unix `/…`나 Windows `C:\…`·`C:/…`. `~`·상대 경로는 받지 않는다 */
export function absolutePath(path: string): boolean {
  const p = path.trim();
  return p.startsWith("/") || /^[A-Za-z]:[\\/]/.test(p);
}

/** 전환은 같은 화면에서 점검을 통과한 그 경로·mount일 때만 연다(답 3A) */
export function canSwitch(checked: { path: string; mount: string } | null, path: string, mount: string): boolean {
  return checked !== null && checked.path === path.trim() && checked.mount === mount.trim() && absolutePath(path);
}
