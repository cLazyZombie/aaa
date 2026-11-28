# 할 일 검수 모델과 계정 검증

- `task_edit::tests::review_patch_round_trips_and_retains_other_fields`는 옛 할 일의 기본값, 각 칸의 독립적인 초기화, CRLF와 기존 메모 보존을 확인한다.
- `task_queue::tests::requested_review_choice_updates_pending_approval_and_logs_user_reason`과 `review_account_changes_keep_the_reserved_implementation_choice`는 coordinator 보호, 사용자 지시 기록, 예약이 승인한 구현 모델과 차례 보존을 확인한다.
- `account_rename::tests::rename_reencrypts_preserves_identity_and_moves_only_account_references`는 계정 별칭 이전 뒤 시작한 thread와 할 일에 새 별칭이 남고, 미확정 완료 보관을 복원할 수 있음을 확인한다.
- `m13::review::remote_review_is_pinned_owned_and_cancellable`은 CLI에서 두 값을 저장하고 thread 정보와 brief에서 읽는다. 원격 agent의 검수 실행기가 기본 계정 대신 지정 계정의 인증을 받는지 대조한다.
- `145-task-review-settings.mjs`는 데스크톱 Chromium과 휴대폰 WebKit에서 실제 host에 검수 설정을 저장하고, 편집 화면을 다시 열고, 각 칸을 기본값으로 돌린다.
