/** 바이트 수를 읽기 좋게(herdr-web-ui `src/lib/bridgeProgress.ts` `formatBytes` 그대로). 파일 보기와 Markdown 그림이 쓴다 */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${Math.max(0, Math.round(bytes))} B`;
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  const megabytes = bytes / (1024 * 1024);
  return `${megabytes < 10 ? Math.round(megabytes * 10) / 10 : Math.round(megabytes)} MB`;
}
