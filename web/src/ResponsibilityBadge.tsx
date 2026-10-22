import type { Responsibility } from "./gen/proto";
import "./ThreadStageBadge.css";

export function ResponsibilityBadge({ value }: { value?: Responsibility | null }) {
  if (!value || value.state === "launcher") return null;
  return <span className="thread-stage is-landing" data-testid="responsibility-badge"
    title={value.state === "detached" ? "개발자 도구 책임 경로가 launcher와 연결되지 않았다. 보고에서 정리 후 새 pane으로 이어 한다" : "개발자 도구 책임 경로를 확인하지 못했다"}>
    {value.state === "detached" ? "책임 연결 끊김" : "책임 확인 못함"}
  </span>;
}
