import type { Responsibility } from "./gen/proto";
import "./ThreadStageBadge.css";

export function ResponsibilityBadge({ value }: { value?: Responsibility | null }) {
  if (!value || value.state === "launcher") return null;
  return <span className="thread-stage is-landing" data-testid="responsibility-badge"
    title={value.state === "detached" ? "macOS가 이 pane의 빌드·실행을 매번 검사해 느려지거나 강제 종료될 수 있다. 정리 후 새 pane으로 옮기면 풀린다." : "이 pane의 빌드 검사 예외 여부를 확인하지 못했다."}>
    {value.state === "detached" ? "느린 빌드" : "느린 빌드?"}
  </span>;
}
