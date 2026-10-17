# xterm.js 5.5.0 IME 패치

`xterm-5.5.0.patch`는 [herdr-web-ui](https://github.com/devswha/herdr-web-ui) `patches/` 아래 xterm 5.5.0 patch([고정 commit `aa624cb`](https://github.com/devswha/herdr-web-ui/tree/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741))를 그대로 옮긴 것이다. 첫 hunk(bun이 붙이는 빈 표지 파일)를 빼고, iPhone 조건(아래)만 더했다.

- 하는 일: 끝난 조합의 보내기를 줄 세우고 이미 보낸 위치(watermark)를 기억한다(조합이 겹쳐도 자모가 따로 나가지 않는다). macOS Safari가 조합 사건 없이 편집기에서 한글을 바꿔 넣는 것과, iPadOS Safari 하드웨어 키보드가 자모마다 keydown을 보내고 편집기에서 조합하는 것을 확정 전까지 보내지 않는다. Backspace 뒤 편집기 비우기, 붙여 넣기·blur·reset 전 확정.
- 빌드: `xterm-source.mjs`가 `node_modules/@xterm/xterm/src`를 cache로 복사하고 이 patch를 붙인 뒤(문맥이 정확히 한 번 맞을 때만) TypeScript로 빌드한다. `vite.config.ts`가 `@xterm/xterm`을 그 결과로 바꾼다. herdr-web-ui `scripts/build-xterm.ts`·`vite.config.ts`와 같은 방식이다.
- **aaa가 바꾼 곳(참고와 다름):** `passHangulKey`와 `beforeinput`의 플랫폼 조건에 `isIphone`을 더했다(`isMac || isIpad || isIphone`). iPhone 화면 키보드도 자모마다 keydown(keyCode 0)을 보내고 편집기에서 조합하는데(사용자 기록 10-17), 참고는 iPhone을 빼서 자모가 따로 나갔다. 사용자 확인으로 참고에도 같은 결함이 있어 aaa에서 직접 고쳤다(plan §8.41).
- xterm 판을 바꾸면 빌드가 멈춘다. patch를 다시 확인한 뒤 바꾼다.
- 시험: `e2e/specs/09-ime.mjs`의 「터미널 한글 IME」 단계가 herdr-web-ui `scripts/terminal-safari-ime-regression.ts`·`scripts/terminal-ipad-ime-regression.ts`의 기록된 사건 순서를 재생한다.

## `xterm-5.5.0-viewport.patch`(aaa, 참고에 없음)

- 하는 일: `Viewport.reset()`은 줄 높이(`_currentRowHeight`)를 0으로 두고 rAF 두 번 뒤(`syncScrollArea` → `_refresh` → `_innerRefresh`)에야 다시 잰다. 그 사이 viewport에 scroll 사건이 오면 `_handleScroll`이 `scrollTop / 0`으로 줄을 구한다. scrollTop이 0이면 NaN이 되어 `ydisp`가 NaN에 굳는다. 그러면 화면이 빈 채로 다시 그려지지 않고, 키·창 크기·아래로 굴리기로도 돌아오지 않는다(QA t-0337 Q1, 데스크톱 Chromium에서 앞 기록을 받을 때 휠을 빠르게 굴리면 6/8). 잴 때까지 그 scroll은 옮기지 않는다. `_innerRefresh`가 scrollTop을 ydisp에 맞춘다. 같은 동안 `getLinesScrolled`(mouse 보고 휠)도 Infinity로 `_wheelPartialScroll`을 NaN에 굳히므로 0을 돌려준다.
- aaa는 앞 기록을 붙일 때(`Term.tsx` `prependHistory`)와 snapshot(`ESC c`)마다 reset한다. herdr-web-ui는 앞 기록을 받지 않아 이 길이 드물다.
- 시험: `e2e/specs/07-scroll-history.mjs`의 「앞 기록 reset 직후 scroll」 단계. 패치를 빼면 ydisp가 NaN이 되어 실패함을 확인했다(plan §8.105).

## 고지

herdr-web-ui의 고지에 따르면 이 patch의 queue/watermark 방식은 joonhoekim의 MIT 제안(https://github.com/xtermjs/xterm.js/pull/6090)을 따른다.

```
MIT License

Copyright (c) 2026 devswha

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

xterm.js 자체도 MIT다: Copyright (c) 2017-2019, The xterm.js authors (https://github.com/xtermjs/xterm.js), Copyright (c) 2014-2016, SourceLair Private Company, Copyright (c) 2012-2013, Christopher Jeffrey. 전문은 `node_modules/@xterm/xterm/LICENSE`에 있다.
