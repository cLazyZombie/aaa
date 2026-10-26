// 실제 PTY 출력을 browser frame 경계에서 나눈다. 시계/고정 sleep 대신 출력·그리기 ACK로 다음 묶음을 보낸다.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { nativeWebkit } from './native-webkit.mjs';
import { aaa, root, browser, webkit, pairPage, inList, base, until } from './harness.mjs';

function observeComposition() {
  window.preeditFrames = []; window.preeditEvents = []; window.preeditSampling = true;
  const input = document.querySelector('.xterm-helper-textarea');
  for (const type of ['compositionstart', 'compositionupdate', 'compositionend', 'beforeinput', 'input', 'blur']) {
    input.addEventListener(type, event => window.preeditEvents.push({ type, data: event.data, trusted: event.isTrusted,
      inputType: event.inputType, composing: event.isComposing, value: input.value, selection: [input.selectionStart, input.selectionEnd] }));
  }
  const sample = () => {
    const view = document.querySelector('.composition-view');
    const rect = view.getBoundingClientRect(), textarea = input.getBoundingClientRect(), style = getComputedStyle(view);
    const frame = {
      time: performance.now(), text: view.textContent, active: view.classList.contains('active'),
      left: rect.left, top: rect.top, width: rect.width, height: rect.height,
      display: style.display, visibility: style.visibility, opacity: style.opacity,
      textarea: { left: textarea.left, top: textarea.top, width: textarea.width, height: textarea.height, value: input.value },
      rows: document.querySelector('.xterm-rows').textContent,
    };
    window.preeditFrames.push(frame);
    if (window.preeditSampling) requestAnimationFrame(sample);
    return frame;
  };
  window.samplePreedit = sample;
  requestAnimationFrame(sample);
}

export async function compositionOutput() {
  const control = `${root}/preedit-output.jsonl`, received = `${root}/preedit-input.bin`;
  fs.writeFileSync(control, ''); fs.writeFileSync(received, '');
  fs.writeFileSync(`${root}/preedit.cjs`, `
    const fs = require('node:fs');
    process.stdin.setRawMode(true);
    let offset = 0;
    fs.watch(${JSON.stringify(control)}, () => {
      const all = fs.readFileSync(${JSON.stringify(control)}, 'utf8');
      for (const line of all.slice(offset).trim().split('\\n').filter(Boolean)) {
        const command = JSON.parse(line);
        if (command.flush) { process.stdout.write(pending); pending = ''; }
        else if (command.reset) { pending = ''; process.stdout.write(command.reset); }
        else process.stdout.write(command);
      }
      offset = all.length;
    });
    let pending = '';
    process.stdin.on('data', data => { fs.appendFileSync(${JSON.stringify(received)}, data); pending += data; });
    process.stdout.write('\\x1b[2J\\x1b[3;1HWorking 42s\\x1b[6;1Hprompt> ');
  `);
  aaa('tab', 'create', '--machine', 'labA', '--label', 'preedit', '--', process.execPath, `${root}/preedit.cjs`);
  const output = data => fs.appendFileSync(control, JSON.stringify(data) + '\n');
  const artifacts = process.env.AAA_IME_CAPTURE ? path.resolve(process.env.AAA_IME_CAPTURE) : `${root}/diagnostics/preedit`;
  fs.mkdirSync(artifacts, { recursive: true });
  const wk = await webkit.launch();
  const engines = [['chromium', browser], ['webkit-replay', wk]];
  if (process.platform === 'darwin') engines.push(['webkit-native', null]);
  const failures = [];
  try {
    for (const [name, engine] of engines) {
      const native = !engine;
      let p, close;
      if (native) {
        p = nativeWebkit(root); close = () => p.close();
        await p.load(base);
        await p.waitForFunction(() => document.querySelector('#access-gate-code'));
        await p.evaluate(code => {
          const input = document.querySelector('#access-gate-code');
          Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, code);
          input.dispatchEvent(new Event('input', { bubbles: true }));
        }, aaa('web', 'pair').code);
        await p.waitForFunction(() => !document.querySelector('form button').disabled);
        await p.evaluate(() => document.querySelector('form').requestSubmit());
        await p.waitForFunction(() => document.querySelector('[data-testid="pane-preedit"]'));
        await p.evaluate(() => document.querySelector('[data-testid="pane-preedit"]').click());
      } else {
        ({ page: p } = await pairPage(engine, aaa('web', 'pair').code, { width: 1280, height: 800 }));
        close = () => p.context().close();
        await inList(p, 'pane-preedit');
      }
      try {
        output('\x1b[2J\x1b[3;1HWorking 42s\x1b[6;1Hprompt> ');
        await p.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('Working 42s') && document.querySelector('.term-box')?.dataset.readonly === 'false');
        await p.evaluate(async () => { await document.fonts.ready; document.querySelector('.xterm-helper-textarea').focus(); });
        await p.evaluate(observeComposition);
        const cdp = name === 'chromium' ? await p.context().newCDPSession(p) : null;
        const mark = async text => {
          if (native) return p.mark(text);
          if (cdp) return cdp.send('Input.imeSetComposition', { text, selectionStart: text.length, selectionEnd: text.length });
          return p.evaluate(text => {
            const input = document.querySelector('.xterm-helper-textarea');
            if (!window.replayComposing) {
              window.replayPrefix = input.value;
              input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
              window.replayComposing = true;
            }
            input.value = window.replayPrefix + text;
            input.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: text }));
            input.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertCompositionText', isComposing: true }));
          }, text);
        };
        const commit = async text => {
          if (native) return p.commit(text);
          if (cdp) return cdp.send('Input.insertText', { text });
          return p.evaluate(text => {
            const input = document.querySelector('.xterm-helper-textarea');
            if (window.replayComposing) {
              input.value = window.replayPrefix + text;
              input.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: text }));
              window.replayComposing = false;
            } else {
              input.value += text;
              input.dispatchEvent(new InputEvent('input', { bubbles: true, composed: true, data: text, inputType: 'insertText' }));
            }
          }, text);
        };
        const offset = fs.statSync(received).size;
        for (const text of ['ㅎ', '하', '한']) {
          if (native) await p.mark(text);
          else if (cdp) await cdp.send('Input.imeSetComposition', { text, selectionStart: text.length, selectionEnd: text.length });
          else await p.evaluate(text => {
            const input = document.querySelector('.xterm-helper-textarea');
            if (!input.value) input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
            input.value = text;
            input.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: text }));
            input.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertCompositionText', isComposing: true }));
          }, text);
          await p.waitForFunction(text => window.preeditFrames.some(frame => frame.text === text && frame.active), text);
        }
        const anchor = await p.evaluate(() => window.preeditFrames.at(-1));
        let index = 0;
        const capture = async label => {
          if (process.env.AAA_IME_CAPTURE) await p.screenshot({ path: `${artifacts}/${name}-${String(index++).padStart(2, '0')}-${label}.png` });
        };
        await capture('start');
        // 한 묶음 갱신과 커서 왕복이 서로 다른 PTY frame인 갱신을 모두 통과시킨다.
        for (let tick = 43; tick <= 45; tick++) {
          output(`\x1b[?25l\x1b[3;1HWorking ${tick}s\x1b[K\x1b[6;9H\x1b[?25h`);
          await p.waitForFunction(tick => window.preeditFrames.some(frame => frame.rows.includes(`Working ${tick}s`)), tick);
          await capture('whole');
          const from = await p.evaluate(() => window.preeditFrames.length);
          output(`\x1b[?25l\x1b[3;1HWorking ${tick}.1s\x1b[K`);
          await p.waitForFunction(({ tick, from }) => window.preeditFrames.slice(from).some(frame => frame.rows.includes(`Working ${tick}.1s`)), { tick, from });
          await capture('status');
          const returned = await p.evaluate(() => window.preeditFrames.length);
          output('\x1b[6;9H\x1b[?25h');
          await p.waitForFunction(from => window.preeditFrames.length > from && document.querySelector('.xterm-cursor'), returned);
          await capture('return');
        }
        const data = await p.evaluate(() => { window.preeditSampling = false; return { frames: window.preeditFrames, events: window.preeditEvents }; });
        const pending = data.frames.filter(frame => frame.time >= anchor.time);
        const bad = pending.filter(frame => frame.text !== '한' || !frame.active || frame.display === 'none' || frame.visibility === 'hidden' || frame.opacity === '0'
          || Math.abs(frame.left - anchor.left) > 1 || Math.abs(frame.top - anchor.top) > 1
          || Math.abs(frame.textarea.left - anchor.textarea.left) > 1 || Math.abs(frame.textarea.top - anchor.textarea.top) > 1);
        fs.writeFileSync(`${artifacts}/${name}.json`, JSON.stringify({ anchor, badFrames: bad.length, ...data }, null, 2));
        assert.equal(fs.statSync(received).size, offset, `${name}: 확정 전 PTY 전송`);
        if (native) await p.commit('한');
        else if (cdp) await cdp.send('Input.insertText', { text: '한' });
        else await p.evaluate(() => document.querySelector('.xterm-helper-textarea').dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '한' })));
        await until(`${name} 확정 PTY`, () => fs.statSync(received).size > offset);
        assert.equal(fs.readFileSync(received).subarray(offset).toString('utf8'), '한', `${name}: 확정은 한 번만`);
        const events = await p.evaluate(() => window.preeditEvents);
        fs.writeFileSync(`${artifacts}/${name}-events.json`, JSON.stringify(events, null, 2));
        if (name !== 'webkit-replay') assert.ok(data.events.filter(event => event.type.startsWith('composition')).every(event => event.trusted), `${name}: 실제 IME 사건`);
        console.log(`     ${name}: ${pending.length} frames, 위치/표시 실패 ${bad.length}, 실제 IME ${name !== 'webkit-replay'}`);
        if (bad.length) failures.push(`${name}: 조합 위치/표시 ${bad.length}/${pending.length} frame 실패; ${JSON.stringify(bad[0])}`);
        // 같은 host/agent/PTY에서 echo를 사건으로 늦춘다. 앞 확정과 다음 begin 사이에 echo를 허용하지 않는다.
        const receivedIs = expected => new Promise((resolve, reject) => {
          const read = () => fs.readFileSync(received).subarray(inputOffset).toString('utf8');
          const watcher = fs.watch(received, check);
          const timer = setTimeout(() => { watcher.close(); reject(new Error(`${name}: PTY 기대 ${JSON.stringify(expected)}, 실제 ${JSON.stringify(read())}`)); }, 20_000);
          function check() { if (read() === expected) { clearTimeout(timer); watcher.close(); resolve(); } }
          check();
        });
        let inputOffset = fs.statSync(received).size;
        // 앞 시험의 단어를 끝낸 뒤 output/echo 보관을 새 fixture로 바꾼다.
        await commit(' '); await receivedIs(' ');
        for (const syllables of [['가', '나', '다', '라'], ['가', '가']]) {
          const label = syllables.length === 4 ? 'continuous' : 'final-consonant';
          inputOffset = fs.statSync(received).size;
          output({ reset: `\x1b[2J\x1b[3;1H${label}\x1b[6;1Hprompt> ` });
          await p.waitForFunction(label => document.querySelector('.xterm-rows').textContent.includes(label), label);
          await p.evaluate(() => { window.preeditFrames = []; window.preeditEvents = []; window.preeditSampling = true; window.samplePreedit(); });
          await mark(label === 'continuous' ? 'ㄱ' : '각');
          await p.waitForFunction(() => document.querySelector('.composition-view').classList.contains('active'));
          const origin = await p.evaluate(() => window.samplePreedit());
          let prefix = '';
          for (let i = 0; i < syllables.length; i++) {
            const syllable = syllables[i];
            if (i || label === 'continuous') await mark(syllable);
            // 각→가가: 입력기의 확정 값 가가 이전 preedit 각을 바꾼다.
            const shown = i === 0 && label !== 'continuous' ? '각' : syllable;
            await p.waitForFunction(text => window.preeditFrames.some(frame => frame.text === text && frame.active), shown);
            const frame = await p.evaluate(() => window.samplePreedit());
            const width = await p.evaluate(() => document.querySelector('.xterm-cursor').getBoundingClientRect().width);
            assert.ok(Math.abs(frame.left - origin.left - i * width * 2) <= 1, `${name}/${label}/${i}: echo 전 시작 열 ${frame.left}, origin ${origin.left}, cell ${width}`);
            await capture(`${label}-${i}-before-echo`);
            if (i + 1 < syllables.length) {
              if (native) await p.transition(syllable, syllables[i + 1]);
              else { await commit(syllable); await mark(syllables[i + 1]); }
              prefix += syllable;
              await receivedIs(prefix);
              output({ flush: true });
              await p.waitForFunction(prefix => document.querySelector('.xterm-rows').children[5].textContent.includes(prefix), prefix);
              await capture(`${label}-${i}-echo`);
              const expectedLeft = origin.left + (i + 1) * width * 2;
              const echo = await p.evaluate(() => window.samplePreedit());
              assert.ok(Math.abs(echo.left - expectedLeft) <= 1, `${name}/${label}: echo 뒤 ${JSON.stringify({ echo, expectedLeft, width, origin })}`);
              // 다음 음절을 조합하는 동안에도 상태 줄 커서 왕복은 분리한다.
              output(`\x1b[3;1H${label}-${i}\x1b[K`);
              await p.waitForFunction(label => document.querySelector('.xterm-rows').textContent.includes(label), `${label}-${i}`);
              const status = await p.evaluate(() => window.samplePreedit());
              assert.ok(Math.abs(status.left - expectedLeft) <= 1 && Math.abs(status.top - origin.top) <= 1, `${name}/${label}: 분리 상태 줄에서 위치 유지`);
              output(`\x1b[6;${9 + (i + 1) * 2}H`);
            } else {
              await commit(syllable); prefix += syllable; await receivedIs(prefix);
            }
          }
          await commit(' '); await receivedIs(prefix + ' '); output({ flush: true });
          await p.waitForFunction(prefix => document.querySelector('.xterm-rows').children[5].textContent.includes(prefix), prefix);
          await capture(`${label}-space`);
          const trace = await p.evaluate(() => { window.preeditSampling = false; return { frames: window.preeditFrames, events: window.preeditEvents }; });
          fs.writeFileSync(`${artifacts}/${name}-${label}.json`, JSON.stringify(trace, null, 2));
          assert.equal(fs.readFileSync(received).subarray(inputOffset).toString('utf8'), prefix + ' ', `${name}/${label}: 확정 순서·중복 없음`);
          console.log(`     ${name}/${label}: ${prefix}+space, echo 전·후/분리 출력 위치 통과`);
        }
        if (cdp) await cdp.detach();
      } catch (error) {
        const trace = await p.evaluate(() => ({ frames: window.preeditFrames, events: window.preeditEvents })).catch(() => null);
        fs.writeFileSync(`${artifacts}/${name}-failure.json`, JSON.stringify({ error: String(error), trace }, null, 2));
        throw error;
      } finally { await close(); }
    }
  } finally { await wk.close(); }
  aaa('pane', 'kill', 'preedit');
  assert.equal(failures.length, 0, failures.join('\n'));
}
