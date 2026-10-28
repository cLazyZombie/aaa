// 아이디어 화면(t-1260, 기획 t-1255)의 화면 밖 규칙: 목록 묶음, 휴대폰 「목록 | 노트 | 대화」 페이지 쓸기, 노트 그림 주소, 노트 고침 머리.
import type { IdeaNote, IdeaSummary } from "./gen/proto";
import { EDGE_PX, SWIPE_PX } from "./herdr/edgeSwipe.ts";

/** 목록: 다듬는 중 먼저, 그 안에서 최근에 고친 것부터(host가 준 차례를 지킨다) */
export function ideaGroups(ideas: readonly IdeaSummary[]): { name: string; items: IdeaSummary[] }[] {
  const open = ideas.filter((i) => i.status === "open");
  const done = ideas.filter((i) => i.status === "done");
  return [
    ...(open.length ? [{ name: "다듬는 중", items: open }] : []),
    ...(done.length ? [{ name: "완료", items: done }] : []),
  ];
}

/** 목록 줄의 둘째 줄: 「n-0001 · 다듬는 중」·「n-0003 · 완료 · d-1189」 */
export function ideaLine(i: IdeaSummary): string {
  return i.status === "done"
    ? [i.short_id, "완료", ...(i.tasks.length ? [i.tasks.join(", ")] : [])].join(" · ")
    : `${i.short_id} · 다듬는 중`;
}

/** 휴대폰 본문 페이지. 목록은 앱 drawer다(노트에서 오른쪽으로 쓸면 열린다) */
export type IdeaPage = "note" | "chat";

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

/** 사람이 고친 노트를 아직 thread에 알리지 않았으면 다음 말 앞에 붙인다(결정 16). host도 같은 머리를 알아 두 번 붙이지 않는다 */
export function withEditNote(note: Pick<IdeaNote, "edited"> | null, text: string): string {
  // harness 명령(`/compact`)에는 붙이지 않는다: 머리가 붙으면 명령이 아니게 된다
  return note?.edited && !text.startsWith("(노트 고침:") && !text.startsWith("/") ? `(노트 고침: ${note.edited})\n${text}` : text;
}

/** 노트 칸에서 고른 자리의 `##` 절 이름: 고른 글 앞의 마지막 h2 */
export function sectionBefore(root: Element, range: Range): string | null {
  let found: string | null = null;
  for (const h of Array.from(root.querySelectorAll("h2"))) {
    if (range.comparePoint(h, 0) > 0) break;
    found = h.textContent?.trim() || null;
  }
  return found;
}

/** 할 일로 넘길 첫 초안: 제목은 아이디어 제목, 메모는 노트의 「결정」 줄(없으면 빈 메모) */
export function firstDraft(note: IdeaNote): { title: string; repo: string; notes: string } {
  const lines = note.text.split("\n");
  const at = lines.findIndex((l) => /^##\s+결정\s*$/.test(l));
  const decided: string[] = [];
  if (at >= 0) {
    for (const l of lines.slice(at + 1)) {
      if (/^#{1,2}\s/.test(l)) break;
      if (l.trim()) decided.push(l);
    }
  }
  return { title: note.idea.title, repo: "", notes: [`${note.idea.short_id} 정리본대로 한다.`, ...decided].join("\n") };
}
