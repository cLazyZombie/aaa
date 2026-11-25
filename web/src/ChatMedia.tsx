import { useCallback, useEffect, useRef, useState } from "react";
import { CirclePlay } from "lucide-react";
import { useLightbox } from "./Lightbox";
import { reportMarkdownImage } from "./reportImages";
import { isVideoPath } from "./reportMedia";

function chatMediaUrl(src: string): string | null {
  if (/[?#]/.test(src)) return null;
  const base = "/api/v1/chat-media/";
  const url = reportMarkdownImage(base, src);
  if (!url || !/^r-\d{4,}\/.+\.(png|jpe?g|gif|webp|mp4|webm)$/i.test(url.slice(base.length))) return null;
  return url;
}

/** 사용자 업로드와 같은 크기의 미리보기에서 보고 사본의 원본을 연다. */
export function useChatMedia(active: boolean) {
  const lightbox = useLightbox();
  useEffect(() => {
    if (!active) lightbox.close();
  }, [active, lightbox.close]);
  const render = useCallback((src: string, alt: string) => {
    const url = chatMediaUrl(src);
    if (!url) return null;
    const video = isVideoPath(url);
    const label = alt || (video ? "첨부 영상" : "첨부 그림");
    return <ChatMediaThumb key={url} url={url} label={label} video={video} expanded={lightbox.shown}
      onOpen={() => lightbox.open({ items: [{ src: url, alt: label, video: video || undefined }], at: 0 })} />;
  }, [lightbox.open, lightbox.shown]);
  return { render, node: active ? lightbox.node : null };
}

function ChatMediaThumb({ url, label, video, expanded, onOpen }: {
  url: string; label: string; video: boolean; expanded: boolean; onOpen: () => void;
}) {
  const [failed, setFailed] = useState(false);
  const [poster, setPoster] = useState<string | null>(null);
  const preview = useRef<HTMLVideoElement>(null);
  const releasePreview = (element: HTMLVideoElement) => {
    element.removeAttribute("src");
    element.load();
  };
  return <button type="button" className="chat-attachment-thumb chat-reply-media" title={label}
    data-video={video || undefined} aria-label={`${label} ${video ? "크게 재생" : "확대"}`} disabled={failed} onClick={() => {
      if (preview.current) releasePreview(preview.current);
      onOpen();
    }}>
    {failed ? <span role="status">{video ? "영상을 표시할 수 없음" : "이미지를 표시할 수 없음"}</span>
      : video ? <>
        {poster ? <img src={poster} alt="" /> : (
          <video ref={preview} src={expanded ? undefined : `${url}#t=0.001`} preload="metadata" muted playsInline aria-hidden="true"
            onError={() => setFailed(true)} onLoadedData={(event) => {
              const frame = event.currentTarget;
              if (!frame.videoWidth || !frame.videoHeight) return;
              const scale = Math.min(1, 320 / Math.max(frame.videoWidth, frame.videoHeight));
              const canvas = document.createElement("canvas");
              canvas.width = Math.max(1, Math.round(frame.videoWidth * scale));
              canvas.height = Math.max(1, Math.round(frame.videoHeight * scale));
              const context = canvas.getContext("2d");
              if (!context) return;
              // 실제 재생기와 같은 영상이 미리보기에서 계속 열려 있으면 WebKit이 탐색을 끝내지 못할 수 있다.
              context.drawImage(frame, 0, 0, canvas.width, canvas.height);
              setPoster(canvas.toDataURL("image/jpeg"));
              releasePreview(frame);
            }} />
        )}
        <CirclePlay aria-hidden="true" />
      </> : <img src={url} alt={label} loading="lazy" onError={() => setFailed(true)} />}
  </button>;
}
