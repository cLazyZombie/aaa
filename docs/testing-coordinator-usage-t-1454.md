# 창별 구독 사용량 관측 표시

## 확인한 원인

coordinator의 계정 실행 참조·로그인 세대가 host 수집 자료와 일치했고, 상태줄 파일과
host 저장 자료 모두 주간 관측을 최근에도 받고 있었다. 5시간 창만 오래된 상태였다.
공개 기록에는 실제 계정·머신 이름·pane ID·경로·상태 원문을 옮기지 않았다.

`registered_claude_usage`가 두 창 중 가장 오래된 시각을 계정 `checked_at`으로 골랐고,
오래된 창 하나의 상태를 계정 전체에 붙여 주간 관측까지 끊긴 것처럼 표시했다.
5시간 창이 현재 입력에서 빠진 것인지 유효하지 않아 보존된 것인지는 원시 stdin을
저장하지 않으므로 구별하지 못한다. 최신 주간 관측을 통해 수집 경로 전체의 단절과는 구별했다.

## 변경

- `UsageWindow.observed_at`·`problem` 선택 필드로 창별 실제 시각과 상태를 전달한다.
  구판 응답·로컬 조회는 기존 계정 시각·오류를 그대로 쓴다.
- 계정 시각은 최근 창의 관측 시각이다. 모든 창이 문제일 때만 계정 전체에 창 오류를 올린다.
- 웹의 각 창에 관측 경과를 표시한다. 오래된 세션 숫자는 `마지막`으로 표시하고 주간 갱신과 나눈다.
  칩도 선택한 창의 시각·문제를 쓴다.
- 60분부터 시간·분, 24시간부터 일·시간을 쓴다. 없는 작은 단위는 생략한다.
- 없는 창의 시각을 새로 만들거나 리셋 후 잔량을 합성하지 않는다. 계정 귀속·그룹 매핑은 바꾸지 않았다.

## 필요한 회귀

- Rust `registered_sidebar_keeps_each_window_age_when_only_weekly_observations_arrive`:
  오래된 세션 0%와 최신 주간 9%가 함께 올 때 개별 시각·숫자·오류와 최근 계정 시각을 검사한다.
  입력 순서를 뒤집어도 결과가 같아야 한다.
- 웹 시간 유닛: 분→시간→일 경계와 2908분 사례를 실제 대기 없이 확인한다.
- E2E 124: 기존 마지막 숫자·리셋·조회 실패 보존에 더해 같은 혼합 관측을 PC·휴대폰
  Chromium/WebKit에서 확인한다. DOM의 실제 표시를 기다리며 고정 sleep·시한 변경은 없다.

## 참고한 공개 구현

- [Claude Code 상태줄 공식 계약](https://code.claude.com/docs/en/statusline):
  각 창은 독립적으로 빠질 수 있고 리셋 뒤 사라질 수 있다. 없는 창을 새 관측으로 취급하지 않는다.
- [ccstatusline](https://github.com/sirmalloc/ccstatusline)
  ([MIT](https://github.com/sirmalloc/ccstatusline/blob/main/LICENSE)):
  세션·주간 위젯과 초기화 뒤 오래된 창의 취급을 비교했다. aaa는 마지막 숫자를 명시해 보존한다.
- [ccusage](https://github.com/ccusage/ccusage)
  ([MIT](https://github.com/ccusage/ccusage/blob/main/apps/ccusage/LICENSE)):
  상태줄 입력의 세션 비용 표시와 비교했다. 이 작업은 비용 추정 대신 공급자가 알린 창별 한도를 쓴다.

외부 코드를 복사하거나 런타임 의존성을 추가하지 않았다.

## 검증 결과

- `aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa-host --lib usage::tests::`:
  사용량·자동화 관련 13개 통과(실행 0.50초).
- `aaa verify -- env AAA_UPDATE_TS=1 nice -n 15 cargo test --profile verify --locked -p aaa-proto typescript`:
  타입 생성·최신성 2개 통과.
- `aaa verify -- nice -n 15 cargo clippy --profile verify --locked -p aaa-host -p aaa-proto --all-targets -- -D warnings`:
  strict Clippy 통과.
- `aaa verify -- … bash scripts/ci-local.sh --only web,e2e,public --spec 124`:
  타입·번들·웹 유닛 685개, E2E 124의 네 기기·엔진 조합, 공개 검사 통과.
  E2E 각 단계는 0.7~2.4초이며 전체 파일은 준비·정리 포함 8.3초였다.
  자기 작업 폴더의 Playwright 1.64.1 패키지와 기존 공용 브라우저 캐시를 썼다.
- `aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa-proto`: 전체 50개 통과.
- PC Chromium·휴대폰 WebKit 성공 화면에서 창별 시각·마지막 숫자의 표시와 잘림 없음을 직접 확인했다.

개발 검사는 미커밋 변경 때문에 workspace 원장을 수집하지 않았다고 알렸다. 각 명령 자체는 성공했다.
검증 로그는 작업 폴더 target 아래에만 두었다. 착지 gate·양 OS cross는 착지 창의 최종 통합 SHA에서 수행한다.
운영 서비스·사용자 설정·계정 인증은 바꾸지 않았다.
