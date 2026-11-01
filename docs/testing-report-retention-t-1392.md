# 완료 보고의 과거 판 정리(t-1392)

## 바뀐 계약

q-2134의 1A에 따라 끝난 thread의 최신 이름 사본과 최신 r 판, 완료 할 일의 본문·이력에 직접 첨부한 특정 r 판을 자동 정리에서 보호한다.
완료 할 일이 참조하는 다른 이름의 보고서도 최신 이름·최신 판은 남긴다.
참조하지 않은 과거 판은 기한·용량 정리 대상이다. 미완료 작업의 모든 판·실시간 등록·보관함 고정 판·읽지 못한 자료의 보호는 유지한다.
보고서를 모든 판과 함께 영구 삭제하는 동작은 참조 자료를 없애므로 기존대로 거부한다.

원인은 보고서 전체 영구 삭제를 막는 목록의 `busy`가 각 과거 판의 정리 보호에도 전파되던 것이다.
`library_inventory`에서 완료 참조를 최신 이름·최신 판·명시한 판의 집합으로 만들고, 미완료 thread 여부는 출처마다 한 번 읽는다.
화면 용량과 정리 미리보기는 같은 inventory를 사용한다.

## 회귀 재현

기존 `archived_r_attachment_protects_its_entire_report_family_from_delete_and_prune`를 새 계약의
`archived_r_attachment_protects_exact_version_and_latest_but_prunes_unreferenced_history`로 바꿨다.
수정 전 코드에서 참조한 옛 판·참조하지 않은 중간 판·최신 판을 만들면 정리 결과가 빈 목록이어서 실패했다.
수정 뒤에는 참조한 옛 판과 최신 결과를 읽을 수 있고 중간 판만 정리된다. 최신 manifest가 없어진 경우도 확인한다.
필요한 이유: 특정 판의 참조가 다른 과거 판까지 보호해 상한 정리가 멈추는 회귀를 막는다.

`completed_thread_prunes_old_versions_below_capacity_and_preserves_history_refs_after_restart`를 추가했다.
실제 host·DB·사본·manifest로 1MiB 상한을 넘긴 뒤 미완료 상태에서는 모든 판이 보호되고,
완료 상태에서는 참조하지 않은 판만 미리보기와 같은 목록으로 지워지는지 확인한다.
완료 할 일을 복원한 뒤에도 이력의 thread·r 첨부, 최신 이름과 최신 판의 실제 내용이 유지되고,
재시작 뒤에도 보존되며 보호 용량과 화면 집계가 일치해야 한다.
완료 보관 참조가 아직 없어도 최신 결과는 보호하고 두 과거 판만 기한 정리 후보에 들어가는지도 확인한다.
필요한 이유: 보호 합계가 상한을 잘못 넘겨 삭제 후보가 사라지는 실제 정리 동작과 결과 보존을 함께 검증한다.

두 시험 모두 시각을 인자로 주고 실제 응답으로 끝낸다. 고정 sleep·재시도·시한 연장은 없다.

기존 `library_links`의 기한·용량 정리 시험은 삭제 가능한 사본에 `copy-` 이름을 쓴다.
최신 thread 결과를 계속 삭제 대상으로 두는 옛 fixture를 새 계약에 맞춰 바꿨으며, 기한 경계·순서·6시간 주기·읽기 전용 미리보기·링크 만료 단언은 유지한다.
`prune_uses_publication_age_and_protects_live_sources`는 완료 보관 참조가 없는 최신 thread 사본과 본문을 함께 남기는 단언으로 바꿨다.
`retention_protected_overflow_only_notifies_once`는 참조가 없는 완료 thread의 최신 결과만으로 상한을 넘겨도 해당 프로젝트에 한 번만 알리고 재시작 뒤 중복 알림을 만들지 않는지 검사한다.

## 관련 owner 검사

```sh
nice -n 15 cargo test --profile verify --locked -p aaa-host --lib -- report_ids::tests:: library_links::tests:: --test-threads=1
nice -n 15 cargo clippy --profile verify --locked -p aaa-host --all-targets -- -D warnings
cargo fmt --all -- --check
bash scripts/check-public.sh
```

보고 판·정리·공유 링크 owner 56개가 통과했다. 시험 실행 합계는 2.23초이며 고정 sleep은 없다.
관련 aaa-host의 모든 target에 대한 strict Clippy와 fmt·공개 저장소 검사도 통과했다.

착지 gate는 착지 창에서 최종 통합 SHA에 실행한다. 운영 자료 삭제·배포는 이 개발 검사에 포함하지 않는다.
