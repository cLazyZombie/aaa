// E103의 Chromium 핸들 조회를 수동적으로 관측한다. Promise·대기·응답은 바꾸지 않는다.
// Playwright 1.64.1 in-process 연결. E103 합성 fixture의 DOM/Runtime만 원문으로 남긴다.
// Network·console 사건은 수집하지 않는다. 운영 페이지/임의 evaluate의 기록에 사용하지 않는다.
import fs from "node:fs";

const connections = new WeakMap();
const expressionKind = params => {
  const expression = params?.arguments?.find(arg => typeof arg.value === "string" && arg.value.includes("=>"))?.value ?? "";
  if (expression.includes("window.__aaaRetentionEvaluation =")) return "before-state";
  if (expression.includes("evaluation: window.__aaaRetentionEvaluation")) return "diagnostic";
  if (expression === "(r) => r.element") return "lookup-element";
  if (expression.includes("log: r.log")) return "lookup-result";
  if (expression.includes("previewNode")) return "preview";
  if (expression.includes("querySelectorAll")) return "selector";
  return "other";
};

export function retentionProtocol(page, directory) {
  const impl = page._connection.toImpl(page);
  const client = impl.delegate._mainFrameSession._client;
  const connection = client._connection, session = client._sessionId;
  let shared = connections.get(connection);
  if (!shared) {
    const previous = connection._protocolLogger;
    const listeners = new Map();
    const logger = (direction, message) => {
      previous(direction, message);
      listeners.get(message.sessionId)?.(direction, message);
    };
    shared = { previous, listeners, logger };
    connections.set(connection, shared);
    connection._protocolLogger = logger;
  }
  let context = null;
  const pending = new Map();
  fs.mkdirSync(directory, { recursive: true });
  const buffer = [];
  let writeError = null;
  const log = row => { if (!writeError) buffer.push({ at: Date.now(), session, ...row }); };
  // 시한이 걸린 RPC 안에서는 디스크 I/O를 하지 않는다. 단계/페이지 회수 때 원문을 쓴다.
  const flush = () => {
    if (writeError || !buffer.length) return writeError;
    try {
      fs.appendFileSync(`${directory}/retention-protocol.jsonl`, buffer.map(row => JSON.stringify(row)).join("\n") + "\n");
      buffer.length = 0;
    } catch (error) {
      // 기록 실패를 재시도하거나 finally/close에서 최초 API 오류를 덮지 않는다.
      writeError = error;
      console.error("retention-protocol-write-failure", String(error));
    }
    return writeError;
  };
  shared.listeners.set(session, (direction, message) => {
    if (direction === "send") {
      if (!context || !/^(DOM|Runtime)\./.test(message.method)) return;
      const { engine, width, role, trial, pageNo, phase } = context;
      const row = { id: message.id, method: message.method, engine, width, role, trial, pageNo, phase,
        expression: expressionKind(message.params), contextId: message.params?.executionContextId };
      const protocol = structuredClone(message);
      pending.set(message.id, { row, start: performance.now(), protocol });
      log({ event: "send", ...row, protocol });
    } else if (pending.has(message.id)) {
      const { row, start } = pending.get(message.id);
      pending.delete(message.id);
      log({ event: message.error ? "error" : "receive", ...row, ms: performance.now() - start,
        errorCode: message.error?.code, protocol: structuredClone(message) });
    } else if (/^Runtime\.executionContext(Created|Destroyed|sCleared)$/.test(message.method)) {
      log({ event: message.method, contextId: message.params?.executionContextId ?? message.params?.context?.id,
        protocol: structuredClone(message) });
    }
  });
  const snapshot = event => {
    for (const { row, start, protocol } of pending.values()) log({ event, ...row, ms: performance.now() - start, protocol });
  };
  page.once("close", () => {
    snapshot("pending-at-close");
    flush();
    shared.listeners.delete(session);
    if (!shared.listeners.size) {
      if (connection._protocolLogger === shared.logger) connection._protocolLogger = shared.previous;
      connections.delete(connection);
    }
  });
  return {
    at: value => { context = value; if (value === null) flush(); },
    // renderer에 다시 묻기 전의 경계다. 추가 진단·close가 pending을 바꾸기 전에 저장한다.
    failed: () => {
      const { engine, width, role, trial, pageNo, phase } = context ?? {};
      log({ event: "failure-boundary", engine, width, role, trial, pageNo, phase, pending: pending.size });
      snapshot("pending-at-failure");
      const error = flush();
      if (error) throw error;
    },
  };
}
