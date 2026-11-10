# Codex 인증 갱신 실패 수리·선제 갱신·원인 알림 검증(t-1688)

## 변경과 경계

11-09 운영에서 Codex pane이 「Failed to refresh token: 503 Service Unavailable」로 멈췄다. 503은 OpenAI가 아니라 aaa pane 중계의
표지였다. host가 Claude 조회 인증(JSON)을 통째로 가림 규칙에 넣어 `"refresh_token"` 키 조각이 규칙이 됐고, agent→host 링크 가림이
갱신 요청 본문을 깨뜨려 host가 OpenAI에 묻기 전에 실패했다. 설계는 plan §8.562, 동작은 [계정](accounts.md)에 있다.

- 결함 수리(첫 커밋, 따로 착지할 수 있다): JSON 인증은 토큰 값만 등록, 갱신 요청은 링크 가림에서 뺌, host는 판을 본문보다 먼저 봄,
  실패를 계정·원인·단계·HTTP 상태·오류 코드로 WARN.
- 기능(둘째 커밋, 사용자 답 1B·2A): host 선제 갱신(만료 24시간 전, 실패 5분 뒤 재시도, 거부면 멈춤), 갱신 실패 원인·멈춘 thread 알림과
  회복 알림, pane 중계의 상태·오류 코드 전달(502 망·504 host 무응답·503 aaa 안), 계정 목록의 인증 만료 예정·마지막 갱신·갱신 실패
  (FRONT_API 1.166), 끊김 판정을 Codex 0.167의 영구 실패 분류로 좁힘.

공통 Rust·protocol·생성 TypeScript·웹 화면 변경이다. Windows 전용 경로·production 산출물은 바꾸지 않았다.

## 꼭 필요한 시험

| owner | 지키는 계약 | 왜 필요한가 |
| --- | --- | --- |
| m13 `accounts::account_credentials_parallel_refresh_and_all_records_are_redacted` | 같은 host에 Claude 조회 로그인(JSON)이 있어도 원격·로컬 pane의 Codex 갱신이 링크를 거쳐 돈다. 거부된 갱신은 pane에 401·`refresh_token_reused`로 돌아간다. 조회 토큰도 누출 검사에 넣었다 | 11-09 운영 회귀의 실제 조합이다. 제품 수리를 되돌리면 첫 갱신 물결에서 실패하고, host 로그에 운영과 같은 `JSON syntax … [accounts.rs:454]`가 남는 것을 확인했다 |
| m13 `accounts::host_renews_codex_auth_a_day_before_expiry_and_reports_the_provider_reason` | 금고 access token(JWT `exp`)이 하루 안이면 tick이 제공자에게 한 번 갱신해 판을 올린다. 거부된 계정은 원인(401·코드)·만료 예정을 목록에 보이고 끊김 알림 하나를 내며 다시 묻지 않는다. CLI 목록 글에도 원인이 있다 | 선제 갱신·멈춤 규칙과 원인 표시를 실제 tick·금고·가짜 제공자로 잇는다 |
| aaa-common `redact::tests::json_credentials_register_token_values_but_not_key_names` | 조회 OAuth JSON을 등록해도 키 이름·갱신 요청 글은 그대로이고 토큰 값만 가린다. setup-token은 값 전체 | 가림 규칙의 근본 원인을 가장 좁게 고정한다 |
| aaa-host `accounts::tests::an_old_version_is_served_before_the_request_body_is_read` | 다시 로그인으로 금고 판이 오르면 옛 판 pane은 본문이 깨져 있어도 최신 인증을 받는다. 지금 판의 깨진 본문은 aaa 안 실패(중계 503)다 | 재시작 없이 회복하는 경로 |
| aaa-host `accounts::tests::only_refresh_token_rejections_are_permanent_and_only_the_code_is_read` | 401·400 `invalid_grant`·`refresh_token_*`만 끊김, 오류 코드는 세 자리의 짧은 낱말만 읽는다 | 선제 갱신이 다른 400으로 하루 일찍 끊김·자동 배정 제외로 가지 않게 한다. 제공자 본문(설명)을 싣지 않는다 |
| aaa-host `account_meta::tests::refresh_failures_alert_with_the_cause_and_recovery_names_stalled_threads` | 일시 실패 두 번은 알리지 않고, pane이 멈추면 원인·thread와 함께 한 번 알린다. 일시 실패는 끊김이 아니다. 갱신·다시 로그인하면 실패 항목을 끝내고 「회복」에 멈췄던 thread를 싣는다. 거부는 코드·로그인 명령을 실은 끊김 하나 | 답 2A의 알림·회복 순서(끝내기 뒤 회복 항목)를 고정한다 |
| aaa-session `account::tests::refresh_failures_keep_only_the_provider_status_and_code` | 중계는 제공자 상태·코드를 그대로, 코드 꼴이 아니면 버리고, aaa 안 실패는 503 | Codex 화면이 원인을 정확히 보이는 경계 |
| 웹 `accountsModel.test.mjs` 갱신 실패 글 | host와 같은 원인 글에 횟수·처음 실패·멈춘 thread | 웹과 CLI·알림의 글이 갈라지지 않게 한다 |

기존 시험의 바뀐 단언: m13 병렬 갱신 시험의 재개 pane은 거부된 갱신에서 503 대신 401과 `{"error":"refresh_token_reused"}`를 받는다
(의도한 계약 변경). `account_meta::tests::automatic_refresh_preserves_the_edited_email`은 갱신 시각(`refreshed_ms`)만 새로 남는 것을
허용한다. 시험 삭제·시한 확대는 없다. 새 대기는 사건(`wait_for`, tick 완료)이며 고정 sleep은 없다.

## 실행

검사는 macOS 개발 Mac에서 `aaa verify -- <명령>`으로 차례로 실행했다(겹쳐 돌리지 않았다). 모두 통과했다.

| 명령 | 결과 |
| --- | --- |
| `scripts/verify.py develop --base <branch 시작> --spec 118,125 --package aaa-common --package aaa-proto --package aaa-session --package aaa-agent --package aaa-host` | Rust 1228개 통과·실패 0, 같은 crate의 strict clippy, web(타입·번들·웹 유닛), E2E118(인증 갱신)·125(요금제), 공개 검사 |
| `cargo test -p aaa --bin aaa` | 103개 통과(CLI 계정 목록 글) |
| `cargo test -p aaa --test deps` | 의존 경계 통과 |
| `cargo test -p aaa --test m13 -- accounts::` | 20개 통과 |
| `scripts/ci-linux-clippy.sh` | Linux strict cross clippy 통과 |

첫 develop 실행은 web 단계의 `check_fixture_secrets.py`가 redact 시험의 직접 문자열(`"refresh_token":"…"`)을 막아 실패했다.
그 단언을 토큰 값만 뺀 조회 JSON을 실행 중에 만들어 비교하는 것으로 바꾸고 다시 돌려 통과했다.

시험 시간: m13 병렬 갱신 시험은 이 변경 전부터 단독 약 5.1초였다(조회 로그인 단계를 뺀 같은 시험 세 번 5.06·5.18·5.14초).
조회 로그인 단계는 약 0.1~0.2초를 더한다(5.20·5.35초). 5초 지향을 넘는 기존 시간은 이 변경에서 줄이지 않았다.
새 m13 선제 갱신 시험은 단독 1.45초다.
