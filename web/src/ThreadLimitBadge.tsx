import type { ThreadLimitOverride } from "./gen/proto";
import "./ThreadStageBadge.css";

/** 한도 무시 시작의 영속 기록. 현재 열린 수가 줄어도 출처는 남는다. */
export function ThreadLimitBadge({ start }: { start?: ThreadLimitOverride | null }) {
  if (!start) return null;
  const detail = `${start.actor} · ${new Date(start.at_ms).toLocaleString()}${start.user_request ? ` · 사용자 지시: ${start.user_request}` : ""}`;
  return <span className="thread-stage is-landing" data-testid="thread-over-limit" title={detail}>한도 초과로 시작</span>;
}
