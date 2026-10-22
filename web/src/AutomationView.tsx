// 자동화 본문 화면(t-0628 시안 1A·3A): 사이드바 카드를 누르면 지금 대화창 자리에 그 규칙 설정(RuleDetail)이, 「고치기」·「새 자동화」를
// 누르면 같은 자리에 편집(AutomationEditor)이 나온다. 주소는 `#automation=<project>/<name>`(새 자동화는 이름 없이 `<project>/`).
// 「답할 것」처럼 한 번 연 뒤에는 숨긴 채 그려 둔다: 다른 화면에 다녀와도 쓰던 편집 초안이 그대로다.
import { useState } from "react";
import type { ThreadView } from "./gen/proto";
import { AutomationEditor } from "./AutomationEditor";
import { AutomationPage, RuleDetail } from "./AutomationDetail";
import { canEditAutomations } from "./automationForm";
import { openThreads, type Automations } from "./automationData";

export type AutomationTarget = { project: string; name: string | null };

/** 고른 것(`automation:<project>/<name>`)의 대상. 프로젝트 이름에는 /가 없다. 이름이 비면 새 자동화 */
export function automationTarget(sel: string | null): AutomationTarget | null {
  if (!sel?.startsWith("automation:")) return null;
  const rest = sel.slice("automation:".length);
  const at = rest.indexOf("/");
  if (at <= 0) return null;
  return { project: rest.slice(0, at), name: rest.slice(at + 1) || null };
}

export function AutomationView({ shown, target, data, canControl, threads, onClose, onOpen, onThread }: {
  shown: boolean;
  target: AutomationTarget | null;
  data: Automations;
  canControl: boolean;
  threads: ThreadView[];
  /** 본문을 닫고 앞 화면으로 */
  onClose: () => void;
  /** 다른 규칙 설정으로(`replace`: 지금 주소 칸을 바꾼다, 새로 만든 규칙) */
  onOpen: (project: string, name: string, replace?: boolean) => void;
  onThread: (project: string, id: string) => void;
}) {
  const { rows, loading, frontApi, load } = data;
  // 편집 중인 규칙(`<project>/<name>`). 다른 규칙을 고르면 그 규칙은 설정 화면이다
  const [editing, setEditing] = useState<string | null>(null);
  const canEdit = canEditAutomations(frontApi);
  const project = target?.project ?? "";
  const key = target ? `${target.project}/${target.name ?? ""}` : "";
  const rule = target?.name ? rows.find((r) => r.project === project)?.rules.find((r) => r.name === target.name) : undefined;
  const page = (title: string, text: string, testid: string) =>
    <AutomationPage title={title} onClose={onClose} testid={testid}><p className="automation-dim">{text}</p></AutomationPage>;
  let body;
  if (!target) body = page("자동화", "왼쪽 「자동화」 목록에서 자동화를 고른다.", "automation-empty");
  else if (target.name === null)
    body = canControl && canEdit
      ? <AutomationEditor key={key} project={project} onClose={onClose} onSaved={async (name) => { await load(); onOpen(project, name, true); }} />
      : page("새 자동화", frontApi === undefined ? "읽는 중…" : "이 기기나 host에서는 새 자동화를 만들 수 없다.", "automation-empty");
  // 편집 중에 다른 기기가 지워도 편집 화면은 남는다: 저장하면 host가 「지워졌다」 충돌로 알린다
  else if (editing === key && !rule?.web_read_only_reason)
    // 닫을 때도 다시 읽는다: 그 사이 지워졌으면 바로 「지워졌다」 안내다
    body = <AutomationEditor key={key} project={project} name={target.name} onClose={() => { setEditing(null); void load(); }} onSaved={async () => { await load(); setEditing(null); }} />;
  else if (!rule)
    body = loading
      ? page(target.name, "읽는 중…", "automation-empty")
      : page(target.name, `${project}/${target.name}은 지워졌거나 이름이 바뀌었습니다. 왼쪽 목록에서 다른 자동화를 고른다.`, "automation-missing");
  else
    body = <RuleDetail key={key} project={project} rule={rule} canControl={canControl} canEdit={canEdit} running={openThreads(threads, project, rule.name)}
      onEdit={() => setEditing(key)} onChanged={load} onThread={(id) => onThread(project, id)} onClose={onClose} onDeleted={onClose} />;
  return <div className={`automation-screen${shown ? "" : " is-hidden"}`} inert={!shown} data-testid="automation-view" data-shown={shown ? "true" : "false"}>{body}</div>;
}
