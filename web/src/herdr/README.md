# herdr-web-ui에서 그대로 옮긴 파일

이 폴더의 `.ts` 파일은 [herdr-web-ui](https://github.com/devswha/herdr-web-ui) [고정 commit `aa624cb`](https://github.com/devswha/herdr-web-ui/tree/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741)의 `src/lib/` 파일을 바꾸지 않고 옮긴 것이다(aaa는 참고 프로젝트 동작을 그대로 따른다).

| 파일 | 하는 일 |
|---|---|
| `terminalGlyphs.ts` | xterm DOM renderer가 칸보다 좁게 그린 글자(한글 등)를 키우고 칸 가운데 둔다. 조합 상자(`.composition-view`)의 글꼴 크기·자간·여백을 그 줄에 맞춘다(`fitView`). NFC, 아이콘 크기 |
| `terminalBoxGlyphs.ts` | 선·블록 문자를 글꼴 대신 칸 크기 상자로 그린다 |

aaa에서 쓰는 곳: `web/src/Term.tsx`(`adjustTerminalGlyphs`). 조합 상자의 색은 herdr-web-ui가 정하지 않아(xterm.css 검은 배경 그대로) aaa가 `web/src/compositionView.ts`에서 그 칸 색과 실제 커서 줄 자리에 맞춘다(사용자 지시·보고, plan §8.37).

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
