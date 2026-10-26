// 비동기 생성부터 실험실이 소유한다. 단계 Promise가 분리돼도 자식을 놓치지 않는다.
export function cleanupScope() {
  const pending = new Set();
  let closing = false;
  const own = (acquire, release) => {
    if (closing) throw new Error("정리 중인 실험실에서 자원을 만들 수 없다");
    const value = Promise.resolve().then(acquire);
    let disposal;
    const close = () => disposal ??= value.then(release).finally(() => pending.delete(close));
    pending.add(close);
    return { value, close };
  };
  const close = async () => {
    closing = true;
    const results = await Promise.allSettled([...pending].reverse().map(dispose => dispose()));
    const errors = results.filter(result => result.status === "rejected").map(result => result.reason);
    if (errors.length) throw new AggregateError(errors, "실험실 자원 정리 실패");
  };
  return { own, close };
}
