# herdr-web-ui에서 그대로 옮긴 파일

이 폴더의 `.ts` 파일은 [herdr-web-ui](https://github.com/devswha/herdr-web-ui) [고정 commit `aa624cb`](https://github.com/devswha/herdr-web-ui/tree/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741)의 `src/lib/` 파일을 바꾸지 않고 옮긴 것이다(aaa는 참고 프로젝트 동작을 그대로 따른다).

| 파일 | 하는 일 |
|---|---|
| `terminalGlyphs.ts` | xterm DOM renderer가 칸보다 좁게 그린 글자(한글 등)를 키우고 칸 가운데 둔다. 조합 상자(`.composition-view`)의 글꼴 크기·자간·여백을 그 줄에 맞춘다(`fitView`). NFC, 아이콘 크기 |
| `terminalBoxGlyphs.ts` | 선·블록 문자를 글꼴 대신 칸 크기 상자로 그린다 |
| `viewport.ts` | 화면 키보드가 올라오면 보이는 높이(visualViewport)를 `--app-height`로 알리고 `data-keyboard`를 단다. `.app`이 그 높이가 되어 머리·터미널·입력 막대·키 막대가 키보드 위에 남는다. 페이지는 (0, 0)에 둔다 |
| `viewportResume.ts` | 다시 보일 때(visibilitychange·pageshow) 위를 다시 잰다 |
| `keyboard.ts` | `keyboardUp`, `keyboardHoldsGrid`(키보드는 화면 아래를 가릴 뿐이므로 높이만 줄면 PTY 크기를 바꾸지 않는다), `keyboardViewportReference` |
| `keys.ts` | 키 처리: `clipboardKey`(Ctrl+C·V를 자판 위치로도 읽는다), `modifyOtherKeysLevel`(`CSI > 4 ; n m` 추적), `ctrlEnterSequence`(modifyOtherKeys면 `CSI 27;5;13~`), 키 막대 유지 키(`StickyModifiers`, chord는 `terminalChord`·`keyFromData`·`physicalKey`) |
| `osc52.ts` | `parseOsc52`: pane 프로그램의 OSC 52 클립보드 쓰기(질의는 거절, 100 KiB 상한) |
| `keyBar.ts` | 키 막대 항목(기본 Esc·Tab·^C·Ctrl·Alt·Shift·Enter·↑↓←→), 저장값 거르기(`sanitizeKeyBarItems`), 키 이름 → bytes(`keyBarInputSequence`, DECCKM) |
| `KeyBarSettings.tsx`·`KeyBarSettings.css` | 설정 「키 바 편집」: 키 목록 순서·지우기, 키 추가, 고정 보조 키, 사용자 조합, 기본값으로 되돌리기(`src/components/KeyBarSettings.*`). CSS는 그대로, tsx는 lucide·i18n·useSettings 대신 aaa Icon·한국어 문구·`settings.ts`, 단추 class만 aaa 것 |
| `fontFamily.ts` | 설정 「터미널 글꼴」: 쉼표로 적은 글꼴 목록을 CSS 값으로 거르고(`sanitizeFontFamily`), 그 face를 받을 때까지 기다린다(`loadFontStack`). aaa는 `terminalFontStack` 대신 Meslotendard 목록 앞에 붙인다(plan §8.64) |
| `installedFonts.ts` | 「찾아보기」: 이 기기에 깔린 글꼴 목록(Chromium `queryLocalFonts`), 이름 거르기, 고정폭 판정(`isMonospaceFamily`) |
| `presence.ts` | 이 화면을 지금 쓰는지(보이고 focus가 있다): events WS로 host에 알린다. 쓰는 화면이 있으면 host는 Web Push를 보내지 않는다(`watchPageInUse`, plan §8.69) |
| `alertSound.ts`·`alertTurns.ts` | 알림 소리(질문은 오르는·완료는 내리는 두 음, 울리는 동안 온 완료는 겹치지 않음)와 탭 여럿 가운데 한 탭만 울리기(BroadcastChannel). plan §8.70 |
| `Droplet.css` | 앱 안 알림 모양(검은 카드가 내려와 펼쳐지고 접힘). plan §8.70 |
| `wakeLock.ts`·`visibility.ts` | 설정 「화면 켜짐 유지」: 보이는 동안만 screen wake lock을 걸고, 늦게 온 lock도 놓는다(`useScreenWakeLock`, `usePageVisible`) |
| `keyRepeat.ts` | 꾹 누르면 반복(`createKeyRepeat`: 400ms 뒤 50ms마다, 10px 넘게 밀면 취소) |
| `draft.ts` | 끊긴 동안 친 글(붙잡은 입력): 글·붙여넣기를 모으고 Enter·줄바꿈은 빼서 알리며, 1024자 상한, 저장 기록은 24시간(`restoreDraft`) |
| `terminalDraft.ts` | 입력줄 초안: 머신·pane마다 localStorage(32,768자 상한, 막히면 메모리). 보내는 동안 친 글은 두고 보낸 앞부분만 지운다 |
| `terminalWidths.ts` | 이모지·ZWJ 묶음·VS16·새 이모지를 herdr처럼 두 칸으로 센다(`matchHerdrWidths`, `@xterm/addon-unicode-graphemes` 위에 보정 셋, plan §8.53) |
| `wheelSteps.ts` | 한 손가락 끌기를 휠 걸음으로(`wheelSteps`·`finalStep`, plan §8.55) |
| `status.ts` | agent 상태 말·herdr TUI 글자(`indicatorGlyph`)·여러 pane 대표 상태(`rollupStatus`). `src/lib/status.ts`에서 `AgentStatus` import 한 줄만 그 타입 선언으로 바꿨다(plan §8.71) |
| `preferences.ts` | 화면들이 함께 쓰는 설정 이름(`SHARED_SETTING_KEYS`)·문서 형·한도. `src/lib/`가 아니라 `shared/preferences.ts`를 바꾸지 않고 옮겼다. host 쪽은 `crates/aaa-host/src/prefs.rs`가 Rust로 옮겼다(plan §8.79) |
| `sidebarPins.ts` | 목록 고정 열쇠(`machine:pane`)·순서·나누기 대상(이 기기 사본은 localStorage). 바꾸지 않았다(plan §8.79) |
| `tokens.css` | 디자인 token: 색(기본 amber, 밝음, charcoal·catppuccin 팔레트)·글자·크기·움직임·밀도. `src/styles.css` 18~494줄 그대로(plan §8.71) |
| `primitives.css` | 공용 조각 `visually-hidden`·`icon-button`·`btn`·`badge`. `src/styles.css` 549~561·981~1116·1149~1205줄 그대로 |
| `Sidebar.css`·`AgentSidebar.css`·`Machines.css` | 왼쪽 목록(머신 묶음·줄·상태 칸·Agents 구역·바닥 줄). `src/components/` 것 그대로. aaa 마크업은 `web/src/Sidebar.tsx` |
| `phone.ts` | `src/lib/phone.ts`의 세 함수 그대로: `isLoopbackHost`, `deviceLabel`(pairing 기기 이름 「iPhone · Safari」), `takePairCode`(QR 주소의 `?pair=`를 읽고 바로 지움). plan §8.81 |
| `clipboard.ts` | `src/lib/clipboard.ts` 그대로: `copyText`(clipboard API가 없으면 copy 명령, 그래도 안 되면 고르기) |
| `QrCode.tsx`·`QrCode.css` | `src/components/QrCode.*` 그대로: QR을 inline SVG로(`qrcode-generator` 2.0.4, MIT). 설정 → 폰·기기 |
| `AccessGate.css`·`DevicesPanel.css`·`PhonePanel.css`·`ConfirmDialog.css` | `src/components/` 것 그대로. 마크업은 `web/src/AccessGate.tsx`·`DevicesPanel.tsx`·`PhonePanel.tsx`·`ConfirmDialog.tsx` |
| `modal.css` | `src/styles.css` 1269~1451줄 그대로: `.modal-scrim`·`.modal`·머리·몸·바닥, 휴대폰 sheet 움직임, 키보드가 있을 때, `.input`·`.select` |
| `useFocusTrap.ts`·`sheets.ts`·`sheetHistory.ts`·`useMediaQuery.ts` | 창 안에 Tab 가두기·여는 곳으로 focus 돌려주기, 휴대폰 sheet 아래로 밀어 닫기(`watchSheetExits`는 main.tsx), 시스템 「뒤로」로 닫기. `sheetHistory.ts`는 history 열쇠 이름만 `aaa:sheet`로 바꿨다 |
| `terminalFileLinks.ts` | `isWebLink`: 웹 링크는 http(s)만 연다. `fileUriPath`는 보고 Markdown의 `file://` 링크를 파일 이름으로 읽을 때 쓴다(`markdown.ts`) |
| `markdown.ts`·`markdownHtml.ts` | 보고서 Markdown 파서(`parseMarkdown`: 제목·문단·목록·할 일·인용·code block·표·수식 원문, 굵게·기울임·지움·code·링크·파일 링크, `foldCode`). aaa는 HTML을 해석하지 않게(`html=false`) 부른다. 차이 목록 R1, plan §8.88 |
| `Markdown.css` | 그 모양: `src/components/ChatView.css`의 `.markdown` 규칙과 `src/components/HighlightedCode.css` `.hl-code`를 바꾸지 않고 골라 옮겼다(그림·수식·mermaid·skill·파일 단추·chat 말풍선 규칙은 뺐다). 맨 앞 `--chat-fs-body` 한 줄만 aaa가 둔다(참고는 `.chat-view`가 정한다) |

agent 표시 `AgentMark`(plan §8.49·§8.56): `AgentMark.tsx`·`AgentMark.css`·`agentMark.ts`는 `src/components/AgentMark.tsx`·`src/components/AgentMark.css`·`src/lib/agentMark.ts`를 그대로 옮겼다(`AgentMark.tsx`는 import 경로만). `agentSvgMarks.ts`는 `src/components/agentSvgMarks.ts`에서 droid(Factory favicon)·maki(maki.sh favicon) 두 줄만 뺐다. 참고가 그 출처의 사용 허락을 적지 않아서다. 그 agent는 머리글자 표시(FallbackMark)로 보인다. 참고는 Claude·Codex·Cursor·OpenCode·omp·gjc·omo 표시를 같은 저자의 chatmux llm-logo-provider 묶음에서 옮겼다고 적는다. `agentSvgMarks.ts`의 나머지는 LobeHub Icons(`@lobehub/icons-static-svg`, 아래 MIT)다. **Claude 로고는 Anthropic, Codex(OpenAI) 로고는 OpenAI의 상표이며, 그 밖의 표시도 각 소유자의 것이다.** aaa는 pane에서 도는 agent를 가리키는 데만 쓰며 그들과의 관계를 뜻하지 않는다.

그대로 복사하지 못하고 옮긴 파일(import·문구만 바꿈, 고지는 같다): `web/src/herdr/droplet.ts`(← `src/lib/droplet.ts`, 알림 대상을 주소 hash 하나로, pane 상태 판정 함수는 N3에서), `web/src/herdr/Droplet.tsx`(← `src/components/Droplet.tsx`, 아이콘·문구·머리 재기만), `web/src/push.ts`(← `src/lib/push.ts`, aaa API), `web/src/PushTestControls.tsx`(← `src/components/PushTestControls.tsx`, 문구), `web/src/alerts.ts`(← `src/App.tsx` 알림 부분·`src/lib/notifications.ts`), `web/src/herdr/install.ts`(← `src/lib/install.ts`, `t("…")`를 참고 `i18n.ko.ts`의 한국어로, 앱 설치 단추·설정 「설치」 줄, plan §8.68), `web/src/herdr/pinchFontSize.ts`(← `src/lib/pinchFontSize.ts`, settings import 경로만), `web/src/usage.ts`(← `src/lib/usage.ts`), `web/src/UsageMeters.tsx`(← `src/components/UsageMeters.tsx`), `web/src/TerminalInput.tsx`, `web/src/KeyBar.tsx`, `web/src/SettingsDialog.tsx`(← `SettingsDialog.tsx`·`SettingsControls.tsx` 일부). `web/src/preferences.ts`·`web/src/preferencesSync.ts`(← `src/lib/preferencesSync.ts`, 순수 규칙과 hook을 나눔, 설정 store 자리만 aaa 것, plan §8.79), `web/test/preferences.test.mjs`(← `src/lib/preferencesSync.test.ts`). `web/src/Markdown.tsx`(← `src/components/Markdown.tsx`, 렌더만: 문법 강조·KaTeX·mermaid·그림·앱 안 파일 열기는 aaa에 아직 없어 참고의 대체 모양(code 글자·원문·대체 글·이름 뒤 경로)으로, 문구는 참고 `i18n.ko.ts`, plan §8.88). host `crates/aaa-host/src/usage.rs`는 `server/usage.ts`를 Rust로 옮긴 것이다(plan §8.45).

아이콘은 참고처럼 `lucide-react`(1.47.0, ISC, 참고 bun.lock과 같은 판)를 쓴다.

aaa에서 쓰는 곳: `web/src/Term.tsx`(`adjustTerminalGlyphs`, `matchHerdrWidths`, `wheelSteps`, `keyboardHoldsGrid`·`keyboardUp`, 키 처리·OSC 52·웹 링크·유지 키), `web/src/KeyBar.tsx`(참고 `KeyBar.tsx`를 옮김), `web/src/TerminalInput.tsx`(참고 `TerminalInput.tsx`를 옮김, `terminalDraft.ts`), `web/src/main.tsx`(`viewport.ts`), `web/src/api.ts`(`presence.ts`: events WS `presence`). 조합 상자의 색은 herdr-web-ui가 정하지 않아(xterm.css 검은 배경 그대로) aaa가 `web/src/compositionView.ts`에서 그 칸 색과 실제 커서 줄 자리에 맞춘다(사용자 지시·보고, plan §8.37).

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

LobeHub Icons(`agentSvgMarks.ts`):

```
MIT License

Copyright (c) 2023 LobeHub

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
