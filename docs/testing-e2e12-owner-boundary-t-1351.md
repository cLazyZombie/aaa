# E2E 12 실패 자식의 정리 경계(t-1351)

## 후보와 실패 경로

빈 alt 후보는 Markdown 파서·미디어 E2E 115·문서·CLI 안내만 바꿨다. 실패 경로인
`thread-lifetime.mjs`·fixture·`cleanup-scope.mjs`·공통 harness는 시작 main과 같은 blob이었다.
실패한 nested fixture는 기본 t1 셸만 열며 빈 alt 자료를 만들지 않는다. E2E 115도 그 실행에서 통과했다.

첫 통합 후보의 전체 gate에서 E2E 12의 「떨어진 작업의 WebKit 자식도 자연 종료」가
`20000ms 안에 실패한 실험실 정리 완료이 되지 않았다`로 실패했다. 재시도하지 않고 gate를 중단했다.

## 이력과 측정

- 기존 owner는 t-1123의 정리 소유권 수리에서 추가됐다. 당시 일반 병렬 기록은 2.111초다.
  이후 이 파일의 변경은 문서·HTTP/route 관측 경계였고, 실패 fixture의 전체 실험실 재생성은 남아 있었다.
- 최근 main CI 15회는 성공 12회·실패 1회·취소 2회다. 실패 회차 `40851405288`의 공개 annotations는
  Rust owner 실패이며 같은 WebKit 정리 오류는 발견하지 못했다. 동일 실패가 반복됐다는 근거로 쓰지 않는다.
- 착지 시작 전에는 다른 cargo 실행이 끝난 것을 확인했다. 실행은 10 CPU, Rust 6슬롯·E2E 4 worker였다.
  E2E 시작 load는 37.55, 실패 worker 종료 때 86.69였다. 원문 자식 로그의 생성부터 최종 기록까지는 28.779초다.
- 원문에는 세부 정리 단계 시각이 없어 당시 느린 호출 하나를 확정할 수 없다. 따라서 원래 실행을 단순 재실행해
  성공 여부로 판단하지 않고, 원본 fixture의 동작을 바꾸지 않는 loader 계측을 한 번 수행했다.
- 진단 실행은 전체 9.879초, WebKit 소유 자원 close 0.243초, 관련 없는 Chromium close 4.729초,
  session host 정리 1.389초였다. 실제 WebKit은 exit 0, 원래 owner 실패는 exit 1로 끝났다.
  이 진단 통과를 원래 실패가 사라졌다는 근거로 쓰지 않는다.

## 원인과 구조 수리

검사하려는 계약은 「분리된 생성 작업도 scope가 소유하고 원래 작업 실패 뒤 실제 WebKit을 닫는다」다.
기존 fixture는 이를 확인하면서 별도의 host·agent·Chromium·앱 페이지·PTY를 만들었다. 부모의 20초는
WebKit 정리가 아니라 이 별도 실험실의 준비·WebKit·Chromium 종료·session host 정리 전체에 걸려 있었다.
실패 문구도 IPC 대기를 「페이지가 답하지 않음」으로 표시했다. 부하 아래 관련 없는 준비·정리가 계약 실패로 섞이는 경계다.

`cleanupScope.run`으로 작업 완료/실패와 scope.close의 공통 경계를 뽑았다. 실제 runSpec과 실제 WebKit fixture가
이 경계를 함께 사용한다. fixture는 WebKit 자식 하나만 띄워 미완료 작업의 실패를 만들고, 같은 scope 정리 뒤
자식 exitCode와 원래 실패·자연 exit 1을 검사한다. 중복 host·agent·Chromium·PTY 준비를 없앴다.
제품 코드는 바꾸지 않았고, 20초·단계 120초 상한과 WebKit exit·원래 오류·자연 종료 단언은 유지했다.
작업/정리가 모두 실패하면 두 오류를 모두 보존한다.

## 필요한 검사와 결과

- 기존 scope 소유권 시험 2개는 유지했다. 새 2개는 공통 run 경계가 분리된 늦은 생성도 회수하고 원래 오류를 보존하는지,
  작업과 정리가 함께 실패하면 두 원인을 보존하는지 확인한다. 실제 runSpec과 fixture가 공유하는 새 경계이므로 필요하다.
- 공통 scope·E2E 구조·고정 sleep 검사 18개 통과.
- 수정한 E2E 12 전체는 22.5초에 통과했고 실패 자식 keeper는 0.473초였다.
- 실제 WebKit의 소유권 누락 대조는 owner 끝에 exitCode=null로 기존 단언이 실패하는 것을 확인했다.
  그 자식만 close 명령으로 회수한 뒤 원래 오류·자연 exit 1도 보존했다.
- 새 최종 SHA의 전체 gate·cross·Windows 결과는 착지 보고 첨부에 보존한다.

## 참고

[Playwright BrowserServer.close](https://playwright.dev/docs/api/class-browserserver#browser-server-close)
([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE))의 실제 프로세스 종료 보장과
[Node child process exit](https://nodejs.org/api/child_process.html#event-exit)
([MIT](https://github.com/nodejs/node/blob/main/LICENSE))의 종료 코드·시그널 구분을 따른다.
외부 코드·의존성은 추가하지 않았다.
