export type ActionProgress = (message: string) => void;

/** host의 보고서 변경 한도(500개)를 지킨다. 성공한 묶음만 진행에 더하며 실패 뒤 자동 재전송하지 않는다. */
export async function runBatches<T>(
  items: readonly T[],
  apply: (batch: T[]) => Promise<void>,
  progress: ActionProgress,
): Promise<void> {
  const snapshot = [...items];
  let done = 0;
  progress(`처리 중 ${done}/${snapshot.length}`);
  try {
    while (done < snapshot.length) {
      const batch = snapshot.slice(done, done + 500);
      await apply(batch);
      done += batch.length;
      progress(`처리 중 ${done}/${snapshot.length}`);
    }
  } catch (reason) {
    throw new Error(`${snapshot.length}개 중 ${done}개 완료 후 중단: ${reason instanceof Error ? reason.message : String(reason)}`);
  }
}
