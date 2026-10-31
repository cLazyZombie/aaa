// 부모 E2E가 실패한 단계의 미완료 Promise와 WebKit 자식을 사건으로 관찰한다.
import { createRequire } from 'node:module';
import path from 'node:path';
import { cleanupScope } from '../lib/cleanup-scope.mjs';
const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), 'noop.js'));
const { webkit } = require('playwright');
const specCleanup = cleanupScope();
let server;
try {
  await specCleanup.run(async () => {
    let fail;
    const failure = new Promise((_, reject) => { fail = reject; });
    const operation = (async () => {
      if (process.argv[2] === 'unowned') server = await webkit.launchServer({ headless: true });
      else server = await specCleanup.own(() => webkit.launchServer({ headless: true }), value => value.close()).value;
      fail(new Error('thread-lifetime-owner-failed'));
      // 단계 상한에서 떨어져 나간 작업처럼 자기 finally에 도달하지 않는다.
      await new Promise(() => {});
    })();
    await Promise.race([operation, failure]);
  });
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
process.send({ event: 'owner-finished', pid: server.process().pid, exit: server.process().exitCode });
if (server.process().exitCode === null) {
  // 회귀가 생겨도 부모는 실제 원인을 확인한 뒤 이 자식만 사건으로 정리할 수 있다.
  process.once('message', async () => { await server.close(); process.disconnect(); });
} else process.disconnect();
