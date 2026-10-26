import type { LibraryFile } from "./gen/proto";

/** 보존 판은 agent에서 대신 읽지 않으므로 index.html이 있어도 누락을 먼저 알린다. */
export function LibraryCopyNotice({ files }: { files: readonly LibraryFile[] }) {
  const missing = files.filter((f) => f.on_agent);
  if (missing.length === 0) return null;
  return <details className="banner library-copy-notice" data-testid="library-copy-missing">
    <summary>보고 사본에 빠진 파일 {missing.length}개</summary>
    <p>아래 파일은 host 사본에 없다. 원본 작업 폴더를 정리하면 잃을 수 있다.</p>
    <ul>{missing.map((f) => <li key={f.path}>{f.path}</li>)}</ul>
  </details>;
}
