// Worker와 악성 입력 시험이 함께 쓰는 제한된 gzip 해제. 본문은 기록하지 않는다.
export const PAYLOAD_MAX = 768 * 1024;
export const WIRE_MAX = 1024 * 1024;

export function decodeBase64(value: unknown): Uint8Array<ArrayBuffer> {
  if (typeof value !== "string" || value.length > 4 * Math.ceil(PAYLOAD_MAX / 3) || value.length % 4 !== 0 ||
      !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) throw new Error("터미널 base64 상한/형식");
  const text = atob(value);
  if (btoa(text) !== value) throw new Error("터미널 base64 끝 비트");
  if (text.length > PAYLOAD_MAX) throw new Error("터미널 본문 상한");
  return Uint8Array.from(text, (c) => c.charCodeAt(0));
}

export function rawLength(value: unknown): number {
  if (typeof value !== "object" || value === null) throw new Error("터미널 압축 metadata");
  const { codec, raw_len } = value as { codec: unknown; raw_len: unknown };
  if (codec !== "gzip" || typeof raw_len !== "number" || !Number.isSafeInteger(raw_len) || raw_len < 0 || raw_len > PAYLOAD_MAX) throw new Error("터미널 codec/원본 크기");
  return raw_len;
}

export async function inflateGzip(data: Uint8Array<ArrayBuffer>, rawLen: number, signal?: AbortSignal): Promise<Uint8Array<ArrayBuffer>> {
  if (!Number.isSafeInteger(rawLen) || rawLen < 0 || rawLen > PAYLOAD_MAX || data.length > PAYLOAD_MAX) throw new Error("터미널 해제 상한");
  const stream = new DecompressionStream("gzip");
  const writer = stream.writable.getWriter();
  const reader = stream.readable.getReader();
  let cancelled = false;
  const cancel = () => {
    cancelled = true;
    void reader.cancel().catch(() => {});
    void writer.abort().catch(() => {});
  };
  signal?.addEventListener("abort", cancel, { once: true });
  // native inflate에 전체 압축 폭탄을 한 번에 넘기지 않는다. 입력은 최대 1 KiB씩 공급한다.
  const feed = (async () => {
    for (let at = 0; at < data.length; at += 1024) {
      if (cancelled || signal?.aborted) throw new Error("터미널 해제 취소");
      await writer.write(data.subarray(at, at + 1024));
    }
    await writer.close();
  })();
  // reader 실패와 동시에 writer가 실패해도 unhandled rejection을 만들지 않는다.
  void feed.catch(() => {});
  try {
    const out = new Uint8Array(rawLen);
    let offset = 0;
    for (;;) {
      if (cancelled || signal?.aborted) throw new Error("터미널 해제 취소");
      const { value, done } = await reader.read();
      if (done) break;
      if (offset + value.length > rawLen) throw new Error("터미널 해제 크기 초과");
      out.set(value, offset);
      offset += value.length;
    }
    await feed;
    if (cancelled || offset !== rawLen) throw new Error("터미널 원본 길이 불일치");
    return out;
  } catch (error) {
    cancel();
    throw error;
  } finally {
    signal?.removeEventListener("abort", cancel);
  }
}
