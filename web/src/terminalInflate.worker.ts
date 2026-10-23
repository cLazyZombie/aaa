import { decodeBase64, inflateGzip } from "./terminalInflate";

// 생성자까지 Worker 안에서 확인한다. 지원 실패는 연결 전에만 identity로 내려간다.
try {
  new DecompressionStream("gzip");
  postMessage({ ready: true });
} catch {
  postMessage({ ready: false });
}
let busy = false;
onmessage = async (event: MessageEvent<{ id: number; data: string; rawLen: number }>) => {
  if (busy) { postMessage({ id: event.data.id, error: true }); return; }
  busy = true;
  const { id, data, rawLen } = event.data;
  try {
    const started = performance.now();
    const bytes = await inflateGzip(decodeBase64(data), rawLen);
    postMessage({ id, bytes, codecMs: performance.now() - started }, { transfer: [bytes.buffer] });
  } catch {
    postMessage({ id, error: true });
  } finally {
    busy = false;
  }
};
