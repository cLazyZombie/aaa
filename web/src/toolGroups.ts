// 대화의 도구 호출 묶음(plan §8.304). t3code(MIT, 4b1024daf)를 aaa 기록으로 옮겼다:
// - 이어진 도구·생각 줄을 한 묶음으로, 하나뿐이면 그대로: `apps/web/src/components/chat/MessagesTimeline.logic.ts:1594~1716`
// - 동작 분류: `packages/client-runtime/src/work-log/presentation.ts:417` `toolGroupAction`, `packages/shared/src/toolActivity.ts:246~300`
//   `toolNameToken`·`classifyToolActivity`
// - 요약 문구: 같은 파일 512줄 `toolGroupActionCount`(바꾼 파일은 서로 다른 파일 수), 529줄 `toolGroupActionLabel`, 589줄
//   `summaryActionPriority`(명령·수정 먼저, 그 밖의 도구 나중), 631줄 `summarizeToolGroup`(두 가지까지, 나머지는 「그 밖의 동작 N개」)
// - 머리 그림: `MessagesTimeline.tsx:3834` `toolGroupSummaryIconName`·716줄 `toolGroupSummaryKind`
// - 펼친 묶음 기억(thread마다): `MessagesTimeline.tsx:612` `rememberedPosition.disclosures.workGroups`
// aaa 기록의 도구 줄은 agent가 만든 「이름: 대표 인자」 글뿐이다(chat.rs `tool_line`, 결과·성공 여부는 빠진다). 그래서 실패 표시는 없다.
import type { ChatMessage } from "./gen/proto";

export type ToolAction = "command" | "edit" | "read" | "code-search" | "search" | "other";
export type ToolGroupKind = ToolAction | "reasoning" | "mixed";

/** 도구 줄의 이름(「이름: 인자」의 앞) */
export function toolName(text: string): string {
  const at = text.indexOf(": ");
  return (at < 0 ? text : text.slice(0, at)).trim();
}

/** 도구 줄의 대표 인자(없으면 null) */
function toolArg(text: string): string | null {
  const at = text.indexOf(": ");
  return at < 0 ? null : text.slice(at + 2).trim() || null;
}

// 참고 toolNameToken: MCP·이름공간 도구(`__`·`.`·`/`)는 이름으로 가르지 않는다
function nameToken(name: string): string | null {
  return !name || /__|[./]/u.test(name) ? null : name.replace(/[_\s-]/gu, "").toLowerCase();
}

const ACTIONS: Readonly<Record<string, ToolAction>> = {
  // Claude Bash, Codex shell·exec_command·local_shell
  bash: "command", shell: "command", terminal: "command", execcommand: "command", localshell: "command", shellcommand: "command",
  edit: "edit", write: "edit", multiedit: "edit", notebookedit: "edit", applypatch: "edit",
  read: "read", readfile: "read", notebookread: "read", viewimage: "read",
  grep: "code-search", glob: "code-search", find: "code-search", rg: "code-search", ls: "code-search",
  websearch: "search", webfetch: "search",
};

export function toolAction(text: string): ToolAction {
  const token = nameToken(toolName(text));
  return (token === null ? undefined : ACTIONS[token]) ?? "other";
}

function actionCount(action: ToolAction, texts: readonly string[]): number {
  if (action !== "edit") return texts.length;
  const files = new Set<string>();
  let unnamed = 0;
  for (const text of texts) {
    const file = toolArg(text);
    if (file === null) unnamed += 1;
    else files.add(file);
  }
  return files.size + unnamed;
}

function actionLabel(action: ToolAction, count: number): string {
  switch (action) {
    case "command": return `명령 ${count}개 실행`;
    case "edit": return `파일 ${count}개 수정`;
    case "read": return `파일 ${count}개 읽음`;
    case "code-search": return `코드 ${count}번 검색`;
    case "search": return `웹 ${count}번 검색`;
    case "other": return `도구 ${count}개 사용`;
  }
}

const priority = (action: ToolAction) => (action === "command" || action === "edit" ? 0 : action === "other" ? 2 : 1);

/** 묶음 한 줄 요약(참고 summarizeToolGroup). 생각만 있으면 「생각 (×N)」 */
export function summarizeToolGroup(messages: readonly Pick<ChatMessage, "role" | "text">[]): string {
  const tools = messages.filter((m) => m.role === "tool").map((m) => m.text);
  if (tools.length === 0) return messages.length === 1 ? "생각" : `생각 (×${messages.length})`;
  const groups = new Map<ToolAction, string[]>();
  for (const text of tools) {
    const action = toolAction(text);
    const group = groups.get(action);
    if (group) group.push(text);
    else groups.set(action, [text]);
  }
  const selected = [...groups]
    .map(([action, texts], index) => ({ action, texts, index }))
    .sort((a, b) => priority(a.action) - priority(b.action) || a.index - b.index)
    .slice(0, 2)
    .sort((a, b) => a.index - b.index);
  const labels = selected.map(({ action, texts }) => actionLabel(action, actionCount(action, texts)));
  const rest = tools.length - selected.reduce((n, { texts }) => n + texts.length, 0);
  if (rest > 0) labels.push(`그 밖의 동작 ${rest}개`);
  return labels.join(", ");
}

/** 머리 그림 종류(참고 toolGroupSummaryKind) */
export function toolGroupKind(messages: readonly Pick<ChatMessage, "role" | "text">[]): ToolGroupKind {
  const tools = messages.filter((m) => m.role === "tool");
  if (tools.length === 0) return "reasoning";
  const actions = new Set(tools.map((m) => toolAction(m.text)));
  return actions.size === 1 ? actions.values().next().value! : "mixed";
}

/** 대화 보기의 한 줄: 말 하나, 또는 이어진 도구·생각 줄 둘 이상의 묶음. `indices`는 기록에서의 차례 */
export type ChatDisplayRow =
  | { kind: "message"; id: string; index: number }
  | { kind: "tools"; id: string; indices: number[]; memberIds: string[] };

const groupable = (m: Pick<ChatMessage, "role">) => m.role === "tool" || m.role === "thinking";

/** 이어진 도구·생각 줄을 묶는다. 묶음 id는 첫 줄 id다(참고 `work-group:<첫 항목 id>`). 사람·답 말이 묶음을 끊는다 */
export function groupChatRows(messages: readonly Pick<ChatMessage, "role">[], ids: readonly string[]): ChatDisplayRow[] {
  const rows: ChatDisplayRow[] = [];
  for (let i = 0; i < messages.length; i++) {
    if (!groupable(messages[i]!)) {
      rows.push({ kind: "message", id: ids[i]!, index: i });
      continue;
    }
    let end = i + 1;
    while (end < messages.length && groupable(messages[end]!)) end++;
    if (end - i === 1) rows.push({ kind: "message", id: ids[i]!, index: i });
    else {
      const indices = Array.from({ length: end - i }, (_, n) => i + n);
      rows.push({ kind: "tools", id: ids[i]!, indices, memberIds: indices.map((n) => ids[n]!) });
    }
    i = end - 1;
  }
  return rows;
}

/** 줄 id → 화면 줄 차례. 묶음은 들어 있는 모든 줄 id로도 찾는다: 앞 쪽을 읽어 묶음 앞에 도구 줄이 붙으면 묶음 id(첫 줄)가
 * 바뀌지만, 읽던 자리·펼침은 옛 첫 줄 id로 같은 묶음을 찾는다 */
export function rowIndexById(rows: readonly ChatDisplayRow[]): Map<string, number> {
  const at = new Map<string, number>();
  rows.forEach((row, index) => {
    at.set(row.id, index);
    if (row.kind === "tools") for (const id of row.memberIds) at.set(id, index);
  });
  return at;
}

/** 펼친 묶음(줄 id 모음)을 pane마다 기억한다(최근 100개, chatScroll.ts 자리 기억과 같다) */
const expanded = new Map<string, ReadonlySet<string>>();

export function readExpandedGroups(key: string): ReadonlySet<string> {
  return expanded.get(key) ?? new Set();
}

export function rememberExpandedGroups(key: string, ids: ReadonlySet<string>): void {
  expanded.delete(key);
  if (ids.size === 0) return;
  expanded.set(key, ids);
  if (expanded.size > 100) {
    const oldest = expanded.keys().next().value;
    if (oldest !== undefined) expanded.delete(oldest);
  }
}

/** 묶음이 펼쳐져 있나: 들어 있는 줄 id 가운데 하나라도 펼친 모음에 있으면 */
export function groupExpanded(row: Extract<ChatDisplayRow, { kind: "tools" }>, ids: ReadonlySet<string>): boolean {
  return row.memberIds.some((id) => ids.has(id));
}

/** 펼침을 뒤집은 모음. 접을 때는 묶음의 모든 줄 id를 뺀다 */
export function toggleExpanded(row: Extract<ChatDisplayRow, { kind: "tools" }>, ids: ReadonlySet<string>): ReadonlySet<string> {
  const next = new Set(ids);
  if (groupExpanded(row, ids)) for (const id of row.memberIds) next.delete(id);
  else next.add(row.id);
  return next;
}
