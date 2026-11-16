# 이미지 작업 계정 이름 변경의 초기 응답 경계(t-1919)

## 원 CI 증거와 한계

실패 SHA는 `dfc4acaf74e79eab0e0aa33171e9be7860ba93b8`이다.
[원 실행](https://github.com/cLazyZombie/aaa/actions/runs/43946995158/job/133579177313)의
macOS 15 WebKit E2E 134가 `locator.fill: Timeout 30000ms exceeded.`로 실패했다.
Call log의 대상은 `codex-webkit-1280 새 이름` 입력칸이다. Chromium 단계는 통과했고 휴대폰 단계는 실행되지 않았다.

원 job 로그와 `web-e2e-result-43946995158-1` artifact를 직접 받았다.
artifact에는 실행 시간 기록과 다른 spec의 진단만 있고, 134의 화면·DOM·포인터·요청 기록은 없다.
Call log에도 호출 줄이 없어 세 fill 중 어느 호출인지와 원 실행의 정확한 사건 순서까지 확정할 수는 없다.
실행 부하는 높았으나, 그것만으로 환경 결함이라고 판단하지 않는다. CI 재실행·push는 하지 않았다.

## 로컬 원인 대조

앞선 E2E 133 수리는 가짜 Claude 계정의 초기 사용량 응답이 행을 늘려 native press가 빗나가는 경합이었다.
134는 가짜 Codex 계정을 쓰며 같은 준비 경계가 없었다. 먼저 관측만 더한 원본을 로컬 Mac에서 실행했다.
세 기기 조합은 통과했지만, 실제 host의 사용량 응답이 `조회 지원 안 됨` 한 줄을 추가해 각 행을 21px 늘렸다.

다음 대조에서는 WebKit PC의 첫 사용량 응답을 `route.fetch()`로 받은 뒤 보류했다.
이름 변경 버튼에 실제 `mouse.down()`을 보낸 뒤 응답을 전달하고, 그 내용이 DOM에 반영되면
같은 좌표에서 `mouse.up()`을 보냈다. 고정 sleep이나 임의의 press 시간은 쓰지 않았다.
기존 fill과 30초 시한을 그대로 실행해 원 CI와 같은 대상·오류를 재현했다.

| 사건 | 관측 시작 뒤 시각 | 결과 |
| --- | ---: | --- |
| 사용량 응답 보류 | 126ms | 행 높이 302px, 자동 배정 조회는 완료 |
| pointerdown | 185ms | BUTTON, 좌표 (513, 550) |
| 사용량 DOM 반영 | 187ms | 행 높이 323px, 버튼 위쪽 534→555px |
| pointerup | 194ms | LI, 같은 좌표 (513, 550) |
| click | 195ms | LI, 편집 form 없음 |
| 기존 fill 실패 | 30196ms | `codex-webkit-1280 새 이름`을 찾지 못함 |

실패 화면에도 모달·편집 form이 없고 원래 버튼이 남았다. 사용량 요청은 정상 응답했고,
이름 변경·이미지 작업 조회 요청은 발생하지 않았다. 입력칸 포커스 이전에 누름이 빗나간 것이다.
이는 시험이 늦은 초기 응답을 기다리지 않은 경합으로 분류한다. backend·이름 변경 UI 수리는 필요하지 않다.
원 CI 한 건도 반드시 같은 순서였다고 단정하지는 않는다.

## 수리와 회귀 확인

E2E 134가 첫 누름 전에 `domIs()`로 모든 계정 행의 `조회 지원 안 됨`과
자동 배정 checkbox의 enabled 상태를 기다리도록 했다. 가짜 Codex fixture의 실제 응답과 DOM 반영을 기준으로 한다.
제품 코드·시한·단언·기기 조합은 유지하며 retry·sleep·강제 click·skip을 추가하지 않았다.

수리 대조에서는 같은 보류 응답을 새 준비 대기의 시작에 전달했다.
DOM 반영 뒤 누름은 버튼 위쪽 555px에서 변하지 않았고, pointerdown·pointerup·click 모두 BUTTON에 도착했다.
WebKit PC 및 기존 Chromium PC·WebKit 휴대폰 시나리오가 모두 통과했다.
관측·응답 gate·native press 분리 코드는 작업 전용 `target/`에만 두며 제품 시험에는 넣지 않는다.

기존 계약인 결과 불명 정리 확인·취소, 생성 중 정리 불가, 늦은 조회의 행 부활 방지,
예약 원장 보존, 생성 중 이름 변경 거부, 기존 thread·pane 유지, 새 thread·기본 계정·풀·인증 판 이전을 그대로 검사한다.
`t-1916`은 coordinator 안내와 그 문서만 바꾸며 이 가짜 계정·포인터 경로를 바꾸지 않았다.
귀책을 뒷받침하는 근거는 없으며 되돌리지 않는다.

## 개발 관련 검증

`aaa verify -- env NODE_PATH=<작업 전용 모듈 경로> nice -n 15 python3 scripts/verify.py develop --base b10c039c --spec 133,134`로
웹 타입·번들·유닛 736개, E2E 133·134의 PC Chromium·PC WebKit·휴대폰 WebKit 조합과 공개 검사가 통과했다.
선택한 두 spec만 확인했으며 전체 gate 통과로 기록하지 않는다. Git 변경은 해당 spec과 Markdown 문서뿐이다.
착지 통합·최종 SHA의 gate·push는 하지 않고 후보만 준비한다.

## 참고한 공개 구현

- [Playwright actionability](https://playwright.dev/docs/actionability)·[Route](https://playwright.dev/docs/api/class-route):
  누르기 전 안정성 검사와 실제 응답 보류를 대조했다.
  [Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE).
- [React flushSync](https://react.dev/reference/react-dom/flushSync): DOM 강제 동기 반영을 대조했지만,
  실제 click 대상이 LI였으므로 React 이벤트 처리기를 바꾸지 않았다.
  [MIT](https://github.com/facebook/react/blob/main/LICENSE).

외부 코드·새 제품 의존성은 추가하지 않았다. 검증은 작업 전용 Playwright 1.64.1과 기존 공용 브라우저로 실행한다.
검증 명령·결과는 t-1919의 library 기록에 둔다. 개발 관련 owner는 133·134이며 착지 gate는 승인된 착지 창에서 실행한다.
