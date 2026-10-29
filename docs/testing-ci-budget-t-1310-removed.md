# t-1310에서 삭제한 검사

웹 E2E 102개 파일·515개 단계와 아래 Rust 통합 owner를 삭제했다.

웹의 남긴 검사 열은 일부 관련 규칙의 기존 owner다. 정확한 대체가 없는 부분도 있다. 삭제한 화면의 CSS·장치별 실제 렌더와 전체 사용자 흐름까지 보장하지 않는다. 고유 프로세스·권한·대량 경계는 [측정 기록](testing-ci-budget-t-1310.md)에 따로 적었다.

## 웹 E2E 파일

모든 삭제 파일은 `web/e2e/specs/` 아래다. 화면 모델·파서·표시 규칙의 반복과 낮은 우선순위 화면 검사를 줄였다.

| 삭제 파일 | 단계 수 | 남긴 빠른 검사 |
| --- | ---: | --- |
| `03-keys.mjs` | 8 | [keyRepeat](../web/test/keyRepeat.test.mjs) · [mobile-composition](../web/test/mobile-composition.test.mjs) · [pastedContent](../web/test/pastedContent.test.mjs) |
| `04-app-shell.mjs` | 9 | [idleLease](../web/test/idleLease.test.mjs) · [sidebarPanel](../web/test/sidebarPanel.test.mjs) |
| `06-settings.mjs` | 16 | [preferences](../web/test/preferences.test.mjs) · [settingsModel](../web/test/settingsModel.test.mjs) · [terminalTheme](../web/test/terminalTheme.test.mjs) |
| `08-input-line.mjs` | 2 | [composerRecall](../web/test/composerRecall.test.mjs) · [mobile-composition](../web/test/mobile-composition.test.mjs) |
| `10-soft-keyboard.mjs` | 2 | [composition-anchor](../web/test/composition-anchor.test.mjs) · [mobile-composition](../web/test/mobile-composition.test.mjs) |
| `11-agent-panes.mjs` | 12 | [paneLens](../web/test/paneLens.test.mjs) · [stateLine](../web/test/stateLine.test.mjs) · [toolGroups](../web/test/toolGroups.test.mjs) |
| `14-shared-preferences.mjs` | 3 | [preferences](../web/test/preferences.test.mjs) |
| `15-ios-rules.mjs` | 4 | [preferences](../web/test/preferences.test.mjs) · [stateTone](../web/test/stateTone.test.mjs) |
| `16-file-viewer.mjs` | 6 | [sourcePreview](../web/test/sourcePreview.test.mjs) · [filePreviewText](../web/test/filePreviewText.test.mjs) · [highlight](../web/test/highlight.test.mjs) |
| `17-push-in-use.mjs` | 5 | [stateTone](../web/test/stateTone.test.mjs) · [idleLease](../web/test/idleLease.test.mjs) |
| `18-push-stalled.mjs` | 1 | [idleLease](../web/test/idleLease.test.mjs) |
| `19-turn-alerts.mjs` | 2 | [stateTone](../web/test/stateTone.test.mjs) · [stateLine](../web/test/stateLine.test.mjs) |
| `20-terminal-queries.mjs` | 1 | [vtWidths](../web/test/vtWidths.test.mjs) |
| `21-library-reports.mjs` | 6 | [reportLinks](../web/test/reportLinks.test.mjs) · [reportUpload](../web/test/reportUpload.test.mjs) |
| `22-chat-surface.mjs` | 6 | [appRef](../web/test/appRef.test.mjs) · [chatScroll](../web/test/chatScroll.test.mjs) |
| `24-sidebar-membership.mjs` | 5 | [workspaceSelection](../web/test/workspaceSelection.test.mjs) · [roster](../web/test/roster.test.mjs) |
| `25-ctrl-enter.mjs` | 7 | [receivedInput](../web/test/receivedInput.test.mjs) · [messageQueue](../web/test/messageQueue.test.mjs) |
| `26-activity-bar.mjs` | 7 | [sidebarPanel](../web/test/sidebarPanel.test.mjs) · [sidebarWidth](../web/test/sidebarWidth.test.mjs) · [sidebarOrder](../web/test/sidebarOrder.test.mjs) |
| `27-machine-stats.mjs` | 3 | [machineStats](../web/test/machineStats.test.mjs) |
| `28-thread-summary.mjs` | 28 | [summaryCard](../web/test/summaryCard.test.mjs) · [stateTone](../web/test/stateTone.test.mjs) |
| `29-scroll-hold.mjs` | 3 | [chatScroll](../web/test/chatScroll.test.mjs) |
| `30-chat-reading.mjs` | 2 | [chatScroll](../web/test/chatScroll.test.mjs) · [retainedChats](../web/test/retainedChats.test.mjs) |
| `30-thread-preparation.mjs` | 1 | [chatScroll](../web/test/chatScroll.test.mjs) · [retainedChats](../web/test/retainedChats.test.mjs) |
| `31-render-guards.mjs` | 3 | [retainedChats](../web/test/retainedChats.test.mjs) · [offThread](../web/test/offThread.test.mjs) |
| `32-usage-limit.mjs` | 2 | [usageProblem](../web/test/usageProblem.test.mjs) |
| `33-app-badge.mjs` | 1 | [appBadge](../web/test/appBadge.test.mjs) |
| `34-automations.mjs` | 6 | [automationModel](../web/test/automationModel.test.mjs) · [automationForm](../web/test/automationForm.test.mjs) |
| `35-composer-send-height.mjs` | 2 | [receivedInput](../web/test/receivedInput.test.mjs) · [rolloutStatus](../web/test/rolloutStatus.test.mjs) |
| `35-rollout.mjs` | 2 | [receivedInput](../web/test/receivedInput.test.mjs) · [rolloutStatus](../web/test/rolloutStatus.test.mjs) |
| `36-thread-numbers.mjs` | 1 | [appIds](../web/test/appIds.test.mjs) |
| `38-chat-report-close.mjs` | 3 | [reportLinks](../web/test/reportLinks.test.mjs) · [recentHistory](../web/test/recentHistory.test.mjs) |
| `40-automation-detail.mjs` | 3 | [automationModel](../web/test/automationModel.test.mjs) |
| `41-chat-tool-groups.mjs` | 2 | [toolGroups](../web/test/toolGroups.test.mjs) |
| `42-thread-diff.mjs` | 9 | [diffTree](../web/test/diffTree.test.mjs) · [diffContext](../web/test/diffContext.test.mjs) · [sourcePreview](../web/test/sourcePreview.test.mjs) |
| `43-tasks.mjs` | 6 | [tasksModel](../web/test/tasksModel.test.mjs) · [taskSortModel](../web/test/taskSortModel.test.mjs) |
| `44-chat-find.mjs` | 5 | [chatFind](../web/test/chatFind.test.mjs) |
| `45-compacting.mjs` | 4 | [contextUsage](../web/test/contextUsage.test.mjs) |
| `46-task-edit.mjs` | 18 | [tasksModel](../web/test/tasksModel.test.mjs) |
| `47-task-thread.mjs` | 9 | [tasksModel](../web/test/tasksModel.test.mjs) · [taskArchiveModel](../web/test/taskArchiveModel.test.mjs) · [taskSortModel](../web/test/taskSortModel.test.mjs) |
| `48-automation-threads.mjs` | 5 | [automationModel](../web/test/automationModel.test.mjs) |
| `49-automation-memory.mjs` | 3 | [automationModel](../web/test/automationModel.test.mjs) |
| `50-context-gauge.mjs` | 7 | [contextUsage](../web/test/contextUsage.test.mjs) |
| `51-slash-commands.mjs` | 9 | [slashCommands](../web/test/slashCommands.test.mjs) |
| `52-task-attach.mjs` | 10 | [taskAttach](../web/test/taskAttach.test.mjs) |
| `54-task-repo.mjs` | 4 | [tasksModel](../web/test/tasksModel.test.mjs) |
| `55-automation-edit.mjs` | 9 | [automationForm](../web/test/automationForm.test.mjs) |
| `56-operate.mjs` | 10 | [operateModel](../web/test/operateModel.test.mjs) |
| `57-legacy-special-panes.mjs` | 1 | [roster](../web/test/roster.test.mjs) |
| `58-accounts.mjs` | 8 | [accountsModel](../web/test/accountsModel.test.mjs) |
| `59-report-images.mjs` | 3 | [reportImages](../web/test/reportImages.test.mjs) |
| `60-admin.mjs` | 11 | [adminModel](../web/test/adminModel.test.mjs) |
| `61-settings-admin.mjs` | 8 | [settingsModel](../web/test/settingsModel.test.mjs) · [adminModel](../web/test/adminModel.test.mjs) |
| `62-asks-close.mjs` | 6 | [asksModel](../web/test/asksModel.test.mjs) |
| `63-deploy.mjs` | 6 | [deployModel](../web/test/deployModel.test.mjs) · [rolloutStatus](../web/test/rolloutStatus.test.mjs) |
| `64-automation-failure.mjs` | 2 | [automationModel](../web/test/automationModel.test.mjs) |
| `65-account-quota.mjs` | 7 | [accountQuotaModel](../web/test/accountQuotaModel.test.mjs) |
| `66-composer-wrap.mjs` | 3 | [composerRecall](../web/test/composerRecall.test.mjs) · [mobile-composition](../web/test/mobile-composition.test.mjs) |
| `67-task-queue.mjs` | 4 | [tasksModel](../web/test/tasksModel.test.mjs) |
| `68-safe-mode.mjs` | 4 | [stateLine](../web/test/stateLine.test.mjs) · [errorDiagnostic](../web/test/errorDiagnostic.test.mjs) |
| `70-task-ids.mjs` | 3 | [appIds](../web/test/appIds.test.mjs) |
| `71-report-fetch.mjs` | 1 | [apiRequest](../web/test/apiRequest.test.mjs) · [reportUpload](../web/test/reportUpload.test.mjs) |
| `73-report-ids.mjs` | 7 | [appIds](../web/test/appIds.test.mjs) |
| `74-question-review.mjs` | 2 | [reviewNotes](../web/test/reviewNotes.test.mjs) |
| `75-task-owners.mjs` | 2 | [tasksModel](../web/test/tasksModel.test.mjs) |
| `76-task-one-list.mjs` | 8 | [taskSortModel](../web/test/taskSortModel.test.mjs) |
| `77-app-ids.mjs` | 6 | [appIds](../web/test/appIds.test.mjs) · [appRef](../web/test/appRef.test.mjs) |
| `78-account-chat.mjs` | 2 | [accountsModel](../web/test/accountsModel.test.mjs) |
| `80-report-list.mjs` | 1 | [reportLinks](../web/test/reportLinks.test.mjs) |
| `81-account-pool.mjs` | 7 | [accountsModel](../web/test/accountsModel.test.mjs) · [accountQuotaModel](../web/test/accountQuotaModel.test.mjs) |
| `82-cli-zero.mjs` | 10 | [appIds](../web/test/appIds.test.mjs) · [appRef](../web/test/appRef.test.mjs) |
| `83-cli-zero-gaps.mjs` | 3 | [appIds](../web/test/appIds.test.mjs) |
| `84-connection-banner.mjs` | 2 | [errorDiagnostic](../web/test/errorDiagnostic.test.mjs) |
| `85-thread-park.mjs` | 4 | [retainedChats](../web/test/retainedChats.test.mjs) |
| `86-task-history.mjs` | 5 | [taskArchiveModel](../web/test/taskArchiveModel.test.mjs) |
| `87-body-scroll.mjs` | 10 | [chatScroll](../web/test/chatScroll.test.mjs) · [sidebarPanel](../web/test/sidebarPanel.test.mjs) |
| `88-report-video.mjs` | 6 | [reportMediaProbe](../web/test/reportMediaProbe.test.mjs) |
| `89-pane-responsibility.mjs` | 1 | [stateTone](../web/test/stateTone.test.mjs) |
| `90-workspace-metrics.mjs` | 7 | [retentionProtocol](../web/test/retentionProtocol.test.mjs) |
| `91-coordinator-exit.mjs` | 1 | [stateLine](../web/test/stateLine.test.mjs) |
| `92-task-after.mjs` | 3 | [tasksModel](../web/test/tasksModel.test.mjs) |
| `94-notice-input.mjs` | 8 | [offThread](../web/test/offThread.test.mjs) |
| `95-github-dark.mjs` | 1 | [palette](../web/test/palette.test.mjs) · [theme](../web/test/theme.test.mjs) |
| `96-theme-gallery.mjs` | 4 | [palette](../web/test/palette.test.mjs) · [theme](../web/test/theme.test.mjs) · [terminalTheme](../web/test/terminalTheme.test.mjs) |
| `98-machine-info.mjs` | 6 | [machineIdentity](../web/test/machineIdentity.test.mjs) · [machineDetail](../web/test/machineDetail.test.mjs) |
| `101-reports-mark-read.mjs` | 1 | [reportLinks](../web/test/reportLinks.test.mjs) |
| `102-inbox-composer.mjs` | 9 | [messageQueue](../web/test/messageQueue.test.mjs) |
| `104-chat-lifecycle.mjs` | 5 | [retainedChats](../web/test/retainedChats.test.mjs) · [retentionTimers](../web/test/retentionTimers.test.mjs) · [retentionFailure](../web/test/retentionFailure.test.mjs) |
| `105-recent-screens.mjs` | 6 | [recentHistory](../web/test/recentHistory.test.mjs) · [recentScreens](../web/test/recentScreens.test.mjs) |
| `106-automation-v2.mjs` | 2 | [automationModel](../web/test/automationModel.test.mjs) |
| `107-task-row-layout.mjs` | 1 | [taskSortModel](../web/test/taskSortModel.test.mjs) |
| `108-report-repos.mjs` | 8 | [reportRepos](../web/test/reportRepos.test.mjs) |
| `112-short-id-boundaries.mjs` | 2 | [appIds](../web/test/appIds.test.mjs) |
| `115-chat-send-feedback.mjs` | 5 | [receivedInput](../web/test/receivedInput.test.mjs) · [inputReceipt](../web/test/inputReceipt.test.mjs) |
| `116-task-queue-retirement.mjs` | 3 | [tasksModel](../web/test/tasksModel.test.mjs) |
| `117-selection-quote.mjs` | 3 | [selectionQuote](../web/test/selectionQuote.test.mjs) |
| `118-record-search.mjs` | 4 | [chatFind](../web/test/chatFind.test.mjs) · [reportLinks](../web/test/reportLinks.test.mjs) |
| `119-auto-merge.mjs` | 5 | [tasksModel](../web/test/tasksModel.test.mjs) · [taskSortModel](../web/test/taskSortModel.test.mjs) |
| `120-review-notes.mjs` | 3 | [reviewNotes](../web/test/reviewNotes.test.mjs) |
| `121-navigation-shortcuts.mjs` | 1 | [recentHistory](../web/test/recentHistory.test.mjs) |
| `122-ideas.mjs` | 4 | [ideas](../web/test/ideas.test.mjs) |
| `123-chat-chip-answer.mjs` | 4 | [questionAnswer](../web/test/questionAnswer.test.mjs) |
| `124-idea-recent-screens.mjs` | 4 | [recentScreens](../web/test/recentScreens.test.mjs) · [ideas](../web/test/ideas.test.mjs) |

예외와 남긴 통합 경계: E12의 HTTP·보고서 sandbox/권한/페이지화, E23의 실제 첨부·대기 전송·iPhone 경합, E39의 기록 이동, E53·69의 실제 질문 답, E99의 작업 폴더 권한을 계속 실행한다.

## 남긴 웹 파일에서 삭제한 반복

| 파일 | 줄인 반복 | 유지한 계약 |
| --- | --- | --- |
| E07 | 글꼴·스크롤/뷰포트 조합 등 6단계 | 실제 출력 backpressure·재시작 후 6,000줄 history |
| E09 | Chromium iPad 중복·입력 진단 UI | 실제 IME·iPhone 기록·화면 키보드 WebKit iPad |
| E12 | 줄 배치·정렬·편의 메뉴 등 11단계 | push 암호화·공유 sandbox·2,500개 원장·두 엔진 응답 경합/수명 |
| E23 | 첨부 4기기×3방식→desktop paste/drop·iPhone picker, Enter 4→2기기 | 빈 MIME·권한·원문 bytes·기록/ACK·늦은 iPhone 첫 응답 |
| E37 | 12조합→Chromium desktop Claude·WebKit phone Codex | 2,400행·전체 prepend·동시 resize·복귀·중복/누락/위치 |
| E39 | 390/430·Safari/PWA 반복, 엔진별 같은 목적지 순회 | fragment·WebKit swipe/Back·빈 hash gate·설정 복귀 |
| E53 | 기본 카드/문법 중복·늦은 응답 4→2조합 | 원문 선택지/직접 답 전달·새 보고와 늦은 ACK 경합 |
| E97 | 엔진×폭 직교 반복·plain/ANSI 벤치마크 | Worker 실패 종류·Chromium/WebKit 압축 history·한글+ANSI 회선/ACK |
| E100 | 자동 결정 51개를 실제 명령으로 만드는 페이지 규모 반복 | 실제 자동 답·수동 예외·끄기·Reader 거부·두 기기 삭제 동기화 |
| E103 | 3회 반복×4조합·80ms 벤치마크→1회×2조합 | 같은 DOM·초안·과거 위치·보류한 응답 전 복귀·재요청 수 |
| E109 | 같은 목적지/복귀 방식 2회→1회 | 두 엔진·6가지 이동·history 선행·첫 휠 SGR bytes·실제 입력/Reader |
| E113 | 3시나리오×4기기→시나리오마다 한 기기 | 500/601개·500/101 묶음·부분 실패/수동 재개·DB/사본/원본·권한 |
| E114 | 시나리오별 기기 반복→각각 한 기기 | inbox 500·빈 MIME·업로드 부분 실패·자리 500·자동화 500 변환 |

E113·114는 desktop Chromium과 phone WebKit에 시나리오를 나눴다. 같은 시나리오를 모든 엔진·폭에서 반복하는 커버리지는 삭제했다. 독립 시나리오는 새 실험실에서 각각 실행한다.

## Rust 통합 owner

파일은 `crates/aaa/tests/` 아래다. 아래 삭제는 새 ignore/filter를 추가하지 않으며, 남긴 Rust 전체와 doctest를 계속 실행한다.

| 삭제 owner | 이유·남긴 검사 |
| --- | --- |
| `cli_takes_no_project_argument` | 낮은 우선순위 CLI 형식·표시·기본값 흐름 제거. CLI/host의 해당 기능 unit과 실제 thread 생성·권한·재시작 keeper 유지. |
| `project_cli_keeps_legacy_names_with_a_stderr_warning` | 낮은 우선순위 CLI 형식·표시·기본값 흐름 제거. CLI/host의 해당 기능 unit과 실제 thread 생성·권한·재시작 keeper 유지. |
| `project_create_options_and_set` | 낮은 우선순위 CLI 형식·표시·기본값 흐름 제거. CLI/host의 해당 기능 unit과 실제 thread 생성·권한·재시작 keeper 유지. |
| `host_starts_only_with_one_project` | 낮은 우선순위 CLI 형식·표시·기본값 흐름 제거. CLI/host의 해당 기능 unit과 실제 thread 생성·권한·재시작 keeper 유지. |
| `routine_seconds_only_in_tests` | 낮은 우선순위 CLI 형식·표시·기본값 흐름 제거. CLI/host의 해당 기능 unit과 실제 thread 생성·권한·재시작 keeper 유지. |
| `machine_health` | 낮은 우선순위 CLI 형식·표시·기본값 흐름 제거. CLI/host의 해당 기능 unit과 실제 thread 생성·권한·재시작 keeper 유지. |
| `thread_start_warns_about_similar_tasks_without_blocking` | 낮은 우선순위 CLI 형식·표시·기본값 흐름 제거. CLI/host의 해당 기능 unit과 실제 thread 생성·권한·재시작 keeper 유지. |
| `profiles_come_from_host_and_harness_overrides_are_local` | 기본값·옵션 전달의 큰 흐름 중복 제거. CLI 파서·host profile/권한 및 실제 restart 실패/교체 owner 유지. |
| `start_defaults_profile_kind_and_task` | 기본값·옵션 전달의 큰 흐름 중복 제거. CLI 파서·host profile/권한 및 실제 restart 실패/교체 owner 유지. |
| `restart_with_another_profile` | 기본값·옵션 전달의 큰 흐름 중복 제거. CLI 파서·host profile/권한 및 실제 restart 실패/교체 owner 유지. |
| `nudge_only_marks_the_set_without_a_coordinator_or_when_off` | inbox 묶음/꺼짐 판정의 전체 프로세스 반복 제거. host inbox/notify owner와 실제 inbox 전달 owner 유지. |
| `thread_state_items_follow_groups` | inbox 묶음/꺼짐 판정의 전체 프로세스 반복 제거. host inbox/notify owner와 실제 inbox 전달 owner 유지. |
| `coordinator_flow` | 낮은 우선순위 CLI 형식·표시·기본값 흐름 제거. CLI/host의 해당 기능 unit과 실제 thread 생성·권한·재시작 keeper 유지. |
| `task_note_append_warning_and_title_validation` | 메타데이터·기본값·입력 검사의 CLI 중복 제거. host tasks/task_edit/task_repo/task_queue owner와 실제 task 편집·thread 연결·대량 E114 유지. |
| `task_repo_field_picks_machine_and_path` | 메타데이터·기본값·입력 검사의 CLI 중복 제거. host tasks/task_edit/task_repo/task_queue owner와 실제 task 편집·thread 연결·대량 E114 유지. |
| `task_selected_model_and_machine_are_changed_only_by_user` | 메타데이터·기본값·입력 검사의 CLI 중복 제거. host tasks/task_edit/task_repo/task_queue owner와 실제 task 편집·thread 연결·대량 E114 유지. |
| `summary_file_is_validated_before_start_or_message` | 메타데이터·기본값·입력 검사의 CLI 중복 제거. host tasks/task_edit/task_repo/task_queue owner와 실제 task 편집·thread 연결·대량 E114 유지. |
| `thread_start_rejects_machine_prefixed_repo_before_recording` | 메타데이터·기본값·입력 검사의 CLI 중복 제거. host tasks/task_edit/task_repo/task_queue owner와 실제 task 편집·thread 연결·대량 E114 유지. |
| `doctor_files_brings_the_coordinator_guide_to_existing_projects` | 안내 문구 배열/정확한 문구 대조를 폐지. CLI 파서·기능별 권한/프로세스 owner 유지. |
| `memory_and_restart` | 기억/재시작의 큰 흐름 중복 제거. `m13/automation_start.rs`의 Claude/Codex compact·resume 기억 회수와 host lifecycle 기억 owner 유지. |
| `rollout::rollout_transfer_preflight_and_resume_keep_the_same_jobs` | 실제 실행 파일의 다중 복제·서명·전송/복구 장기 반복을 제거. 실제 CLI 배포/재시작은 `m13::coordinator_deployment_is_gated_and_survives_backend_restarts`, 상태·영수증·원격 경로는 `aaa-host/src/rollout{.rs,/web_tests.rs}` 유지. 복합 다중 머신 복구의 실물 통합 범위는 줄었다. |
| `rollout::rollout_rejected_update_and_remote_recovery_keep_the_same_jobs` | 실제 실행 파일의 다중 복제·서명·전송/복구 장기 반복을 제거. 실제 CLI 배포/재시작은 `m13::coordinator_deployment_is_gated_and_survives_backend_restarts`, 상태·영수증·원격 경로는 `aaa-host/src/rollout{.rs,/web_tests.rs}` 유지. 복합 다중 머신 복구의 실물 통합 범위는 줄었다. |
| `rollout::rollout_shared_host_agent_recovery_and_inflight_cancel_preserve_baselines` | 실제 실행 파일의 다중 복제·서명·전송/복구 장기 반복을 제거. 실제 CLI 배포/재시작은 `m13::coordinator_deployment_is_gated_and_survives_backend_restarts`, 상태·영수증·원격 경로는 `aaa-host/src/rollout{.rs,/web_tests.rs}` 유지. 복합 다중 머신 복구의 실물 통합 범위는 줄었다. |
| `ci::ci_conditional_failure_restart_catchup_and_outage` | 감시→자동화의 프로세스 전체 반복을 제거. `aaa-host/src/ci/tests.rs`의 저장/재시작·중복·실패 회복과 자동화 start owner 유지. |
| `ci::automation_ci_reuses_open_thread_and_closes_only_after_new_report_with_memory` | 감시→자동화의 프로세스 전체 반복을 제거. `aaa-host/src/ci/tests.rs`의 저장/재시작·중복·실패 회복과 자동화 start owner 유지. |
| `git_watch::git_push_restart_force_delete_and_outage` | Git 변경 감시의 전체 프로세스 반복을 제거. `aaa-host/src/git_watch_tests.rs`와 agent Git 경계 유지. |
| `git_watch::deploy_proposals_count_main_commits_across_restart_and_success_without_installing` | Git 변경 감시의 전체 프로세스 반복을 제거. `aaa-host/src/git_watch_tests.rs`와 agent Git 경계 유지. |
| `accounts::account_trust_coordinator_codex_local_worktree` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_coordinator_codex_remote_worktree` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_coordinator_codex_remote_dir` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_coordinator_claude_local_worktree` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_coordinator_claude_local_dir` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_coordinator_claude_remote_dir` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_user_codex_local_dir` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_user_codex_remote_worktree` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_user_codex_remote_dir` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_user_claude_local_worktree` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_user_claude_local_dir` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `accounts::account_trust_user_claude_remote_worktree` | 정책×harness×머신×폴더 16조합을 4경계로 축소. 같은 파일의 남긴 trust owner 4개와 실제 로그인/인증 가림 owner 유지. |
| `notify_webhook` | 실제 coordinator→webhook 전체 흐름의 저우선순위 반복 제거. `aaa-host/src/notify.rs` owner 유지. 실제 HTTP 송수신을 포함한 이 통합 범위는 삭제했다. |
| `routines` | CLI를 반복하는 일정 관리 통합 흐름 제거. host automation/model·scheduler·권한 owner와 automation_start 통합 유지. |
| `project_lifecycle` | 이름·메모·보관 등 프로젝트 메타데이터 CLI 반복 제거. `aaa-host/src/project.rs`와 project-open 권한/기동 통합 유지. |
| `thread_adopt_existing_pane` | 기존 pane 채택의 두 harness 전체 반복 제거. `aaa-host/src/adopt.rs` 및 채택 pane의 resolve 정리 통합 유지. 실제 채택 시작 흐름의 통합 범위는 줄었다. |
| `thread_adopt_claude_started_in_a_shell` | 기존 pane 채택의 두 harness 전체 반복 제거. `aaa-host/src/adopt.rs` 및 채택 pane의 resolve 정리 통합 유지. 실제 채택 시작 흐름의 통합 범위는 줄었다. |
| `library_view` | 자료 목록/HTTP 보기의 전체 CLI 반복 제거. host library/catalog/link와 E12의 실제 보고서 sandbox·권한 검사 유지. |
| `task_display_ids_survive_rename_move_restart_delete_and_archive` | 표시 ID 변경의 긴 CLI 반복 제거. `aaa-host/src/task_ids.rs`·`db/task_ids.rs`와 실제 task 연결/편집 owner 유지. |
| `accounts::account_advisor_local` | advisor 자동 선택의 머신별 전체 흐름 제거. host 계정/profile·advisor hook unit 유지. advisor 기동의 별도 통합 범위는 삭제했다. |
| `accounts::account_onboarding_explicit_default_remote` | 최초 실행의 머신별 직교 반복과 명시 default 중복 제거. default-local·yolo-remote·split-bypass-local·inline-bypass-remote 유지. 각 옵션의 전 머신 반복은 줄었다. |
| `accounts::account_onboarding_explicit_default_local` | 최초 실행의 머신별 직교 반복과 명시 default 중복 제거. default-local·yolo-remote·split-bypass-local·inline-bypass-remote 유지. 각 옵션의 전 머신 반복은 줄었다. |
| `accounts::account_onboarding_inline_bypass_local` | 최초 실행의 머신별 직교 반복과 명시 default 중복 제거. default-local·yolo-remote·split-bypass-local·inline-bypass-remote 유지. 각 옵션의 전 머신 반복은 줄었다. |
| `accounts::account_onboarding_split_bypass_remote` | 최초 실행의 머신별 직교 반복과 명시 default 중복 제거. default-local·yolo-remote·split-bypass-local·inline-bypass-remote 유지. 각 옵션의 전 머신 반복은 줄었다. |
| `accounts::account_onboarding_yolo_local` | 최초 실행의 머신별 직교 반복과 명시 default 중복 제거. default-local·yolo-remote·split-bypass-local·inline-bypass-remote 유지. 각 옵션의 전 머신 반복은 줄었다. |
| `accounts::account_onboarding_default_remote` | 최초 실행의 머신별 직교 반복과 명시 default 중복 제거. default-local·yolo-remote·split-bypass-local·inline-bypass-remote 유지. 각 옵션의 전 머신 반복은 줄었다. |
| `auto_resolve_and_reopen` | 자동 resolve/다시 열기 편의 흐름 제거. host lifecycle unit과 실제 dirty/unmerged 보존·수동 resolve/다시 시작 owner 유지. |
| `turn_end_notices_follow_reports` | inbox 전달·알림 편의 흐름의 전체 프로세스 반복 제거. host inbox/notify unit, post_sender_is_judged_not_claimed의 실제 호출자 경계와 대량 inbox frame 통합 유지. |
| `project_open_starts_or_shows_the_coordinator` | 기본 coordinator 열기의 CLI 반복 제거. project_open_follows_coordinator_safety와 실제 coordinator 배포/사용자 요청 owner 유지. |
| `post_from_thread_reaches_coordinator_inbox` | inbox 전달·알림 편의 흐름의 전체 프로세스 반복 제거. host inbox/notify unit, post_sender_is_judged_not_claimed의 실제 호출자 경계와 대량 inbox frame 통합 유지. |
| `inbox_and_nudge` | inbox 전달·알림 편의 흐름의 전체 프로세스 반복 제거. host inbox/notify unit, post_sender_is_judged_not_claimed의 실제 호출자 경계와 대량 inbox frame 통합 유지. |
| `thread_read_and_keys` | 터미널 읽기/키의 CLI 편의 반복 제거. E02 터미널·E09 IME와 원격 thread 읽기 권한 통합 유지. |
| `accounts::account_advisor_remote` | advisor 자동 선택의 머신별 전체 흐름 제거. host 계정/profile·advisor hook unit 유지. advisor 기동의 별도 통합 범위는 삭제했다. |
| `thread_park::account_unpark_keeps_auth_and_history_on_the_same_machine` | 같은 머신 전용 반복 제거. 같은 파일의 account_unpark_keeps_auth_when_moving_and_returning이 계정·history·왕복 복구를 계속 검사한다. |
| `automation_start::legacy_mission_files_are_inert_and_convert_with_memory_and_pending_input` | 구형 mission 이관의 낮은 우선순위 전체 흐름 제거. host automation 변환·memory unit 유지. CLI 이관의 별도 통합 범위는 줄었다. |
| `image::probe_completion_is_published_only_after_exit_code_is_written` | 가짜 이미지 upstream·프로브·재시작의 전체 CLI 흐름 제거. host imagegen의 계정·원격 전달·저장/권한 unit은 유지한다. 이 이미지 API의 실제 프로세스 통합은 삭제했다. |
| `image::host_account_images_transfer_to_local_and_remote_threads_without_credentials_or_duplicates` | 가짜 이미지 upstream·프로브·재시작의 전체 CLI 흐름 제거. host imagegen의 계정·원격 전달·저장/권한 unit은 유지한다. 이 이미지 API의 실제 프로세스 통합은 삭제했다. |
| `image::host_and_thread_restart_preserve_generated_images_and_coordinator_scope` | 가짜 이미지 upstream·프로브·재시작의 전체 CLI 흐름 제거. host imagegen의 계정·원격 전달·저장/권한 unit은 유지한다. 이 이미지 API의 실제 프로세스 통합은 삭제했다. |

## 시험용 fake 준비

줄 단위 FAKE_AGENT만 사용하는 14개 owner가 기존 `report_shell_world`를 재사용한다. 실제 PTY·brief·Git·보고/정리는 그대로 실행하며, TUI가 없는 fake에 필요 없는 화면 안정/입력 간격만 기존 시험 설정으로 0으로 둔다. CLI의 실제 Claude/paste owner와 제품 기본 간격은 바꾸지 않았다.

- `failed_threads_restart_from_the_place`
- `manual_threads_always_have_tasks_and_archive_the_result`
- `parallel_threads_enforce_capacity_before_launch`
- `post_sender_is_judged_not_claimed`
- `remote_dir_thread_survives_host_restart`
- `resolve_keeps_dirty_worktree_and_unmerged_branch`
- `restart_spawn_failure_keeps_the_old_pane_until_a_successful_replacement`
- `task_attachments_reach_a_remote_agent`
- `task_board_reads_tasks_and_follows_file_edits`
- `task_edit_keeps_bytes_conflicts_and_logs`
- `task_queue_runs_possible_fifo_after_resolve_without_coordinator`
- `task_thread_link_and_auto_clean`
- `worker_session_cli_is_read_only`
- `worktree_thread_round_trip`
