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
- 복귀 직후 스크롤(t-0839): `_ignoreNextScrollEvent`가 켜진 자체 동기화와 사용자 휠이 같은 native scroll 사건으로 합쳐지면,
  무조건 사건을 버리던 xterm은 viewport만 0, buffer.ydisp는 맨 아래(회귀의 43줄)에 남겼다. 현재 viewport가 직전에 DOM에 쓴 위치와
  같을 때만 무시하고 다른 위치로 이동한 사건은 처리한다. `xterm-source.test.mjs`는 빌드에 쓰는 실제 패치 메서드를 실행해
  자체 동기화·합쳐진 위로 이동·reset의 0 높이를 대조한다. E2E93은 WebKit·Chromium 두 폭에서 복귀 직후 첫 휠과 기록을 본다.
- 연속 키(t-0870): 비교 기준이 현재 `buffer.ydisp`면 PageUp의 DOM 동기화 → PageDown → 늦은 PageUp native scroll 순서에서
  새 목표를 29줄 되돌린다. `_innerRefresh`가 쓴 `scrollTop`을 되읽어 반올림·clamp 뒤 실제 위치를 보관한다.
  자체 동기화 사건을 무시한 뒤에도 `syncScrollArea()`로 새 목표를 반영한다. 다음 키는 사건 전의 `_lastScrollTop`을 보고
  동기화를 생략할 수 있기 때문이다. 유닛은 실제 upstream 메서드로 논리 위치·다음 frame의 DOM 위치와 사용자 이동을 확인하고,
  E2E03은 재연결·resize 뒤 Chromium·WebKit의 native scroll 도착 경계에서 같은 앱 단축키를 호출한다.

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

## Viewport 해제 뒤 callback(t-0945)

`xterm-5.5.0-viewport.patch`는 upstream `Disposable._isDisposed`도 확인한다.
생성자의 task와 `reset()`의 frame은 viewport가 끝난 뒤에도 도착할 수 있다.
`syncScrollArea`는 해제된 renderer 크기를 읽지 않고, `_innerRefresh`는 DOM을 갱신하지 않는다.
기존 deferred terminal disposal은 유지한다. 실제 upstream 메서드 대조는
`web/test/xterm-source.test.mjs`, 실제 WebKit 연결/시계/오류 검사는 E2E84가 맡는다.

## 배율 monitor의 수명(t-0957)

`xterm-5.5.0-lifecycle.patch`는 CoreBrowserService의 ScreenDprMonitor를 기존
Disposable에 등록하는 한 줄이다. upstream 5.5.0은 이 monitor를 해제하지 않아
터미널을 다시 만들 때마다 window resize와 media query listener를 하나씩 남긴다.
실제 upstream 모듈의 20회 생성/해제 대조와 E2E102의 전역 listener 수로 확인한다.
xterm의 기존 MIT 저작권·라이선스 고지를 유지한다.
