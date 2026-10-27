import { diagnosticReason, logIgnoredFailure } from "./errorDiagnostic.ts";
import { useRef, useState } from "react";
import { flushSync } from "react-dom";
import { ReportMediaStatus } from "./ReportMediaStatus";

/** 첫 누름에서 주소를 넣어 재생한 뒤 native controls로 멈춤·탐색·음소거 해제를 제공한다. */
export function ReportVideo({ src, label }: { src: string; label: string }) {
  const [requested, setRequested] = useState<string | null>(null);
  const [failure, setFailure] = useState<{ src: string; reason: string } | null>(null);
  const failed = failure?.src === src ? failure.reason : null;
  const armed = requested === src;
  const video = useRef<HTMLVideoElement>(null);
  return <span className="markdown-video-wrap">
    <video key={src} ref={video} className="markdown-video" aria-label={label || "시안 영상"} src={armed ? src : undefined} controls={armed} muted playsInline preload="none"
      onError={() => setFailure({ src, reason: `미디어 오류 ${video.current?.error?.code ?? "unknown"}` })} onLoadedMetadata={() => setFailure(null)} />
    {!armed && !failed && <button type="button" className="markdown-video-play" aria-label={`${label || "시안 영상"} 재생`}
      onClick={() => {
        // 주소 없는 native 재생 단추는 동작하지 않는다. 첫 단추만 보이고,
        // iOS의 사용자 누름 안에서 src·controls commit과 play를 끝낸다.
        flushSync(() => setRequested(src));
        void video.current?.play().catch((error) => { logIgnoredFailure(error, "영상 재생"); setFailure({ src, reason: diagnosticReason(error) }); });
      }}>▶ 재생</button>}
    {failed && <span role="alert">영상을 재생할 수 없습니다. 파일 형식이나 읽기 권한을 확인하세요. ({failed})<ReportMediaStatus src={src} /></span>}
  </span>;
}
