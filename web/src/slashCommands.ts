// 입력칸 「/」 목록의 규칙(plan §8.323). herdr-web-ui `src/lib/compose.ts` 229~271줄의 순위(prefix < 단어 경계 < 부분 < 글자 순서 <
// 설명)와 사용 횟수를 옮겼다. 다른 곳: harness 명령은 메시지 맨 앞에서만 연다(t3code 「A provider expands a slash command only when
// it opens the whole message」). 줄 중간·경로(`/Users/…`)에서는 열지 않는다. 확인창·숨김·터미널 알림은 agent 표의 분류를 따른다.
import type { CommandKind, CommandList, CommandSource, SlashCommand } from "./gen/proto";

/** 커서 앞 글이 「/이름」 하나뿐이고 커서가 그 끝에 있으면 이름(빈 글 포함). 아니면 null */
export function slashQuery(text: string, caret: number): string | null {
  const before = text.slice(0, caret);
  const match = /^\/([^\s/]*)$/.exec(before);
  if (!match) return null;
  const after = text.slice(caret);
  if (after !== "" && !/^\s/.test(after)) return null;
  return match[1]!;
}

/** 목록에 낼 명령: 숨김(`danger`)을 빼고, 작업 중이면 쉴 때만 쓰는 것을 뺀다 */
export function listed(list: CommandList | null, working: boolean): SlashCommand[] {
  return (list?.commands ?? []).filter((c) => c.kind !== "danger" && !(working && c.idle_only));
}

const BOUNDARY = /[:\-_.]/;

/** 이름 하나와 찾는 글의 순위(작을수록 앞). 맞지 않으면 null */
function nameTier(name: string, q: string): number | null {
  if (name.startsWith(q)) return 0;
  for (let i = 1; i < name.length; i++) if (BOUNDARY.test(name[i - 1]!) && name.startsWith(q, i)) return 1;
  if (name.includes(q)) return 2;
  let at = 0;
  for (const ch of name) if (ch === q[at]) at++;
  return at === q.length ? 3 : null;
}

export function tier(c: SlashCommand, query: string): number | null {
  const q = query.toLowerCase();
  if (q === "") return 0;
  const tiers = [c.name, ...c.aliases].map((n) => nameTier(n.toLowerCase(), q)).filter((t): t is number => t !== null);
  if (tiers.length > 0) return Math.min(...tiers);
  return q.length >= 2 && c.description.toLowerCase().includes(q) ? 4 : null;
}

const SOURCE_ORDER: CommandSource[] = ["builtin", "skill", "user", "project", "plugin"];

/** 순위 순 목록. 같은 순위는 쓴 횟수, 출처(내장 먼저), 이름 순 */
export function rank(commands: readonly SlashCommand[], query: string, usage: Readonly<Record<string, number>> = {}): SlashCommand[] {
  return commands
    .map((c) => ({ c, t: tier(c, query) }))
    .filter((x): x is { c: SlashCommand; t: number } => x.t !== null)
    .sort((a, b) => a.t - b.t
      || (usage[b.c.name] ?? 0) - (usage[a.c.name] ?? 0)
      || SOURCE_ORDER.indexOf(a.c.source) - SOURCE_ORDER.indexOf(b.c.source)
      || a.c.name.localeCompare(b.c.name))
    .map((x) => x.c);
}

export const SOURCE_LABEL: Record<CommandSource, string> = {
  builtin: "기본",
  skill: "skill",
  user: "내 명령",
  project: "프로젝트",
  plugin: "plugin",
};

/** 빈 「/」: 출처별 묶음(herdr-web-ui Built in·User·Project·Skills·Plugins 머리) */
export function groups(commands: readonly SlashCommand[]): { source: CommandSource; items: SlashCommand[] }[] {
  return SOURCE_ORDER.map((source) => ({ source, items: commands.filter((c) => c.source === source) })).filter((g) => g.items.length > 0);
}

/** 보낼 메시지가 harness 명령 한 줄이면 그 명령과 인자가 있는지. 다른 이름(`/quit`)도 찾는다 */
export function commandOf(list: CommandList | null, text: string): { command: SlashCommand; args: boolean } | null {
  const match = /^\/([^\s/]+)(\s+\S[\s\S]*)?$/.exec(text.trim());
  if (!match || !list) return null;
  const name = match[1]!.toLowerCase();
  const command = list.commands.find((c) => c.insert.startsWith("/") && (c.name.toLowerCase() === name || c.aliases.includes(name)));
  return command ? { command, args: match[2] !== undefined } : null;
}

export type SendAction = "send" | "confirm" | "picker" | "screen";

/** 보내기 전·뒤에 할 일: 확인창, 보낸 뒤 터미널 알림(선택창은 인자 없이 보냈을 때만), 그냥 보냄 */
export function sendAction(kind: CommandKind, args: boolean): SendAction {
  if (kind === "confirm" || kind === "danger") return "confirm";
  if (kind === "picker") return args ? "send" : "picker";
  if (kind === "screen") return "screen";
  return "send";
}

const USAGE_KEY = "aaa:slash-usage";

/** 이 기기에서 고른 횟수(herdr-web-ui `herdr-web-ui:slash-usage`). 저장소를 못 쓰면 빈 표 */
export function readUsage(): Record<string, number> {
  try {
    const raw = JSON.parse(localStorage.getItem(USAGE_KEY) ?? "{}");
    return raw && typeof raw === "object" ? raw as Record<string, number> : {};
  } catch {
    return {};
  }
}

export function bumpUsage(name: string): void {
  try {
    const usage = readUsage();
    usage[name] = (usage[name] ?? 0) + 1;
    localStorage.setItem(USAGE_KEY, JSON.stringify(usage));
  } catch {
    // 개인 창·막힌 저장소: 순위만 덜 맞는다
  }
}
