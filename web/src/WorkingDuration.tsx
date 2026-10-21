// 목록 줄의 작업 시간(plan §8.266). t3code(MIT) `apps/web/src/components/Sidebar.tsx:860` `WorkingDurationLabel`(1451e7cb2):
// 스스로 초마다 바뀌어 줄 전체가 아니라 이 글자만 다시 그리고, 공유 시계(nowClock.ts)라 작업 중인 모든 줄이 같은 frame에 바뀐다.
// 화면 읽기 프로그램이 초마다 읽지 않게 aria-hidden이다(참고 2356~2363줄).
import { useNowSecond } from "./nowClock";
import { formatWorkingDuration } from "./sidebarModel";

export function WorkingDuration({ startedMs }: { startedMs: number | null }) {
  if (startedMs === null) return null;
  return <Ticking startedMs={startedMs} />;
}

function Ticking({ startedMs }: { startedMs: number }) {
  const now = useNowSecond();
  return (
    <span className="working-duration" aria-hidden="true" title="현재 작업 상태에 들어온 뒤 경과 시간" data-testid="working-duration">
      {formatWorkingDuration(now - startedMs)}
    </span>
  );
}
