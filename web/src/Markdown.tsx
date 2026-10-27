import { logIgnoredFailure } from "./errorDiagnostic.ts";
// 보고서 Markdown(차이 목록 R1). herdr-web-ui `src/components/Markdown.tsx`(aa624cb)의 렌더를 옮기고, 파서는
// `src/lib/markdown.ts`·`markdownHtml.ts`를 그대로 쓴다(herdr/markdown.ts). HTML은 해석하지 않는다(parseMarkdown html=false).
// 참고에서 뺀 것: 수식(KaTeX)은 원문 그대로, mermaid는 code block. code block은 참고처럼 문법 강조한다(HighlightedCode).
// 그림과 파일 열기는 그것을 주는 곳(파일 보기: `MarkdownImageContext`·`OpenFileContext`)에서만 된다. 그 밖(보고서 등)은
// 참고의 「그림을 그리지 않는 곳」처럼 대체 글, 「열 수 없는 곳」처럼 이름 뒤 경로다.
// 문구는 참고 i18n.ko.ts 그대로다.
import { createContext, Fragment, memo, useCallback, useContext, useEffect, useId, useMemo, useRef, useState, type CSSProperties, type MouseEvent, type ReactNode } from "react";
import { Check, Copy, ImageOff } from "lucide-react";
import { foldCode, parseMarkdown, type InlineNode, type ListBlock, type MarkdownBlock } from "./herdr/markdown.ts";
import { codeIsFilePath, OpenFileContext, splitFilePaths } from "./herdr/filePaths.ts";
import { fileUriPath } from "./herdr/terminalFileLinks.ts";
import { imageSlotStyle, standaloneImage } from "./herdr/imageBox.ts";
import type { MarkdownImagePreview } from "./herdr/markdownImages.ts";
import { CHAT_HIGHLIGHT_LIMIT, languageForFence, normalizeCode } from "./herdr/highlight.ts";
import { FilePathLink } from "./FilePathLink.tsx";
import { formatBytes } from "./formatBytes.ts";
import { HighlightedCode } from "./HighlightedCode.tsx";
import { browserClick } from "./reportLinks.ts";
import { appLinkOf, appRefOf, splitAppRefs } from "./appRef.ts";
import { AppIdLink, AppRefChip, AppRefContext } from "./AppRefChip.tsx";
import { splitAppIds } from "./appIds";
import { isVideoPath } from "./reportMedia";
import { ReportVideo } from "./ReportVideo";
import { ReportMediaStatus } from "./ReportMediaStatus";
import "./herdr/Markdown.css";

/** Stats an image before it can load; null context keeps chat images as links. */
type MarkdownImageResolver = (src: string) => Promise<MarkdownImagePreview | null>;
export const MarkdownImageContext = createContext<MarkdownImageResolver | null>(null);
/** library 첨부를 읽는 본문만 제공한다. */
export const MarkdownAttachmentContext = createContext<((src: string) => string | null) | null>(null);

/**
 * 링크를 새 창 대신 앱 안에서 연다(대화 보기, plan §8.300): 열었으면 true. 없거나 false면 링크 그대로(새 창)다. 보조 키·가운데
 * 단추는 묻지 않고 브라우저 몫이다
 */
export const OpenLinkContext = createContext<((href: string) => boolean) | null>(null);

/**
 * An image that holds its box until it decodes (lib/imageBox.ts). With `slot`, the wrapper is the
 * box while the bytes arrive and after a failure, where a note stands in for the picture, so the
 * row around it moves at most once, when the natural size is in. Decoded, the wrapper gives its box
 * up (`display: contents`) and the one image renders bare, nothing remounted. Without `slot` (a badge
 * in a sentence) nothing is held and a failure shows `fallback`. `src` is null while an address is
 * still being resolved: the slot stands from the first paint. (참고 Markdown.tsx `ReservedImage`)
 */
function ReservedImage({ src, alt, width, className, slot, slotStyle, fallback }: {
  src: string | null; alt: string; width?: number; className?: string; slot: boolean; slotStyle?: CSSProperties; fallback?: ReactNode;
}) {
  // keyed by address: another one starts from the slot again and never takes the old one's answer
  const [settled, setSettled] = useState<{ src: string; failed: boolean } | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const phase = src !== null && settled?.src === src ? (settled.failed ? "failed" : "decoded") : "loading";
  // a cached image is complete before `onLoad` can fire
  const complete = useCallback((image: HTMLImageElement | null) => {
    imageRef.current = image;
    if (image !== null && src !== null && image.complete && image.naturalWidth > 0) setSettled({ src, failed: false });
  }, [src]);
  if (phase === "failed" && !slot) return <>{fallback}</>;
  const image = src === null ? null : (
    <img ref={complete} className={className} src={src} alt={alt} width={width} loading="lazy"
      onLoad={() => setSettled({ src, failed: false })} onError={() => setSettled({ src, failed: true })} />
  );
  if (!slot) return image;
  if (phase === "failed") {
    return (
      <span className="image-slot is-failed" style={slotStyle}>
        <span className="image-slot-note"><ImageOff aria-hidden="true" /><span>이미지를 표시할 수 없음</span>{alt !== "" && <span>{alt}</span>}
          {src !== null && <ReportMediaStatus src={src} image={() => imageRef.current} />}
        </span>
      </span>
    );
  }
  if (phase === "loading") return <span className="image-slot is-loading" style={slotStyle}>
    {image}<span className="image-slot-note"><span role="status">{src === null ? "이미지 주소 확인 중" : "이미지 불러오는 중"}</span>
      {src !== null && <ReportMediaStatus src={src} image={() => imageRef.current} />}
    </span>
  </span>;
  return <span className="image-slot">{image}</span>;
}

function MarkdownImage({ src, alt, width, fallback, resolve, interactive, standalone }: {
  src: string; alt: string; width?: number; fallback: ReactNode; resolve: MarkdownImageResolver; interactive: boolean; standalone: boolean;
}) {
  const [loaded, setLoaded] = useState<{ src: string; resolve: MarkdownImageResolver; preview: MarkdownImagePreview | null } | null>(null);
  useEffect(() => {
    let live = true;
    resolve(src).then(
      (preview) => { if (live) setLoaded({ src, resolve, preview }); },
      () => { if (live) setLoaded({ src, resolve, preview: null }); },
    );
    return () => { live = false; };
  }, [src, resolve]);
  const preview = loaded?.src === src && loaded.resolve === resolve ? loaded.preview : undefined;
  if (preview === null) return <>{fallback}</>;
  if (preview?.kind === "too-large") {
    const message = `이 이미지는 ${formatBytes(preview.size)}입니다. 보려면 다운로드하세요.`;
    return interactive ? <a href={preview.downloadUrl} download>{message}</a> : <span>{message}</span>;
  }
  // still being stat'ed: a standalone image's slot stands from the first paint, a badge shows its alt
  if (preview === undefined && !standalone) return <span>{alt}</span>;
  return <ReservedImage src={preview?.url ?? null} alt={alt} width={width} className="markdown-image" slot={standalone} slotStyle={standalone ? imageSlotStyle(width) : undefined} fallback={fallback} />;
}

/**
 * `interactive`가 false면 링크·파일 이름 안이다: 누를 것을 겹치지 않는다. `standalone`은 그 줄이 그림 하나뿐이라는 뜻이다
 * (imageBox.ts): 그림이 풀리는 동안 자리를 잡는다(참고 Inline)
 */
function Inline({ nodes, interactive = true, standalone = false }: { nodes: InlineNode[]; interactive?: boolean; standalone?: boolean }) {
  const context = useContext(OpenFileContext);
  const open = interactive ? context : null;
  const imageResolver = useContext(MarkdownImageContext);
  const openLink = useContext(OpenLinkContext);
  const attachment = useContext(MarkdownAttachmentContext);
  // 앱 안 참조(`aaa:question/…`·`aaa:report/…`, plan §8.329)는 칩이다. 링크 안(interactive=false)에서는 누를 것을 겹치지 않는다
  const refContext = useContext(AppRefContext);
  const refs = interactive ? refContext : null;
  const follow = (href: string) =>
    openLink === null ? undefined : (e: MouseEvent<HTMLAnchorElement>) => {
      if (!browserClick(e) && openLink(href)) e.preventDefault();
    };
  return (
    <>
      {nodes.map((node, index) => {
        const key = `${node.type}-${index}`;
        switch (node.type) {
          case "text": {
            const paths = (text: string) =>
              open === null ? text : splitFilePaths(text).map((part, n) => (typeof part === "string" ? <Fragment key={n}>{part}</Fragment> : <FilePathLink key={n} path={part.path} code={false} open={open} />));
            if (refs === null) return <span key={key}>{paths(node.value)}</span>;
            const ids = (text: string) => splitAppIds(text, refs.exists).map((part, n) => typeof part === "string"
              ? <Fragment key={n}>{paths(part)}</Fragment> : <AppIdLink key={n} at={part} refs={refs} />);
            return (
              <span key={key}>
                {splitAppRefs(node.value).map((part, n) => (typeof part === "string" ? <Fragment key={n}>{ids(part)}</Fragment> : <AppRefChip key={n} at={part} refs={refs} />))}
              </span>
            );
          }
          case "code": {
            const ids = refs === null ? [] : splitAppIds(node.value, refs.exists);
            if (ids.length === 1 && typeof ids[0] !== "string") return <code key={key}><AppIdLink at={ids[0]!} refs={refs!} /></code>;
            const ref = refs === null ? null : appRefOf(node.value);
            if (ref !== null) return <AppRefChip key={key} at={ref} refs={refs!} />;
            const file = fileUriPath(node.value);
            if (open !== null && file !== null) return <FilePathLink key={key} path={file} code open={open} />;
            // agent는 주소를 backtick에 넣곤 한다: 눈에는 code로 두고 열린다
            if (interactive && /^https?:\/\/\S+$/i.test(node.value))
              return (
                <a key={key} className="markdown-code-link" href={node.value} target="_blank" rel="noopener noreferrer" onClick={follow(node.value)}>
                  <code>{node.value}</code>
                </a>
              );
            return open !== null && codeIsFilePath(node.value) ? <FilePathLink key={key} path={node.value} code open={open} /> : <code key={key}>{node.value}</code>;
          }
          case "math":
            return <span key={key}>{`\\(${node.value}\\)`}</span>;
          case "image": {
            const video = interactive && isVideoPath(node.src) ? attachment?.(node.src) : null;
            if (video) return <ReportVideo key={key} src={video} label={node.alt} />;
            return (
              <span key={key} data-markdown-copy={`![${node.alt}](${node.src})`}>
                {imageResolver === null ? (
                  <Inline nodes={node.fallback} interactive={interactive} />
                ) : (
                  <MarkdownImage src={node.src} alt={node.alt} width={node.width} resolve={imageResolver} interactive={interactive} standalone={standalone}
                    fallback={<Inline nodes={node.fallback} interactive={interactive} />} />
                )}
              </span>
            );
          }
          case "strong":
            return (
              <strong key={key}>
                <Inline nodes={node.children} interactive={interactive} />
              </strong>
            );
          case "em":
            return (
              <em key={key}>
                <Inline nodes={node.children} interactive={interactive} />
              </em>
            );
          case "del":
            return (
              <del key={key}>
                <Inline nodes={node.children} interactive={interactive} />
              </del>
            );
          case "link": {
            const video = interactive && isVideoPath(node.href) ? attachment?.(node.href) : null;
            if (video) return <ReportVideo key={key} src={video} label={node.children.map((n) => "value" in n ? n.value : "").join("")} />;
            const ref = refs === null ? null : (appRefOf(node.href) ?? appLinkOf(node.href, location.origin));
            if (ref !== null) return <AppRefChip key={key} at={ref} refs={refs!} />;
            return (
              <a key={key} href={node.href} target="_blank" rel="noopener noreferrer" onClick={follow(node.href)}>
                <Inline nodes={node.children} interactive={false} standalone={standalone} />
              </a>
            );
          }
          case "file": {
            const url = attachment?.(node.path);
            if (url && interactive && isVideoPath(node.path)) return <ReportVideo key={key} src={url} label={node.children.map((n) => "value" in n ? n.value : "").join("")} />;
            if (url && interactive) return <a key={key} href={url} target="_blank" rel="noopener noreferrer"><Inline nodes={node.children} interactive={false} /></a>;
            // 이름을 누르면 파일을 연다. 열 곳이 없으면 이름 뒤에 경로를 보인다(참고: Codex 터미널처럼)
            return open !== null ? (
              <button key={key} type="button" className="markdown-file" title={`${node.path} 열기`} onClick={() => open(node.path)}>
                <Inline nodes={node.children} interactive={false} />
              </button>
            ) : (
              <span key={key}>
                <Inline nodes={node.children} interactive={false} /> (<code>{node.path}</code>)
              </span>
            );
          }
        }
      })}
    </>
  );
}

/** 목록, 항목 안의 목록·표. 할 일 항목은 상자가 글머리표 자리에(참고 List) */
function List({ block }: { block: ListBlock }) {
  const Tag = block.ordered ? "ol" : "ul";
  const id = useId();
  return (
    <Tag className="markdown-list" start={block.ordered ? block.start : undefined}>
      {block.items.map((item, index) => (
        <li key={index} className={item.checked === undefined ? undefined : "markdown-task"}>
          {item.checked !== undefined && (
            <span
              className="markdown-task-box"
              role="checkbox"
              aria-checked={item.checked}
              aria-disabled="true"
              aria-labelledby={item.content.length === 0 ? undefined : `${id}-${index}`}
              aria-label={item.content.length === 0 ? "빈 할 일" : undefined}
            >
              {item.checked && <Check aria-hidden="true" />}
            </span>
          )}
          {item.checked === undefined ? (
            <Inline nodes={item.content} />
          ) : (
            <span id={`${id}-${index}`}>
              <Inline nodes={item.content} />
            </span>
          )}
          {item.blocks !== undefined && <Blocks blocks={item.blocks} />}
        </li>
      ))}
    </Tag>
  );
}

/** A fence in no language, or in one that says plain output (a log, a command's answer): the files it names open. */
const PLAIN_FENCE = /^(?:|text|txt|plain|plaintext|log|console|output|shell-session)$/i;

/**
 * 접힌 code의 가린 꼬리: 글만 `hidden="until-found"`로 둔다. 대화 안 찾기(chatFindText.ts)와 브라우저 찾기가 이 글을 찾으면 `beforematch`로
 * 편다(참고 t3code `data-thread-find-fold`·revealFolded). 강조 표시는 펼친 뒤에 한다
 */
function FoldedTail({ value, head, onReveal }: { value: string; head: string; onReveal: () => void }) {
  const tail = useMemo(() => normalizeCode(value).split("\n").slice(head.split("\n").length).join("\n"), [value, head]);
  const reveal = useRef(onReveal);
  reveal.current = onReveal;
  const attach = useCallback((node: HTMLPreElement | null) => {
    if (node === null) return;
    // React는 hidden을 참·거짓으로만 쓴다: 값은 여기서 둔다
    node.setAttribute("hidden", "until-found");
    node.addEventListener("beforematch", () => reveal.current());
  }, []);
  return <pre ref={attach} className="hl-code markdown-code-tail" data-chat-find-fold=""><code>{tail}</code></pre>;
}

/** code block: 머리에 언어와 복사, 긴 것은 접고 「n줄 모두 보기」(참고 CodeBlock, 안쪽 스크롤 없음) */
function CodeBlock({ language, value }: { language: string; value: string }) {
  const open = useContext(OpenFileContext);
  const grammar = languageForFence(language);
  // the tag decides, not the grammar: code in a language with no grammar here is still code
  const plain = PLAIN_FENCE.test(language.trim().split(/\s+/)[0] ?? "");
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const fold = useMemo(() => foldCode(value), [value]);
  const copy = async () => {
    let ok = false;
    try {
      await navigator.clipboard.writeText(value);
      ok = true;
    } catch (ignoredError) { logIgnoredFailure(ignoredError, "Markdown.tsx:282");
      // clipboard API가 없거나 브라우저가 거절했다
    }
    setCopied(ok);
    setCopyFailed(!ok);
    if (ok) window.setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="markdown-code" data-language={language}>
      <div className="markdown-code-header" data-chat-find-ignore="">
        <span>{language || "text"}</span>
        <button type="button" className="icon-button markdown-code-copy" onClick={() => void copy()} aria-label={copied ? "코드 복사됨" : "코드 복사"}>
          {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
        </button>
      </div>
      {copyFailed && (
        <p className="markdown-code-error" role="alert" data-chat-find-ignore="">
          복사하지 못했습니다. 텍스트를 선택해 직접 복사하세요.
        </p>
      )}
      {/* a block in no language is output (a log, a command's answer): the files it names open */}
      <HighlightedCode code={fold !== null && !expanded ? fold.head : value} language={grammar} limit={CHAT_HIGHLIGHT_LIMIT} open={plain ? open : null} />
      {fold !== null && !expanded && <FoldedTail value={value} head={fold.head} onReveal={() => setExpanded(true)} />}
      {fold !== null && (
        <button type="button" className="markdown-code-more" data-chat-find-ignore="" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
          {expanded ? "접기" : `${fold.lines}줄 모두 보기`}
        </button>
      )}
    </div>
  );
}

function Blocks({ blocks }: { blocks: MarkdownBlock[] }) {
  return (
    <>
      {blocks.map((block, index): ReactNode => {
        const key = `${block.type}-${index}`;
        switch (block.type) {
          case "heading":
            // agent 제목은 몇 단계든 h3다: 앱의 h1·h2가 위 개요로 남는다. 단계는 class로(참고 Blocks)
            return (
              <h3 key={key} data-markdown-heading={block.level} className={`markdown-h${block.level}${block.align ? " markdown-center" : ""}`}>
                <Inline nodes={block.content} />
              </h3>
            );
          case "paragraph":
            return (
              <p key={key} className={block.align ? "markdown-center" : undefined}>
                {block.lines.map((line, lineIndex) => (
                  <span key={lineIndex}>
                    <Inline nodes={line} standalone={standaloneImage(line)} />
                    {lineIndex < block.lines.length - 1 && <br />}
                  </span>
                ))}
              </p>
            );
          case "list":
            return <List key={key} block={block} />;
          case "blockquote":
            return (
              <blockquote key={key}>
                <Blocks blocks={block.blocks} />
              </blockquote>
            );
          case "code":
            return <CodeBlock key={key} language={block.language} value={block.value} />;
          case "math":
            return <p key={key}>{`\\[${block.value}\\]`}</p>;
          case "hr":
            return <hr key={key} />;
          case "table":
            return (
              <div className="markdown-table-wrap" key={key}>
                <table>
                  <thead>
                    <tr>
                      {block.header.map((cell, cellIndex) => (
                        <th key={cellIndex}>
                          <Inline nodes={cell} />
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {block.rows.map((row, rowIndex) => (
                      <tr key={rowIndex}>
                        {row.map((cell, cellIndex) => (
                          <td key={cellIndex}>
                            <Inline nodes={cell} />
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
        }
      })}
    </>
  );
}

/** 그린 Markdown. props가 원시 값이라 memo가 부모의 다시 그리기를 건너뛴다(참고 Markdown) */
export const Markdown = memo(function Markdown({ children, className }: { children: string; className?: string }) {
  const blocks = useMemo(() => parseMarkdown(children), [children]);
  return (
    <div className={className === undefined ? "markdown" : `markdown ${className}`}>
      <Blocks blocks={blocks} />
    </div>
  );
});
