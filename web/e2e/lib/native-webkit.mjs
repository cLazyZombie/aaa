// macOS WebKit의 NSTextInputClient를 직접 호출한다. 합성 DOM 사건 없이 실제 marked text를 만든다.
// NSWindow는 화면에 올리지 않으며 OS 입력기·사용자 키보드 설정·GUI focus를 바꾸지 않는다.
import { execFileSync, spawn } from 'node:child_process';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

export function nativeWebkit(root) {
  const executable = `${root}/native-webkit`;
  execFileSync('clang', ['-fobjc-arc', '-framework', 'Cocoa', '-framework', 'WebKit', fileURLToPath(new URL('./native-webkit.m', import.meta.url)), '-o', executable]);
  const child = spawn(executable, [], { stdio: ['pipe', 'pipe', 'pipe'] });
  let serial = 0, stderr = '';
  const pending = new Map();
  child.stderr.on('data', data => { stderr += data; });
  const fail = error => { for (const { reject, timer } of pending.values()) { clearTimeout(timer); reject(error); } pending.clear(); };
  child.on('error', fail);
  child.on('exit', (code, signal) => fail(new Error(`native WebKit 종료: code=${code}, signal=${signal}, ${stderr}`)));
  const lines = readline.createInterface({ input: child.stdout });
  lines.on('line', line => {
    const message = JSON.parse(line);
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id); clearTimeout(request.timer);
    if (message.error) request.reject(new Error(message.error)); else request.resolve(message.result);
  });
  const send = (op, args = {}) => new Promise((resolve, reject) => {
    const id = ++serial;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`native WebKit ${op} 응답 없음: ${stderr}`)); }, 20_000);
    pending.set(id, { resolve, reject, timer });
    child.stdin.write(JSON.stringify({ id, op, ...args }) + '\n');
  });
  const evaluate = (fn, arg) => send('eval', { js: `return (${fn})(${JSON.stringify(arg) ?? 'undefined'});` });
  return {
    load: url => send('load', { url }),
    evaluate,
    waitForFunction: (fn, arg) => send('eval', { js: `return new Promise((resolve, reject) => {
        const deadline = performance.now() + 20_000;
        const check = () => {
          try {
            if ((${fn})(${JSON.stringify(arg) ?? 'undefined'})) resolve(true);
            else if (performance.now() >= deadline) reject(new Error('native WebKit 조건 없음'));
            else requestAnimationFrame(check);
          } catch (error) { reject(error); }
        };
        check();
      });` }),
    mark: text => send('mark', { text }),
    commit: text => send('commit', { text }),
    transition: (commit, mark) => send('transition', { commit, mark }),
    key: (text, code, flags = 0) => send('key', { text, code, flags }),
    keyboard: () => send('keyboard'),
    interpret: (text, code, flags = 0) => send('interpret', { text, code, flags }),
    resize: (width, height) => send('resize', { width, height }),
    screenshot: ({ path }) => send('shot', { path }),
    close: async () => {
      if (child.exitCode !== null || child.signalCode) { lines.close(); return; }
      const exited = new Promise(resolve => child.once('exit', resolve));
      await send('quit');
      await exited; lines.close();
    },
  };
}
