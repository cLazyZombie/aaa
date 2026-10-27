// E109 전용 관측. 페이지가 닫혀도 최초 오류·정리 순서·브라우저 종료를 Node에 남긴다.
import fs from 'node:fs';
import path from 'node:path';
import { observePopupLifecycle } from './popup.mjs';

const describe = error => ({ message: error.message, stack: error.stack });
export function returnDiagnostics(browser, directory, width, phase) {
  fs.mkdirSync(directory, { recursive: true });
  const file = path.join(directory, `return-lifecycle-${width}.json`);
  const events = [];
  let primary;
  let writeFailed = false;
  const record = (event, detail = {}) => {
    events.push({ at: performance.now(), event, phase: { ...phase() }, ...detail });
    try { fs.writeFileSync(file, JSON.stringify(events, null, 2)); }
    catch (error) {
      if (!writeFailed) console.error(`E109 수명 기록 저장 실패: ${error}`);
      writeFailed = true;
    }
  };
  // 저장소 고정 Playwright 1.64.1의 in-process 연결에서 소유한 자식만 관측한다.
  const options = browser._connection.toImpl(browser).options;
  const child = options.browserProcess.process;
  const logs = () => [...options.browserLogsCollector.recentLogs()];
  child.stderr.on('data', bytes => record('browser.stderr', { text: bytes.toString() }));
  child.once('exit', (code, signal) => record('browser.exit', { code, signal, logs: logs() }));
  record('browser.ready', { pid: child.pid, logs: logs() });
  return {
    record,
    watch: (page, role) => observePopupLifecycle(page, event => record(event, { role })),
    failed: (error, role = 'controller') => {
      primary ??= error;
      record('failure', { role, error: describe(error), logs: logs() });
    },
    // Renderer 조회 자체가 실패해도 원래 오류를 바꾸지 않는다.
    capture: async (error, collect, role = 'controller') => {
      const diagnostic = { phase: { ...phase() }, role, error: describe(error) };
      const failure = path.join(directory, `return-failure-${width}${role === 'controller' ? '' : `-${role}`}.json`);
      fs.writeFileSync(failure, JSON.stringify(diagnostic, null, 2));
      try { Object.assign(diagnostic, await collect()); }
      catch (secondary) { diagnostic.diagnosticError = describe(secondary); }
      fs.writeFileSync(failure, JSON.stringify(diagnostic, null, 2));
    },
    cleanup: async (role, close) => {
      record('cleanup.start', { role });
      try { await close(); record('cleanup.end', { role }); }
      catch (error) {
        record('cleanup.failure', { role, error: describe(error) });
        if (!primary) throw error;
      }
    },
  };
}
