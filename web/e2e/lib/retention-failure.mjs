// 추가 페이지 진단이 실패해도 최초 오류와 그때까지 아는 페이지 상태를 보존한다.
export async function retentionFailure(error, context, requests, record, diagnose, snapshot = () => {}) {
  record({ event: "failure", ...context, original: { message: error.message, stack: error.stack }, requests });
  try {
    snapshot();
  } catch (protocolError) {
    record({ event: "protocol-failure", ...context, protocolError: String(protocolError) });
  }
  try {
    record({ event: "diagnostic", ...context, state: await diagnose(), stateSource: "live" });
  } catch (diagnosticError) {
    record({ event: "diagnostic-failure", ...context, diagnosticError: String(diagnosticError), stateSource: "before" });
  }
  throw error;
}
