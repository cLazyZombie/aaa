# 사용량 조회 오류와 작업 인증 건강 분리 검증

계정 목록은 정상인데 새 작업이 `인증 끊김`으로 대기하는 회귀를 고친다. 배정에 쓰는 사용량 캐시의
`Expired`·`Locked`는 사용량 helper의 실패일 수 있다. Codex helper는 중앙 갱신과 회전 경합을 피하려고
자기 loopback에서 갱신을 401로 차단하므로, 이 결과만으로 실제 작업 인증의 폐기를 증명하지 못한다.

## 변경 계약

- 공통 배정 준비 경로에서 실제 금고 인증을 읽고 계정 메타의 갱신 거부·만료를 확인한다.
- 사용량 `Expired`·`Locked`만으로 후보를 다시 제외하지 않는다. 같은 세대의 마지막 정상 관측을
  사용하고, 기록도 없으면 기존 미관측 배정 규칙을 따른다. 실제 인증 잠김·거부·만료와 확실한
  현재 한도·자동화 사용량 문턱은 계속 제외한다.
- Codex 사용량 로그에는 별칭을 담은 span, 실패 RPC 단계, 구조화된 HTTP 상태, 사용량 오류 종류,
  실제 로컬 갱신 차단 여부를 남긴다. 제공자 message의 숫자로만 추론한 경우 구조화된 상태는
  없음으로 남긴다. 제공자 원문·인증 조각·code·URL·stderr를 추가로 기록하지 않는다.
- 프로토콜·인증 저장 형식·중앙 갱신·원격 로그인 화면은 바꾸지 않는다.

## 회귀와 owner

| 시험 | 꼭 필요한 이유 |
| --- | --- |
| `account_pool::tests::persisted_usage_auth_errors_do_not_block_a_healthy_account_after_reopen` | Codex 가짜 금고·건강한 목록·저장된 Expired/Locked·host 재열기로 운영 회귀를 재현한다. 마지막 잔량 81%와 기록 없는 배정을 모두 확인한다. |
| `account_pool::tests::real_auth_failures_still_block_and_old_rejections_do_not_poison_a_new_login` | 실제 중앙 갱신 거부·메타 만료·금고 키 부재는 계속 막으며, 새 로그인 뒤 옛 판의 거부를 적용하지 않는다. |
| `quota_rpc::diagnostic_tests::rpc_failure_logs_keep_stage_status_and_local_refusal_without_provider_material` | 실제 tracing event에서 초기화/조회 단계·HTTP 상태·로컬 차단을 남기고, 제공자 응답에 심은 가짜 token·URL·코드의 8바이트 조각을 기록하지 않는다. |
| `quota_rpc::tests::refresh_refusal_reads_the_post_body_before_closing` | 기존 HTTP 사건 기반 거부 시험에 실제 차단 표지를 확인해, 상태만 추측해서 로그에 쓰지 않는다. |
| `m13::accounts::automatic_assignment_uses_persisted_weekly_quota_after_host_restart` | 기존 재시작 E2E의 잔량 많은 계정에 Expired를 남긴다. 실제 host→공통 배정→agent→PTY 자식에 선택한 인증이 전달되는 계약을 지킨다. |

기존 `only_confirmed_limits_and_broken_auth_exclude_an_account`는 수리 전 사용량 Expired/Locked를
실제 인증 고장처럼 주입했다. 건강 판정은 공통 준비 경로에서 검증하도록 위의 새 회귀로 분리하고,
이 시험은 `only_confirmed_usage_limits_exclude_an_authenticated_candidate`로 이름을 바꿔 최신 100%·
유효한 429 차단을 유지한다. 기존 미관측 시험에는 조회 Expired/Locked 경우를 더한다.
시한·기대값 완화·재시도로 실패를 덮지 않는다. 새 시험은 DB·loopback·가짜 인증만 쓰고 고정 sleep을 쓰지 않는다.

## 참고

- [Codex의 API 401 복구와 실제 refresh 거부 구분](https://github.com/openai/codex/blob/rust-v0.165.0/codex-rs/login/src/auth/manager.rs#L1926), Apache-2.0.
- [cc-switch의 영구/일시 refresh 거부 분류](https://github.com/farion1231/cc-switch/blob/ba3d80d87ce7e3fe1ed52a5d5cbe9fa79eb5898e/src-tauri/src/proxy/providers/codex_oauth_auth.rs), MIT.

두 구현의 책임 분리만 참고하며 외부 코드·의존성을 추가하지 않는다. 이 수리에서 기존 중앙 갱신의
400/401 분류까지 바꾸지 않는다. 원격 기기의 재인증 흐름은 별도 설계다.

## 실행 기록

개발의 관련 owner 검사이며 최종 착지 gate는 승인된 착지 창의 통합 SHA에서 수행한다.
수리 전 새 저장 캐시 회귀는 `Expired, history=true: … 인증 끊김`으로 0.66초에 실패했다.

| 개발 owner | 결과 |
| --- | --- |
| `aaa-host --lib account_pool::tests::` | 20개 통과, 전체 1.26초 |
| `aaa-host --lib quota::tests::` | 4개 통과, 전체 0.70초 |
| `aaa-host --lib account_meta::tests::` | 6개 통과, 전체 0.83초 |
| `aaa-host --lib automation::usage::tests::` | 3개 통과, 전체 0.01초 |
| 새 RPC 진단·비노출 owner | 1개 통과, 0.02초 |
| 기존 loopback 갱신 차단 owner | 1개 통과, 0.05초 |
| 실제 host 재시작·배정 m13 E2E | 1개 통과, 2.29초 |
| `aaa --test deps` | 의존 경계 2개 통과 |
| strict Clippy | aaa-host 전체 target, aaa의 m13·deps target 통과 |
| fmt·diff·공개 검사 | 통과 |

모든 Cargo 검사는 `nice -n 15`·`--profile verify --locked`로 이 worktree의 target에서 수행했다.
빌드 캐시를 유지하고 job 수를 지정하지 않았다. 수리 전 원인을 확인한 회귀 실패 외에 관련 owner
검증 실패·재실행·시한 연장은 없다. 운영 인증·provider API·로그인·서비스 설정은 바꾸지 않았다.
착지 전체 gate·양 OS cross·Windows 전달 빌드는 이 개발 검사에 포함하지 않는다.
