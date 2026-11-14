# CLI resolve의 미처리 후속 차단(t-1851)

## 동작과 범위

host의 수동 resolve가 실제 CLI 연결(`local:` actor)에서 왔으면, 저장된 후속 원장의 미처리 항목을
먼저 검사한다. `정리: worktree와 branch를 지운다`에서 앞뒤 공백·콜론 뒤 공백·끝 `. 。 !`만 정규화한 항목을 제외하며 다른 정리 문구나
실제 후속은 거부한다. 거부 오류에는 각 후속 hash·문구와 `--task d-NNNN`·`--reason-file` 예시가 있다.
이미 아는 후속이면 마지막 사본·기동 복구·상태 변경·세션 종료·Git 정리 전에 돌아온다.

마지막 사본을 수집해서 새 후속을 발견했을 때도 종료 전에 다시 검사한다. 이 경우 보고 사본·원장은
최신으로 수집하지만 실행 자리와 할 일은 끝내지 않는다. 보고 수집 잠금을 종료 상태 저장까지 유지해
수집과 종료 사이의 후속 검사가 다른 보고 수집에 추월당하지 않게 한다.

사람의 웹 직접 끝내기에는 기존 「후속 남음」 카드를 유지한다. CLI 우회 옵션은 추가하지 않았다.
자동화 시작 출처·빈 Next·보고 없는 실패는 미처리 후속이 없으면 막지 않는다. `--reopen`과
`--dry-run`은 기존 경로이며 `--skip-copy`는 후속 검사를 우회하지 않는다. FRONT_API 변경은 없다.

후속 할 일은 같은 프로젝트의 활성 목록·완료 보관에서 확인한다. d-ID는 기존 번호표를 풀고,
긴 ID는 해당 프로젝트의 실제 할 일과 직접 대조한다. 처리 원장에는 항상 d-ID로 저장한다.

## 검증

모든 검증 명령은 `aaa verify -- <명령>`으로 실행한다.

- host의 CLI 차단·처리 뒤 종료·최종 사본 차단·빈 Next/자동화/실패 경계 시험: 최초 3개 통과.
- 실제 CLI 통합 `resolve_followups::cli_resolve_requires_followups_before_changing_the_thread`: 통과.
  오류 종료와 hash·문구·처리 예시, thread 상태·pane·Git HEAD·dirty 파일·TASKS.md 보존,
  확인 필요 카드 미생성, 긴 ID 연결 뒤 d-ID 저장과 resolve 성공을 확인한다.
- 첫 host 전체 시험: 916개 통과·2개 무시·신규 fixture 1개 실패. host 하나에 프로젝트를 둘 만들려 한
  시험 구성 오류였다. 다른 프로젝트의 실제 할 일을 별도 host에서 만들도록 수정했다.
- 기존 정리 전용 자동 완료 시험은 CLI actor로 실행하고, 다른 후속이 섞인 직접 종료 시험은
  웹 actor로 실행해 양쪽 경계를 유지한다.

- 첫 전체 ci-local: web·fmt·build·public 통과. Rust 실행 단위 348개 중 두 fixture가 실패했다.
  후속이 없는 adopted 응답의 `followups` 생략을 도우미가 허용하지 않았고, 왕복 시험의 정확한 사건 목록에
  이번에 추가한 명시적 후속 처리 사건 두 개가 빠져 있었다. 빈 원장을 허용하고 원래 다섯 사건의 순서를
  유지한 채 처리 사건 두 개까지 정확히 단언하도록 고쳤다.
- 첫 전체 브라우저 실행에서는 spec 23·53·115·121·123·130의 CLI 종료가 공통 가짜 보고의 미처리 후속에
  막혔다. 기존 CLI 종료 준비에 `finishFixtureFollowups`를 명시적으로 추가했다. 각 owner의 화면·응답·시간
  단언은 유지하며, 제품의 검사나 원장의 후속을 시험 전용 우회로 건너뛰지 않는다.
- 차단 함수를 제거한 변형에서 신규 실제 CLI 통합 시험은 「미처리 후속은 CLI 오류 종료」 단언으로 실패했다.
  제품 소스를 원래 바이트와 SHA-256으로 대조해 복원했다. 원시 기록은 `target/t-1851-mutation.log`에 있다.

수리 뒤 Rust 실행 단위 348개·Python 전체와 strict Clippy(`aaa-host`·`aaa`, all-targets)는 통과했다.
브라우저는 53개 작업 중 공유 질문 도우미의 종료 준비 한 곳이 빠진 spec 53만 실패했다. 그 도우미와
같은 방식으로 종료하는 측정 도구도 후속을 명시적으로 처리하도록 맞췄다. Rust 소스는 이 수리에서 바뀌지 않았다.
수리 뒤 웹 유닛 720개·브라우저 53개 작업 전체·공개 검사도 통과했다. 착지 창의 최종 통합 SHA gate와 배포는 별도다.

## 참고한 공개 구현

- [Git branch](https://git-scm.com/docs/git-branch)의 미병합 branch 기본 삭제 거부처럼,
  처리 전제조건을 만족하지 못하면 실행 자리 변경 전에 오류로 끝내는 방식을 따랐다.
  [라이선스](https://github.com/git/git/blob/master/COPYING)는 GPL-2.0이며 코드를 가져오지 않았다.
- [GitHub CLI close](https://cli.github.com/manual/gh_pr_close)의 명시적인 종료와 사유 기록을 비교했다.
  [라이선스](https://github.com/cli/cli/blob/trunk/LICENSE)는 MIT다. aaa에서는 별도 강제 종료 옵션을
  추가하지 않고 기존 후속 원장의 사유와 웹의 직접 끝내기를 사용한다. 외부 코드는 가져오지 않았다.

## 정리 전용 문구의 표기 차이 추가

사용자가 관찰한 끝 마침표 사례를 반영했다. 앞뒤 공백·`정리:` 뒤 공백과 끝의 `.`, `。`, `!`만
무시한다. 본문의 다른 공백·대상·행동·물음표는 그대로 구별한다. 정리 자동 완료와 CLI 거부 예외가
같은 `is_resolve_cleanup`을 사용한다. 원장의 원문·hash는 바꾸지 않는다.

판정 유닛은 허용 5개·거부 6개를 비교한다. 실제 host의 정리 완료 keeper도 표준·마침표·공백 없는
콜론/일본어 마침표·공백/느낌표 네 보고로 실행해 CLI 종료 성공·처리 영수증·할 일 자동 보관과
확인 필요 알림 미생성을 함께 확인한다. 전체 gate는 이 추가 범위를 반영한 소스에서 다시 실행해 통과했다.

## 최종 결과

추가된 정리 문구 정규화까지 포함한 macOS 전체 ci-local과 workspace strict Clippy가 모두 통과했다.

| 검사 | 결과 |
| --- | --- |
| 웹 타입·번들·유닛 | 720개 통과 |
| fmt·전체 빌드 | 통과 |
| Rust 전체 | 실행 단위 348개 통과 |
| Python 검사 | 통과 |
| Chromium·WebKit E2E 전체 | 42개 spec, 독립 작업 53개 모두 통과 |
| workspace all-targets strict Clippy | 통과 |
| 공개 검사 | 통과 |

최종 실행은 `aaa verify -- nice -n 15 env NODE_PATH=<작업 전용 Playwright 1.64.1 경로>
bash scripts/ci-local.sh --only web,fmt,build,clippy,test,e2e,public`이며 약 197초였다.
전역 npm과 브라우저 캐시는 바꾸지 않았다. 최종 원시 증거는 `target/ci-local/t-1851-final/`에 있다.
미커밋 후보의 실행은 workspace 검증 원장 자동 집계 대상이 아니므로 이 기록에 직접 결과를 남긴다.
착지 창의 최종 통합 SHA gate와 병합·배포는 별도다.
