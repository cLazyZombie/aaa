/** 파일별 성공을 보관한다. 실패하면 새 요청을 멈추고 이미 보낸 요청이 끝난 뒤 오류를 낸다.
 * 완료 순서는 경로 순서와 다르므로 성공 개수로 다음 시작 위치를 정하지 않는다. */
export async function uploadReportFiles(
  paths: readonly { path: string; index: number }[],
  completed: Set<string>,
  put: (entry: { path: string; index: number }) => Promise<unknown>,
  progress: (done: number) => void,
): Promise<void> {
  const pending = paths.filter(entry => !completed.has(entry.path));
  let next = 0;
  const failures: Error[] = [];
  const worker = async () => {
    while (failures.length === 0 && next < pending.length) {
      const entry = pending[next++]!;
      try {
        await put(entry);
        completed.add(entry.path);
        progress(completed.size);
      } catch (cause) {
        failures.push(new Error(`${entry.path}: ${cause instanceof Error ? cause.message : String(cause)}`, { cause }));
      }
    }
  };
  // Uppy XHR/p-limit의 유한 worker 방식. 큰 파일도 최대 네 개만 요청 본문으로 보낸다.
  await Promise.all(Array.from({ length: Math.min(4, pending.length) }, worker));
  if (failures.length) throw new AggregateError(failures, `파일 ${completed.size}/${paths.length}개 올림. ${failures.map(error => error.message).join("; ")}`);
}
