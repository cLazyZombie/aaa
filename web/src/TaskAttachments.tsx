// task 첨부(plan §8.319, 사용자 10-20 「할일에 이미지 첨부할수있게 해」·「붙여넣기도 지원하게」·「폰에서 캡쳐 후 복사해서 바로 붙이게」·
// 「md·html 문서도 task에 첨부(링크)」). 메모의 첨부 줄(tasksModel `attachmentsOf`)을 그림은 썸네일 줄, 문서·링크는 줄 목록으로 보인다.
// 그림을 누르면 크게 보고, 문서는 파일 보기(md는 그려서, html은 보고서처럼 sandbox 페이지)로, 바깥 주소는 새 창으로 연다. 조작 기기는
// 붙여 넣기(⌘V·Ctrl+V, 휴대폰은 「붙여넣기 칸」을 길게 눌러 나오는 시스템 「붙여넣기」)·끌어 놓기·파일 고르기·「문서·링크」(주소·보고서
// 문서 고르기)로 붙이고 ×로 뗀다. 썸네일 줄·×·업로드 중 칸은 GitHub·Linear의 설명 칸 첨부, 붙여 넣기 추출은 herdr-web-ui Composer
// (이 저장소 ChatComposer.tsx `onPaste`·`normalizeComposerImageFile`)를 따른다.
//
// 휴대폰(iOS Safari)은 편집할 수 있는 요소에서만 길게 누름 메뉴에 「붙여넣기」가 뜬다: 입력칸이 없는 상세에도 붙일 수 있게 글은 받지
// 않는 contenteditable 칸(`inputmode="none"`: 자판이 뜨지 않는다)을 둔다. 「클립보드에서」 단추는 `navigator.clipboard.read()`로
// 읽는다(iOS는 그 자리에 「붙여넣기」 허용 말풍선을 띄운다).
import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent } from "react";
import { createPortal } from "react-dom";
import { ChevronLeft, ChevronRight, ClipboardPaste, ExternalLink, FileCode, FileText, Globe, ImagePlus, Link2, Loader2, X } from "lucide-react";
import type { LibraryFile, LibraryReport } from "./gen/proto";
import { call } from "./api";
import { usePhoneSheetBack } from "./herdr/sheetHistory";
import { normalizeComposerImageFile } from "./herdr/composerImageMime";
import { ATTACH_TYPES, DOC_ACCEPT, attachmentsOf, linkFromText, reportDoc, taskImageUrl, type TaskAttachment } from "./tasksModel";

/** 올리는 중인 파일 칸(그림은 이 기기의 object URL로 미리 본다) */
export type PendingUpload = { id: number; preview: string | null; name: string };

/** 클립보드·끌어 놓기 자료의 파일들(이름 없는 붙여 넣은 그림에 형식을 붙인다) */
export function filesOf(data: DataTransfer | null): File[] {
  if (!data) return [];
  const fromItems = Array.from(data.items ?? [])
    .filter((item) => item.kind === "file")
    .map((item) => item.getAsFile())
    .filter((f): f is File => f !== null);
  const files = fromItems.length > 0 ? fromItems : Array.from(data.files ?? []);
  return files.map(normalizeComposerImageFile);
}

/** 붙여 넣은 글이 주소·보고서 문서 경로 하나면 링크 첨부 */
export function linkOf(data: DataTransfer | null): TaskAttachment | null {
  return data ? linkFromText(data.getData("text/plain")) : null;
}

/** `navigator.clipboard.read()`의 그림들. 읽을 수 없는 브라우저면 null */
async function clipboardImages(): Promise<File[] | null> {
  const clip = navigator.clipboard as Clipboard | undefined;
  if (!clip || typeof clip.read !== "function") return null;
  const items = await clip.read();
  const out: File[] = [];
  for (const item of items) {
    const type = item.types.find((t) => (ATTACH_TYPES as readonly string[]).includes(t));
    if (!type) continue;
    const blob = await item.getType(type);
    const ext = type === "image/jpeg" ? "jpg" : type.split("/")[1];
    out.push(new File([blob], `clipboard.${ext}`, { type }));
  }
  return out;
}

const DOC_ICON = { upload: FileText, report: FileCode, url: Globe } as const;
const DOC_WORD = { upload: "올린 문서", report: "보고서 문서", url: "바깥 주소" } as const;

export function TaskAttachments({
  project,
  notes,
  saved,
  canEdit,
  busy,
  pending,
  onFiles,
  onLink,
  onRemove,
  onOpen,
  onCover,
}: {
  project: string;
  /** 지금 메모(폼이면 고치는 중인 글) */
  notes: string;
  /** 파일에 저장된 메모: 여기 없는 첨부는 「저장 전」이다 */
  saved?: string;
  canEdit: boolean;
  busy?: boolean;
  pending?: readonly PendingUpload[];
  onFiles?: (files: File[]) => void;
  onLink?: (a: TaskAttachment) => void;
  onRemove?: (a: TaskAttachment) => void;
  /** 문서 열기(파일 보기). 없으면 새 창으로 연다 */
  onOpen?: (a: TaskAttachment) => void;
  /** 그림 크게 보기가 떴다·닫혔다(상세 시트가 그동안 휴대폰 뒤로 기록을 넘긴다, sheetHistory `usePhoneSheetBack`) */
  onCover?: (covered: boolean) => void;
}) {
  const all = attachmentsOf(notes);
  const images = all.filter((a) => a.kind === "image");
  const docs = all.filter((a) => a.kind !== "image");
  const stored = saved === undefined ? null : new Set(attachmentsOf(saved).map((a) => a.target));
  const [open, setOpen] = useState<number | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [linking, setLinking] = useState(false);
  const picker = useRef<HTMLInputElement>(null);
  const canRead = typeof navigator !== "undefined" && typeof navigator.clipboard?.read === "function";
  const viewing = open !== null && images[open] !== undefined;
  useEffect(() => {
    onCover?.(viewing);
  }, [viewing, onCover]);
  useEffect(() => () => onCover?.(false), [onCover]);
  const unsaved = (a: TaskAttachment) => stored !== null && !stored.has(a.target);
  const paste = (e: ClipboardEvent<HTMLDivElement>) => {
    // 이 칸의 붙여 넣기는 파일과 주소만 받는다(그 밖의 글은 버린다). 상세 밖 붙여 넣기 처리(TaskForm)가 다시 받지 않게 막는다
    e.preventDefault();
    const files = filesOf(e.clipboardData);
    const link = files.length === 0 ? linkOf(e.clipboardData) : null;
    if (files.length === 0 && !link) {
      setNote("클립보드에 그림·문서·주소가 없다. 스크린샷을 「복사」한 뒤 붙여 넣는다");
      return;
    }
    setNote(null);
    if (link) onLink?.(link);
    else onFiles?.(files);
  };
  const remove = (a: TaskAttachment, i: number, what: string) =>
    canEdit && onRemove ? (
      <button type="button" className="task-attach-remove" aria-label={`${what} ${i + 1} 떼기`} title="떼기(파일은 남아 기록에서 되살릴 수 있다)" disabled={busy} data-testid="task-attach-remove" onClick={() => onRemove(a)}>
        <X aria-hidden="true" />
      </button>
    ) : null;
  if (!canEdit && all.length === 0) return null;
  return (
    <section className="task-detail-section task-attach" data-testid="task-attach" data-count={all.length} aria-label="첨부">
      <h3>
        첨부 <span className="tasks-count">{all.length}</span>
      </h3>
      {(images.length > 0 || (pending?.length ?? 0) > 0) && (
        <ul className="task-attach-grid" aria-label="첨부 그림">
          {images.map((a, i) => (
            <li key={a.target} className="task-attach-item" data-path={a.target} data-testid="task-attach-item">
              <button type="button" className="task-attach-thumb" aria-label={`그림 ${i + 1} 크게 보기`} onClick={() => setOpen(i)}>
                <img src={taskImageUrl(project, a.target)} alt={a.label} loading="lazy" decoding="async" />
              </button>
              {unsaved(a) && <span className="task-attach-badge">저장 전</span>}
              {remove(a, i, "그림")}
            </li>
          ))}
          {pending?.map((p) => (
            <li key={`up-${p.id}`} className="task-attach-item is-pending" data-testid="task-attach-pending">
              <span className="task-attach-thumb">
                {p.preview ? <img src={p.preview} alt="" /> : <FileText className="task-attach-file" aria-hidden="true" />}
                <Loader2 className="task-attach-spin" aria-hidden="true" />
              </span>
              <span className="sr-only" role="status">
                {p.name} 올리는 중
              </span>
            </li>
          ))}
        </ul>
      )}
      {docs.length > 0 && (
        <ul className="task-docs" aria-label="첨부 문서·링크">
          {docs.map((a, i) => {
            const Icon = DOC_ICON[a.kind as keyof typeof DOC_ICON];
            const where = a.kind === "report" ? `보고서 ${reportDoc(a.target)?.name ?? ""} · ${reportDoc(a.target)?.path ?? ""}` : a.kind === "url" ? a.target : DOC_WORD.upload;
            return (
              <li key={a.target} className="task-doc" data-kind={a.kind} data-target={a.target} data-testid="task-doc">
                <button
                  type="button"
                  className="task-doc-open"
                  aria-label={`${a.label} 열기(${DOC_WORD[a.kind as keyof typeof DOC_WORD]})`}
                  data-testid="task-doc-open"
                  onClick={() => {
                    if (a.kind === "url" || !onOpen) window.open(a.kind === "url" ? a.target : taskImageUrl(project, a.target), "_blank", "noopener,noreferrer");
                    else onOpen(a);
                  }}
                >
                  <Icon aria-hidden="true" />
                  <span className="task-doc-text">
                    <span className="task-doc-label">{a.label}</span>
                    <span className="task-doc-where">{where}</span>
                  </span>
                  {a.kind === "url" && <ExternalLink className="task-doc-ext" aria-hidden="true" />}
                </button>
                {unsaved(a) && <span className="task-attach-badge is-inline">저장 전</span>}
                {remove(a, i, "문서")}
              </li>
            );
          })}
        </ul>
      )}
      {canEdit && onFiles && (
        <div className="task-attach-add">
          <div
            className="task-attach-paste"
            contentEditable={!busy}
            suppressContentEditableWarning
            inputMode="none"
            role="textbox"
            aria-label="붙여넣기 칸"
            aria-multiline="false"
            data-placeholder="여기를 길게 눌러 붙여넣기 · ⌘V"
            data-testid="task-attach-paste"
            tabIndex={0}
            spellCheck={false}
            onPaste={paste}
            // 글은 받지 않는다(자판·받아쓰기로 들어온 것도 지운다)
            onBeforeInput={(e) => e.preventDefault()}
            onInput={(e) => {
              e.currentTarget.textContent = "";
            }}
            onDrop={(e) => {
              e.preventDefault();
              onFiles(filesOf(e.dataTransfer));
            }}
          />
          <button type="button" className="btn" disabled={busy} data-testid="task-attach-pick" onClick={() => picker.current?.click()}>
            <ImagePlus aria-hidden="true" />
            사진·파일
          </button>
          {onLink && (
            <button type="button" className="btn" disabled={busy} aria-expanded={linking} data-testid="task-attach-link" onClick={() => setLinking((v) => !v)}>
              <Link2 aria-hidden="true" />
              문서·링크
            </button>
          )}
          {canRead && (
            <button
              type="button"
              className="btn btn-ghost"
              disabled={busy}
              data-testid="task-attach-clipboard"
              onClick={() => {
                setNote(null);
                clipboardImages().then(
                  (files) => {
                    if (!files || files.length === 0) setNote("클립보드에 그림이 없다");
                    else onFiles(files);
                  },
                  (e: unknown) => setNote(e instanceof Error && e.name === "NotAllowedError" ? "클립보드 읽기를 허용하지 않았다. 붙여넣기 칸을 길게 눌러 붙여 넣는다" : `클립보드를 읽지 못했다: ${e instanceof Error ? e.message : String(e)}`),
                );
              }}
            >
              <ClipboardPaste aria-hidden="true" />
              클립보드에서
            </button>
          )}
          <input
            ref={picker}
            type="file"
            accept={`${ATTACH_TYPES.join(",")},${DOC_ACCEPT}`}
            multiple
            hidden
            data-testid="task-attach-input"
            onChange={(e) => {
              const files = Array.from(e.target.files ?? []).map(normalizeComposerImageFile);
              e.target.value = "";
              onFiles(files);
            }}
          />
        </div>
      )}
      {canEdit && linking && onLink && (
        <LinkForm
          project={project}
          busy={busy}
          onCancel={() => setLinking(false)}
          onAdd={(a) => {
            setLinking(false);
            onLink(a);
          }}
        />
      )}
      {canEdit && <p className="muted task-attach-help">그림(PNG·JPEG·GIF·WebP)·md·html, 한 개 16MiB까지. 상세가 열린 동안 어디서든 붙여 넣거나 끌어다 놓아도 이 task에 붙는다.</p>}
      {note && (
        <p className="task-form-error" role="alert" data-testid="task-attach-note">
          {note}
        </p>
      )}
      {open !== null && images[open] && <Viewer project={project} list={images} at={open} onAt={setOpen} onClose={() => setOpen(null)} />}
    </section>
  );
}

/** 「문서·링크」: 주소나 `library/<보고서>/<경로>`를 넣거나, 이 프로젝트 보고서에서 md·html 문서를 고른다 */
function LinkForm({ project, busy, onAdd, onCancel }: { project: string; busy?: boolean; onAdd: (a: TaskAttachment) => void; onCancel: () => void }) {
  const [target, setTarget] = useState("");
  const [label, setLabel] = useState("");
  const [reports, setReports] = useState<LibraryReport[] | null>(null);
  const [report, setReport] = useState("");
  const [files, setFiles] = useState<LibraryFile[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    call({ op: "library_reports", project })
      .then((r) => live && setReports(r.kind === "library_reports" ? r.reports.filter((x) => x.project === project && !x.problem) : []))
      .catch(() => live && setReports([]));
    return () => {
      live = false;
    };
  }, [project]);
  useEffect(() => {
    if (!report) return;
    let live = true;
    setFiles(null);
    call({ op: "library_open", project, name: report })
      .then((r) => live && setFiles(r.kind === "library" ? r.files.filter((f) => /\.(md|html?)$/i.test(f.path)) : []))
      .catch(() => live && setFiles([]));
    return () => {
      live = false;
    };
  }, [project, report]);
  const link = linkFromText(target, label);
  return (
    <div
      className="task-link-form"
      role="group"
      aria-label="문서·링크 붙이기"
      data-testid="task-doclink-form"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onCancel();
        }
      }}
    >
      {reports !== null && reports.length > 0 && (
        <div className="task-field-row">
          <label className="task-field">
            <span>보고서</span>
            <select className="input" value={report} onChange={(e) => setReport(e.target.value)} data-testid="task-doclink-report">
              <option value="">고르기…</option>
              {[...reports]
                .sort((a, b) => b.updated_at_ms - a.updated_at_ms)
                .map((r) => (
                  <option key={r.name} value={r.name}>
                    {r.title ? `${r.title} (${r.name})` : r.name}
                  </option>
                ))}
            </select>
          </label>
          {report && (
            <label className="task-field">
              <span>문서</span>
              <select
                className="input"
                value=""
                disabled={files === null || files.length === 0}
                onChange={(e) => {
                  if (!e.target.value) return;
                  setTarget(`library/${report}/${e.target.value}`);
                  setProblem(null);
                }}
                data-testid="task-doclink-file"
              >
                <option value="">{files === null ? "읽는 중…" : files.length === 0 ? "md·html 문서 없음" : "고르기…"}</option>
                {files?.map((f) => (
                  <option key={f.path} value={f.path}>
                    {f.path}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
      )}
      <label className="task-field">
        <span>주소나 보고서 문서 경로</span>
        <input className="input" value={target} inputMode="url" placeholder="https://… 또는 library/t-0482/plan.md" onChange={(e) => setTarget(e.target.value)} data-testid="task-doclink-target" />
      </label>
      <label className="task-field">
        <span>제목(비우면 파일 이름)</span>
        <input className="input" value={label} onChange={(e) => setLabel(e.target.value)} data-testid="task-doclink-label" />
      </label>
      {problem && (
        <p className="task-form-error" role="alert">
          {problem}
        </p>
      )}
      <div className="task-form-actions">
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy || target.trim() === ""}
          data-testid="task-doclink-add"
          onClick={() => {
            if (!link) setProblem("http(s) 주소나 library/<보고서>/<경로>.md·.html만 붙인다");
            else onAdd(link);
          }}
        >
          붙이기
        </button>
        <button type="button" className="btn btn-ghost" onClick={onCancel}>
          닫기
        </button>
      </div>
    </div>
  );
}

/** 크게 보기: 화면을 덮고 Esc·바깥 누름·휴대폰 뒤로가 닫는다. 여럿이면 ←→로 넘긴다 */
function Viewer({ project, list, at, onAt, onClose }: { project: string; list: readonly TaskAttachment[]; at: number; onAt: (i: number) => void; onClose: () => void }) {
  usePhoneSheetBack(() => onClose());
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    close.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      } else if (e.key === "ArrowLeft" && at > 0) onAt(at - 1);
      else if (e.key === "ArrowRight" && at < list.length - 1) onAt(at + 1);
    };
    // 상세의 Esc(닫기)보다 먼저 받는다
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [at, list.length, onAt, onClose]);
  const a = list[at];
  if (!a) return null;
  const url = taskImageUrl(project, a.target);
  // 상세 시트(휴대폰 전체 화면 시트는 transform이 있다) 밖, 문서 맨 위에 그린다: 시트 안이면 시트 머리와 겹친다
  return createPortal(
    <div className="task-viewer" role="dialog" aria-modal="true" aria-label={`그림 ${at + 1}/${list.length}`} data-testid="task-viewer" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="task-viewer-bar">
        <span>
          {at + 1} / {list.length}
        </span>
        <a className="btn btn-ghost" href={url} target="_blank" rel="noreferrer">
          <ExternalLink aria-hidden="true" />
          원본
        </a>
        <button ref={close} type="button" className="btn btn-ghost" aria-label="닫기" data-testid="task-viewer-close" onClick={onClose}>
          <X aria-hidden="true" />
        </button>
      </div>
      <img className="task-viewer-img" src={url} alt={a.label} onClick={(e) => e.stopPropagation()} />
      {at > 0 && (
        <button type="button" className="task-viewer-nav is-prev" aria-label="앞 그림" onClick={() => onAt(at - 1)}>
          <ChevronLeft aria-hidden="true" />
        </button>
      )}
      {at < list.length - 1 && (
        <button type="button" className="task-viewer-nav is-next" aria-label="다음 그림" onClick={() => onAt(at + 1)}>
          <ChevronRight aria-hidden="true" />
        </button>
      )}
    </div>,
    document.body,
  );
}

/** 끌어 놓기에 파일이 실렸는가(글·링크 끌기는 받지 않는다) */
export function dragHasFiles(e: DragEvent): boolean {
  return Array.from(e.dataTransfer?.types ?? []).includes("Files");
}
