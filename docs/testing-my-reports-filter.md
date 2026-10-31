# 내 보고서 자동 등록 기준(t-1361)

## 원인과 변경

`library_archive::complete_thread_report`가 완료 표지가 없어도 thread 상태가 `Resolved`면 마지막 보고를
사용자 보고로 등록했다. 같은 함수가 회수·resolve·host 시작 때의 기존 사본 복구에서 호출돼,
진행 중 보고를 남긴 thread를 종료하면 「내 보고서」에 안 봄으로 올라왔다.

일반 thread의 등록 기준은 기존 `report_mark::done` 또는 `report_mark::needs_user`다.
`[완료]`·`[알림]`·`[결정 필요]`·`[검수 대기]`·`[확인 필요]`·옛 `[사람 필요]`를 허용한다.
진행·착지·병합 준비·보류 준비·표지 없는 보고는 종료 여부와 관계없이 작업 보고로 남긴다.
자동화의 새 결함·질문 카드 필터, 명시적 `--to-user`, 원본 사본과 보관한 r 판은 유지한다.
기존 자동 등록 원장이 있는 진행 보고는 작업 보고로 돌리고, 시작 때 본문 hash가 기록과 다른 사본은 분류하지 않는다.
마지막 사용자 보고의 판·열람 기록은 남겨 같은 완료 본문으로 돌아와도 다시 안 봄으로 바꾸지 않는다.

## 필요한 회귀 시험

- `orch::report_tests::working_reports_stay_in_work_reports_after_resolve_and_restart`:
  실제 회수·종료·DB 다시 열기에서 진행 보고와 중간 작업 단계, 표지 없는 보고가 사용자 목록·안 봄 수에 섞이지 않는 계약.
- `orch::report_tests::user_attention_reports_are_listed_before_resolve_and_keep_seen_on_restart`:
  완료·사용자 판단 표지가 종료 전에 등록되고, 재수집·종료·재시작으로 읽음을 잃지 않는 계약.
- `orch::report_tests::backfill_moves_automatically_listed_progress_to_work_reports_and_preserves_archive`:
  구판 오등록 복구, 원본·보관 판 보존, 완료 뒤 진행 보고로 전환하는 계약.
- 기존 `automation_reports_reach_my_reports_only_with_an_alert`에 검수 대기·질문 절이 남은 진행 보고를 추가해 자동화 예외를 보존한다.
- `m13::report_events::resolving_a_working_report_keeps_it_out_of_my_reports`:
  실제 host·agent·CLI를 거친 회수와 종료에서 웹이 받는 보고 목록의 `to_user`·`unseen`을 확인한다.

가짜 agent 응답·보고 회수 사건·resolve 응답으로 끝낸다. 고정 sleep·재시도·시한 연장·기대값 완화는 없다.
기존에 잘못된 동작을 기대하던 `resolved_last_report_and_existing_completed_reports_are_automatically_listed`를
첫 시험으로 대체한다. 종료만으로 진행 보고를 사용자에게 올려야 한다는 기대를 제거하고 실제 종료 경계까지 검증한다.

## 참고 후보와 라이선스

- [Herdr Projects ticker](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/ticker.rs),
  [MIT](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE):
  보고 회수·thread 전이·사용자에게 필요한 상태를 구별하는 기존 구조를 참고했다. 종료만으로 보고를 올리는 기준은 채택하지 않았다.
- [GitLab To-Do List](https://docs.gitlab.com/user/todos/),
  [라이선스](https://github.com/gitlabhq/gitlabhq/blob/master/LICENSE)(제품 코드 MIT, 문서 CC BY-SA 4.0):
  사용자 입력이 필요한 항목을 작업 전체 활동과 구별하는 제품 기준을 참고했다.

외부 코드·의존성은 추가하지 않았다.

## 검증 결과

개발 owner 검증은 모두 통과했다. 모든 Cargo 호출은 `nice -n 15`, `--profile verify --locked`를 사용했다.

| 검사 | 결과 |
| --- | --- |
| `cargo test -p aaa-host --lib orch::report_tests::` | 22개, 전체 실행 1.96초 |
| `cargo test -p aaa-host --lib library_links::tests::` | 30개, 전체 실행 1.53초 |
| `cargo test -p aaa-host --lib report_ids::tests::` | 25개, 전체 실행 1.35초 |
| `cargo test -p aaa-host --lib guide::tests::` | 필터 일치 4개, 전체 실행 0.00초 |
| `cargo test -p aaa --test m13 report_events::resolving_a_working_report_keeps_it_out_of_my_reports -- --exact` | 1개, 2.14초 |
| `cargo clippy -p aaa-host --all-targets -- -D warnings` | 통과 |
| `cargo fmt --all -- --check`·`git diff --check` | 통과 |
| `bash scripts/check-public.sh` | 통과 |

위 시간은 시험 실행 부분이며 컴파일을 포함하지 않는다. 새 시험을 포함한 각 owner 묶음 전체가 5초 미만이다.

원래 `library_archive.rs`를 잠시 적용한 RED 대조에서도 새 시험 두 개가 실제 잘못된 동작에서 실패했다.
진행 보고 시험은 resolve 뒤 `to_user=true, unseen=true`에서 실패했고, 사용자 확인 보고 시험은
`[결정 필요]` 보고를 종료 전에 등록하지 못해 실패했다. 후자의 의도한 단언 실패 뒤 기존 실험실 destructor의
Host 해제 단언도 연쇄 실패했다. 제품 코드 원복 뒤 같은 시험이 통과했다.
시험 작성 중 enum 이름을 `ResolveWhy::User`로 잘못 써 컴파일 오류가 났으며 기존 `ResolveWhy::Manual`로 수정했다.
시한·기대값을 바꿔 통과시키거나 실패를 재시도로 덮지 않았다.

기록 원본은 작업 전용 `target/verification/t-1361/`에 있다.
착지 gate는 승인된 착지 창의 최종 통합 SHA에서 실행한다. 이 개발 확인은 전체 gate·양 OS cross·Windows 전달 검증을 대신하지 않는다.
