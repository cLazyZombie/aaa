# 기획 트리 전송 실패 격리(t-1969)

웹 기획의 `TypeError: Load failed` 화면을 조사했다. 기존 웹은 전체 저장소 트리를 HTTP 한 번으로 받아
전송이 끊기면 첫 트리를 전혀 그리지 못했다. host의 저장소별 agent 오류 처리는 이미 따로였지만,
그 바깥 HTTP 실패는 전체 오류였다.

당시 서버 로그에는 기획 HTTP의 시작·완료 기록이 없었다. 조사 시점 CLI는 정상이며, 이 사실만으로 당시
연결 단절을 agent·ID 원장·응답 크기 탓이라고 단정할 수 없다. 운영 Safari의 당시 Network 기록도 없어
그 단절의 직접 원인은 미확정이다. 이번 수리는 확인한 실패 확산 경계와 재발 진단을 다룬다.

## 변경과 owner

- `aaa-proto`: FRONT_API 1.182, `DesignTree.repo`와 `cached`. 생략하면 기존 전체 읽기다. AGENT_LINK는 그대로다.
- `aaa-host` `design`: host의 저장소 목록·지난 트리를 바로 돌려주고 저장소 하나를 따로 읽는다. 머신 이름을
  풀지 못한 때도 지난 트리를 보존한다. 저장소별 agent 오류와 요청 시작·완료·소요 시간을 로그에 남긴다.
- 웹 `designLoad`: 목록 뒤 저장소별 HTTP를 동시에 보내 각각 도착할 때 그린다. 한 저장소의 전송 오류는
  그 저장소에만 남기고 지난 트리를 보존한다. 자동 재시도는 없다. 끝난 조회의 후속 응답은 그리지 않는다.
- E2E 135의 origin 경고는 저장소 응답이 도착한 사건을 기다린다. 목록이 먼저 표시되는 동작에 맞춘 대기다.

## 검사

| 검사 | 보존한 계약 |
| --- | --- |
| host `design::tests::` | agent 없이 목록과 지난 트리 읽기, 단일 저장소 선택, 없는 저장소 거부, 하나의 머신 오류에도 전체 목록 유지, 작은 dispatcher 스택, 기존 대상·경로 경계 |
| proto 전체와 TypeScript 생성 검사 | 생략 필드 기본값·정본 타입과 웹 계약 |
| 웹 `designLoad.test.mjs` | 느린 응답 이전 정상 결과, 전송 실패의 지난 트리·오류 보존, 요청당 1회, 끝난 조회 무시, 목록 실패를 빈 성공으로 바꾸지 않음 |
| E2E 140 | 실제 agent와 저장소: HTTP 하나를 보류하고 하나를 끊어도 정상 문서를 열 수 있음, 저장소별 오류, 수동 복구, 다음 실패 때 지난 트리 보존. Chromium·WebKit |
| 기존 E2E 135·138·139 | 실제 origin의 문서·그림·영상·Range·긴 트리·작은 화면, 저장소 필터, 기획서 ID·원장·표지 |
| 웹 타입·번들·유닛 전체 | 웹 정적 계약과 기존 규칙 |
| host·proto strict Clippy, `aaa` deps | 관련 Rust owner와 역할별 crate 의존 경계 |

웹 유닛 744개·host 4개·proto 63개·의존 경계 2개와 관련 strict Clippy가 통과했다.
기존 E2E 135·138·139 및 최종 E2E 140의 Chromium·WebKit이 통과했다.

실행은 `aaa verify -- <명령>`으로 했다. 개발 owner 범위 검사이며 최종 통합 SHA의 착지 gate를 대신하지 않는다.
Windows 전용 코드·시험은 바꾸지 않았다.

## 시험 준비에서 찾은 문제

- 첫 타입 생성은 bool에 `ts(optional)`만 붙여 컴파일이 거부됐다. 기존 계약처럼 `as = "Option<bool>"`로
  고친 뒤 생성 검사와 타입 검사를 통과했다.
- 첫 E2E 진입은 전역 Playwright 1.59.1을 발견하고 실행 전에 거부했다. 작업 폴더에 준비한 1.64.1을
  `NODE_PATH`로 지정했다. 전역 패키지·공용 브라우저·시한은 바꾸지 않았다.
- 새 E2E의 첫 WebKit 단계는 앞 Chromium이 남긴 패널 선택으로 요청이 가로채기 설치 전에 나가 멈췄다.
  가로채기를 페이지 열기 전에 설치하고, 브라우저마다 서로 다른 저장소 이름으로 빈 host 캐시를 보장했다.
  실패 원문은 작업 폴더에 보존했고 기존 단언과 상한을 유지했다. 또 WebKit의 Service Worker를 거친 POST는
  Playwright route를 지나지 않았다. 별도 최소 재현에서 worker 허용은 가로채기 0회, 차단은 1회임을 확인한 뒤
  네트워크 주입 시험에만 기존 `serviceWorkers: "block"` 옵션을 썼다. 앱의 Service Worker는 바꾸지 않았다.
  도착 대기는 페이지 열기 전 설치한 `waitForRequest`로 끝내며, 실패 때도 단계 안 finally가 브라우저를 정리한다.

## 참고한 공개 구현

- [TanStack Query 병렬 조회](https://tanstack.com/query/latest/docs/framework/react/guides/parallel-queries)
  ([MIT](https://github.com/TanStack/query/blob/main/LICENSE)): 독립 조회가 서로를 막지 않는 방식을 참고했다.
- [SWR 오류와 지난 데이터](https://swr.vercel.app/docs/error-handling)
  ([MIT](https://github.com/vercel/swr/blob/main/LICENSE)): 데이터와 오류를 함께 유지하는 방식을 참고했다.

외부 코드나 새 런타임 의존성은 추가하지 않았다.

외부 작업이 `/tmp/aaw-*`를 지워 실행 중이던 E2E 140의 실험실이 사라진 일이 보고되었다.
그 실행은 외부 간섭으로 무효 처리하고 후보 결함 판단에서 제외했다. 독립 WebKit 최소 재현은
그 경로를 쓰지 않았으므로 Service Worker 가로채기 결과는 별도 증거로 유지한다.
