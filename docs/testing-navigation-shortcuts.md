# 웹 화면 이동 단축키와 PWA 단어 삭제 검증

t-1259, 2026-10-28. macOS arm64에서 실행했다.

## 동작과 참고

- Option+O는 coordinator, Option+Q는 답할 것, Option+T는 모든 thread, Option+D는 할 일이다. 기존 단축키 설정에서 기록·끄기·초기화한다. 입력 중에도 이동하며 IME 조합과 229 사건은 제외한다.
- 기존 herdr-web-ui(MIT)에서 가져온 `exactChordFromEvent`·기록·충돌 판정·터미널 차단 구조를 확장했다. Option 특수 문자는 물리 `code`로 판정한다. 이미 저장한 조합이 새 기본값보다 우선한다.
- 단어 삭제 후보는 [GNU Readline의 unix-word-rubout](https://tiswww.case.edu/php/chet/readline/rluserman.html#Killing-Commands)과 [fish의 Ctrl+W](https://fishshell.com/docs/current/interactive.html#shared-bindings)를 비교했다. 공백 경계를 쓰는 Readline 동작을 택했다. 한글과 영문에 같은 기준을 쓰며 경로·문장부호는 한 덩어리로 지운다. [Readline은 GPLv3](https://tiswww.case.edu/php/chet/readline/rltop.html), [fish는 주로 GPLv2](https://github.com/fish-shell/fish-shell/blob/master/COPYING)이다. 둘의 소스 코드는 복사하지 않았다.
- 브라우저 예약 키는 [Chrome 공식 단축키](https://support.google.com/chrome/answer/157179)를 참고했다. Ctrl+W는 일반 탭의 기본 동작을 가로채지 않는다. OS·브라우저가 페이지 전에 소비하는 키는 앱에서 보장할 수 없다.

## 자동 검증

| 검증 | 결과 |
|---|---|
| `npm run typecheck --prefix web` | 통과 |
| `npm run build --prefix web` | 통과 |
| `cargo build --profile e2e -p aaa` | 웹 번들을 포함한 빌드 통과 |
| `npm test --prefix web` | 653개 통과 |
| 웹 E2E `121-navigation-shortcuts` | Chromium·WebKit × 일반 탭·standalone, 기능 12단계 통과 |
| 웹 E2E `03-keys --grep '앱 단축키'` | 기존 설정·키 차단·패널 전환 회귀 통과 |
| `git diff --check`·`scripts/check-public.sh` | 통과 |

Playwright 1.64.1을 작업 전용 도구 폴더에 두고 공용 캐시의 Chromium 1248·WebKit 2370을 사용했다. 웹 E2E는 실제 임시 host·agent·PTY를 사용하며 정한 시간을 자지 않는다. 새 기능 단계는 각각 0.058~1.223초였다. 기존 E03은 두 E2E가 함께 도는 조건에서 5.868초였다.

새 유닛 시험 `composerWordDelete.test.mjs` 두 개는 공백 경계와 선택·커서 범위를 지킨다. 한글·영문 혼합, 여러 공백, 탭·줄바꿈·NBSP, 경로·문장부호, 이모지, 빈 글·맨 앞을 검사한다. 새 E2E는 화면 이동이 초안을 보존하고 키를 PTY로 보내지 않는지, 특수 문자와 조합 제외, 반복 입력, 설정의 기본값·충돌·끄기·기록·초기화, PWA 제한, 선택 삭제·커서·되돌리기·초안 저장을 확인한다.

## 시험 준비에서 확인한 문제

- 처음에는 `demo` 프로젝트를 만들기 전에 coordinator를 열어 준비가 실패했다. fixture에서 프로젝트를 명시적으로 생성했다.
- 터미널로 전환한 pane으로 돌아오면서 대화 입력창을 먼저 기다려 실패했다. 먼저 해당 pane의 표시를 기다리고 보기 전환 뒤 입력창을 기다리도록 고쳤다.
- WebKit은 Playwright `fill`과 뒤따른 연속 삭제를 한 undo 묶음으로 처리했다. 앱 없는 기본 textarea에서도 같은 현상을 확인했다. 저장된 초안을 불러온 조건처럼 초기 값을 설정한 뒤 삭제 전 상태로 되돌아가는 원래 단언을 유지했다. 제품 구현·기대값·시한은 이 원인 때문에 바꾸지 않았다.

실제 설치 앱 창의 OS 키 전달과 실제 한글 입력기는 자동화하지 않았다. standalone 조건과 IME 사건을 주입한 headless 검증이다. 운영에서 키가 OS·브라우저에 예약되어 있으면 설정에서 바꿀 수 있다. 병합·배포는 착지 창에서 진행한다.

## 착지 검증 절차

- 기준 main은 `9121d473`(t-1257), 후보는 `d6df46e7`이다. 전용 Mac worktree에서 no-ff 통합한다. plan은 main §8.537 다음 §8.538, 새 E2E는 main E120 다음 E121이며 wire 변경이 없어 FRONT_API 1.145·AGENT_LINK 1.11을 유지한다.
- 충돌 표지 네 종류와 testing.md의 링크 한 줄·1MiB 미만을 감사한다. LLVM PATH, worktree 안 Playwright 1.64.1과 기존 공용 브라우저 캐시를 사용하고 전체 CI에는 salt를 넣지 않는다.
- nice 15 전체 ci-local과 별도 공개 검사, Linux/Windows strict cross, 별도 Windows production release, 개인정보 검사를 순서대로 수행한다. 시작·첫 실패 uptime과 원문을 보존하며 첫 검증 실패에서 후속 작업을 멈춘다.
- 모두 통과하면 main의 fast-forward push·원격 SHA를 확인하고, 실제 착지 SHA salt의 Windows 설치기를 격리 전달 폴더에 보낸 뒤 SHA를 대조한다. 설치·운영 서비스 변경·GitHub CI 대기는 하지 않는다. 실제 결과와 전달 영수증은 thread 보고에 남긴다.
