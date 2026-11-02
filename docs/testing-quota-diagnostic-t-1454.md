# 비밀 없는 한도 입력·OAuth 실패 진단

q-2439 1A로 승인한 진단 수리다. 표시 수리 착지 뒤의 별도 후보이며 설치·배포 승인은 포함하지 않는다.

## 저장 경계

상태줄 파서는 창을 수용하거나 보존하는 바로 그 분기에서 고정 enum 진단을 만든다.
agent는 `quota.json`에 기존 `QuotaSnapshot` 필드를 유지하고 로컬 `input_diagnostic`을 덧붙인다.
숫자와 진단을 한 번의 기존 private 원자 쓰기로 함께 갱신하며 이력은 늘리지 않는다.
`QuotaSnapshot` 독자는 추가 필드를 버리므로 protocol·agent link 응답·웹 형식은 바뀌지 않는다.
인증·대화·원시 payload·임의 필드명·거부된 원래 문자열/숫자는 진단에 복제하지 않는다.

창이 없거나 거부되면 이전 숫자·관측 시각을 그대로 보존한다. 기존 파서는 잘못된 리셋 형식을
리셋 미상으로 수용하므로 이를 `accepted` + `reset=invalid`로 드러낸다. 진단 추가로 파서 정책을 바꾸지 않는다.

OAuth 조회의 실패 HTTP 코드는 host의 계정·로그인 세대에 결속된 `Reading.http_status`에 저장한다.
401·403·429와 기타 HTTP 오류를 구별하고, HTTP 응답이 없는 실패·성공에서는 앞 코드를 이어 쓰지 않는다.
예전 캐시의 코드 없는 `Reading`도 역직렬화한다. 계정 alias·자격증명·공급자 오류 본문을 추가로 저장하지 않는다.
로그는 같은 상태 코드와 고정 사유(`authentication_failed`, `access_denied`, `query_rate_limited`)를 남긴다.

사용량 endpoint의 429를 모델 한도로 취급하지 않는 기존 배정 계약과 Retry-After는 보존한다.
401·403을 모두 `unsupported`로 다루던 운영 판단은 유지하면서 진단 코드로 원인을 구별한다.
화면 문구나 계정 인증 정책을 이 수리에 섞지 않았다.

## 필요한 회귀와 결과

- 공통 파서 `quota::claude::tests::diagnoses_the_parser_decision_and_keeps_previous_windows`:
  창 누락·상위/창 형식 오류·사용률 오류·지난 리셋의 거부를 나누고, 정상 주간만 갱신되는 경우를 본다.
  기존 리셋 미상 수용과 0%도 검사한다. 공통 quota owner 4개 통과.
- agent `quota::tests::per_pane_files_keep_window_times_and_reject_a_different_login` 확장:
  실제 파일의 진단·정상 관측 보존·로그인 세대·600 권한·임의 키와 비밀 값 미복제를 확인한다. 1개 통과(0.06초).
- host `registered_oauth_diagnostic_distinguishes_http_status_without_saving_error_bodies`:
  loopback 가짜 서버의 401·403·429·500·성공을 나누고 인증·오류 본문 미보존을 확인한다.
  기존 OAuth 이력·재시작·429 backoff 시험에는 실제 DB에 403·429 코드가 남는 단언을 보탰다.
  usage owner 14개 통과(0.57초), quota cache owner 5개 통과(0.46초).
- Rust E2E `accounts::claude_statusline_diagnostic_records_missing_and_rejected_windows_without_raw_input`:
  시험용 pane이 실행 명세의 statusLine 명령을 그대로 호출하고 CLI→인증된 hook→agent 파일 저장을 거친다.
  TCP gate로 각 호출 완료 사건을 받은 뒤 파일을 읽는다. 주간만 도착·사용률 거부와 비밀 8바이트 조각 미노출을 검사한다.
  새 E2E 1개 통과(1.39초). 원시 입력은 실제 인증이 아닌 가짜 canary다.

공통·agent·host·CLI 관련 owner의 native strict Clippy(`--all-targets -- -D warnings`)도 통과했다.

모든 검증은 `aaa verify --`로 실행했다. 시한·허용 오차·기존 기대값을 완화하지 않았다.
전체 착지 gate와 양 OS cross는 별도 착지 창의 최종 통합 SHA에서 수행한다.

## 참고

[Claude Code 상태줄 공식 계약](https://code.claude.com/docs/en/statusline)의 창별 독립 누락을 따른다.
[앞선 조사와 비교 구현](testing-coordinator-usage-t-1454.md)에 적은 ccstatusline·ccusage(MIT)와 비교한 뒤,
기존 aaa 파서·private 쓰기를 사용했다. 외부 코드를 복사하거나 의존성을 추가하지 않았다.
