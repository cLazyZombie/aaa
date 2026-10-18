// 파일 보기(결정 20 D7, herdr-web-ui `src/components/FileViewer.tsx`·`FileViewer.css` aa624cb). 웹 터미널의 파일 경로 링크로 연다.
// 옮긴 것: 대화 상자(휴대폰은 화면 가득), 경로 찾기(같은 이름이 여럿이면 고르기), 그림·동영상·소리·PDF, HTML 페이지(sandbox
// 안 페이지와 소스), 글(Markdown 미리보기와 소스, 줄 번호, 줄 참조 표시), 새 탭·내려받기·파일 복사·닫기, 문구(참고 i18n.ko.ts).
// 소스는 참고처럼 문법 강조한다(HighlightedCode, 긴 것은 worker).
// 폴더 경로면 그 폴더 목록을 그린다(참고 DirectoryBrowser, plan §8.148).
// 아직 옮기지 않은 것(plan §8.125 남은 것): 긴 소스 가상화(VirtualSource). 「파일 뷰어」 설정은 plan §8.149.
// aaa가 다른 곳: 허용 폴더(§6.5) 밖이면 그 까닭을 말한다(참고는 서버 사용자가 읽을 수 있는 것을 다 연다).
// thread 결과물도 이것으로 연다(차이 목록 R1): 결과물 목록의 보기 경로(`/v/`, opaque origin sandbox)로 읽고, 경로는 결과물
// 안 상대 경로다. 문서 안 링크·그림은 결과물 맨 위를 `/`로 두고 풀어 결과물 밖으로 나가지 않는다(plan §8.167).
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Braces, Check, Copy, Download, ExternalLink, TriangleAlert, X } from "lucide-react";

import { ApiError, call } from "./api.ts";
import { DirectoryBrowser } from "./DirectoryBrowser.tsx";
import type { FileInfo, LibraryFile } from "./gen/proto.ts";
import { Markdown, MarkdownImageContext } from "./Markdown.tsx";
import { formatBytes } from "./formatBytes.ts";
import { CodeLines, useHighlightedLines } from "./HighlightedCode.tsx";
import { type MarkdownWidth, useMarkdownWidth, useTerminalToggle } from "./settings";
import { copyText } from "./herdr/clipboard.ts";
import { fileLineTarget, OpenFileContext, pathParts } from "./herdr/filePaths.ts";
import { resolveFromDocument } from "./herdr/fileReader.ts";
import { languageForPath, normalizeCode } from "./herdr/highlight.ts";
import { markdownImagePreview } from "./herdr/markdownImages.ts";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap.ts";
import { useSheetSwipe } from "./herdr/sheets.ts";
import "./herdr/FileViewer.css";

/** 글 파일은 앞부분만 받는다(참고 filePreviewText.ts TEXT_PREVIEW_BYTES·SOURCE_PREVIEW_LINES) */
const TEXT_PREVIEW_BYTES = 256 * 1024;
const SOURCE_PREVIEW_LINES = 200_000;
/** 그림은 이 크기까지 바로 그린다(참고 markdownImages.ts) */
const MAX_INLINE_IMAGE_BYTES = 20 * 1024 * 1024;
/** 저장한 너비는 설정 값이지 class 이름이 아니다: 한쪽 이름을 바꿔도 다른 쪽은 그대로다(참고 TextFileView MARKDOWN_CLASS) */
/** 결과물 HTML의 sandbox(ThreadPanel 결과물 frame과 같다) */
export const LIBRARY_SANDBOX = "allow-scripts allow-popups allow-popups-to-escape-sandbox allow-downloads";
const MARKDOWN_CLASS: Record<MarkdownWidth, string> = {
  readable: "file-viewer-markdown file-viewer-markdown-readable",
  full: "file-viewer-markdown",
};


type Loaded = { text: string; truncated: boolean; lines: number };

/** 파일 보기가 읽는 곳: pane의 파일(허용 폴더 안) 또는 thread 결과물 */
export type FileSource = { pane: string } | { project: string; thread: string };
/** 찾은 파일과 그 읽기 경로, 또는 같은 이름의 후보들 */
type Found = { info: FileInfo; url: string | null } | { candidates: string[] };
/** 결과물 목록: 보기 경로 맨 위와 상대 경로별 파일 */
type Library = { base: string; files: Map<string, LibraryFile> };

const encodePath = (p: string) => p.split("/").map(encodeURIComponent).join("/");
/** 결과물 파일의 경로는 맨 위를 `/`로 둔 것이다(문서 기준 경로 풀기가 절대 경로를 바란다). 보일 때는 뗀다 */
const libraryInfo = (f: LibraryFile): FileInfo => ({
  path: `/${f.path}`,
  name: f.path.split("/").pop() ?? f.path,
  size: f.size,
  modified_ms: 0,
  mime: f.mime,
  kind: f.kind,
});
const notFound = (path: string) => new ApiError({ code: "not_found", message: `결과물에 없다: ${path}` });

/** 받은 앞부분을 보이는 글로: 잘렸으면 마지막 온전한 줄까지, 줄 수 상한까지(참고 readFilePreviewText·shownSource) */
export function previewText(bytes: Uint8Array, size: number): Loaded {
  let text = new TextDecoder("utf-8").decode(bytes);
  let truncated = bytes.length < size;
  if (truncated) {
    const cut = text.lastIndexOf("\n");
    text = cut < 0 ? "" : text.slice(0, cut);
  }
  const lines = text.split("\n");
  if (lines.length > SOURCE_PREVIEW_LINES) {
    truncated = true;
    lines.length = SOURCE_PREVIEW_LINES;
  }
  return { text: lines.join("\n"), truncated, lines: lines.length };
}

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
export function FileViewer({ path: asked, source: from, onClose, onOpen }: { path: string; source: FileSource; onClose: () => void; onOpen: (path: string) => void }) {
  const pane = "pane" in from ? from.pane : null;
  const project = "project" in from ? from.project : null;
  const thread = "thread" in from ? from.thread : null;
  // 결과물 목록은 한 번 받는다(보기 경로는 2시간 산다)
  const library = useRef<Promise<Library> | null>(null);
  const locate = useCallback(
    async (path: string): Promise<Found> => {
      if (pane !== null) {
        const r = await call({ op: "pane_file", pane, path });
        if (r.kind !== "pane_file") throw new Error("pane_file");
        return r.info === null ? { candidates: r.candidates } : { info: r.info, url: r.url };
      }
      library.current ??= call({ op: "thread_library", project: project ?? "", thread: thread ?? "" }).then((r) => {
        if (r.kind !== "library") throw new Error("thread_library");
        return { base: r.base, files: new Map(r.files.map((f) => [f.path, f])) };
      });
      const lib = await library.current;
      const rel = path.replace(/^\/+/, "");
      const f = lib.files.get(rel);
      if (f === undefined) throw notFound(rel);
      return { info: libraryInfo(f), url: lib.base + encodePath(rel) };
    },
    [pane, project, thread],
  );
  const [path, setPath] = useState(asked);
  const target = useMemo(() => fileLineTarget(path), [path]);
  const wrapCode = useTerminalToggle("wrapCode");
  const markdownWidth = useMarkdownWidth();
  const [info, setInfo] = useState<FileInfo | null>(null);
  const [url, setUrl] = useState<string | null>(null);
  const [candidates, setCandidates] = useState<string[] | null>(null);
  const [problem, setProblem] = useState<Problem | null>(null);
  // 폴더 경로였으면 그 폴더(실제 경로): 파일 대신 폴더 목록을 그린다
  const [directory, setDirectory] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<Loaded | null>(null);
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
    setInfo(null);
    setUrl(null);
    setCandidates(null);
    setProblem(null);
    setLoaded(null);
    setDirectory(null);
    void (async () => {
      try {
        const r = await locate(target.path);
        if (cancelled) return;
        if ("candidates" in r) {
          setCandidates(r.candidates);
          return;
        }
        setInfo(r.info);
        setUrl(r.url);
        if (r.info.kind !== "text" || r.url === null) return;
        // 빈 파일은 범위가 없다(416): 읽지 않는다
        if (r.info.size === 0) {
          setLoaded(previewText(new Uint8Array(), 0));
          return;
        }
        const res = await fetch(r.url, { signal: abort.signal, headers: { range: `bytes=0-${TEXT_PREVIEW_BYTES - 1}` } });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const body = new Uint8Array(await res.arrayBuffer());
        if (!cancelled) setLoaded(previewText(body.subarray(0, TEXT_PREVIEW_BYTES), r.info.size));
      } catch (e) {
        if (cancelled) return;
        // 폴더는 파일처럼 pane 폴더 기준으로 읽는다(참고 FileViewer: 404면 폴더 목록을 물어 본다). 폴더도 아니면 파일 오류 그대로.
        // 결과물 목록에는 폴더가 없다
        if (pane !== null && e instanceof ApiError && e.body.code === "not_found") {
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
      abort.abort();
    };
  }, [locate, pane, target.path]);

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
        const r = await locate(path);
        if ("candidates" in r) return r;
        if (r.url !== null) urls.set(r.info.path, r.url);
        return r.info;
      },
      (path, download) => `${urls.get(path) ?? ""}${download ? "?download=1" : ""}`,
    );
  }, [documentPath, locate]);
  const fullPath = info?.path ?? target.path;
  const shownPath = pane === null ? fullPath.replace(/^\/+/, "") : fullPath;
  const { stem, extension } = pathParts(info?.name ?? shownPath);
  const { folder } = pathParts(shownPath);
  const textFile = info?.kind === "text";
  const markdown = textFile && /\.(?:md|markdown)$/i.test(info.name);
  // 제 script로 그리는 페이지: origin도 network도 없는 sandbox(host PAGE_CSP). 결과물은 보기 경로가 이미 sandbox 페이지다
  // (host VIEW_CSP: 제 폴더의 형제 파일을 읽는다)
  const htmlPage = textFile && info.mime === "text/html";
  const pageUrl = url === null ? null : pane === null ? url : `${url}?render=1`;
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
  // 소스는 이름으로 고른 문법으로 칠한다(참고 FileViewer useHighlightedLines). 긴 것은 worker가 칠하고 그동안 글 그대로다
  const codeShown = textFile && loaded !== null && view === "code" && (!htmlPage || source);
  const sourceText = useMemo(() => (codeShown ? normalizeCode(loaded.text) : ""), [codeShown, loaded]);
  const code = useHighlightedLines(sourceText, codeShown ? languageForPath(info.path) : null);
  const sourceRef = useRef<HTMLPreElement>(null);
  // 표시할 줄: 받은 앞부분 안이면 표시, 밖이면 알림(참고 lineRequest)
  const notes: string[] = [];
  let marked: number | null = null;
  if (textFile && loaded !== null && view === "code" && (!htmlPage || source) && target.line !== null) {
    if (target.line <= loaded.lines) marked = target.line;
    else if (loaded.truncated) notes.push(`${target.line}번째 줄은 미리보기 범위를 벗어납니다. 나머지를 읽으려면 다운로드하세요.`);
    else notes.push(`이 파일은 ${loaded.lines}줄이며 ${target.line}번째 줄은 없습니다.`);
  }
  // 잘린 것은 글을 볼 때만 말한다(페이지는 파일 전체를 그린다)
  const cutShort = textFile && (!htmlPage || source) && loaded?.truncated === true;
  if (cutShort)
    notes.push(
      loaded.lines >= SOURCE_PREVIEW_LINES
        ? `이 파일의 처음 ${SOURCE_PREVIEW_LINES.toLocaleString("ko")}줄만 보입니다. 나머지를 읽으려면 다운로드하세요.`
        : `전체 ${formatBytes(info.size)} 중 처음 ${formatBytes(TEXT_PREVIEW_BYTES)}만 보입니다.`,
    );
  const cutBytes = cutShort && loaded.lines < SOURCE_PREVIEW_LINES;
  if (codeShown && code.tooLong) notes.push("너무 길어 강조 표시를 생략했습니다");
  // 표시한 줄을 가운데로. worker의 색은 자리를 바꾸지 않으니 줄·보기·글이 바뀔 때만 굴린다(참고 TextFileView)
  useLayoutEffect(() => {
    if (marked !== null) sourceRef.current?.querySelector(".hl-line.is-marked")?.scrollIntoView({ block: "center", inline: "nearest" });
  }, [path, view, source, marked, loaded]);

  const body = (() => {
    if (directory !== null && pane !== null) return <DirectoryBrowser key={directory} start={directory} pane={pane} onOpenFile={onOpen} />;
    if (problem !== null)
      return (
        <p className="file-viewer-note" role="alert">
          {problem.kind === "missing"
            ? pane === null
                ? "결과물에 이 파일이 없습니다."
                : "이 경로에 읽을 수 있는 파일이 없습니다."
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
              <iframe
                className="file-viewer-page"
                src={pageUrl ?? undefined}
                sandbox={pane === null ? LIBRARY_SANDBOX : "allow-scripts"}
                title={info.name}
              />
            </>
          );
        if (loaded === null) return <p className="file-viewer-note">여는 중…</p>;
        // 참고 TextFileView: Markdown 너비와 긴 줄 바꿈은 「파일 뷰어」 설정(이 기기)
        return (
          <>
            {htmlPage && modes}
            <div className="file-viewer-content" data-testid={view === "markdown" ? "file-viewer-markdown" : "file-viewer-code"}>
              {view === "markdown" ? (
                <OpenFileContext.Provider value={openLink}>
                  <MarkdownImageContext.Provider value={imagePreview}>
                    <Markdown className={MARKDOWN_CLASS[markdownWidth]}>{loaded.text}</Markdown>
                  </MarkdownImageContext.Provider>
                </OpenFileContext.Provider>
              ) : (
                <CodeLines ref={sourceRef} className="file-viewer-text" lines={code.lines} open={null} lineNumbers wrap={wrapCode} markedLine={marked} />
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

  return (
    <div className="modal-scrim file-viewer-scrim" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="modal file-viewer" role="dialog" aria-modal="true" aria-label={info?.name ?? path} ref={surface} tabIndex={-1} data-testid="file-viewer">
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
                    <span>{cutBytes ? `${formatBytes(info.size)} 중 ${formatBytes(TEXT_PREVIEW_BYTES)}` : formatBytes(info.size)}</span>
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
        <div className="file-viewer-body">{body}</div>
      </section>
    </div>
  );
}
