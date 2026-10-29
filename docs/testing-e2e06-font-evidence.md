# E2E06 글꼴 저장 실패 조사와 진단 보존(t-1292)

## 확인한 범위

- 기준은 수정 전 main `29c780a4`다. 미push 통합본 `003a1be4`는 사용하지 않았다.
- 원래 실패는 `phone: 터미널 글꼴 저장 실패`다. 원본에는 실제 입력값·저장값·키 대상·IME 상태가 없고,
  휴대폰 context가 먼저 닫혀 공통 진단에는 별도의 1280px 화면만 남았다.
- 시작 전 ps에 다른 작업의 엔진 시험이 있었지만 실패 순간 전체 시스템 ps는 없다.
  부하만으로 그 작업과 실패의 인과관계를 확정하지 않는다. 원문·부하와 ps 근거는 작업 보고의 library에 보존했다.

## 원인 사건 실험

| 실험 | 결과 |
| --- | --- |
| 기준 main의 입력·keydown·저장·렌더 사건 기록 | 정상 저장. 원래 실패 미재현 |
| 첫 터미널 Enter 이후 React MessageChannel 작업 보류 | 예약 작업 1개가 보류돼도 새 글꼴 저장 정상 |
| 합성 input과 Enter를 같은 DOM 작업 안에서 발생 | React 반영과 저장 정상 |
| 기준 main E2E06 전체 흐름, 최소 사건 기록 | 전체 통과. 해당 글꼴 단계 6575ms |

예약 작업 지연과 입력 직후의 React 반영을 원인으로 단정할 수 없다.
기준 main이 통과한 것은 첫 실패를 수리했다는 증거가 아니다. 시험 결함·제품 결함 분류는 미확정이다.

## 별도 변경: 실패 기기 진단 누락 보완

`font-settings-diagnostics.mjs`가 입력·focus·blur·keydown/up·조합 사건의 최근 64개를 기록한다.
사건 당시 입력값·React 반영값·localStorage 저장값·키 코드·IME 여부를 남기고, 실패한 기기의 context를 닫기 전에
viewport·현재 입력칸·저장값·사건을 `diagnostics/font-settings-<기기>.json`에 쓴다.
원래 오류를 다시 던지고 기존 입력 순서·단언·시한은 모두 유지한다. 제품 코드는 바꾸지 않았다.

- 진단 추가 뒤 원래 글꼴 단계 Chromium 1280px·WebKit 390px 통과(6869ms).
- 별도 일회성 fault injection에서 마지막 휴대폰 Enter만 IME 조합 Enter(229)로 보냈다.
  제품의 원래 IME 계약대로 저장을 보류해 같은 오류 문구가 발생했고, JSON에 390px 기기·이전 저장값·새 입력값·229 사건이 남았다.
  이는 진단 보존을 검증한 주입 실험이며 원래 실패의 원인 재현으로 세지 않는다.
- 웹 E2E 구조·고정 sleep 검사 통과. 새 sleep·재시도·시한 변경·기대값 완화는 없다.

원래 실패 실험실과 주입 실험실은 자료를 복사한 뒤 session host 신원을 확인하는 기존 정리 helper로 정리했다.
다음 착지는 최신 main 위에 새로 통합해야 하며, 이 기록은 착지 승인이나 저장 실패 수리 완료를 뜻하지 않는다.

## 확인한 구현 자료

- [Playwright v1.64.1 WebKit 입력 경로](https://github.com/microsoft/playwright/blob/v1.64.1/packages/playwright-core/src/server/webkit/wkInput.ts)
  (Apache-2.0)와 같은 tag의 WebKit patch: 키 입력은 protocol callback을 기다리므로 단순히 Enter 전송 반환만 앞선다고 가정하지 않았다.
- [React 제어 입력 계약](https://react.dev/reference/react-dom/components/input)(React 구현 MIT): 입력 상태의 동기 반영과 저장 동작을 구별했다.
- 외부 코드는 복사하지 않았다.
