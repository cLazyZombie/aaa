// 볼 자료와 답을 한 카드에 둔다. HTML은 기존 보고서와 같은 sandbox, 음원은 자동 재생 없이 카드 안 controls다.
import { useMemo, useRef, useState } from "react";
import type { LibraryView, ThreadQuestion } from "./gen/proto";
import { ApiError, call } from "./api";
import { LIBRARY_SANDBOX } from "./FileViewer";
import { ReportMarkdown } from "./ReportsView";
import { useFrameImages, useLightbox } from "./Lightbox";
import { questionMaterials, type QuestionMaterial } from "./questionMaterials";
import "./QuestionReports.css";
import { ReportVideo } from "./ReportVideo";

type Open = { key: string; state: "loading" } | { key: string; state: "error"; message: string } | { key: string; state: "ready"; view: LibraryView | null; url: string | null };
export function QuestionReports(props: { items: ThreadQuestion[]; body?: string | null; reportId?: string | null; project?: string; base?: string | null }) {
  const materials = useMemo(() => questionMaterials({ ...props, origin: location.origin }), [props.items, props.body, props.reportId, props.project, props.base]);
  const [open, setOpen] = useState<Open | null>(null);
  const lightbox = useLightbox();
  const frame = useRef<HTMLIFrameElement>(null);
  const url = open?.state === "ready" ? open.url : null;
  useFrameImages(frame, url, lightbox.open);
  const toggle = async (m: QuestionMaterial) => {
    if (open?.key === m.key) { setOpen(null); return; }
    if (m.kind === "url") { setOpen({ key: m.key, state: "ready", view: null, url: m.url }); return; }
    setOpen({ key: m.key, state: "loading" });
    try {
      const v = await call({ op: "library_open", project: m.project, name: m.name });
      if (v.kind !== "library") throw new Error("보고서 응답이 아니다");
      const url = v.files.some((f) => f.path === "index.html" && !f.on_agent) ? v.base + "index.html" : null;
      setOpen((now) => now?.key === m.key ? { key: m.key, state: "ready", view: v, url } : now);
    } catch (e) { setOpen((now) => now?.key === m.key ? { key: m.key, state: "error", message: e instanceof ApiError ? e.message : String(e) } : now); }
  };
  if (!materials.length) return null;
  const media = url ? new URL(url, location.origin).pathname.toLowerCase() : "";
  const chosen = materials.find((m) => m.key === open?.key);
  return <div className="question-reports" data-testid="question-reports">
    <div className="question-report-links">{materials.map((m) => <button type="button" className="ghost small" key={m.key} aria-expanded={open?.key === m.key} onClick={() => void toggle(m)} data-testid="question-report-open">보고서 열기 <span>{m.label}</span></button>)}</div>
    {open && <section className="question-report-preview" role="region" aria-label="보고서 미리보기" data-testid="question-report-preview">
      <div className="question-report-tools"><span>{chosen?.label}</span><button type="button" className="ghost small" onClick={() => setOpen(null)}>미리보기 닫기</button>
        {url && <a href={url} target="_blank" rel="noopener noreferrer">새 창</a>}
      </div>
      {open.state === "loading" ? <p role="status">보고서를 읽고 있다…</p> : open.state === "error" ? <p role="alert">{open.message}</p>
        : /\.(wav|mp3|ogg|m4a|flac)$/.test(media) ? <audio controls preload="none" src={url!} data-testid="question-audio" />
        : /\.(mp4|webm|mov)$/.test(media) ? <ReportVideo src={url!} label={chosen?.label ?? "시안 영상"} />
        : /\.(png|jpe?g|svg|webp|gif)$/.test(media) ? <img src={url!} alt={chosen?.label ?? "시안"} />
        : url ? <iframe ref={frame} src={url} title={chosen?.label ?? "보고서"} sandbox={LIBRARY_SANDBOX} referrerPolicy="no-referrer" data-testid="question-report-frame" />
        : open.view?.report ? <ReportMarkdown text={open.view.report} base={open.view.base} onImages={lightbox.open} />
        : <p>이 판에는 본문이나 index.html이 없다.</p>}
    </section>}
    {lightbox.node}
  </div>;
}
