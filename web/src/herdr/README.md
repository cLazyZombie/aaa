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
| `keyRepeat.ts` | 꾹 누르면 반복(`createKeyRepeat`: 400ms 뒤 50ms마다, 10px 넘게 밀면 취소) |
| `draft.ts` | 끊긴 동안 친 글(붙잡은 입력): 글·붙여넣기를 모으고 Enter·줄바꿈은 빼서 알리며, 1024자 상한, 저장 기록은 24시간(`restoreDraft`) |
| `terminalDraft.ts` | 입력줄 초안: 머신·pane마다 localStorage(32,768자 상한, 막히면 메모리). 보내는 동안 친 글은 두고 보낸 앞부분만 지운다 |
| `terminalWidths.ts` | 이모지·ZWJ 묶음·VS16·새 이모지를 herdr처럼 두 칸으로 센다(`matchHerdrWidths`, `@xterm/addon-unicode-graphemes` 위에 보정 셋, plan §8.53) |
| `wheelSteps.ts` | 한 손가락 끌기를 휠 걸음으로(`wheelSteps`·`finalStep`, plan §8.55) |
| `terminalFileLinks.ts` | `isWebLink`: 웹 링크는 http(s)만 연다. 파일 링크 함수는 아직 aaa가 쓰지 않는다 |

agent 표시 `AgentMark`(plan §8.49·§8.56): `AgentMark.tsx`·`AgentMark.css`·`agentMark.ts`는 `src/components/AgentMark.tsx`·`src/components/AgentMark.css`·`src/lib/agentMark.ts`를 그대로 옮겼다(`AgentMark.tsx`는 import 경로만). `agentSvgMarks.ts`는 `src/components/agentSvgMarks.ts`에서 droid(Factory favicon)·maki(maki.sh favicon) 두 줄만 뺐다. 참고가 그 출처의 사용 허락을 적지 않아서다. 그 agent는 머리글자 표시(FallbackMark)로 보인다. 참고는 Claude·Codex·Cursor·OpenCode·omp·gjc·omo 표시를 같은 저자의 chatmux llm-logo-provider 묶음에서 옮겼다고 적는다. `agentSvgMarks.ts`의 나머지는 LobeHub Icons(`@lobehub/icons-static-svg`, 아래 MIT)다. **Claude 로고는 Anthropic, Codex(OpenAI) 로고는 OpenAI의 상표이며, 그 밖의 표시도 각 소유자의 것이다.** aaa는 pane에서 도는 agent를 가리키는 데만 쓰며 그들과의 관계를 뜻하지 않는다.

그대로 복사하지 못하고 옮긴 파일(import·문구만 바꿈, 고지는 같다): `web/src/herdr/pinchFontSize.ts`(← `src/lib/pinchFontSize.ts`, settings import 경로만), `web/src/usage.ts`(← `src/lib/usage.ts`), `web/src/UsageMeters.tsx`(← `src/components/UsageMeters.tsx`), `web/src/TerminalInput.tsx`, `web/src/KeyBar.tsx`, `web/src/SettingsDialog.tsx`(← `SettingsDialog.tsx`·`SettingsControls.tsx` 일부). host `crates/aaa-host/src/usage.rs`는 `server/usage.ts`를 Rust로 옮긴 것이다(plan §8.45).

aaa에서 쓰는 곳: `web/src/Term.tsx`(`adjustTerminalGlyphs`, `matchHerdrWidths`, `wheelSteps`, `keyboardHoldsGrid`·`keyboardUp`, 키 처리·OSC 52·웹 링크·유지 키), `web/src/KeyBar.tsx`(참고 `KeyBar.tsx`를 옮김), `web/src/TerminalInput.tsx`(참고 `TerminalInput.tsx`를 옮김, `terminalDraft.ts`), `web/src/main.tsx`(`viewport.ts`). 조합 상자의 색은 herdr-web-ui가 정하지 않아(xterm.css 검은 배경 그대로) aaa가 `web/src/compositionView.ts`에서 그 칸 색과 실제 커서 줄 자리에 맞춘다(사용자 지시·보고, plan §8.37).

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
