// thread 줄의 fast mode 표지(t-1511 시안 1A·2A): 자동 병합 표지와 같은 꼴의 노란 번개. 값은 모델 관측의 speed(agent가 Claude 상태줄
// `fast_mode`·Codex OSC 제목 `fast-mode`에서 읽는다)이고 표준이거나 모르면 그리지 않는다.
import { Zap } from "lucide-react";
import { speedWord } from "./summaryCard";
import "./FastMark.css";

export function FastMark({ speed }: { speed?: string | null }) {
  const word = speedWord(speed);
  if (!word) return null;
  return (
    <span className="fast-mark" role="img" aria-label={word} data-testid="fast-mark" title={word}>
      <Zap aria-hidden="true" />
    </span>
  );
}
