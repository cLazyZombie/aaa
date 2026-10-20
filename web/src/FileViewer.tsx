// 파일 보기(결정 20 D7, herdr-web-ui `src/components/FileViewer.tsx`·`FileViewer.css` aa624cb). 웹 터미널의 파일 경로 링크로 연다.
// 옮긴 것: 대화 상자(휴대폰은 화면 가득), 경로 찾기(같은 이름이 여럿이면 고르기), 그림·동영상·소리·PDF, HTML 페이지(sandbox
// 안 페이지와 소스), 글(Markdown 미리보기와 소스, 줄 번호, 줄 참조 표시), 새 탭·내려받기·파일 복사·닫기, 문구(참고 i18n.ko.ts).
// 소스는 참고처럼 문법 강조한다(HighlightedCode, 긴 것은 worker). 2,000줄 넘는 소스는 참고처럼 `@pierre/diffs`의 가상화한 줄
// (herdr/VirtualSource.tsx, 제 worker가 5,000줄·256K자까지 칠함)로 그리고, 256KiB 넘는 소스는 8MiB까지 읽는다(plan §8.175).
// 폴더 경로면 그 폴더 목록을 그린다(참고 DirectoryBrowser, plan §8.148). 「파일 뷰어」 설정은 plan §8.149.
// aaa가 다른 곳: 허용 폴더(§6.5) 밖이면 그 까닭을 말한다(참고는 서버 사용자가 읽을 수 있는 것을 다 연다).
// thread 결과물도 이것으로 연다(차이 목록 R1): 결과물 목록의 보기 경로(`/v/`, opaque origin sandbox)로 읽고, 경로는 결과물
// 안 상대 경로다. 문서 안 링크·그림은 결과물 맨 위를 `/`로 두고 풀어 결과물 밖으로 나가지 않는다(plan §8.168).
// 대화 글의 보고서 주소(`/v/<cap>/…`, plan §8.300)도 같은 보기 경로라 목록 없이 그 주소의 HEAD(종류·크기)로 연다.
import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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
import { readFilePreviewText, SOURCE_PREVIEW_LINES, type FilePreviewText } from "./herdr/filePreviewText.ts";
import { canHighlight, countLines, languageForPath, normalizeCode } from "./herdr/highlight.ts";
import { markdownImagePreview } from "./herdr/markdownImages.ts";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap.ts";
import { useSheetSwipe } from "./herdr/sheets.ts";
import { lineRequest, readBudget, shownSource, sourceLayout, virtualHighlightable } from "./herdr/sourcePreview.ts";
import { useAppearance } from "./theme";
import { useFrameImages, useLightbox } from "./Lightbox.tsx";
import { markdownImages } from "./reportImages.ts";
import "./herdr/FileViewer.css";

// 가상화한 줄과 그 강조는 따로 받는 조각이다: 그것이 필요한 첫 소스와 함께 읽는다(참고 TextFileView)
const VirtualSource = lazy(() => import("./herdr/VirtualSource.tsx"));

/** 그림은 이 크기까지 바로 그린다(참고 markdownImages.ts) */
const MAX_INLINE_IMAGE_BYTES = 20 * 1024 * 1024;
/** 저장한 너비는 설정 값이지 class 이름이 아니다: 한쪽 이름을 바꿔도 다른 쪽은 그대로다(참고 TextFileView MARKDOWN_CLASS) */
/** 결과물 HTML의 sandbox(ThreadPanel 결과물 frame과 같다) */
export const LIBRARY_SANDBOX = "allow-scripts allow-popups allow-popups-to-escape-sandbox allow-downloads";
const MARKDOWN_CLASS: Record<MarkdownWidth, string> = {
  readable: "file-viewer-markdown file-viewer-markdown-readable",
  full: "file-viewer-markdown",
};


/**
 * 파일 보기가 읽는 곳: pane의 파일(허용 폴더 안), thread 결과물, thread 작업 폴더의 파일(보고서 안 링크, plan §8.171), 또는
 * 보기 경로 맨 위(`/v/<cap>/`, 대화의 보고서 주소, plan §8.300)
 */
export type FileSource = { pane: string } | { project: string; thread: string } | { worktree: { project: string; thread: string } } | { view: string };
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
/** 보기 경로가 알린 종류(host `library::content_type`)의 파일 보기 갈래 */
const kindOfMime = (mime: string): FileInfo["kind"] =>
  mime.startsWith("image/") ? "image"
    : mime.startsWith("video/") ? "video"
      : mime.startsWith("audio/") ? "audio"
        : mime === "application/pdf" ? "pdf"
          : mime.startsWith("text/") || mime === "application/json" ? "text"
            : "binary";
/** 보기 경로의 파일: 목록 없이 그 주소의 HEAD로 종류·크기를 읽는다(열람 기록은 문서 탐색만 남는다, host `report_navigation`) */
async function viewFile(base: string, path: string): Promise<Found> {
  const rel = path.replace(/^\/+/, "");
  const url = base + encodePath(rel);
  const res = await fetch(url, { method: "HEAD", credentials: "same-origin" });
  if (res.status === 404) throw notFound(rel);
  if (!res.ok) throw new ApiError({ code: "unavailable", message: `보기 경로가 ${res.status}로 답했다(2시간이 지난 주소일 수 있다)` });
  const mime = (res.headers.get("content-type") ?? "").split(";")[0]!.trim() || "application/octet-stream";
  const name = rel.split("/").pop() || "index.html";
  return { info: { path: `/${rel}`, name, size: Number(res.headers.get("content-length") ?? 0), modified_ms: 0, mime, kind: kindOfMime(mime) }, url };
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
  // 보고서 안 링크는 pane 대신 그 thread의 머신·작업 폴더에서 찾는다. 찾은 뒤는 pane 파일과 같은 읽기 경로(`/f/`)다
  const worktree = "worktree" in from ? from.worktree : null;
  const viewBase = "view" in from ? from.view : null;
  const isLibrary = project !== null || viewBase !== null;
  // 결과물 목록은 한 번 받는다(보기 경로는 2시간 산다)
  const library = useRef<Promise<Library> | null>(null);
  const locate = useCallback(
    async (path: string): Promise<Found> => {
      if (pane !== null) {
        const r = await call({ op: "pane_file", pane, path });
        if (r.kind !== "pane_file") throw new Error("pane_file");
        return r.info === null ? { candidates: r.candidates } : { info: r.info, url: r.url };
      }
      if (worktree !== null) {
        const r = await call({ op: "thread_file", project: worktree.project, thread: worktree.thread, path });
        if (r.kind !== "pane_file") throw new Error("thread_file");
        return r.info === null ? { candidates: r.candidates } : { info: r.info, url: r.url };
      }
      if (viewBase !== null) return viewFile(viewBase, path);
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
    [pane, project, thread, worktree?.project, worktree?.thread, viewBase],
  );
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
  // 결과물 페이지(보기 경로)·Markdown의 그림을 누르면 크게(plan §8.349)
  const lightbox = useLightbox();
  const page = useRef<HTMLIFrameElement>(null);
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
        const r = await locate(target.path);
        if (cancelled) return;
        if ("candidates" in r) {
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
      clearTimeout(deadline);
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
  const shownPath = isLibrary ? fullPath.replace(/^\/+/, "") : fullPath;
  const { stem, extension } = pathParts(info?.name ?? shownPath);
  const { folder } = pathParts(shownPath);
  const textFile = info?.kind === "text";
  const markdown = textFile && /\.(?:md|markdown)$/i.test(info.name);
  // 제 script로 그리는 페이지: origin도 network도 없는 sandbox(host PAGE_CSP). 결과물은 보기 경로가 이미 sandbox 페이지다
  // (host VIEW_CSP: 제 폴더의 형제 파일을 읽는다)
  const htmlPage = textFile && info.mime === "text/html";
  const pageUrl = url === null ? null : isLibrary ? url : `${url}?render=1`;
  useFrameImages(page, htmlPage && !source && isLibrary ? pageUrl : null, lightbox.open);
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
    if (directory !== null && pane !== null) return <DirectoryBrowser key={directory} start={directory} pane={pane} onOpenFile={onOpen} />;
    if (problem !== null)
      return (
        <p className="file-viewer-note" role="alert">
          {problem.kind === "missing"
            ? isLibrary
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
            <iframe
              ref={page}
              className="file-viewer-page"
              src={pageUrl ?? undefined}
              sandbox={isLibrary ? LIBRARY_SANDBOX : "allow-scripts"}
              title={info.name}
            />
          );
        if (loaded === null) return <p className="file-viewer-note">여는 중…</p>;
        // 참고 TextFileView: Markdown 너비와 긴 줄 바꿈은 「파일 뷰어」 설정(이 기기)
        return (
          <>
            <div
              className={virtual !== null && view === "code" ? "file-viewer-content file-viewer-content-virtual" : "file-viewer-content"}
              data-testid={view === "markdown" ? "file-viewer-markdown" : "file-viewer-code"}
              onClick={(e) => {
                const got = view === "markdown" ? markdownImages(e.currentTarget, e.target) : null;
                if (!got) return;
                e.preventDefault();
                lightbox.open(got);
              }}
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
        {/* 머리는 어느 폭에서나 한 줄이다(plan §8.302): 참고의 좁은 화면 두 줄(file-viewer-header-stacked)은 휴대폰 내용 칸을 줄였다 */}
        <header className="modal-header file-viewer-header">
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
            {/* HTML의 페이지·소스도 Markdown과 같은 머리 토글이다(참고 t3code FilePreviewPanel의 rendered/source 토글, plan §8.302) */}
            {(markdown || htmlPage) && (
              <div className="file-viewer-group">
                <button
                  type="button"
                  className="icon-button file-viewer-action"
                  aria-pressed={markdown ? mode === "code" : source}
                  aria-label="소스 보기"
                  title="소스 보기"
                  onClick={() => (markdown ? setChosen({ path, mode: mode === "code" ? "preview" : "code" }) : setSource(!source))}
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
      {lightbox.node}
    </div>
  );
}
