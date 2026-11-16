// 옛 아이디어 화면(t-1260, 읽기 전용 t-1894 4단계)의 화면 밖 규칙: 휴대폰 「노트 | 대화」 페이지 쓸기, 노트 그림 주소, 할 일 메모의
// 아이디어 링크, 옮기기 상태와 옮긴 곳. 진행 모델 고르기는 기획 대화·구현 맡기기가 함께 쓴다.
import type { DesignRepoView, IdeaMoved, IdeaSummary } from "./gen/proto";
import { EDGE_PX, SWIPE_PX } from "./herdr/edgeSwipe.ts";

/** 휴대폰 본문 페이지. 목록은 앱 drawer다(노트에서 오른쪽으로 쓸면 열린다) */
export type IdeaPage = "note" | "chat";

/** 고를 수 있는 진행 모델: 그 머신이 맡을 수 있는 profile(`assignable`). 지금 값이 목록에 없으면 앞에 둔다 */
export function ideaModelChoices(owners: readonly { machine: string; profiles: string[] }[], hostMachine: string | null, chosen: string): string[] {
  const usable = owners.find((o) => o.machine === hostMachine)?.profiles ?? [];
  return [...new Set([chosen, ...usable])];
}

/**
 * 「페이지처럼」(결정 24): 노트에서 ← 쓸면 대화, 대화에서 → 쓸면 노트. 노트에서 →(목록)는 drawer가 받는다.
 * 왼쪽 끝 24px은 브라우저 뒤로 쓸기 자리라 받지 않는다(edgeSwipe와 같은 규칙). 세로로 더 움직이면 스크롤이다
 */
export function ideaSwipe(page: IdeaPage, startX: number, dx: number, dy: number): IdeaPage | "claim" | "pending" | "ignore" {
  if (startX <= EDGE_PX) return "ignore";
  if (Math.abs(dx) < 10 && Math.abs(dy) < 10) return "pending";
  if (Math.abs(dy) >= Math.abs(dx)) return "ignore";
  const to: IdeaPage | null = page === "note" && dx < 0 ? "chat" : page === "chat" && dx > 0 ? "note" : null;
  if (to === null) return "ignore";
  return Math.abs(dx) >= SWIPE_PX ? to : "claim";
}

/** 노트 그림·영상 이름(host가 준 내용 hash 이름)인가 */
export const ideaMediaName = (src: string): boolean => /^[0-9a-f]{16}\.(?:png|jpg|gif|webp|mp4|webm)$/.test(src.trim());

/** 노트 그림·영상의 주소. 아이디어 폴더의 이름만 받는다(바깥 경로·주소는 그리지 않는다) */
export function ideaFileUrl(project: string, idea: string, src: string): string | null {
  return ideaMediaName(src)
    ? `/api/v1/projects/${encodeURIComponent(project)}/ideas/${encodeURIComponent(idea)}/files/${encodeURIComponent(src.trim())}`
    : null;
}

/** 할 일 메모의 아이디어 링크 대상(`ideas/n-0001`) → `n-0001` */
export function ideaLinkTarget(path: string): string | null {
  const m = /^ideas\/(n-\d+)$/.exec(path.trim());
  return m ? m[1]! : null;
}

/** 옛 아이디어의 상태: 다듬는 중(옮길 수 있다)·옮기는 중·옮김·완료 */
export type IdeaLegacy = { kind: "open" | "moving" | "moved" | "done"; label: string };

export function ideaLegacy(idea: Pick<IdeaSummary, "status" | "moved">): IdeaLegacy {
  const m = idea.moved;
  if (m?.done) return { kind: "moved", label: `옮김 → ${m.repo} ${m.path}` };
  if (m) return { kind: "moving", label: `옮기는 중 → ${m.repo} · design/${m.branch}` };
  if (idea.status === "done") return { kind: "done", label: "완료" };
  return { kind: "open", label: "다듬는 중" };
}

/**
 * 옮긴 곳을 열 기획 문서(`<저장소>/<경로>`): 경로가 문서면 그것, 폴더면 옮기는 branch가 고친 첫 문서·그 폴더의 README·그 폴더의 첫
 * 문서·기획 폴더 README 차례다
 */
export function movedTarget(m: IdeaMoved, repos: DesignRepoView[]): string {
  if (!m.path.endsWith("/")) return `${m.repo}/${m.path}`;
  const repo = repos.find((r) => r.repo === m.repo);
  const docs = repo?.tree?.docs.map((d) => d.path) ?? [];
  const touched = m.done ? [] : (repo?.branches.find((b) => b.slug === m.branch)?.changed ?? [])
    .filter((c) => c.status !== "D" && /\.md$/i.test(c.path)).map((c) => c.path);
  const path = touched[0] ?? docs.find((p) => p === `${m.path}README.md`) ?? docs.find((p) => p.startsWith(m.path))
    ?? (repo?.folder ? `${repo.folder}/README.md` : `${m.path}README.md`);
  return `${m.repo}/${path}`;
}
