// thread 줄과 모델 요약의 속도 표지: 표준이거나 모르면 그리지 않는다.
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
