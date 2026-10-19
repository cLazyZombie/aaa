// 보낸 글 다시 부르기의 목록과 걸음(plan §8.266). t3code(MIT) `apps/web/src/components/chat/composerPromptHistory.ts`(1451e7cb2)
// `recallableComposerPrompt`(78줄)·`buildComposerPromptHistoryEntries`(125줄)·`stepComposerPromptHistory`(145줄)를 옮겼다:
// 목록은 시간 차례, 연달아 같은 글은 뒤 것으로 바꾸고, ↑(backward)는 끝에서 앞으로, ↓(forward)는 가장 새 것 너머에서 빈 칸·자리 없음.
// 지금 글이 부른 글과 다르면(고쳤으면) 걷지 않는다.
// 다른 곳(aaa 구조): 기록 글에 id가 없다(PTY 화면·대화 파일에서 읽고 host가 앞을 자른다). 자리 id는 같은 글 가운데 끝에서 몇 번째인지로
// 두고, 참고처럼 id로 못 찾으면 같은 글의 마지막 자리로 간다. 참고가 빼는 꼬리 블록(review_comment·terminal_context·칩)은 aaa 글에
// 없어 「Ultrathink:」 머리만 뺀다. 자동화·다른 agent 글을 빼는 표지(attribution)는 aaa 기록에 없어 사람 글을 모두 넣는다.
import type { ChatMessage } from "./gen/proto";

const CLAUDE_ULTRATHINK_PREFIX = "Ultrathink:\n";

export interface RecallEntry {
  readonly id: string;
  readonly prompt: string;
}

export interface RecallPosition {
  readonly entryId: string;
  readonly recalled: string;
}

export interface RecallStep {
  readonly position: RecallPosition | null;
  readonly prompt: string;
}

export function recallablePrompt(messageText: string): string {
  let prompt = messageText.trim();
  if (prompt.startsWith(CLAUDE_ULTRATHINK_PREFIX)) prompt = prompt.slice(CLAUDE_ULTRATHINK_PREFIX.length);
  return prompt.trim();
}

export function recallEntries(messages: readonly Pick<ChatMessage, "role" | "text">[]): RecallEntry[] {
  const entries: { prompt: string }[] = [];
  for (const message of messages) {
    if (message.role !== "user") continue;
    const prompt = recallablePrompt(message.text);
    if (prompt.length === 0) continue;
    const previous = entries[entries.length - 1];
    if (previous && previous.prompt === prompt) continue;
    entries.push({ prompt });
  }
  // 같은 글 가운데 끝에서 몇 번째인가(앞이 잘려도, 새 글이 붙기 전까지 바뀌지 않는다)
  const seen = new Map<string, number>();
  const ids: string[] = [];
  for (let i = entries.length - 1; i >= 0; i--) {
    const prompt = entries[i]!.prompt;
    const n = seen.get(prompt) ?? 0;
    seen.set(prompt, n + 1);
    ids[i] = `${n}:${prompt}`;
  }
  return entries.map((entry, i) => ({ id: ids[i]!, prompt: entry.prompt }));
}

function findActive(entries: readonly RecallEntry[], position: RecallPosition): number {
  const byId = entries.findIndex((entry) => entry.id === position.entryId);
  if (byId >= 0) return byId;
  for (let i = entries.length - 1; i >= 0; i--) if (entries[i]!.prompt === position.recalled) return i;
  return -1;
}

/** 한 걸음. null은 걷지 않음(키를 입력창에 둔다) */
export function recallStep(input: {
  readonly direction: "backward" | "forward";
  readonly entries: readonly RecallEntry[];
  readonly position: RecallPosition | null;
  readonly currentPrompt: string;
}): RecallStep | null {
  const { entries, position, currentPrompt } = input;
  const activeIndex = position && position.recalled === currentPrompt ? findActive(entries, position) : -1;

  if (input.direction === "backward") {
    if (activeIndex < 0 && currentPrompt.length > 0) return null;
    const entry = entries[activeIndex < 0 ? entries.length - 1 : activeIndex - 1];
    if (!entry) return null;
    return { position: { entryId: entry.id, recalled: entry.prompt }, prompt: entry.prompt };
  }

  if (activeIndex < 0) return null;
  const entry = entries[activeIndex + 1];
  if (!entry) return { position: null, prompt: "" };
  return { position: { entryId: entry.id, recalled: entry.prompt }, prompt: entry.prompt };
}

const MIRRORED = [
  "fontFamily", "fontSize", "fontWeight", "fontStyle", "letterSpacing", "lineHeight", "textTransform", "wordSpacing",
  "tabSize", "textIndent", "wordBreak", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft",
] as const;

/**
 * 커서가 첫(start)·끝(end) 「보이는」 줄에 있는가(참고 `ComposerPromptEditorTiptap.tsx:1660` `isCaretOnVisualEdge`). 참고는 편집기의
 * caret 사각형을 재지만 textarea는 caret 자리를 주지 않아, 같은 글꼴·폭의 보이지 않는 거울에 커서 앞 글과 표지를 놓고 잰다.
 * 문턱은 참고대로 줄 높이의 반이다
 */
export function caretOnVisualEdge(box: HTMLTextAreaElement, edge: "start" | "end"): boolean {
  const value = box.value;
  if (value.length === 0) return true;
  if (box.selectionStart !== box.selectionEnd) return false;
  const caret = box.selectionStart;
  if (edge === "start" ? value.slice(0, caret).includes("\n") : value.slice(caret).includes("\n")) return false;
  const style = getComputedStyle(box);
  const mirror = document.createElement("div");
  for (const name of MIRRORED) mirror.style[name] = style[name];
  Object.assign(mirror.style, {
    position: "absolute", top: "0", left: "-9999px", visibility: "hidden", boxSizing: "border-box", border: "0",
    width: `${box.clientWidth}px`, whiteSpace: "pre-wrap", overflowWrap: "break-word",
  });
  const marker = document.createElement("span");
  marker.textContent = "​";
  mirror.append(value.slice(0, caret), marker, value.slice(caret));
  document.body.append(mirror);
  try {
    const threshold = marker.offsetHeight / 2;
    return edge === "start"
      ? marker.offsetTop - parseFloat(style.paddingTop) < threshold
      : mirror.scrollHeight - parseFloat(style.paddingBottom) - (marker.offsetTop + marker.offsetHeight) < threshold;
  } finally {
    mirror.remove();
  }
}
