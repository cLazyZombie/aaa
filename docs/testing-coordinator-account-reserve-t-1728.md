# coordinator 계정 예약분 검증(t-1728)

## 범위

자동 배정의 주간 잔량에서 실행 중 coordinator 몫을 빼고 점수·후순위를 계산한다.
요금제 정의 데이터에 $100=10%p, $200=5%p를 둔다. Claude·Codex 모두 해당하며 다른 금액·미확인은 0%p다.
원래 관측은 고치지 않고 선택 이유에 원래 잔량·예약분·차감 후 잔량을 남긴다.

현재 coordinator는 host pane의 프로젝트 소속·실행 상태와 저장된 실행 인증의 계정 별칭으로 식별한다.
기본 계정·coordinator_agent_args는 다음 실행 설정이므로 현재 계정의 근거로 쓰지 않는다.
여러 프로젝트에서 쓰는 모든 계정을 모으되 같은 계정은 한 번만 차감한다. 연결 단절 중에는 예약분을 유지한다.
로컬 로그인처럼 host에 실행 인증 기록이 없으면 계정 식별이 불가능하며 예약분도 없다.

## 최소 회귀 시험

- `account_plan::tests::coordinator_reserve_uses_the_selected_price_tier`: 두 harness의 금액별 정책과 모호한 감지값 비추정을 검사한다.
- `account_pool::tests::coordinator_reserve_changes_ranking_and_records_effective_weekly_quota`: 40→35/30의 실제 선택 역전과 선택 기록,
  마지막 관측·0 하한·보조 모델 창·명시 계정 경계를 검사한다.
- `account_pool::tests::allocation_reserves_each_running_coordinator_account_across_projects_once`: 실제 Host·DB·금고·pane 기록으로
  여러 프로젝트·중복 coordinator·변경된 기본 계정·종료·원격·사람 셸을 구분하고 최종 계정 선택까지 검사한다.

시간을 재우거나 실제 인증·사용자 설정·운영 프로세스를 쓰지 않는다. 모든 자원은 시험 전용 임시 폴더에 둔다.
웹·protocol·Windows 전용 경로는 변경하지 않는다. 별도 웹 E2E는 추가하지 않고 정규 전체 검증의 기존 E2E를 실행한다.

## 결과

첫 관련 owner 실행은 26건 통과·새 fixture 1건 실패였다. 새 프로젝트 생성은 host당 하나로 제한되므로,
다중 프로젝트 fixture는 기존 프로젝트 폴더를 읽는 경계로 구성하도록 수정했다. 제품의 생성 제한은 바꾸지 않았다.
수정 뒤 `cargo test --profile verify --locked -p aaa-host --lib account_pool::tests::`의 27건이 모두 통과했다.
또한 Codex $200 요금제 예약분을 5→0으로 바꾸자 실제 Host 배정 시험이 `0.0 != 5.0`으로 실패했다.
변형은 원복했다. 검증은 모두 `aaa verify -- <명령>`으로 실행한다.

정규 전체 검증은 `ci-local.sh`의 기본 web·fmt·build·test·e2e·public 단계를 실행한다.
관련 Rust owner는 `cargo clippy --profile verify --locked -p aaa-host --all-targets -- -D warnings`로 확인한다.
Playwright npm 패키지는 작업 폴더에 1.64.1을 두며, 기존 공용 브라우저 캐시를 읽는다.
전체 검증 결과·후보 SHA·로그 위치는 작업 보고의 library 검증 기록에 남긴다.
최종 통합 SHA의 착지 gate는 승인받은 착지 창에서 별도로 한 번 수행한다.

## 비교한 공개 구현

- [LiteLLM 예산 라우팅](https://docs.litellm.ai/docs/proxy/provider_budget_routing),
  [라이선스](https://github.com/BerriAI/litellm/blob/main/LICENSE): 공급자별 예산 데이터를 라우팅 정책에서 읽는 접근을 참고했다.
  핵심은 MIT이며 별도 enterprise 경로 예외가 있다. 코드·의존성은 가져오지 않았다.
- [Bifrost 공급자 라우팅](https://github.com/maximhq/bifrost/blob/dev/docs/providers/provider-routing.mdx),
  [Apache-2.0](https://github.com/maximhq/bifrost/blob/main/LICENSE): 실행 문맥과 용량 정책을 분리하는 접근을 참고했다.
  aaa에는 요청 프록시 대신 이미 있는 시작 시점의 계정 배정과 요금제 데이터가 맞아 그 경로를 확장했다.

## 적용과 한계

기존 profile·safety·계정 설정은 바꾸지 않는다. 요금제 미확인은 사람이 웹 설정에서 선택해야 한다.
예약분은 선택 점수의 여유분으로, 다른 후보가 없을 때의 시작이나 이미 실행 중인 작업의 소비를 차단하지 않는다.
main 병합·push·배포는 별도 승인 뒤 진행한다.
