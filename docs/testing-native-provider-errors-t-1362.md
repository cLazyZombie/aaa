# native 공급자 오류 원인 검증(t-1362)

## 계약

완료된 Claude/Codex turn의 구조화된 오류를 과부하·인증·구독/결제·모델·요청 제한·미확인으로 구별한다.
오류 본문을 분석하지 않으며 모르는 코드를 원문 그대로 전송하지 않는다. HTTP 429는 구독 고갈로 단정하지 않는다.
화면 감지의 활동·질문·승인 상태는 그대로 두고, native 원인과 실행/완료 시각만 별도로 표시한다.

실행 세대는 agent가 정한다. hook의 startup/resume와 셸 앞 harness 획득 때 실행 시작을 갱신하고,
session 사건에도 같은 세대 값을 넣어 host와 다시 연결한 agent가 같은 값을 쓴다. 대화를 발견한 시각은 실행 시작이 아니다.
같은 실행에서 발견 전에 끝난 turn도 받아야 한다. 원격 agent의 시각은 host 벽시계와 비교하지 않는다.

## 꼭 필요한 시험

- `aaa-agent provider_error::completed_native_codes_distinguish_causes_without_reading_sensitive_text`:
  두 공급자의 같은 원인·모르는 코드·재시도 중 오류를 구별하고 오류 원문이 출력 계약에 들지 않는지 본다.
- `aaa-agent provider_error::other_sessions_sidechains_tools_old_turns_and_success_do_not_restore_an_error`:
  다른 세션·하위 agent·도구 결과·지난 turn 완료로 현재 오류를 만들지 않는지 본다.
- `aaa-agent plan::native_errors_wait_for_complete_jsonl_and_match_the_current_execution`:
  실제 파일에 줄을 나눠 쓰고, 완성 전에는 내보내지 않으며 완성 후 HTTP 401 union을 읽는지 본다.
  이전 실행·다른 세션 관측도 거른다.
- `aaa-host harness::native_errors_are_bound_to_machine_session_generation_and_latest_turn`:
  실제 host pane에 사건을 적용해 머신·세션·세대·최신 turn·제출 시각 경계를 검사한다.
  오류 표시가 화면 활동이나 메시지를 바꾸지 않는지도 본다.
- `aaa-host harness::discovered_session_uses_the_agents_execution_generation_not_discovery_time`:
  늦게 발견한 현재 실행의 완료 오류가 세대 불일치로 버려지던 결함을 지킨다.
- `web/test/providerFailure.test.mjs`: native 원인·시각·코드 표시, 구판/미완료/성공에서의 부재,
  화면 오류 본문으로 native 원인을 추측하지 않음, 유실 상태 우선순위를 확인한다.
- 웹 E2E119: 실제 격리 JSONL → PID/기록 연결 → agent → host → 웹 경로로 두 공급자의 다섯 원인,
  새 turn에서 지우기, 다른 세션·도구·지난 turn 완료 뒤 정상 완료를 확인한다.
  데스크톱 Chromium과 휴대폰 WebKit(390px)이 같은 결과를 받고 긴 코드·시각이 가로로 넘치지 않는지도 잰다.
  기록을 직접 쓰고 DOM 또는 완료 관측을 울타리로 기다린다. 고정 sleep·실제 공급자 요청은 없다.

새 시험은 위 계약과 발견한 회귀를 지키는 데 필요하다. 구현 상수의 모양이나 커버리지 수를 맞추기 위한 시험은 추가하지 않았다.

## 최초 실패와 수리

1. 첫 E2E119의 Claude 과부하 표시가 30초 안에 나타나지 않았다. agent의 `harness.json`에는 분류가 있었지만
   host가 세션 발견 시각을 실행 세대로 삼아, pane 시작 시각을 보낸 agent 관측을 거부했다.
   session 사건에 agent의 세대 값을 함께 싣고 host의 초기 경계도 같은 세대 시각으로 세웠다.
   원래 시한·분류·화면 단언을 유지하고 전용 host 회귀를 추가했다.
2. 수리 후 Claude의 다섯 원인·새 turn·다른 세션/도구 단계가 통과했다. Codex 준비는 native ID를 찾지 못했다.
   시험용 Python fixture를 대화 0줄로 줄여 메타데이터 버퍼가 flush되지 않았다.
   최소 한 줄로 메타데이터 flush를 보장했다. 제품 동작·기대값·시한을 바꾸지 않았다.

3. strict Clippy가 host 경계 검사의 중첩 `if`를 지적했다. 같은 조건을 let-chain으로 정리한 뒤 strict 검사가 통과했다.

## 실행 결과

- protocol 전체 50개 통과. TypeScript 타입도 Rust 정본에서 다시 생성했다.
- agent 전체 193개 통과.
- host harness owner 22개 통과.
- 웹 타입·번들 및 웹 유닛 전체 671개 통과.
- E2E119 통과: 데스크톱 Chromium·휴대폰 WebKit에서 두 공급자 경로를 확인했다. 각 기능 단계는 3초 이내였다.
- 관련 `aaa-agent`·`aaa-host`·`aaa-proto`의 all-targets strict Clippy 통과.
- `cargo test --profile verify --locked -p aaa --test deps`의 2개 의존 경계 검사 통과.
- fmt·공개 검사 통과. 새 파일을 staging한 뒤의 공개 검사도 통과했다.

개발의 관련 owner만 실행한다. 착지 창을 받지 않았으므로 최종 통합 SHA의 전체 gate·양 OS cross·Windows production·병합·push는 실행하지 않는다.
최초 실패와 수리 후 실행 로그는 t-1362의 library에 보존한다. 관련 검사 통과를 전체 gate 통과라고 부르지 않는다.

## 참고한 공개 구현

- [shepherdr provider-error.ts](https://github.com/eliasstravik/shepherdr/blob/c3f0f594/server/common/provider-error.ts)와
  [Codex reader](https://github.com/eliasstravik/shepherdr/blob/c3f0f594/server/common/readers/codex.ts),
  [Claude reader](https://github.com/eliasstravik/shepherdr/blob/c3f0f594/server/common/readers/claude.ts)
  ([MIT](https://github.com/eliasstravik/shepherdr/blob/c3f0f594/LICENSE)):
  native 완료 오류와 turn 경계를 함께 읽는 접근을 참고했다. 원문 정규식 추론·자동 이어하기 정책은 가져오지 않았다.
- [OpenAI Codex protocol](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)
  ([Apache-2.0](https://github.com/openai/codex/blob/main/LICENSE)):
  완료 사건의 `error`와 `codex_error_info`의 문자열/HTTP 상태 union을 확인했다.
- [Claude SDK 메시지 계약](https://platform.claude.com/docs/en/agent-sdk/typescript#sdkassistantmessage):
  `authentication_failed`·`overloaded`·`billing_error`·`rate_limit`·`model_not_found`의 의미를 확인했다.

외부 코드·새 런타임 의존성은 추가하지 않았다. Playwright npm 패키지만 작업 폴더에 저장소 요구 판으로 두고,
이미 있는 공용 Chromium/WebKit 캐시를 쓴다. 브라우저 설치·시스템 설정 변경은 없다.
