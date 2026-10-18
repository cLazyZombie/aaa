// 파일 보기(결정 20 D7, herdr-web-ui `src/components/FileViewer.tsx`·`FileViewer.css` aa624cb). 웹 터미널의 파일 경로 링크로 연다.
// 옮긴 것: 대화 상자(휴대폰은 화면 가득), 경로 찾기(같은 이름이 여럿이면 고르기), 그림·동영상·소리·PDF, HTML 페이지(sandbox
// 안 페이지와 소스), 글(Markdown 미리보기와 소스, 줄 번호, 줄 참조 표시), 새 탭·내려받기·파일 복사·닫기, 문구(참고 i18n.ko.ts).
// 소스는 참고처럼 문법 강조한다(HighlightedCode, 긴 것은 worker). 2,000줄 넘는 소스는 참고처럼 `@pierre/diffs`의 가상화한 줄
// (herdr/VirtualSource.tsx, 제 worker가 5,000줄·256K자까지 칠함)로 그리고, 256KiB 넘는 소스는 8MiB까지 읽는다(plan §8.171).
// 폴더 경로면 그 폴더 목록을 그린다(참고 DirectoryBrowser, plan §8.148). 「파일 뷰어」 설정은 plan §8.149.
// aaa가 다른 곳: 허용 폴더(§6.5) 밖이면 그 까닭을 말한다(참고는 서버 사용자가 읽을 수 있는 것을 다 연다).
import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Braces, Check, Copy, Download, ExternalLink, TriangleAlert, X } from "lucide-react";

import { ApiError, call } from "./api.ts";
import { DirectoryBrowser } from "./DirectoryBrowser.tsx";
import type { FileInfo } from "./gen/proto.ts";
import { Markdown, MarkdownImageContext } from "./Markdown.tsx";
import { formatBytes } from "./formatBytes.ts";
import { CodeLines, useHighlightedLines } from "./HighlightedCode.tsx";
import { type MarkdownWidth, useMarkdownWidth, useTerminalToggle } from "./settings";
import { copyText } from "./herdr/clipboard.ts";
import { fileLineTarget, OpenFileContext, pathParts } from "./herdr/filePaths.ts";
import { resolveFromDocument } from "./herdr/fileReader.ts";
import { readFilePreviewText, SOURCE_PREVIEW_LINES, type FilePreviewText } from "./herdr/filePreviewText.ts";
import { canHighlight, countLines, languageForPath, normalizeCode } from "./herdr/highlight.ts";
import { markdownImagePreview } from "./herdr/markdownImages.ts";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap.ts";
import { useSheetSwipe } from "./herdr/sheets.ts";
import { lineRequest, readBudget, shownSource, sourceLayout, virtualHighlightable } from "./herdr/sourcePreview.ts";
import { useAppearance } from "./theme";
import "./herdr/FileViewer.css";

// 가상화한 줄과 그 강조는 따로 받는 조각이다: 그것이 필요한 첫 소스와 함께 읽는다(참고 TextFileView)
const VirtualSource = lazy(() => import("./herdr/VirtualSource.tsx"));

/** 그림은 이 크기까지 바로 그린다(참고 markdownImages.ts) */
const MAX_INLINE_IMAGE_BYTES = 20 * 1024 * 1024;
/** 저장한 너비는 설정 값이지 class 이름이 아니다: 한쪽 이름을 바꿔도 다른 쪽은 그대로다(참고 TextFileView MARKDOWN_CLASS) */
const MARKDOWN_CLASS: Record<MarkdownWidth, string> = {
  readable: "file-viewer-markdown file-viewer-markdown-readable",
  full: "file-viewer-markdown",
};


/** 열 수 없는 까닭. 그 밖의 오류(agent가 파일 보기를 모름·연결 끊김)는 host가 말한 까닭을 붙인다 */
type Problem = { kind: "missing" } | { kind: "outside" } | { kind: "unreadable"; message: string | null };

function CopyFileButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const label = copied ? "파일 복사됨" : "파일 복사";
  return (
    <button type="button" className="icon-button file-viewer-action" aria-label={label} title={label} onClick={() => void copyText(text).then((ok) => ok && setCopied(true))}>
      {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
    </button>
  );
}

/** 파일 보기 안에서 고른 다른 파일(Markdown의 링크·경로)은 `onOpen`으로 연다: 같은 기록 칸을 바꾼다(참고 App onOpen) */
export function FileViewer({ path: asked, pane, onClose, onOpen }: { path: string; pane: string; onClose: () => void; onOpen: (path: string) => void }) {
  const [path, setPath] = useState(asked);
  const target = useMemo(() => fileLineTarget(path), [path]);
  const wrapCode = useTerminalToggle("wrapCode");
  const markdownWidth = useMarkdownWidth();
  const { resolvedTheme } = useAppearance();
  const [info, setInfo] = useState<FileInfo | null>(null);
  const [url, setUrl] = useState<string | null>(null);
  const [candidates, setCandidates] = useState<string[] | null>(null);
  const [problem, setProblem] = useState<Problem | null>(null);
  // 폴더 경로였으면 그 폴더(실제 경로): 파일 대신 폴더 목록을 그린다
  const [directory, setDirectory] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<FilePreviewText | null>(null);
  // 줄 참조가 있으면 소스, 아니면 미리보기. 새 경로는 옛 선택을 쓰지 않는다
  const [chosen, setChosen] = useState<{ path: string; mode: "preview" | "code" } | null>(null);
  const mode = chosen?.path === path ? chosen.mode : target.line === null ? "preview" : "code";
  // HTML 파일은 페이지로 보이고, 고르면 소스로. 줄 참조면 소스로 연다
  const [source, setSource] = useState(false);
  const surface = useFocusTrap<HTMLElement>(true);
  useSheetSwipe(surface, onClose);
  useEffect(() => setPath(asked), [asked]);
  useEffect(() => setSource(target.line !== null), [path, target.line]);

  useEffect(() => {
    let cancelled = false;
    const abort = new AbortController();
    let deadline: ReturnType<typeof setTimeout> | undefined;
    setInfo(null);
    setUrl(null);
    setCandidates(null);
    setProblem(null);
    setLoaded(null);
    setDirectory(null);
    void (async () => {
      try {
        const r = await call({ op: "pane_file", pane, path: target.path });
        if (cancelled || r.kind !== "pane_file") return;
        if (r.info === null) {
          setCandidates(r.candidates);
          return;
        }
        setInfo(r.info);
        setUrl(r.url);
        if (r.info.kind !== "text" || r.url === null) return;
        // 글은 정한 만큼만 오간다: 크기와 상관없는 Range와 줄 상한(readBudget: Markdown은 미리보기 예산, 더 큰 소스는 소스 예산).
        // 시한은 요청에 걸고 읽기는 그 남은 시간을 쓴다. 느린 연결이면 그때까지 온 것을 잘린 앞부분으로 보인다(참고 FileViewer)
        const budget = readBudget(languageForPath(r.info.path), r.info.size);
        const started = Date.now();
        deadline = setTimeout(() => abort.abort(), budget.timeoutMs);
        try {
          const res = await fetch(r.url, { signal: abort.signal, headers: { range: `bytes=0-${budget.bytes - 1}` } });
          clearTimeout(deadline);
          const timeoutMs = Math.max(1, budget.timeoutMs - (Date.now() - started));
          const body = await readFilePreviewText(res, budget.bytes, { signal: abort.signal, maxLines: budget.lines, timeoutMs, partialOnTimeout: true });
          if (!cancelled) setLoaded(body);
        } finally {
          clearTimeout(deadline);
        }
      } catch (e) {
        if (cancelled) return;
        // 폴더는 파일처럼 pane 폴더 기준으로 읽는다(참고 FileViewer: 404면 폴더 목록을 물어 본다). 폴더도 아니면 파일 오류 그대로
        if (e instanceof ApiError && e.body.code === "not_found") {
          try {
            const r = await call({ op: "pane_dir", pane, path: target.path, hidden: false });
            if (cancelled) return;
            if (r.kind === "pane_dir") {
              setDirectory(r.listing.path);
              return;
            }
          } catch (dirError) {
            // 허용 폴더 밖 폴더는 그 까닭을 말한다. 읽을 수 있는 폴더가 아니면 파일 오류를 둔다
            if (cancelled) return;
            if (dirError instanceof ApiError && dirError.body.code === "forbidden") {
              setProblem({ kind: "outside" });
              return;
            }
          }
          if (cancelled) return;
        }
        if (e instanceof ApiError && e.body.code === "not_found") setProblem({ kind: "missing" });
        else if (e instanceof ApiError && e.body.code === "forbidden") setProblem({ kind: "outside" });
        else setProblem({ kind: "unreadable", message: e instanceof ApiError ? e.message : null });
      }
    })();
    return () => {
      cancelled = true;
      clearTimeout(deadline);
      abort.abort();
    };
  }, [pane, target.path]);

  useEffect(() => {
    // 맨 위 겹이라 키를 받는다. 그 위에 native dialog가 떠 있으면 그것이 먼저다
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === "Escape" && !nativeModalOver(surface.current)) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, surface]);

  // Markdown의 링크는 그 문서가 있는 폴더 기준이다(참고 TextFileView openLink). 앱은 다시 그릴 때마다 새 onOpen을 주므로
  // ref로 읽어, 문맥 값은 파일이 바뀔 때만 바뀌고 memo한 Markdown은 그대로 둔다
  const documentPath = info?.path ?? null;
  const onOpenRef = useRef(onOpen);
  useLayoutEffect(() => {
    onOpenRef.current = onOpen;
  });
  const openLink = useCallback((href: string) => {
    if (documentPath !== null) onOpenRef.current(resolveFromDocument(documentPath, href));
  }, [documentPath]);
  // Markdown의 그림도 파일처럼 이 pane의 파일 보기로 읽는다(허용 폴더 안만). 읽기 경로는 그림마다 받는다
  const imagePreview = useCallback((src: string) => {
    if (documentPath === null) return Promise.resolve(null);
    const urls = new Map<string, string>();
    return markdownImagePreview(
      src,
      documentPath,
      async (path) => {
        const r = await call({ op: "pane_file", pane, path });
        if (r.kind !== "pane_file") throw new Error("pane_file");
        if (r.info === null) return { candidates: r.candidates };
        if (r.url !== null) urls.set(r.info.path, r.url);
        return r.info;
      },
      (path, download) => `${urls.get(path) ?? ""}${download ? "?download=1" : ""}`,
    );
  }, [documentPath, pane]);
  const shownPath = info?.path ?? target.path;
  const { stem, extension } = pathParts(info?.name ?? shownPath);
  const { folder } = pathParts(shownPath);
  const textFile = info?.kind === "text";
  const markdown = textFile && /\.(?:md|markdown)$/i.test(info.name);
  // 제 script로 그리는 페이지: origin도 network도 없는 sandbox(host PAGE_CSP)
  const htmlPage = textFile && info.mime === "text/html";
  const pageUrl = url === null ? null : `${url}?render=1`;
  const modes = (
    <div className="segmented file-viewer-modes" role="group" aria-label="HTML 파일 보기 방식">
      <button type="button" aria-pressed={!source} onClick={() => setSource(false)}>
        페이지
      </button>
      <button type="button" aria-pressed={source} onClick={() => setSource(true)}>
        소스
      </button>
    </div>
  );
  const view = markdown && mode === "preview" ? "markdown" : "code";
  // 소스는 보이는 대로의 글(잘린 앞부분은 마지막 줄바꿈 뒤 줄 없이)이다. VIRTUAL_SOURCE_LINES를 넘으면 가상화한 줄로, 작으면
  // 그 worker가 칠하고(virtualHighlightable) 아니면 글 그대로 알림과 함께. 그 아래는 CodeLines(참고 FileViewer)
  const codeShown = textFile && loaded !== null && view === "code" && (!htmlPage || source);
  const language = languageForPath(info?.path ?? target.path);
  const sourceText = useMemo(() => (codeShown ? shownSource(normalizeCode(loaded.text), loaded.truncated) : ""), [codeShown, loaded]);
  const sourceLines = useMemo(() => countLines(sourceText), [sourceText]);
  const layout = sourceLayout(sourceLines);
  const colorable = canHighlight(language);
  const virtual = useMemo(
    () => (codeShown && layout === "virtual" ? { text: sourceText, colored: colorable && virtualHighlightable(sourceText.length, sourceLines) } : null),
    [codeShown, layout, sourceText, colorable, sourceLines],
  );
  // CodeLines의 code는 제 길이 상한이 없다(layout이 묶는다): 포기한 worker만 글 그대로 둔다
  const code = useHighlightedLines(codeShown && layout === "elements" ? sourceText : "", codeShown ? language : null);
  const sourceRef = useRef<HTMLPreElement>(null);
  // 잘린 것은 글을 볼 때만 말한다(페이지는 파일 전체를 그린다). 바이트 예산에서 잘린 것은 크기로, 줄 상한은 줄로, 시한(느린
  // 연결)은 일부만 보인다고만 말한다(그 크기는 예산이 아니다)
  const cutShort = textFile && (!htmlPage || source) && loaded?.truncated === true;
  const budget = textFile ? readBudget(language, info.size) : null;
  const cutBytes =
    cutShort && budget !== null && loaded.truncatedBy !== "lines" && loaded.truncatedBy !== "time" && (loaded.total ?? info.size) > budget.bytes
      ? { shown: budget.bytes, total: loaded.total ?? info.size }
      : null;
  // 표시할 줄: 잘린 앞부분의 줄은 읽은 쪽이 센 온전한 줄이다(상한 바로 뒤 줄은 보이지 않는다, 참고 lineRequest)
  const request = useMemo(() => (codeShown ? lineRequest(target.line, sourceText, loaded.truncated, loaded.lines) : null), [codeShown, target.line, sourceText, loaded]);
  const marked = request?.kind === "shown" ? request.line : null;
  const notes: string[] = [];
  if (request?.kind === "past-preview") notes.push(`${request.line}번째 줄은 미리보기 범위를 벗어납니다. 나머지를 읽으려면 다운로드하세요.`);
  else if (request?.kind === "past-end") notes.push(`이 파일은 ${request.lines}줄이며 ${request.line}번째 줄은 없습니다.`);
  if (cutShort)
    notes.push(
      loaded.truncatedBy === "lines"
        ? `이 파일의 처음 ${SOURCE_PREVIEW_LINES.toLocaleString("ko")}줄만 보입니다. 나머지를 읽으려면 다운로드하세요.`
        : cutBytes !== null
          ? `전체 ${formatBytes(cutBytes.total)} 중 처음 ${formatBytes(cutBytes.shown)}만 보입니다.`
          : "파일의 일부만 표시됩니다. 나머지는 다운로드해 확인하세요.",
    );
  if (codeShown && (code.tooLong || (virtual !== null && colorable && !virtual.colored))) notes.push("너무 길어 강조 표시를 생략했습니다");
  // 표시한 줄을 가운데로. worker의 색은 자리를 바꾸지 않으니 줄·보기·글이 바뀔 때만 굴린다(참고 TextFileView). 가상화한 줄은
  // VirtualSource가 제 줄에 맞춰 굴린다
  useLayoutEffect(() => {
    if (virtual === null && marked !== null) sourceRef.current?.querySelector(".hl-line.is-marked")?.scrollIntoView({ block: "center", inline: "nearest" });
  }, [path, view, source, virtual, marked, loaded]);

  const body = (() => {
    if (directory !== null) return <DirectoryBrowser key={directory} start={directory} pane={pane} onOpenFile={onOpen} />;
    if (problem !== null)
      return (
        <p className="file-viewer-note" role="alert">
          {problem.kind === "missing"
            ? "이 경로에 읽을 수 있는 파일이 없습니다."
            : problem.kind === "outside"
              ? "허용한 폴더 밖의 파일이라 열지 않습니다. 그 머신 agent.toml의 [agent] file_roots에 폴더를 넣으면 열 수 있습니다."
              : `파일을 열 수 없습니다.${problem.message ? ` ${problem.message}` : ""}`}
        </p>
      );
    if (candidates !== null)
      return (
        <div className="file-viewer-choices">
          <p className="file-viewer-note">같은 이름의 파일이 여럿입니다: {target.path.split("/").pop()}</p>
          <ul>
            {candidates.map((c) => (
              <li key={c}>
                <button type="button" className="btn btn-ghost" onClick={() => setPath(c + path.slice(target.path.length))}>
                  {c}
                </button>
              </li>
            ))}
          </ul>
        </div>
      );
    if (info === null || url === null) return <p className="file-viewer-note">여는 중…</p>;
    switch (info.kind) {
      case "image":
        return info.size > MAX_INLINE_IMAGE_BYTES ? (
          <p className="file-viewer-note">이 이미지는 {formatBytes(info.size)}입니다. 보려면 다운로드하세요.</p>
        ) : (
          <img className="file-viewer-media" src={url} alt={info.name} />
        );
      // 흘려 받아 바로 틀고 넘긴다(Range)
      case "video":
        return <video className="file-viewer-media" src={url} controls playsInline preload="metadata" />;
      case "audio":
        return <audio className="file-viewer-audio" src={url} controls preload="metadata" />;
      case "pdf":
        return <iframe className="file-viewer-pdf" src={url} title={info.name} />;
      case "text":
        if (htmlPage && !source)
          return (
            <>
              {modes}
              <iframe className="file-viewer-page" src={pageUrl ?? undefined} sandbox="allow-scripts" title={info.name} />
            </>
          );
        if (loaded === null) return <p className="file-viewer-note">여는 중…</p>;
        // 참고 TextFileView: Markdown 너비와 긴 줄 바꿈은 「파일 뷰어」 설정(이 기기)
        return (
          <>
            {htmlPage && modes}
            <div
              className={virtual !== null && view === "code" ? "file-viewer-content file-viewer-content-virtual" : "file-viewer-content"}
              data-testid={view === "markdown" ? "file-viewer-markdown" : "file-viewer-code"}
            >
              {view === "markdown" ? (
                <OpenFileContext.Provider value={openLink}>
                  <MarkdownImageContext.Provider value={imagePreview}>
                    <Markdown className={MARKDOWN_CLASS[markdownWidth]}>{loaded.text}</Markdown>
                  </MarkdownImageContext.Provider>
                </OpenFileContext.Provider>
              ) : virtual === null ? (
                <CodeLines ref={sourceRef} className="file-viewer-text" lines={code.lines} open={null} lineNumbers wrap={wrapCode} markedLine={marked} />
              ) : (
                <Suspense fallback={<p className="file-viewer-note">여는 중…</p>}>
                  <VirtualSource name={info.path} text={virtual.text} colored={virtual.colored} wrap={wrapCode} theme={resolvedTheme} markedLine={marked} />
                </Suspense>
              )}
            </div>
          </>
        );
      default:
        return (
          <p className="file-viewer-note">
            {info.mime}, {formatBytes(info.size)}. 여기서는 볼 수 없는 파일입니다. 내려받아 여세요.
          </p>
        );
    }
  })();

  // 가상화한 소스는 보기를 채운다: 그 줄은 굴려 보는 창이라, 몸은 글을 따라 자라지 않고 정한 높이를 scroll root에 준다
  const virtualBody = virtual !== null && view === "code";
  return (
    <div className="modal-scrim file-viewer-scrim" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className={virtualBody ? "modal file-viewer file-viewer-virtual" : "modal file-viewer"} role="dialog" aria-modal="true" aria-label={info?.name ?? path} ref={surface} tabIndex={-1} data-testid="file-viewer">
        <header className="modal-header file-viewer-header file-viewer-header-stacked">
          <div className="file-viewer-title">
            <h2 className="modal-title">
              <span className="file-viewer-stem">{stem}</span>
              {extension}
            </h2>
            <p className="file-viewer-meta" title={shownPath}>
              {info &&
                (notes.length > 0 ? (
                  <span className="file-viewer-notice" title={notes.join("\n")}>
                    <TriangleAlert aria-hidden="true" />
                    <span>{cutBytes !== null ? `${formatBytes(cutBytes.total)} 중 ${formatBytes(cutBytes.shown)}` : formatBytes(info.size)}</span>
                    <span className="visually-hidden">{notes.join(". ")}</span>
                  </span>
                ) : (
                  <span className="file-viewer-size">{formatBytes(info.size)}</span>
                ))}
              {folder !== "" && (
                <span className="file-viewer-path">
                  <span dir="ltr">
                    {info && (
                      <span className="file-viewer-dot" aria-hidden="true">
                        ·
                      </span>
                    )}
                    {folder}
                  </span>
                </span>
              )}
            </p>
          </div>
          <div className="file-viewer-actions">
            {markdown && (
              <div className="file-viewer-group">
                <button
                  type="button"
                  className="icon-button file-viewer-action"
                  aria-pressed={mode === "code"}
                  aria-label="소스 보기"
                  title="소스 보기"
                  onClick={() => setChosen({ path, mode: mode === "code" ? "preview" : "code" })}
                >
                  <Braces aria-hidden="true" />
                </button>
              </div>
            )}
            {url !== null && (
              <div className="file-viewer-group">
                <a
                  className="icon-button file-viewer-action"
                  href={htmlPage && !source ? (pageUrl ?? url) : url}
                  target="_blank"
                  rel="noopener"
                  aria-label="새 탭에서 열기"
                  title="새 탭에서 열기"
                >
                  <ExternalLink aria-hidden="true" />
                </a>
                <a className="icon-button file-viewer-action" href={`${url}?download=1`} download={info?.name ?? true} aria-label="내려받기" title="내려받기">
                  <Download aria-hidden="true" />
                </a>
                {textFile && loaded !== null && !loaded.truncated && <CopyFileButton text={loaded.text} />}
              </div>
            )}
          </div>
          <button type="button" className="icon-button file-viewer-action file-viewer-close" aria-label="파일 닫기" title="파일 닫기" onClick={onClose}>
            <X aria-hidden="true" />
          </button>
        </header>
        <div className={virtualBody ? "file-viewer-body file-viewer-body-virtual" : "file-viewer-body"}>{body}</div>
      </section>
    </div>
  );
}
