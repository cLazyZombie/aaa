# 작업 요약 진행 목록 6개 상한(t-1263)

host가 읽는 coordinator 요약과 thread 보고의 `## 진행` 상한을 5개에서 6개로 늘린다.
권장 범위는 3~6개다. 진행 외 절의 3줄, 이름 30자, 전체 1KB 제한은 유지한다.
thread brief·coordinator guide·CLI 도움말·프로토콜 주석과 생성 TypeScript·계획서도 같은 기준으로 맞춘다.
웹은 이미 받은 목록 전체를 표시하므로 렌더링 코드는 그대로 쓰고 주석과 실제 표시 회귀를 갱신한다.

## 필요한 회귀

새 시험을 추가하지 않고 기존 keeper를 늘렸다.

- host `summary::tests::progress_preserves_the_whole_order_and_validates_states_and_limits`:
  완료·진행·대기가 섞인 6개 항목의 순서와 상태를 보존하고 7개를 거부한다.
  필요성: 허용 경계 자체가 달라지는 계약을 검증한다.
- host `summary::tests::report_progress_requires_a_real_bounded_section_and_preserves_order`:
  독립된 보고 절도 6개는 받고 7개는 거부한다. 코드 예제·중복 절·잘못된 상태·30자 경계 검사를 유지한다.
  필요성: coordinator 요약과 thread 보고가 같은 상한을 적용해야 한다.
- host `orch::report_tests::reports_update_progress_preserve_goal_and_keep_later_instructions_across_recollection_and_restart`:
  실제 보고 수집에서 여섯째 항목까지 읽고 재시작 뒤 목록을 보존한다. 7개 보고는 마지막 정상 목록과 미반영 지시를 보존한다.
  필요성: 잘못된 보고 때문에 기존 진행과 지시 경고가 사라지는 회귀를 막는다.
- 웹 E2E28의 기존 전체 진행 목록 단계:
  CLI 요약·실제 보고 감시를 통해 6개 모두 hover·상세·Chromium320/WebKit390 Reader 시트에 보인다.
  `2/6` → `3/6` → `6/6` 완료 수, 순서, 읽기 전용, 긴 이름 줄바꿈·넘침, 새 지시 보존과 목표만 편집하기를 확인한다.
  필요성: 여섯째 항목이 화면에서 잘리거나 완료 수에서 빠지지 않아야 한다.

고정 sleep·실패 재시도·시한 연장·기대값 완화는 추가하지 않는다.

## 검증 결과

- proto TypeScript 생성 계약 2개 통과.
- 웹 타입·번들 빌드와 유닛 651개 통과.
- host 요약 관련 10개 통과(실행 0.01초), 실제 보고 수집 keeper 1개 통과(0.67초).
- 웹 E2E28 전체 통과(30.5초). 전체 진행 목록의 네 단계는 desktop 2.070초·Chromium320 0.839초·WebKit390 1.557초·전체 완료 0.109초로 모두 5초 이하다.
- host·proto all-targets strict clippy, fmt, 공개 저장소 검사와 diff 공백 검사 통과.
- Playwright npm 1.64.1은 작업 폴더에 두고 기존 공용 브라우저 캐시를 썼다. 전역 도구·시스템 설정은 바꾸지 않았다.
- 전체 ci-local·병합·push는 착지 창을 받은 뒤 진행한다.

## 참고

기존 참고 후보인 [herdr v0.9.5 sidebar](https://github.com/herdrdev/herdr/blob/v0.9.5/src/client/shell/sidebar.rs)
([Apache-2.0](https://github.com/herdrdev/herdr/blob/v0.9.5/LICENSE))와
[herdr-projects v0.2.35 progress](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/progress.rs)
([MIT](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE))를 확인했다.
상태 표시와 진행 보고를 구별하는 기존 구조를 유지하고 aaa의 목록 검증 상한만 늘린다.
외부 코드·실행 의존성을 가져오지 않는다.
