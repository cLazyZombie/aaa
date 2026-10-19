// herdr-web-ui ConversationPage: 파일 정체성이 같을 때만 기존 앞 쪽과 최신 쪽을 합친다.
import type { ChatMessage, ChatView } from "./gen/proto";

export function mergeChatPage(current: ChatView | null, incoming: ChatView, older = false): ChatView {
  if (!incoming.history || current?.history !== incoming.history) return incoming;
  const byId = new Map<string, ChatMessage>();
  for (const m of current.messages) if (m.id) byId.set(m.id, m);
  for (const m of incoming.messages) if (m.id) byId.set(m.id, m);
  const messages = [...byId.values()].sort((a, b) => a.id! < b.id! ? -1 : a.id! > b.id! ? 1 : 0);
  // 최신 응답의 pending만 산다. 앞 쪽에는 pending을 보내지 않는다.
  messages.push(...(older ? current : incoming).messages.filter((m) => !m.id));
  const first = current.messages.find((m) => m.id)?.id;
  const added = incoming.messages.find((m) => m.id)?.id;
  // 늦은 이전 쪽 응답이 이미 도달한 처음/더 앞선 cursor를 되돌리지 않는다.
  const extendsStart = !first || (added ? added < first : older);
  const before = current.before === null || !extendsStart ? current.before : incoming.before;
  return { ...incoming, messages, before, truncated: before != null };
}
