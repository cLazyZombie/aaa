import { useEffect, useRef, useState } from "react";
import { formatBytes } from "./formatBytes";
import { readReportMediaStatus, reportMediaTarget, type MediaStatus } from "./reportMediaProbe";

/** 이미지 로딩/실패 또는 영상 실패에서만 제공한다. 파일 확인이 매체 bytes를 다시 받지는 않는다. */
export function ReportMediaStatus({ src, image }: { src: string; image?: () => HTMLImageElement | null }) {
  const target = reportMediaTarget(src, location.origin);
  const controller = useRef<AbortController | null>(null);
  const [state, setState] = useState<{ src: string; busy: boolean; result?: MediaStatus; image?: string } | null>(null);
  useEffect(() => () => controller.current?.abort(), [src]);
  if (!target) return null;
  const current = state?.src === src ? state : null;
  return <span className="report-media-status">
    <button type="button" className="ghost small" disabled={current?.busy} onClick={() => {
      controller.current?.abort();
      const abort = new AbortController(); controller.current = abort;
      const img = image?.();
      const picture = img ? img.naturalWidth > 0 ? `디코드 ${img.naturalWidth}×${img.naturalHeight}`
        : img.complete ? "이미지 읽기 실패" : "이미지 완료 전(지연 로딩 또는 요청 진행 중)" : undefined;
      setState({ src, busy: true });
      void readReportMediaStatus(target.url, abort.signal).then((result) => {
        if (!abort.signal.aborted) setState({ src, busy: false, result, image: picture });
      }, () => { /* 화면/주소가 바뀐 요청의 취소는 표시하지 않는다. */ });
    }}>상태 확인</button>
    {current?.result && <span role="status" className="report-media-result">
      <span>{target.path}</span>
      <span>{current.result.status === null ? "HTTP 응답 없음" : `HTTP ${current.result.status}`}{current.result.size !== null && ` · ${formatBytes(current.result.size)}`}</span>
      <span>{current.result.message}</span>
      {current.image && <span>{current.image}</span>}
    </span>}
  </span>;
}
