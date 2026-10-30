# 계정 사용량 조회와 마지막 주간 잔량 배정 검증

등록 Claude 계정에서 상태줄만 기다리거나 오래된 관측을 배정에서 버리면, 주간 잔량 11%인 계정이
65%인 계정보다 먼저 선택될 수 있었다. 추가 OAuth 조회와 마지막 정상 관측에 따른 배정 순서를 검증한다.

## 동작

- 상태줄을 먼저 수집한다. 최신 공용 5시간·주간 창이 모두 없으면 해당 등록 계정의 인증으로
  `GET https://api.anthropic.com/api/oauth/usage`를 한 번 조회한다. 로컬 로그인·브라우저 쿠키·키체인을
  찾거나 인증을 갱신하지 않는다. 작업 시작은 이 HTTP 요청이나 수집 잠금을 기다리지 않는다.
- `setup-token`의 조회 권한이 부족할 수 있다. 401·403은 이 조회의 미지원으로 표시하고,
  정상 실행 가능한 인증을 고장으로 바꾸지 않는다. 429는 조회만 `Retry-After`까지 미룬다.
- 성공 관측은 계정 인증 세대에 결속해 관측 시각·사용률·리셋 시각을 저장한다. 빈 응답·부분 응답·
  실패 뒤에도 별도로 보관한다. host 재시작·계정 별칭 변경 뒤 유지하고 재로그인 때는 버린다.
- 배정은 공용 주간 잔량을 먼저 비교한다. 현재 정상 조회가 없으면 마지막 기록을 쓴다.
  저장된 주간 리셋 시각을 지났으면 배정 계산에서만 잔량 100%로 본다. 화면의 관측값과 시각은 바꾸지 않는다.
  주간 기록이 없으면 다른 해당 창의 최소 잔량으로 비교한다. 같은 잔량과 완전 미관측 계정끼리는
  진행 중 작업 수·최근 배정 시각·별칭 순으로 고른다. 명시 계정·기존 대화의 계정은 유지한다.

## 필요한 시험

| 시험 | 지키는 계약 |
| --- | --- |
| `last_weekly_quota_ranks_accounts_and_reset_restores_full_capacity` | 11%·65% 비교, 작업 수보다 잔량 우선, 리셋 직전·경계, 모르는 리셋, 현재 조회 우선 |
| `newest_saved_weekly_observation_beats_an_older_still_fresh_source` | 다른 머신의 더 오래된 성공 관측보다 마지막 정상 관측 우선 |
| `last_quota_ignores_invalid_conflicting_and_unrelated_windows` | 잘못된 수치·미래 관측·충돌·다른 모델 제외 |
| `history_survives_empty_partial_failed_and_skewed_snapshots` | 빈 응답·부분 응답·단절·시계 오류 뒤 원래 주간 잔량·관측·리셋 유지 |
| `registered_claude_oauth_persists_history_and_backs_off_without_blocking_start` | loopback HTTP의 등록 인증·출처·비노출·403·429 backoff, 실제 DB 재열기와 배정, 재로그인 초기화 |
| `automatic_assignment_uses_persisted_weekly_quota_after_host_restart` | 실제 CLI→재시작 host→agent→PTY 자식의 선택 계정 인증 전달 |

기존 배정 분산 시험은 잔량을 모르는 계정끼리의 동률 계약으로 유지한다. 기존 하단 사용량 시험은
새 인증으로 조회한 뒤 옛 세대 수치가 남지 않는지 확인하도록 확장했다. 시험 삭제·시한 완화는 없다.
시험 홈만 지정하고 가짜 origin이 없으면 등록 계정 조회도 loopback의 닫힌 포트로 제한한다.

## 조회 수단 조사와 참고

- [공식 Claude Code 상태줄](https://code.claude.com/docs/en/statusline): `rate_limits`는 첫 API 응답 뒤에
  제공되고 각 창이 독립적으로 빠질 수 있다. 기존 상태줄 수집을 계속 우선한다.
- [공식 CLI 명령](https://code.claude.com/docs/en/cli-reference): 독립적인 구독 잔량 JSON 명령을 찾지 못했다.
- [CodexBar Claude 자료원](https://github.com/steipete/CodexBar/blob/main/docs/claude.md)과
  [OAuth 조회 구현](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Claude/ClaudeOAuth/ClaudeOAuthUsageFetcher.swift)
  ([MIT](https://github.com/steipete/CodexBar/blob/main/LICENSE)): OAuth→CLI→웹의 자료원과 마지막 관측 보존을 참고했다.
  기존 aaa HTTP 수집 경로에 같은 OAuth endpoint·beta 헤더를 적용했다. 별도 PTY의 `/usage` 조회는
  Claude 세션을 만들어야 하고, 웹 방식은 등록 계정과 별개인 쿠키가 필요해 도입하지 않았다.
- [ccusage](https://ccusage.com/guide/)
  ([MIT](https://github.com/ryoppippi/ccusage/blob/main/apps/ccusage/LICENSE)): 로컬 토큰·추정 비용 집계이므로
  구독 주간 잔량을 대신하지 않는다. 새 의존성이나 외부 코드는 가져오지 않았다.

실계정의 API 권한·제공자 응답은 가짜 시험의 보증 범위 밖이다. OAuth가 거부돼도 상태줄과 마지막
기록으로 배정한다. 기록도 전혀 없으면 기존 미관측 시작을 허용한다. 착지 전체 gate는 승인된 착지 창에서 수행한다.

## 실행 결과

개발 관련 owner 검사 결과다. 착지 전체 gate를 대신하지 않는다.

- `verify.py develop --base HEAD --package aaa-host --package aaa-proto`: host 787개·proto 50개와 doctest 통과,
  두 crate strict Clippy·공개 검사 통과(기존 host 수동 시험 2개 제외). 마지막 보완 뒤 관련 owner를 다시 확인했다.
- 최종 `account_pool::tests::`: 18개, 1.20초. `quota::tests::`: 4개, 0.72초.
  `usage::tests::`: 12개, 0.78초. `automation::usage::tests::`: 3개 통과.
- 웹 타입·production bundle·유닛 662개 통과. Rust→TypeScript 생성 일치 검사 통과.
- 추가한 실제 host 재시작·배정 E2E는 최종 코드에서 2.19초 통과. 의존 경계 시험 2개 통과.
  관련 host·proto와 m13·deps 시험 target의 strict Clippy, `cargo fmt --all --check`, 공개 검사 통과.
- 실제 계정·운영 quota·브라우저 화면은 시험하지 않았다. Windows 전용 코드 변경은 없다.
