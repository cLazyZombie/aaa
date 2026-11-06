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

export const IDEA_STATE: Record<IdeaSummary["status"], string> = { open: "다듬는 중", done: "완료" };

/** 목록 줄 둘째 줄 오른쪽(thread 줄의 머신 자리): 완료는 넘긴 할 일, 다듬는 중은 전용 thread */
export function ideaRefs(i: IdeaSummary): string {
  return i.status === "done" ? i.tasks.join(", ") : (i.thread ?? "");
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

/** 노트 칸에서 고른 자리의 `##` 절 이름: 고른 글 앞의 마지막 둘째 단계 머리(Markdown.tsx는 머리를 `data-markdown-heading`으로 그린다) */
export function sectionBefore(root: Element, range: Range): string | null {
  let found: string | null = null;
  for (const h of Array.from(root.querySelectorAll('[data-markdown-heading="2"]'))) {
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

// ── 노트 그림 붙이기·편집(t-1575) ──────────────────────────────────────────────

/** 받는 그림·영상(host와 같다: 바이트 머리로 다시 본다) */
export const IDEA_MEDIA_ACCEPT = "image/png,image/jpeg,image/gif,image/webp,video/mp4,video/webm";
/** 한 개 상한(host와 같다) */
export const IDEA_MEDIA_MAX = 16 * 1024 * 1024;
const MEDIA_TYPE_BY_EXT: Readonly<Record<string, string>> = {
  png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp", mp4: "video/mp4", webm: "video/webm",
};

/**
 * 고른·붙인 파일을 올릴 수 있으면 형식이 붙은 파일, 아니면 못 올리는 까닭. 브라우저가 형식을 비우면(일부 파일 관리자) 확장자로 채운다
 * (herdr-web-ui composerImageMime과 같은 방식, 영상까지)
 */
export function ideaMediaFile(file: File): File | string {
  let type = file.type.toLowerCase();
  if (type === "" || type === "application/octet-stream") {
    const ext = file.name.slice(file.name.lastIndexOf(".") + 1).toLowerCase();
    type = file.name.includes(".") ? (MEDIA_TYPE_BY_EXT[ext] ?? "") : "";
    if (type !== "") file = new File([file], file.name, { type, lastModified: file.lastModified });
  }
  if (!Object.values(MEDIA_TYPE_BY_EXT).includes(type)) return "그림(PNG·JPEG·GIF·WebP)·영상(MP4·WebM)만 올린다";
  if (file.size === 0 || file.size > IDEA_MEDIA_MAX) return "1바이트부터 16MiB까지 올린다";
  return file;
}

/** 노트 글 안 그림 하나: `![설명](<이름>)`의 자리. `nth`는 같은 이름 중 몇째인가, `alone`은 그 줄에 그림 하나뿐인가 */
export type IdeaMediaSpot = { name: string; alt: string; start: number; end: number; nth: number; line: number; alone: boolean };

const MEDIA_IMAGE = /!\[([^\]\n]*)\]\(\s*<?([0-9a-f]{16}\.(?:png|jpg|gif|webp|mp4|webm))>?(?:\s+["'][^\n]*?["'])?\s*\)/g;

/** 줄마다 (시작 위치, 줄, 코드 울타리 안인가). 울타리 줄 자체도 안으로 센다 */
function noteLines(text: string): { at: number; line: string; fenced: boolean }[] {
  const out: { at: number; line: string; fenced: boolean }[] = [];
  let fence: string | null = null;
  let at = 0;
  for (const line of text.split("\n")) {
    const t = line.trimStart();
    const mark = t.startsWith("```") ? "```" : t.startsWith("~~~") ? "~~~" : null;
    if (mark) {
      out.push({ at, line, fenced: true });
      fence = fence === mark ? null : fence ?? mark;
    } else out.push({ at, line, fenced: fence !== null });
    at += line.length + 1;
  }
  return out;
}

/** 노트의 폴더 안 그림들(글 차례). 코드 울타리·`` ` `` 안의 예시는 빼고, host 렌더러가 그리는 차례와 같다 */
export function ideaMediaSpots(text: string): IdeaMediaSpot[] {
  const out: IdeaMediaSpot[] = [];
  const seen = new Map<string, number>();
  noteLines(text).forEach(({ at, line, fenced }, index) => {
    if (fenced) return;
    for (const m of line.matchAll(MEDIA_IMAGE)) {
      const pos = m.index ?? 0;
      if ((line.slice(0, pos).match(/`/g)?.length ?? 0) % 2 === 1) continue;
      const name = m[2] ?? "";
      const nth = seen.get(name) ?? 0;
      seen.set(name, nth + 1);
      out.push({ name, alt: m[1] ?? "", start: at + pos, end: at + pos + m[0].length, nth, line: index, alone: line.trim() === m[0] });
    }
  });
  return out;
}

export function ideaMediaSpot(text: string, name: string, nth: number): IdeaMediaSpot | null {
  return ideaMediaSpots(text).find((s) => s.name === name && s.nth === nth) ?? null;
}

/** 그림 설명(host `media_alt`와 같다): 한 줄, `[`·`]`·제어 문자는 빈칸, 200자. 비면 `그림`·`영상` */
export function ideaMediaAlt(alt: string, name: string): string {
  const cleaned = [...alt.replace(/[[\]\p{Cc}]/gu, " ").split(/\s+/).filter(Boolean).join(" ")].slice(0, 200).join("");
  return cleaned || (/\.(mp4|webm)$/.test(name) ? "영상" : "그림");
}

/** 설명 바꾸기 */
export function withMediaAlt(text: string, spot: IdeaMediaSpot, alt: string): string {
  const head = `![${spot.alt}]`;
  return `${text.slice(0, spot.start)}![${ideaMediaAlt(alt, spot.name)}]${text.slice(spot.start + head.length)}`;
}

/** 같은 자리의 그림을 다른 파일로 바꾸기(설명은 둔다) */
export function withMediaName(text: string, spot: IdeaMediaSpot, name: string): string {
  const inner = text.slice(spot.start, spot.end);
  const at = inner.lastIndexOf(spot.name);
  return `${text.slice(0, spot.start)}${inner.slice(0, at)}${name}${inner.slice(at + spot.name.length)}${text.slice(spot.end)}`;
}

/** 그림 지우기: 혼자 선 줄이면 줄째(남는 빈 줄 하나도), 글 가운데면 그림만 */
export function withoutMedia(text: string, spot: IdeaMediaSpot): string {
  if (!spot.alone) return text.slice(0, spot.start) + text.slice(spot.end);
  const lines = text.split("\n");
  lines.splice(spot.line, 1);
  if (spot.line > 0 && lines[spot.line - 1]?.trim() === "" && (lines[spot.line]?.trim() ?? "") === "" && spot.line < lines.length - 1) lines.splice(spot.line, 1);
  return lines.join("\n");
}

/**
 * 혼자 선 그림을 앞·뒤 덩이와 자리 바꾸기. 덩이는 머리 한 줄·혼자 선 그림 한 줄·코드 울타리 하나·빈 줄로 나뉜 글 묶음이고, 사이의 빈 줄은
 * 제자리에 둔다. 옮길 수 없으면 null
 */
export function movedMedia(text: string, spot: IdeaMediaSpot, dir: -1 | 1): string | null {
  if (!spot.alone) return null;
  const lines = noteLines(text);
  const units: [number, number][] = [];
  const alone = new Set(ideaMediaSpots(text).filter((s) => s.alone).map((s) => s.line));
  for (let i = 0; i < lines.length;) {
    const { line, fenced } = lines[i]!;
    if (line.trim() === "" && !fenced) { i++; continue; }
    let j = i + 1;
    if (fenced) {
      while (j < lines.length && !(lines[j]!.fenced && /^\s*(```|~~~)/.test(lines[j]!.line) && j > i)) j++;
      j = Math.min(j + 1, lines.length);
    } else if (!/^#{1,6}(\s|$)/.test(line) && !alone.has(i)) {
      while (j < lines.length && lines[j]!.line.trim() !== "" && !lines[j]!.fenced && !/^#{1,6}(\s|$)/.test(lines[j]!.line) && !alone.has(j)) j++;
    }
    units.push([i, j]);
    i = j;
  }
  const at = units.findIndex(([a, b]) => spot.line >= a && spot.line < b);
  const other = at + dir;
  if (at < 0 || other < 0 || other >= units.length) return null;
  const [[a0, a1], [b0, b1]] = dir < 0 ? [units[other]!, units[at]!] : [units[at]!, units[other]!];
  const raw = text.split("\n");
  return [...raw.slice(0, a0), ...raw.slice(b0, b1), ...raw.slice(a1, b0), ...raw.slice(a0, a1), ...raw.slice(b1)].join("\n");
}

/** 그림 출처 한 줄: `웹 · 사람 · IMG_0001.jpg · 1.2MB` */
export function ideaMediaSource(m: { source: string; by: string; file: string | null; bytes: number }): string {
  const source: Record<string, string> = { web: "웹", file: "파일", library: "thread library", uploads: "첨부", copy: "복사" };
  const size = m.bytes >= 1024 * 1024 ? `${(m.bytes / 1024 / 1024).toFixed(1)}MB` : `${Math.max(1, Math.round(m.bytes / 1024))}KB`;
  const file = m.file ? m.file.split("/").pop() ?? m.file : null;
  return [source[m.source], m.by || null, file, m.bytes > 0 ? size : null].filter(Boolean).join(" · ");
}
