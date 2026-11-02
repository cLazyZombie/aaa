# macOS 웹 E2E 이모지 폭 측정 경합 수리(t-1427)

## 원인과 변경

GitHub job `125030364839`의 E02는 이모지 뒤 표지 X·Y·Z·W가 두 칸이어야 하는데
`[1,1,1,1]`로 측정됐다. 기준 `8989292e`와 직전 통과 `0ee245b6` 사이 터미널 웹 코드는 같다.

기존 측정은 host의 칸 수를 CLI로 읽은 뒤 별도 브라우저 호출에서 화면 너비를 나눴다.
host가 63칸으로 바뀌어도 브라우저는 120칸 snapshot을 아직 그릴 수 있다.
그 순간 두 칸은 `round(2 × 63 / 120) = 1`로 잘못 측정된다.
CI의 연결 기록에도 `snapshot 120x32`, 이어서 `resize 63x25`와 state 응답이 있다.
최초 실패 뒤의 진단은 이미 응답을 적용한 화면일 수 있어 그 기록만으로 동시성을 판단할 수 없다.

폭 fixture에 눈금 `0123456789`를 추가하고, 활성 터미널의 눈금과 표지를 같은 DOM 호출에서 잰다.
제품 폭 공급자와 네 이모지의 `[2,2,2,2]` 계약은 유지한다.

## 꼭 필요한 회귀와 음성 대조

기존 E02 폭 단계에 실제 WebSocket state 응답 보류를 넣었다. host의 새 크기와 화면의 옛 크기가
다른지 확인하고 두 칸을 검사한 뒤, 보류 응답을 풀고 새 크기의 렌더 사건 뒤 다시 검사한다.
부하나 타이머에 의존하지 않고 원래 측정 오류를 잡기 위해 필요한 회귀다.
고정 sleep·재시도·시한 확대·기대값 완화는 추가하지 않았다.

- 기존 측정식만 복원: `[1,1,1,1]`로 실패해 CI와 같은 서명을 결정적으로 재현했다.
- 제품 `matchHerdrWidths` 호출만 제거: 수정한 측정도 `[1,1,1,1]`로 실패했다. 실제 두 칸 계약 위반을 검출한다.
- 두 음성 대조의 변형은 원복했다. 원시 진단은 커밋하지 않는다.

## 관련 owner 확인

검증 스크립트는 모두 `aaa verify -- <명령>`으로 실행했다.
첫 준비 실행은 웹 타입·번들·유닛 678건과 공개 검사가 통과했으나,
전역 Playwright 1.59.1을 발견해 E2E 실행 전 판 검사에서 중단됐다.
작업 전용 Playwright 1.64.1 패키지를 NODE_PATH로 지정했다. 전역 설치와 브라우저 캐시는 바꾸지 않았다.

최종 관련 검사 결과:

- 웹 타입 검사·번들 빌드와 유닛 678건 통과.
- E02 전체 9단계 통과. 크기 응답 전후 두 칸을 검사하는 폭 단계는 473ms.
- 공개 검사와 `git diff --check` 통과.

실행: `aaa verify -- nice -n 15 bash scripts/ci-local.sh --only web,e2e,public --spec 02`.
Playwright 1.64.1의 작업 전용 패키지 경로를 NODE_PATH로 지정했다.

착지는 별도 승인 대상이다. 개발의 관련 검사이며 최종 통합 SHA의 전체 gate는 실행하지 않았다.
shared harness fixture를 바꿨으므로 착지 범위를 웹 전용으로 축소하지 않는다.

## 참고

- [herdr-web-ui terminalWidths.ts](https://github.com/devswha/ovis/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/lib/terminalWidths.ts)
  (MIT)의 두 칸·묶음 계약을 유지했다.
- [xterm.js 5.5 DomRenderer.ts](https://github.com/xtermjs/xterm.js/blob/5.5.0/src/browser/renderer/dom/DomRenderer.ts)
  (MIT)의 현재 buffer 칸 수와 CSS 화면 너비 계산을 대조했다.
- 저장소 E02의 기존 재접속 표지 검사처럼 같은 화면의 눈금으로 측정한다. 외부 코드·의존성은 추가하지 않았다.
