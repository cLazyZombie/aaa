# xterm.js 5.5.0 확장

## IME 입력·표시의 한 소유자(t-1075)

`web/xterm/CompositionHelper.ts`가 조합 범위·확정 FIFO·watermark·Safari replacement·iOS ASCII
교정·입력 연속성·송신 중 상태를 소유한다. 기존 세 겹의 IME 패치(§8.36·t-1001·t-1064)를
이 모듈로 통합했다. `xterm-5.5.0.patch`는 upstream Terminal의 사건·수명 경계를 controller에
연결하고 interface만 확장한다. `xterm-source.mjs`는 npm 소스에 이 bridge를 엄격히 붙이고
controller를 한 번 복사해 빌드한다. xterm 버전과 parser·renderer는 5.5.0 그대로다.

controller는 단어 안 연속 조합의 첫 줄 marker·열과 DOM 시작 offset을 기억한다. 다음 음절의
열은 실제 DOM 확정 범위를 UnicodeService로 세어 정한다. 출력 커서·PTY 에코 시점으로 새
음절 시작점을 잡지 않는다. WebKit의 NBSP→SPACE 정규화도 DOM offset을 바꾸지 않는다.
marker는 단어 안에 한 개뿐이며 일반 키·붙여넣기·blur·취소·reset·삭제된 버퍼에서 해제한다.
현재 조합의 분리된 상태 줄 출력은 위치에 영향을 주지 않는다. 폭 축소 때 시작 열을 clamp하고
연속 확정분의 줄 넘김을 계산한다. marker만으로 모든 임의 reflow의 문자 위치를 복구한다고
가정하지 않는다.

`src/compositionView.ts`만 선택된 상태를 화면에 적용한다. 미리보기와 textarea 좌표·실제 DOM
행 높이·색·글꼴·자간·여백을 같은 호출에서 정한다. 과거 `terminalGlyphs.ts`의 조합 상자
fitView observer와 `compositionView.ts`의 별도 보정 observer는 없앴다. 줄 자체의 글꼴 보정은
유지한다. `Term.tsx`는 controller의 active/pending/sending을 구독해 키 막대·유지 키·CtrlEnter를
판정하며 DOM compositionend 뒤 0ms 타이머로 확정 송신 여부를 따로 추정하지 않는다.

일반 입력이 확정 timer보다 먼저 오면 beforeinput의 DOM 변경 전에 FIFO를 비운다. native
Backspace(isComposing=true)는 preedit 편집에 맡긴다. paste/blur의 finishInput은 native·Safari
replacement 양쪽의 현재 조합과 앞 확정을 먼저 처리한다. 0ms task는 브라우저 DOM 전파와
확정 큐에만 남기며 표시를 다시 덮어쓰는 task는 없다. reset/dispose는 세대와 큐를 버린다.

기존 Safari 무조합 사건 경로, iPhone 하드웨어/화면 키보드와 iOS 영어 실시간 교정 계약은
유지한다. 별도 아래 입력창(TerminalInput)·passive 진단(inputTrace)은 독립 기능이다.

출처는 [herdr-web-ui aa624cb](https://github.com/devswha/herdr-web-ui/tree/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741)의
xterm patch와 terminalGlyphs(MIT), [xterm CompositionHelper](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/input/CompositionHelper.ts)(MIT)다.
queue의 계보는 [xterm PR6090](https://github.com/xtermjs/xterm.js/pull/6090)이다. 초기 aaa의 iPhone
조건은 t-0304 `466069d9`, 영어 교정은 t-1001, 위치 고정은 t-1064에서 왔다. 신규 외부 코드를
가져오지 않고 이 구현을 한 상태·표시 경계로 정리했다. 아래 MIT 고지와 src/herdr/README의
고지는 이동한 코드에도 적용된다.

비교한 [VS Code xterm 연결](https://github.com/microsoft/vscode/blob/4cd688bc6ed94e1859ad89b6f84245675e2e1c22/src/vs/workbench/contrib/terminal/browser/xterm/xtermTerminal.ts)(MIT),
[Ghostty preedit](https://github.com/ghostty-org/ghostty/blob/0fe113a35370667d8c4d6970ee5c1704825dce6b/src/Surface.zig#L2660)(MIT),
[Alacritty Ime 분기](https://github.com/alacritty/alacritty/blob/542147067dd016fc5f49120ef0ba605a98a4e3a7/alacritty/src/event.rs#L2018)(Apache-2.0)의
플랫폼 입력→preedit/commit 상태→표시 경계를 참고한다. 해당 프로젝트의 코드를 복사하지 않았다.
Hyper·Tabby·WezTerm·kitty·ttyd·GoTTY까지의 감사와 라이선스 비교는 작업 보고에 보존한다.

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
실제 upstream 모듈의 20회 생성/해제 대조와 E2E104의 전역 listener 수로 확인한다.
xterm의 기존 MIT 저작권·라이선스 고지를 유지한다.

## `xterm-5.5.0-link-press.patch`(t-1151)

xterm `Linkifier`는 `onRenderedViewportChange`마다 provider 응답을 새로 만들지만,
`mouseup`에서는 `mousedown`의 응답 객체와 같은지를 검사한다. 같은 링크 위에서 누른 채
resize/출력의 그리기가 오면 클릭이 사라진다. provider 순번·대상 문자열·버퍼의 전체 범위가
같고 놓은 자리도 범위 안이면 현재 응답을 한 번 활성화한다. provider·대상·범위가 바뀌거나
누름 없는 추가 놓음은 열지 않는다. 앱의 왼쪽 단추·선택 없음 검사도 유지한다.

출처는 [xterm Linkifier 5.5.0](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/Linkifier.ts)(MIT)다.
외부 코드·의존성을 추가하지 않는다. 기존 실제 소스 패치 유닛은 upstream의 클릭 유실과
수정판의 동일 대상 열기·변경 거부를 대조한다. E2E16은 설정 복귀 뒤 링크를 누른 채 실제
PTY 출력으로 같은 행을 굵게 다시 그리고, 놓은 뒤 선택한 소스 행·강조·줄 바꿈을 검사한다.

### 누른 사이 resize(t-1167)

resize는 현재 링크뿐 아니라 `onRenderedViewportChange` 감시도 해제한다. 따라서 이후 같은
행을 PTY가 다시 그려도 현재 링크가 없는 채로 남아 기존 동일 대상 비교까지 도달하지 않는다.
누름이 있고 resize가 있었으면 `mouseup`의 실제 칸에서 provider를 다시 조회한다. 그 사용자
사건 안에 돌아온 동기 응답에만 기존 provider·대상·전체 범위 비교를 적용한다. 늦은 응답은
클릭을 재생하지 않으며 누름은 먼저 소비하므로 추가 놓음도 열지 않는다. 앱의 왼쪽 단추·선택
검사와 파일 provider의 현재 버퍼 재검증은 그대로다.

`xterm-link-resize.test.mjs`는 실제 upstream 클래스의 생성자·resize·render listener 해제·provider
조회·놓음을 실행한다. E2E16의 기존 wrap 재열기는 누른 사이 실제 창 높이를 한 줄 늘려 크기
응답과 그리드를 기다린 뒤 실제 PTY 행을 다시 그린다. 열 수·링크 좌표는 유지하며 설정 복귀·
중앙 배치·강조·줄 바꿈 검사를 그대로 수행한다. 고정 대기나 다시 클릭하는 경로는 없다.
