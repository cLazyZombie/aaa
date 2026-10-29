# E2E114 변환 응답 실패 조사와 진단(t-1304)

## 확정한 사실과 한계

전체 착지 검증에서 E2E114의 `대량 자동화 변환: webkit 1280`이
`20000ms 안에 host 일괄 응답이 되지 않았다(페이지가 답하지 않음)`으로 실패했다.
실패 작업 종료 때 기록된 부하는 113.49/64.23/34.52였다.
기존 `responseGate.arm`은 클릭 전에 Node 시한을 시작하며, `route.fetch()` 완료를 `arrived`로 알린다.
기존 로그에는 arm·클릭·요청 도착·fetch 시작의 개별 시각이 없어 어느 구간이 시한을 소비했는지 확정할 수 없다.

원본 host 로그와 보존 DB·파일에서 확인한 것은 다음과 같다.

| 자료 | 관측 |
| --- | --- |
| preview 종료 | 07:05:08.066 UTC, 1932ms |
| 파일 변환 종료 | 07:05:27.781 UTC, `resume` 6126ms |
| 변환 파일·보존 사본 | 각각 500개, 옛 원본 0개 |
| 남은 DB journal | `automation_migration:demo` 287,414바이트, `mission_conversion:demo` 2바이트 |
| recover·convert 종료/HTTP 응답 | 기존 로그에 없음 |

파일 변환 완료는 DB 정리·응답 완료를 뜻하지 않는다. 중단된 상태만으로 제품 결함이나 시험 결함을 판정하지 않는다.
별도의 장기 실행 WebContent 프로세스도 읽기만 기록했으며 이번 실패의 원인으로 단정하지 않았다.

## 기준 main 재현 대조

수정 없는 main `2e43cfc6`을 이 작업 폴더에서 빌드했다. 전체 E2E114를 한 번 단독 실행한 결과
76.3초에 통과했고, 해당 WebKit1280의 host 응답은 4458ms였다.
이 대조는 최초 실패의 재현·원인 규명·수리 완료가 아니다. 통과 결과로 최초 실패를 덮지 않는다.
원래 실패와 같은 113 부하를 만들려고 다른 작업이나 관계없는 프로세스를 조작하지 않았다.

재현하지 못했으므로 사용자 지시대로 진단만 추가한다. 제품 코드와 시험의 요청·20초 시한·500개 파일 검증·응답 보류 계약은 유지한다. 재시도·시한 연장·기대값 완화는 없다.

## 추가한 증거

- `responseGate`는 선택적인 관측 함수로 arm·요청 매칭·fetch 시작/끝·보류·release·fulfill·오류를 기록한다.
  기존 호출과 20초 상한은 유지한다.
- E114 변환은 클릭 명령의 시작/끝, 실제 DOM 클릭, API 요청·응답·실패, page crash/close와 browser disconnect를 구분한다.
  요청 본문 전체·query·인증값·계획 내용은 기록하지 않는다.
- 최초 오류와 전체 stack을 renderer 조회보다 먼저 Node에서 원자적으로 저장한다.
  같은 시점의 부하·변환 단계 비용·파일 개수·DB journal 키와 크기를 읽기만 덧붙인다.
- 본문·화면 캡처나 정리 자체가 실패해도 최초 오류는 보존한다. 진단 산출물은
  `diagnostics/bulk-conversion/<engine>-<width>.json`과 실패 화면이며 기존 E2E artifact 수집기에 들어간다.
- 사건은 UTC와 상대 시각을 함께 남겨 arm→클릭→요청→host 응답→전달의 지연과 host 파일/DB 경계를 대조할 수 있다.

## 검증

- 새 진단 유닛 2개는 10.3/5.0ms에 통과했다. 첫 시험은 renderer·화면·정리 실패에도 최초 오류와
  파일/journal 상태가 보존되는지를 본다. 두 번째는 실제 요청에 포함된 비밀 필드를 기록하지 않고
  클릭·요청·응답·실패·종료 사건을 구분하는지를 본다. 진단이 원래 실패를 가리거나 요청 정보를 노출하는 회귀를 막기 위해 필요하다.
- 기준 main에 진단만 더한 E2E114 전체가 75.6초에 통과했다. Chromium1280/390·WebKit1280/390의
  실제 gate 응답은 3592/3853/4017/5083ms였다. 네 조합에서 실제 DOM 클릭과 arm→fetch→보류→release→fulfill→정리
  사건이 기록되고 최초 오류가 없는 정상 종료임을 확인했다. 이 결과도 최초 실패의 원인 규명을 뜻하지 않는다.
- 최종 작업 브랜치의 웹 유닛 전체 660개가 통과했다(3.2초). `git diff --check`와 공개 검사도 통과했다.

## 참고

- [Playwright 사건 구독](https://playwright.dev/docs/events)과
  [route.fetch](https://playwright.dev/docs/api/class-route#route-fetch)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)): 동작 전에 사건을 구독하고 fetch와 fulfill을 구분한다.
- [Puppeteer 요청 가로채기](https://pptr.dev/guides/network-interception)
  ([Apache-2.0](https://github.com/puppeteer/puppeteer/blob/main/LICENSE)): 비동기 처리 중의 요청 상태와 전달 단계를 따로 관찰하는 원칙을 비교했다.
- 외부 코드·새 의존성은 가져오지 않았다.

## 인증 fixture 수리

후속 착지의 fixture 검사에서 새 진단 시험의 직접 token 문자열이 공통 생성 규칙에 걸렸다.
기존 `scripts/fixture-secret.mjs`의 token 생성기로 만들고, 그 생성값이 진단에 남지 않는 원래 단언을 유지했다.
`python3 scripts/check_fixture_secrets.py`와 진단·공통 fixture 웹 유닛 3개가 통과했다. 기대값·예외 목록·검사 규칙은 바꾸지 않았다.
