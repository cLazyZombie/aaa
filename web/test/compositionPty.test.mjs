import assert from 'node:assert/strict';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { once } from 'node:events';
import test from 'node:test';
import { compositionPty } from '../e2e/lib/composition-pty.mjs';

// fs.watch의 통지/합침과 무관하게, 실제 기록 완료 ACK와 정확한 입력 bytes가 있어야만 끝난다.
test('PTY ACK는 대기 전 도착·UTF-8 청크 경계·동일 연결의 다음 입력을 보존한다', { timeout: 5000 }, async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'aaa-pty-ack-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const received = path.join(root, 'input.bin'); fs.writeFileSync(received, '');
  const pty = await compositionPty(received); t.after(() => pty.close());
  const socket = net.createConnection({ port: pty.port, host: '127.0.0.1' });
  t.after(() => socket.destroy()); await once(socket, 'connect');
  socket.write('{"bytes":0}\n'); await pty.ready();
  const bytes = Buffer.from('가나다라');
  let done = false;
  const result = pty.receivedIs('가나다라', 0).then(() => { done = true; });
  // 종성/음절 사이가 아닌 UTF-8 바이트 중간에서 나눠도 replacement 문자를 비교하지 않는다.
  fs.appendFileSync(received, bytes.subarray(0, 2)); socket.write('{"bytes":2}\n');
  // 부모가 이 ACK를 처리했다는 사건: 새 대기는 현재 2 bytes와 즉시 일치한다.
  await pty.receivedIs(bytes.subarray(0, 2), 0);
  assert.equal(done, false);
  fs.appendFileSync(received, bytes.subarray(2));
  assert.equal(done, false, '파일만 보이는 상태는 완료가 아니다');
  // 여러 ACK가 같은 TCP 청크에 와도 마지막 상태를 놓치지 않는다.
  socket.write('{"bytes":3}\n{"bytes":12}\n'); await result;
  await pty.receivedIs('가나다라', 0); // 대기 전에 이미 최종 ACK가 도착했다.
  fs.appendFileSync(received, ' '); socket.write('{"bytes":13}\n');
  await pty.receivedIs(' ', 12);
  fs.appendFileSync(received, 'x'); socket.write('{"bytes":14}\n');
  await assert.rejects(pty.receivedIs('y', 13), /PTY byte 불일치/);
  assert.equal(pty.snapshot().pending, 0);
  socket.destroy(); await once(socket, 'close');
  await assert.rejects(pty.receivedIs('z', 14), /PTY ACK 연결 종료/);
});

test('파일 기록보다 이른 ACK는 성공으로 숨기지 않고 대기를 모두 정리한다', { timeout: 5000 }, async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'aaa-pty-order-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const received = path.join(root, 'input.bin'); fs.writeFileSync(received, '');
  const pty = await compositionPty(received); t.after(() => pty.close());
  const socket = net.createConnection({ port: pty.port, host: '127.0.0.1' });
  t.after(() => socket.destroy()); await once(socket, 'connect');
  const result = pty.receivedIs('가', 0);
  socket.write('{"bytes":3}\n');
  await assert.rejects(result, /PTY ACK가 파일 기록보다 앞섰다/);
  assert.equal(pty.snapshot().pending, 0);
});
