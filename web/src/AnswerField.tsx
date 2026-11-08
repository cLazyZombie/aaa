// 질문 카드의 직접 답 칸(t-1635, 사용자 11-08 「멀티라인이 가능하게 … enter누르면 다음줄로」·「이미지도 붙일수 있게」).
// Enter는 줄바꿈(휴대폰 자판도 같다), 보내기는 단추나 ⌘/Ctrl+Enter다. 한글 조합 중의 Enter는 브라우저 textarea가 조합을
// 끝내는 데만 쓰고 보내지 않는다(`isComposing`·keyCode 229). 그림은 붙여 넣기·끌어 놓기·「그림」 단추로 프로젝트 uploads/에
// 올려(할 일 첨부 `uploadTaskFile`과 같은 길) 답 글 끝에 그림 줄로 적는다. 입력칸은 그림 줄을 뺀 글만, 그림은 그 아래
// 썸네일(누르면 크게, ×로 떼기)로 보인다. 썸네일·올리는 중 칸·붙여 넣기 추출은 할 일 첨부(TaskAttachments)를 따른다.
import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent, type Ref } from "react";
import { ImagePlus, Loader2, X } from "lucide-react";
import { ApiError, uploadTaskFile } from "./api";
import { answerBody, answerImages, pickAnswerImages, withAnswerBody } from "./answerImages";
import { normalizeComposerImageFile } from "./herdr/composerImageMime";
import { filesOf } from "./TaskAttachments";
import { ATTACH_TYPES, taskImageUrl } from "./tasksModel";

type Pending = { id: number; preview: string; name: string };

/** ⌘/Ctrl+Enter(조합 중이 아닐 때)인가 */
export function isSubmitKey(e: { key: string; metaKey: boolean; ctrlKey: boolean; nativeEvent: { isComposing?: boolean; keyCode?: number } }): boolean {
  return e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.nativeEvent.isComposing && e.nativeEvent.keyCode !== 229;
}

export function AnswerField({
  id,
  value,
  project,
  disabled,
  placeholder,
  label,
  labelledBy,
  testId,
  inputRef,
  onChange,
  onImages,
  onRemoveImage,
  onSubmit,
  onUploading,
  onZoom,
}: {
  id?: string;
  /** 답 글 전체(그림 줄 포함) */
  value: string;
  /** 그림을 올릴 프로젝트. 없으면 그림을 붙이지 않는다 */
  project: string | null;
  disabled: boolean;
  placeholder?: string;
  label?: string;
  labelledBy?: string;
  testId: string;
  inputRef?: Ref<HTMLTextAreaElement>;
  /** 입력칸 글이 바뀐 답 전체 */
  onChange: (next: string) => void;
  /** 다 올린 그림 대상들(`uploads/<이름>`). 부모가 그때의 최신 답에 붙인다 */
  onImages: (targets: string[]) => void;
  onRemoveImage: (target: string) => void;
  onSubmit: () => void;
  /** 올리는 중인 파일 수가 바뀌었다(올리는 동안은 보내지 않는다) */
  onUploading: (count: number) => void;
  onZoom: (z: { src: string; alt: string }) => void;
}) {
  const [pending, setPending] = useState<Pending[]>([]);
  const [note, setNote] = useState<string | null>(null);
  const picker = useRef<HTMLInputElement>(null);
  const seq = useRef(0);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => onUploading(pending.length), [pending.length, onUploading]);
  const images = answerImages(value);
  const canAttach = project !== null && !disabled;
  const attach = async (files: File[]) => {
    if (!canAttach || files.length === 0) return;
    const { ok, skipped } = pickAnswerImages(files);
    setNote(skipped.length > 0 ? `붙이지 않은 것: ${skipped.join(", ")}` : null);
    if (ok.length === 0) return;
    const tiles = ok.map((f) => ({ id: ++seq.current, preview: URL.createObjectURL(f), name: f.name || "그림" }));
    setPending((p) => [...p, ...tiles]);
    const added: string[] = [];
    try {
      for (const f of ok) added.push(await uploadTaskFile(project, f));
    } catch (e) {
      if (mounted.current) setNote(`그림을 올리지 못했다: ${e instanceof ApiError || e instanceof Error ? e.message : String(e)}`);
    } finally {
      for (const tile of tiles) URL.revokeObjectURL(tile.preview);
      if (mounted.current) setPending((p) => p.filter((x) => !tiles.some((t) => t.id === x.id)));
    }
    // 그사이 카드를 닫았으면 붙이지 않는다(파일은 uploads/에 남는다)
    if (added.length > 0 && mounted.current) onImages(added);
  };
  const paste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    const files = filesOf(e.clipboardData);
    if (files.length === 0 || !canAttach) return;
    e.preventDefault();
    void attach(files);
  };
  const drop = (e: DragEvent<HTMLTextAreaElement>) => {
    const files = filesOf(e.dataTransfer);
    if (files.length === 0) return;
    e.preventDefault();
    void attach(files);
  };
  return (
    <div className="ask-answer-field" data-testid={`${testId}-field`}>
      <textarea
        id={id}
        ref={inputRef}
        className="input ask-answer-text"
        rows={2}
        value={answerBody(value)}
        disabled={disabled}
        placeholder={placeholder}
        aria-label={label}
        aria-labelledby={labelledBy}
        data-testid={testId}
        enterKeyHint="enter"
        onChange={(e) => onChange(withAnswerBody(value, e.currentTarget.value))}
        onKeyDown={(e) => {
          if (!isSubmitKey(e)) return;
          e.preventDefault();
          onSubmit();
        }}
        onPaste={paste}
        onDragOver={(e) => {
          if (canAttach && Array.from(e.dataTransfer.types).includes("Files")) e.preventDefault();
        }}
        onDrop={drop}
      />
      {(images.length > 0 || pending.length > 0) && (
        <ul className="ask-answer-images" aria-label="붙인 그림">
          {images.map((target, i) => (
            <li key={target} className="ask-answer-image" data-path={target} data-testid={`${testId}-image`}>
              <button type="button" className="ask-answer-thumb" aria-label={`그림 ${i + 1} 크게 보기`}
                onClick={() => project && onZoom({ src: taskImageUrl(project, target), alt: `그림 ${i + 1}` })}>
                {project && <img src={taskImageUrl(project, target)} alt="" loading="lazy" decoding="async" />}
              </button>
              {!disabled && (
                <button type="button" className="ask-answer-remove" aria-label={`그림 ${i + 1} 떼기`} title="떼기" data-testid={`${testId}-image-remove`}
                  onClick={() => onRemoveImage(target)}>
                  <X aria-hidden="true" />
                </button>
              )}
            </li>
          ))}
          {pending.map((p) => (
            <li key={`up-${p.id}`} className="ask-answer-image is-pending" data-testid={`${testId}-image-pending`}>
              <span className="ask-answer-thumb">
                <img src={p.preview} alt="" />
                <Loader2 className="ask-answer-spin" aria-hidden="true" />
              </span>
              <span className="visually-hidden" role="status">{p.name} 올리는 중</span>
            </li>
          ))}
        </ul>
      )}
      {project !== null && (
        <div className="ask-answer-tools">
          <button type="button" className="btn btn-ghost" disabled={!canAttach} title="그림(PNG·JPEG·GIF·WebP), 한 개 16MiB까지. 칸에 붙여 넣거나 끌어 놓아도 붙는다"
            data-testid={`${testId}-pick`} onClick={() => picker.current?.click()}>
            <ImagePlus aria-hidden="true" /> 그림
          </button>
          <input ref={picker} type="file" accept={ATTACH_TYPES.join(",")} multiple hidden data-testid={`${testId}-input`}
            onChange={(e) => {
              const files = Array.from(e.target.files ?? []).map(normalizeComposerImageFile);
              e.target.value = "";
              void attach(files);
            }} />
        </div>
      )}
      {note && <p className="prompt-card-error" role="alert" data-testid={`${testId}-note`}>{note}</p>}
    </div>
  );
}
