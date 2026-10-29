## 할 일·thread별 자동 병합(t-1214, plan §8.530)

- host 유닛 `auto_merge::tests::user_sets_task_and_thread_shows_it`는 실제 Host·SQLite·TASKS.md로 할 일 ID로 켜기 → 목록·thread 보기의 값,
  같은 값 재요청의 `changed=false`, `task_log` 한 줄(off→on·thread), coordinator inbox 한 줄, `t-7` 별칭으로 끄기를 본다.
  필요한 이유: 값·기록·알림이 한 저장 경로에서 함께 바뀌는 계약을 지킨다.
- `legacy_thread_resolves_through_task_link`는 이 칸 전에 띄운 thread(`task_id` 없음)를 할 일 줄 연결로 찾아 채우고, 할 일 없는 thread는
  NotFound로 거부하는지 본다. 필요한 이유: 배포 전부터 열린 thread에서도 화면 스위치가 동작해야 한다.
- `only_user_or_quoting_coordinator_changes_it`는 보기 기기·thread 세션·남의 pane 셸의 거부, 2000바이트 넘는 근거 거부, 세션 밖 CLI의
  근거 없는 변경을 본다. 필요한 이유: 병합 승인 값이라 쓸 수 있는 주체를 좁힌다.
- `brief_text` 기존 시험에 brief 머리의 「자동 병합: on/off」 줄을 더했다. `guide::coordinator_guide_stays_small`(20 KiB·200줄)을
  지키도록 세부 명령은 `aaa guide tasks`로 옮겼다.
- m13 `auto_merge::coordinator_changes_auto_merge_only_with_user_request`는 실제 coordinator pane 안 CLI로 근거 없는 `task edit --auto-merge`·
  `thread auto-merge`의 거부와 무기록, 근거를 준 변경의 기록 actor, coordinator 자신의 변경은 inbox에 남지 않음, `thread start --from-task`
  뒤 `thread show`·`context`·brief 머리의 값, 세션 밖 CLI 변경의 inbox를 본다. 필요한 이유: coordinator 판정은 실제 pane 신원으로만
  재현된다. 기존 `doctor_files_brings_the_coordinator_guide_to_existing_projects`의 AGENTS.md·guide 문구 목록을 새 문구로 바꿨다.
- 새 E2E119 `119-auto-merge`는 실제 host·agent·Chromium 1280px에서 할 일 상세 스위치 → 줄 표지·기록·inbox, 「바로 시작」 칸(할 일 값으로
  열림)과 시작한 thread의 brief·「모든 thread」 표지, thread 화면 스위치로 끄기와 CLI `thread auto-merge`로 켠 값이 열린 화면에 사건으로
  따라오는지, 새 할 일 폼·예약 확인 칸의 저장, WebKit 390px thread 화면 스위치의 폭과 누르기를 본다. 필요한 이유: 사용자가 요청한
  다섯 자리(만들 때·예약·시작·실행 중 보기·thread 안 전환)가 같은 host 값을 쓰는지 화면에서 확인한다. 고정 sleep·재시도·시한 확대는 없고
  host 기록과 DOM 상태로 끝낸다. 단독 실행 약 6초다.
- (t-1293) E2E119 셋째 단계는 「작업」 목록 thread 줄도 본다: 첫 줄(`.workspace-line`)에 표지가 없고, 자동 병합 아이콘이 둘째 줄
  표지 묶음 끝·상태 말 바로 앞에 있으며, 끄면 사라지고 CLI로 켜면 따라온다. 필요한 이유: 사용자가 정한 줄 배치(첫 줄은 가리지 않음)와
  사건 반영을 지킨다. 새 E2E 파일은 만들지 않았다.
- 부분 검증: `cargo test -p aaa-host --lib -- auto_merge brief guide`, `cargo test -p aaa --test m13 -- auto_merge doctor_files`,
  `AAA_UPDATE_TS=1 cargo test -p aaa-proto typescript`, 웹 `tsc`·build·`npm test`, E2E119(Playwright 1.64.1 패키지는 작업 폴더 안,
  브라우저는 공용 캐시). 전체 ci-local·공개 검사·cross는 아래 실행 결과와 정규 착지 차례에서 확인한다.
