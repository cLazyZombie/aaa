// 답 카드의 그림(t-1635, 사용자 11-08 「이미지도 붙일수 있게」): 직접 쓴 답 글 끝의 `![그림](uploads/<16 hex>.<ext>)` 줄 하나가
// 그림 하나다. task 첨부 줄(tasksModel `attachmentOfLine`)과 같은 꼴·같은 프로젝트 uploads/라 host가 thread 머신으로 넘길 때
// task 첨부와 같은 길(`task_attach::refs`)을 쓴다. 입력칸은 그림 줄을 뺀 글만 보이고, 그림은 그 아래 썸네일로 보인다.
import { ATTACH_MAX_BYTES, ATTACH_TYPES, attachmentOfLine } from "./tasksModel.ts";

const isImageLine = (line: string) => attachmentOfLine(line)?.kind === "image";

/** 답 글의 그림 대상들(`uploads/<이름>`, 줄 차례, 같은 것은 한 번) */
export function answerImages(text: string): string[] {
  const out: string[] = [];
  for (const line of text.split("\n")) {
    const a = attachmentOfLine(line);
    if (a?.kind === "image" && !out.includes(a.target)) out.push(a.target);
  }
  return out;
}

/** 입력칸이 보이는 글: 그림 줄을 뺀 나머지. 다듬지 않는다(치는 중인 빈 줄·끝 줄바꿈을 지킨다) */
export function answerBody(text: string): string {
  return text.split("\n").filter((l) => !isImageLine(l)).join("\n");
}

/** 입력칸 글을 고친 답: 글 뒤에 그림 줄을 차례대로 둔다 */
export function withAnswerBody(text: string, body: string): string {
  const lines = text.split("\n").filter(isImageLine);
  if (lines.length === 0) return body;
  return body === "" ? lines.join("\n") : `${body}\n${lines.join("\n")}`;
}

/** 그림 줄 하나를 답 끝에 더한다. 이미 있으면 그대로 */
export function addAnswerImage(text: string, target: string): string {
  if (answerImages(text).includes(target)) return text;
  const line = `![그림](${target})`;
  return text === "" ? line : `${text}\n${line}`;
}

/** 그 그림 줄을 뗀다(파일은 uploads/에 남는다) */
export function removeAnswerImage(text: string, target: string): string {
  return text.split("\n").filter((l) => !(isImageLine(l) && attachmentOfLine(l)?.target === target)).join("\n");
}

/** 붙일 수 있는 그림과 뺀 까닭(그림 형식·1바이트~16MiB, host와 같다) */
export function pickAnswerImages<T extends { name: string; type: string; size: number }>(files: readonly T[]): { ok: T[]; skipped: string[] } {
  const ok: T[] = [];
  const skipped: string[] = [];
  for (const f of files) {
    if (!(ATTACH_TYPES as readonly string[]).includes(f.type)) skipped.push(`${f.name || "이름 없음"}: 그림(PNG·JPEG·GIF·WebP)만`);
    else if (f.size === 0 || f.size > ATTACH_MAX_BYTES) skipped.push(`${f.name || "이름 없음"}: 1바이트부터 16MiB까지`);
    else ok.push(f);
  }
  return { ok, skipped };
}
