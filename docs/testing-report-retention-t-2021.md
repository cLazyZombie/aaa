# 종료한 작업의 보고 사본 자동 정리(t-2021)

t-1392에서 끝난 작업의 최신 이름과 최신 r 판을 무조건 보호했다. 완료 보관의 작업 ID도 첨부 참조처럼 처리했다.
쌓인 보호 사본만으로 합계 상한을 넘으면 기한이 지난 사본까지 정리할 수 없었다.

종료 처리가 끝난 작업의 일반 사본은 기한과 합계 상한을 따른다. 최신 이름과 최신 r 판은 한 묶음으로 정리한다.
완료 할 일의 본문이나 이력에서 실제로 첨부한 보고서는 보호한다. 종료 처리 중과 다시 연 작업의 사본도 보호하며 보관함에 고정한 판은 일반 합계 밖이다.
자동 정리 후에도 끝난 작업의 회수 보고 본문과 완료 기록은 남는다.

## 회귀 검증

- `completed_thread_prunes_old_versions_below_capacity_and_preserves_history_refs_after_restart`는 실제 DB와 사본에서 최신 이름과 최신 r 판까지 후보로 확인한다. 수정 전에는 과거 두 판만 후보여서 실패했다. 종료 처리 중에는 모든 판이 보호되며 작업 ID와 보고 경로를 완료 기록에 추가해도 후보는 유지된다. 이력에 실제 r 판을 첨부하면 참조 판과 최신 묶음은 남고 참조하지 않은 과거 판만 정리된다.
- `prune_uses_publication_age_and_protects_live_sources`와 `changed_retention_limits_apply_on_the_next_pass_and_preserve_protected_reports`는 끝난 작업의 사본을 정리한 뒤 보고 본문, 진행 중 사본, 실시간 원본, 보관함을 확인한다. 시각은 인자로 주입한다.
- `retention_protected_overflow_only_notifies_once`는 진행 중 작업의 보호 사본이 합계 상한을 넘을 때 삭제를 멈추고 알림을 한 번 내는지 확인한다.
- 완료 보관과 복원 시험은 본문과 옛 이력의 명시 첨부가 여전히 보호되는지 확인한다.

```sh
aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa-host --lib -- report_ids::tests:: library_links::tests:: task_queue::tests:: orch::report_tests::
```

운영 미리보기는 읽기 전용으로 확인했다. 작업 상태·보고 판·보관 목록과 파일의 논리 크기·시각을 격리한 실험실에서 재현해 현재 바이너리와 수정 바이너리의 `aaa library prune --dry-run`을 비교했다. 착지 gate는 승인 뒤 최종 통합 후보에서 실행한다.

## 참고 소스

고정 소스에서 작업 종료를 명시적인 상태로 기록하는 herdr-projects, pane 닫기를 별도 동작으로 다루는 herdr, 터미널 닫힘 사건을 소비하는 herdr-web-ui를 확인했다. aaa에서는 작업 원장의 상태와 종료 처리 완료 여부로 보고서 정리 자격을 판단한다. 외부 코드는 가져오지 않았다. [herdr-projects(MIT)](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/threads.rs), [herdr(Apache-2.0)](https://github.com/herdrdev/herdr/blob/v0.9.5/src/app/api/panes.rs), [herdr-web-ui(MIT)](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/server/watch.ts)
