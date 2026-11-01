// herdr-projects inbox의 짧은 event + 대상, herdr-web-ui Droplet의 누르면 원문으로 가는 구조를 따른다.
// 사용자 요청(t-1419): 종류는 활동 막대 아이콘으로, 내용은 한 줄로. 원문은 버리지 않는다.
import type { InboxItem } from "./gen/proto";
import type { DropletNotice } from "./herdr/droplet.ts";

export type NotificationTopic = "asks" | "reports" | "work" | "ideas" | "tasks" | "automations" | "machines" | "workspace";

/** 첫 내용 줄에서 표시용 Markdown만 걷고, 이모지 묶음을 자르지 않는다. */
export function notificationSummary(text: string, limit = 48): string {
  const line = (text.split(/\r?\n/).find(line => line.trim()) ?? "")
    .replace(/^\s*(?:#{1,6}\s+|[-*+]\s+|\d+[.)]\s+)/, "")
    .replace(/^\[(?:완료|진행 중|결정 필요|검수 대기|확인 필요|병합 준비됨|착지)\]\s*/, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/[*`]/g, "").replace(/\s+/g, " ").trim();
  const chars = [...new Intl.Segmenter("ko", { granularity: "grapheme" }).segment(line)].map(part => part.segment);
  return chars.length > limit ? chars.slice(0, limit - 1).join("").trimEnd() + "…" : line;
}

export function dropletContent(notice: DropletNotice): { topic: NotificationTopic; label: string; summary: string; full: string } {
  const topic = notice.topic ?? (notice.target.startsWith("idea=") ? "ideas" : notice.target.startsWith("task=") ? "tasks"
    : notice.target.startsWith("report=") || notice.libraryUrl ? "reports" : notice.kind === "blocked" ? "asks" : "work");
  const label = notice.label ?? (topic === "ideas" ? "아이디어 갱신" : topic === "reports" ? "완료 보고"
    : topic === "tasks" ? "할 일" : notice.kind === "blocked" ? "입력 필요" : notice.kind === "reset" ? "한도 풀림"
    : notice.kind === "ended" ? "터미널 종료" : "작업 완료");
  const original = notice.summary?.trim() || notice.title;
  const subject = notice.subject ? `${notice.subject} · ` : "";
  return { topic, label, summary: subject + notificationSummary(original),
    full: [label, notice.subject, original, original !== notice.title ? notice.title : null, notice.machine].filter(Boolean).join(" · ") };
}

/** host의 구조화된 종류·사건만으로 고른다. 요약 원문의 단어로 종류를 추측하지 않는다. */
export function inboxContent(item: Pick<InboxItem, "item_kind" | "event" | "subject" | "summary">): { topic: NotificationTopic; label: string; summary: string } {
  const { item_kind: kind, event } = item;
  let topic: NotificationTopic = "work";
  if (event === "new report" || kind === "report") topic = "reports";
  else if (event === "report needs user" || event === "blocked on a prompt" || kind === "question" || kind === "routine-approval" || kind === "coordinator-check") topic = "asks";
  else if (kind === "task" || kind === "task-followup") topic = "tasks";
  else if (kind === "idea") topic = "ideas";
  else if (kind === "routine" || kind === "automation" || kind === "deploy_proposal") topic = "automations";
  else if (kind === "outage" || kind === "health" || kind === "session") topic = "machines";
  else if (["system", "config-error", "account", "usage"].includes(kind)) topic = "workspace";
  const labels: Record<string, string> = {
    "new report": "새 보고", "report needs user": "확인 필요", "blocked on a prompt": "입력 필요",
    resolved: "작업 종료", merged: "병합 완료", "task needs check": "할 일 확인", "needs approval": "승인 필요",
    down: "연결 끊김", recovered: "다시 연결됨", "sessions ended": "세션 종료", "not launched": "시작 실패",
    "brief not delivered": "지시 전달 실패", "message not delivered": "메시지 전달 확인", "post notice for user": "새 소식",
  };
  return { topic, label: labels[event] ?? event, summary: notificationSummary(item.summary) };
}
