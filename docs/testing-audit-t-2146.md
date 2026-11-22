# 남은 시험 감사 후보 적용 검증 (t-2146)

[t-2142의 시험 감사](testing-structure-t-2142.md)가 남긴 후보 61건 가운데 57건을 적용했다. 동작은 바꾸지 않았다.
늘 통과하던 단언을 실제로 실패할 수 있게 고치고, 더 강한 경계가 같은 계약을 보는 시험을 합치거나 지우고,
production 호출자가 없는 코드를 그 시험과 함께 지웠다.

## 단언 수정

고치거나 옮긴 단언은 대상 코드를 한 곳 망가뜨려 실패하는 것을 본 뒤 되돌렸다.

| 무엇이 틀렸나 | 고친 것 | 변형 | 실패한 시험 |
| --- | --- | --- | --- |
| 기대값을 검증 대상 helper가 만든다 | literal 단계, 표의 ack 기대 열 | `[병합 준비됨]`을 단계로 읽지 않음 / `[알림]`을 끝난 보고로 보지 않음 | `report_version_failure_keeps_…`, `automation_reports_reach_my_reports_only_with_an_alert` |
| 보는 칸에 그 글이 들어가지 않는다 | inbox의 사건 칸으로 판정 | 입력 접수가 옛 기다림을 지우지 않음 | `codex_message_and_next_do_not_revive_previous_waiting_report` |
| 한 창이 빠져도 통과하는 하한 | 쉼·입력창 확인 두 창의 합 | 쉼 창을 0으로 | `turn_end_nudge_wakes_the_ticker_…` |
| 빈 기록을 지우고 「재사용 없음」을 본다 | 답한 기록을 실제로 남긴 뒤 지움 | 기록을 지울 때 q-ID 행도 지움 | `ids_are_project_local_persistent_and_not_reused_after_history_deletion` |
| guard가 아니라 JSON 오류로 거절된다 | 유효한 설정을 가리키는 symlink | symlink 검사를 뺌 | `bypass_settings_symlink_preserves_its_target` |
| 이름은 GET만인데 method를 보지 않는다 | `--method GET`·hostname·경로 머리 | GET을 POST로 | `calls_are_get_only_and_reject_header_or_path_injection` |
| 취소하지 않은 작업 때문에 통과한다 | 그 작업을 먼저 끝낸 뒤 확인 | 취소 때 바이트 permit을 먼저 반환 | `codec_slots_and_cancelled_jobs_keep_their_byte_permits_until_finished` |
| 양쪽 조각이 8바이트 이상이라 CSI·개행 경로 없이도 가려진다 | 한쪽을 8바이트보다 짧게 끊음 | 개행 이어 붙이기를 끔 / CSI 건너뛰기를 끔 | `bytes_mask_a_whole_secret_split_by_ansi_or_newlines_and_keep_the_rest` |
| 지금 fixture에 들어갈 수 없는 옛 literal | 지금 fixture 인증의 조각 | hook 글 응답을 가리지 않음 | `image_reply_preserves_encoded_collisions_…` |
| 없는 경로라서 거절된다 | 실제 폴더로 형식 guard 확인 | `..` 검사를 뺌 | `work_dir_must_be_thread_folder` |
| 다른 상한·형식 오류로 거절된다 | 유효한 내용의 큰 입력과 정상 실행을 함께 | 파일별 크기 상한을 뺌 | `malformed_or_oversized_inputs_remain_pending_…` |
| 표지를 시험이 손으로 세운다 | 실제 점검 실행기 안에서 새 작업을 넣음 | 표지를 세우지 않음 / 내리지 않음 | `candidate_inspection_waits_for_no_running_job` |
| 필수 칸이 없어서 거절된다 | 유효한 승인 견본에 칸 하나만 더함 | 모르는 칸 거부를 뺌 | `public_and_private_fixtures_roundtrip_with_versioned_shapes` |
| 겹친 설정의 다른 오류로 거절된다 | 겹침 파일을 치우고 규칙별 오류 글 단언 | claude effort에 ultra 허용 | `host_profiles_and_machine_overrides` |
| 이름이 맞지 않으면 자식이 0개를 돌리고 성공한다 | 자식이 단언 뒤 표지를 찍음 | 자식 시험 경로를 바꿈 | `resource_locks_serialize_processes` |
| 빈 목록에서도 통과하는 `all()` | 비어 있지 않음을 함께 단언 | 변경 목록·단계 목록을 비움 | `install_dry_run_is_read_only_…`, `deploy_start` |
| 모든 payload가 같은 바이트다 | 플랫폼마다 다른 바이트 | Windows 입구에 Linux 자산 SHA를 넘김 | `test_generated_windows_entry_is_the_existing_release_owner` |
| 셋이 각각 허용되는지만 본다 | 허용 집합이 정확히 셋 | owner를 넷으로 | `test_only_the_three_documented_owners_have_the_stall_exception` |

실패할 수 없던 단언은 지웠다: `hook_reports_safe_locations_…`의 자식이 만들 수 없는 경로 글,
`pump_lets_go_once_the_session_host_is_gone`의 「또 붙었다」, 머신 이름표 시험의 세션 불변·생성자 인자 되읽기,
`display_probe_failure_…`의 자기 왕복, `file_backup`의 원본에 없는 폴더 부재, `test_literal_authentication_…`의 중복 scan.
벽시계 상한 둘(agent `plan`·`command`)도 뺐다.

`normal_launch_does_not_create_settings_and_invalid_settings_are_rejected`의 「오류 글에 설정 원문이 없다」는 지금 구현에서는
실패하지 않지만 그 함수의 오류 글 비공개를 보는 유일한 단언이라 다시 읽고 남겼다.

## 외부 알림

외부 알림(webhook·Web Push)에 들어오는 항목은 coordinator 사용자 post와 host 이상 두 종류뿐인데, 시험은 닿지 않는 thread 보고·머신 사건
fixture로 형식을 고정하고 있었다. 먼저 시험을 두 종류로 옮겨 통과시키고(production 그대로), 그다음 닿지 않는 분기를 지웠다.
지운 뒤 시험은 기대값 수정 없이 통과했다.

- `bodies`·`payloads`: JSON key와 상한 단언은 그대로다. host 이상의 제목·주소·자리, 텔레그램 미리보기 끔, 실제로 넘치는 길이의 상한을 더 본다.
- 가림 값 표는 보고 첫 줄에서 coordinator post 제목(`request_title`)으로 옮겼다.
- mute는 음소거한 프로젝트의 post가 대기열에 들어가지 않는지로 본다.

| 변형 | 실패한 시험 |
| --- | --- |
| 자른 뒤에 가림 | `request_title_hides_private_values_before_shortening` |
| 텔레그램 글 상한을 없앰 / 고정 말 없는 사건의 120자를 늘림 | `bodies` |
| push 본문 크기를 escape 전 길이로 셈 | `payloads` |
| 프로젝트 mute를 무시 | `coordinator_can_request_the_user_without_nudging_itself` |
| 고르는 대상이 아닌 종류를 막음 | `device_alert_kinds` |
| 끝낸 항목을 거르지 않음 | `drops_what_was_done_meanwhile` |

## 지운 production 코드

| 자리 | 지운 것 | 줄 |
| --- | --- | ---: |
| `aaa-common` `redact` | 청크 사이 보류(`Stream`)와 접두부 표. `bytes()` 하나로 접었다 | −45 |
| `aaa-host` `notify`·`push` | 보고 첫 줄 요약, 머신 사건 묶음, post 밖의 고정 말, thread 제목 인자, mute 예외, 「보기:」 줄, thread-state 갈래 | −213 |
| `aaa-agent` `workspace_metrics/lines` | 시험 전용 입구 `collect_with_elapsed`와 branch 추정 | −22 |
| `aaa-agent` `design` | 만드는 곳이 없는 `Stale` 오류 | −16 |

`redact`는 옛 구현과 무작위 입력 20만 건의 결과를 대조했다(임시 시험, 커밋하지 않음).

## 지운 시험

| 지운 시험 | 같은 계약을 보는 곳 |
| --- | --- |
| `notify::alert_lines`, `notify::machine_items_go_out_once`, `notify::mute_keeps_errors_only` | 닿지 않는 분기의 시험이다. 남는 계약은 `bodies`와 post 시험이 본다 |
| `post::question_guard_accepts_optional_feedback_and_existing_card_reply_methods` | 구절은 `question_guard/tests.rs`의 순수 표(없던 13개를 옮김), 배선은 형제 시험 셋 |
| `prefs::keeps_the_web_sidebar_width` | `starts_empty_keeps_only_shared_names_…` |
| `prompt_box::the_mark_lines_up_with_the_session_hosts_text_and_ansi` | `thread_read_marks_a_predicted_prompt_in_the_box` |
| `design::marks_count_only_callout_blocks_like_the_web` | `marks_match_the_shared_web_fixture` |
| `setup_records_the_current_short_build_version` | `fresh_install_…`(제목), `build_salt_produces_…`·`tests/software.rs`(12자리 표기) |
| `recovery::status_failure_retains_…` | `install_status_is_read_only_and_input_authorization_cannot_open_writes` |
| `accounts::credential_canary_…` | `secret_k0::byte_scan_…` |
| `parallel_threads_enforce_capacity_before_launch` | `thread_stages::report_stages_release_queue_capacity_…` |
| `post_fixture_replies_to_complete_json_before_write_eof`, `burst_fd_assertion_accepts_the_same_two_fd_slack_as_the_wait` | 제품 코드를 실행하지 않는 fixture 자기 시험이다 |
| Python `test_separate_build_directory_uses_reported_output` | 같은 도우미를 쓰는 두 시험과 `build_metadata` |

합친 계약은 keeper가 같은 변형에서 실패하는지 봤다(카드 상태 조회를 빼면 질문 감시 형제 시험이, 한도 거부 뒤 기록을 남기면
`thread_stages`가, 누출 검색 조각 범위를 줄이면 `byte_scan`이, 마지막 표지 덩어리를 세지 않으면 견본 시험이 실패한다).

이름을 단언에 맞춘 시험: `rules_agree_on_the_shared_cases`, `install_schema_is_current`,
`same_or_older_release_selects_nothing_even_without_assets`, `machine_label_applies_at_once_and_persists_over_the_configured_name`,
`display_probe_failure_is_unknown_and_output_is_cut_to_the_first_line`, `drops_what_was_done_meanwhile`,
Python `test_direct_cli_on_a_local_machine_is_rejected_before_cargo`. 웹 E2E 122의 단계 이름은 「이동」을 「누름 대상」으로 바꿨다.

## 시험 지원 코드

- m13: 읽는 시험이 없는 가짜 agent 분기와 profile, 늘 같은 값을 받던 준비 인자. 지우기 전에 다섯 분기에 표지를 심어 m13 전체를 돌렸고 닿지 않았다.
- 실험실: 쓰이지 않는 helper 셋.
- 웹 E2E: harness 가짜 agent의 읽는 spec 없는 분기 셋, 쓰이지 않는 import. 103의 단언 없는 단계는 정리 블록으로 옮겼고,
  72는 sshd가 없으면 단계 0개로 통과하던 것을 실패로 바꿨다.

## 하지 않은 것

- 위의 유지 1건.
- `fill_brief`의 `{{aaa:inbox_list}}`·`{{aaa:git_reads}}` 자리: 적는 코드는 없지만 사용자 글에 있으면 치환된다. 지우면 동작이 바뀐다.
- Windows 전용 시험 둘(`windows_runner_reports_whether_an_openssh_server_exists`, 수동 진단 `cooked_read_product_lifetimes`):
  Windows 시험을 바꾸려면 Windows에서 돌린 결과가 있어야 한다.

## 전후

| 항목 | 전 | 후 |
| --- | ---: | ---: |
| Rust 시험(macOS) | 1,757 | 1,744 |
| Python 시험 | 105 | 104 |
| 웹 유닛 | 718 | 718 |
| 웹 E2E | spec 47 | spec 47 |

시험 이름 목록(`--list`)의 차이는 삭제 13개와 이름 변경 8개로, 계획한 것과 같다.

## 검증

- 좁은 검사: lane마다 바꾼 crate의 유닛 시험과 strict Clippy, 손댄 통합 시험(m13 전체, install·deploy_start·file_backup·post_cli·resource_limits), Python 시험, 바꾼 웹 E2E spec.
- 전체 gate(`scripts/ci-local.sh`): web·fmt·build·test·e2e·public 모두 통과. Rust 검증 작업 353개(시험 1,736개 통과 · 무시 8개), Python 104개, 웹 유닛 718개, 웹 E2E spec 47개(작업 58개, 재확인 0).
- strict Clippy: macOS 네이티브, Linux·Windows cross(workspace 전체). production을 지운 crate는 지운 직후에도 세 대상에서 따로 봤다.
- Windows 전용 코드와 시험은 바꾸지 않았다.
