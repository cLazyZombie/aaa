// 파일 변경 통지는 write 완료 ACK가 아니다. fixture가 기록한 뒤 같은 연결로 누적 byte 수를 알린다.
import fs from 'node:fs';
import net from 'node:net';
import readline from 'node:readline';

export async function compositionPty(received) {
  let socket, lines, bytes = 0, ready = false, failure;
  const pending = new Set(), events = [];
  const record = (event, extra = {}) => {
    events.push({ at: performance.now(), event, bytes, visible: fs.statSync(received).size, ...extra });
    if (events.length > 80) events.shift();
  };
  const changed = () => { for (const check of [...pending]) check(); };
  const fail = error => { failure ??= error; changed(); };
  const server = net.createServer(connection => {
    if (socket) { connection.destroy(); return fail(new Error('PTY ACK 연결이 둘이다')); }
    socket = connection;
    socket.on('error', fail);
    socket.on('close', () => fail(new Error('PTY ACK 연결 종료')));
    lines = readline.createInterface({ input: socket });
    lines.on('line', line => {
      try {
        const ack = JSON.parse(line);
        if (!Number.isSafeInteger(ack.bytes) || ack.bytes < bytes) throw new Error(`잘못된 PTY ACK: ${line}`);
        bytes = ack.bytes; ready = true;
        record('ack');
        if (fs.statSync(received).size < bytes) throw new Error('PTY ACK가 파일 기록보다 앞섰다');
        changed();
      } catch (error) { fail(error); }
    });
  });
  server.on('error', fail);
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => { server.removeListener('error', reject); resolve(); });
  });
  const wait = (what, check) => new Promise((resolve, reject) => {
    const finish = error => { clearTimeout(timer); pending.delete(test); error ? reject(error) : resolve(); };
    const test = () => {
      try { if (failure) throw failure; if (check()) finish(); }
      catch (error) { finish(error); }
    };
    const timer = setTimeout(() => finish(new Error(`5000ms 안에 ${what}: ${JSON.stringify(events)}`)), 5000);
    pending.add(test); test();
  });
  return {
    port: server.address().port,
    ready: () => wait('PTY 연결', () => ready),
    output: command => {
      if (failure) throw failure;
      if (!ready) throw new Error('PTY 준비 전에 출력했다');
      socket.write(JSON.stringify(command) + '\n');
    },
    receivedIs: (expected, offset) => {
      const want = Buffer.from(expected);
      return wait(`PTY 기대 ${JSON.stringify(expected)}`, () => {
        if (bytes < offset) return false;
        // ACK 이전의 부분 UTF-8을 문자열로 잘라 비교하지 않는다.
        const actual = fs.readFileSync(received).subarray(offset, bytes);
        if (actual.length > want.length || !want.subarray(0, actual.length).equals(actual))
          throw new Error(`PTY byte 불일치: 기대 ${want.toString('hex')}, 실제 ${actual.toString('hex')}`);
        return actual.equals(want);
      });
    },
    snapshot: () => ({ ready, bytes, failure: failure?.message, pending: pending.size, events }),
    close: async () => {
      fail(new Error('PTY 관측 종료')); lines?.close(); socket?.destroy();
      await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    },
  };
}

// agent가 띄운 실제 PTY 자식에서 실행한다. 조합 중에는 입력을 보관하고 명시한 flush에서만 echo한다.
export async function runCompositionPty(port, received) {
  process.stdin.setRawMode(true);
  const socket = net.createConnection({ port, host: '127.0.0.1' });
  socket.on('close', () => process.stdin.destroy());
  socket.on('error', error => { console.error(`PTY ACK socket: ${error.stack}`); process.exitCode = 1; process.stdin.destroy(); });
  let pending = Buffer.alloc(0), bytes = fs.statSync(received).size;
  readline.createInterface({ input: socket }).on('line', line => {
    const command = JSON.parse(line);
    if (command.flush) { process.stdout.write(pending); pending = Buffer.alloc(0); }
    else if (command.reset) { pending = Buffer.alloc(0); process.stdout.write(command.reset); }
    else process.stdout.write(command);
  });
  process.stdin.on('data', data => {
    fs.appendFileSync(received, data);
    pending = Buffer.concat([pending, data]); bytes += data.length;
    socket.write(JSON.stringify({ bytes }) + '\n');
  });
  socket.once('connect', () => {
    socket.write(JSON.stringify({ bytes }) + '\n');
    process.stdout.write('\x1b[2J\x1b[3;1HWorking 42s\x1b[6;1Hprompt> ');
  });
}
