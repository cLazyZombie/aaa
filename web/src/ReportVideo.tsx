import { diagnosticReason, logIgnoredFailure } from "./errorDiagnostic.ts";
import { useRef, useState } from "react";

/** 소리·새 탭 없이 첫 누름에서 재생한다. native controls로 멈춤·탐색·음소거 해제를 제공한다. */
export function ReportVideo({ src, label }: { src: string; label: string }) {
  const [failed, setFailed] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const video = useRef<HTMLVideoElement>(null);
  return <span className="markdown-video-wrap">
    <video ref={video} className="markdown-video" aria-label={label || "시안 영상"} src={src} controls muted playsInline preload="metadata"
      onError={() => setFailed(`미디어 오류 ${video.current?.error?.code ?? "unknown"}`)} onLoadedMetadata={() => setFailed(null)}
      onPlay={() => setPlaying(true)} onPause={() => setPlaying(false)} onEnded={() => setPlaying(false)} />
    {!playing && !failed && <button type="button" className="markdown-video-play" aria-label={`${label || "시안 영상"} 재생`}
      onClick={() => { void video.current?.play().catch((error) => { logIgnoredFailure(error, "영상 재생"); setFailed(diagnosticReason(error)); }); }}>▶ 재생</button>}
    {failed && <span role="alert">영상을 재생할 수 없습니다. 파일 형식이나 읽기 권한을 확인하세요. ({failed})</span>}
  </span>;
}
