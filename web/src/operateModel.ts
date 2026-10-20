// 운영 화면의 순수 규칙(관리 GUI W4, plan §8.343): thread 끝내기·다시 시작·Next, 「이 aaa」·「저장소」 설정, 알림함.
// 판정의 정본은 host다(거부·검사). 화면은 고를 수 있는 것만 보이고 기본값을 정한다.
import { apiAtLeast } from "./automationModel.ts";
import { repoName, REPO_DIR } from "./tasksModel.ts";
import type { InboxItem, PostEnvelope, RepoChoice, ResolvePreview, ThreadView } from "./gen/proto";

/** 설정 읽기·저장소 고치기·끝내기 미리보기·보고서 정리 실행은 1.77 이상 host만 안다 */
export const canOperate = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 77);

/** thread 화면 머리의 동작 중 지금 할 수 있는 것. `running`은 그 thread의 agent pane이 도는가 */
export function threadActions(t: Pick<ThreadView, "status" | "thread_kind" | "cleanup_pending" | "brief">, running: boolean): {
  resolve: boolean;
  reopen: boolean;
  restart: boolean;
  brief: boolean;
} {
  const ended = t.status === "resolved" && !t.cleanup_pending;
  return {
    // 정리가 끊긴 thread는 끝내기로 이어 간다(host `resolve_pending`)
    resolve: t.status !== "starting" && !ended,
    reopen: ended,
    // 도는 세션은 다시 띄우지 않는다. 데려온 thread는 사람이 띄운 pane이라 다시 띄울 방법을 모른다(host가 거부한다)
    restart: !running && !ended && t.status !== "starting" && t.thread_kind !== "adopted",
    // 받은 brief는 다시 보내지 않는다(host가 거부한다: 더 보낼 것은 메시지로)
    brief: running && !ended && t.brief !== "delivered",
  };
}

/** 끝내기 창의 처음 선택: 위험하면(또는 모르면) worktree를 남긴다 */
export function resolveDefaults(p: ResolvePreview | null): { keep: boolean } {
  return { keep: p ? p.keep_default : true };
}

/** 끝내기 요청. 남기기와 사본 없이 끝내기(`skip_copy`)는 함께 켤 수 있고, 버리기(`discard_uncopied`)는 남기기와 함께 쓰지 않는다 */
export function resolveRequest(t: Pick<ThreadView, "project" | "id">, o: { keep: boolean; skipCopy: boolean; discard: boolean }) {
  return {
    op: "thread_resolve" as const,
    project: t.project,
    thread: t.id,
    keep_worktree: o.keep,
    reopen: false,
    skip_copy: o.skipCopy,
    discard_uncopied: !o.keep && o.discard,
  };
}

/** `PATH[@MACHINE]`(host `parse_repo_arg`): `@` 뒤가 머신 이름 꼴일 때만 머신이다 */
export function parseRepoArg(text: string): RepoChoice {
  const arg = text.trim();
  const at = arg.lastIndexOf("@");
  if (at > 0) {
    const machine = arg.slice(at + 1);
    if (/^[A-Za-z0-9._-]+$/.test(machine)) return { path: arg.slice(0, at), machine };
  }
  return { path: arg, machine: null };
}
export const repoArg = (r: RepoChoice): string => (r.machine ? `${r.path}@${r.machine}` : r.path);

/** 「저장소」 페이지의 묶음: 할 일 저장소 칸이 고르는 이름(경로 마지막 이름)마다 머신별 경로. 끝에 저장소 없는 작업 폴더 */
export type RepoGroup = { name: string; rows: Array<{ repo: RepoChoice; machine: string | null }> };
export function repoGroups(repos: RepoChoice[], localMachine: string | null): RepoGroup[] {
  const out: RepoGroup[] = [];
  for (const repo of repos) {
    const name = repoName(repo.path);
    let g = out.find((x) => x.name === name);
    if (!g) out.push((g = { name, rows: [] }));
    g.rows.push({ repo, machine: repo.machine ?? localMachine });
  }
  return out;
}
export { REPO_DIR };

/** 설정 값 하나의 꼴 검사(host `set_in`과 같은 범위). 통과하면 null, 아니면 까닭 */
export function checkSetting(key: "max_parallel_threads" | "auto_resolve_days", value: string): string | null {
  const n = Number(value.trim());
  const lo = key === "max_parallel_threads" ? 1 : 0;
  return Number.isInteger(n) && n >= lo && n <= 1000 ? null : `${lo}~1000의 수다`;
}

/** 사용자가 답할 요청 post의 inbox 항목(host `asks::is_user_request`와 같은 기준): 「답할 것」이 다룬다(plan §8.345) */
export const isUserRequest = (i: Pick<InboxItem, "item_kind" | "event">): boolean => i.item_kind === "post" && i.event === "post request for user";

/** 알림함 배지: 끝내지 않은 항목 수(응답에 싣지 못한 것 포함). 사용자가 답할 요청은 「답할 것」 배지가 세므로 뺀다 */
export const inboxOpen = (items: InboxItem[], omittedOpen: number): number => items.filter((i) => !i.done && !isUserRequest(i)).length + omittedOpen;

/** post 탭에서 뺄 것: 사용자가 답할 요청은 「답할 것」에서 다룬다(같은 자료를 두 곳에 두지 않는다) */
export const needsAnswer = (p: Pick<PostEnvelope, "post_type" | "needs_user" | "to">): boolean =>
  p.to === "user" && p.post_type === "request" && p.needs_user;
