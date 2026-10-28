# host DB 백업 검증(t-1242)

2026-10-28 Windows x86_64에서 격리된 SQLite DB·프로젝트 폴더·CLI·host 자식 프로세스로 확인했다. 운영 DB·NAS·host 서비스와 시스템 설정은 변경하지 않았다. 실행 파일은 이 worktree의 Cargo 산출물을 썼다.

## 필요한 회귀

| 시험 | 지키는 계약 |
| --- | --- |
| `backup::tests::wal_backup_restores_auth_history_and_rotates_only_its_files` | 열린 WAL의 인증·예약 이력을 복원하며, 다른 이름의 파일·미완성 파일을 보존하고 보관 개수와 시계 역행을 처리한다 |
| `backup::tests::pinned_snapshot_allows_writer_and_keeps_one_committed_version` | 읽기 snapshot을 잡은 직후 원본을 수정해도 쓰기가 성공하고 사본에는 같은 시점의 내용만 남는다 |
| `backup::tests::failed_backup_and_wrong_key_or_corruption_preserve_existing_data` | snapshot 검증·설정·목적지 실패 때 기존 백업을 보존하고, 잘못된 키·잘린 암호문·덮어쓰기를 거부한다 |
| `backup::tests::daily_due_catches_up_once_and_handles_clock_reversal` | 주입한 시각으로 일일 경계·놓친 차례·시계 역행을 검사한다 |
| `backup::tests::scheduled_failure_is_durable_and_notifies_inbox_once` | 정기 실패를 DB·inbox에 남기고 같은 날 차례를 반복하지 않는다 |
| `backup::tests::scheduled_success_survives_restart_without_duplicate_backup` | 성공 기록과 마지막 시도가 host 재개 뒤에도 유지된다 |
| `aaa-config`의 `backup_rejects_unsafe_or_incomplete_enabled_settings` | 설정 없는 기본 꺼짐과 필수 경로·공개키, 시간·보관 범위의 거부를 검사한다 |
| `db_backup::encrypted_db_backup_cli_restores_live_host_without_overwrite` | 실제 CLI 프로세스로 키 생성→열린 원본 백업→새 DB 복원·재열기와 운영 파일 덮어쓰기 거부를 연결한다 |
| `db_backup::host_ticker_backs_up_once_and_remembers_attempt_after_restart` | 실제 host ticker가 백업하고 재시작 뒤 tick을 끝내도 중복 실행하지 않는다 |

개인키·암호문은 실행 중 생성한다. 인증 데이터 fixture는 `aaa_test_support::token`을 쓴다. 고정 sleep은 추가하지 않았다. worker 완료는 상태 변경 사건으로, 실제 host 준비·백업은 CLI 결과와 성공 기록으로, 재시작 후 판정은 `ticker tick` 완료로 끝낸다. 5초는 실패 안전 상한이며 정상 실행에서 그 시간을 기다리지 않는다.

## 실행 결과

- `cargo test -p aaa-host --lib backup:: --locked`: 6개 통과, 최종 묶음 0.46초.
- `cargo test -p aaa-config --lib --locked`: 36개 통과, 묶음 0.13초.
- `cargo test -p aaa --test db_backup --test deps --locked`: CLI·host E2E 2개 통과, 의존 경계 2개 통과(0.02초). 최종 오류 처리 수정 뒤 `--test db_backup`도 통과(묶음 2.38초).
- 백업 설정 예제를 추가한 뒤 `examples_are_valid_and_warning_free` 통과(0.02초).
- `cargo clippy -p aaa-host -p aaa-config -p aaa --all-targets --locked -- -D warnings`: 통과.
- `python scripts/check_fixture_secrets.py`: 통과.
- `cargo fmt --all --check`·`git diff --cached --check`: 통과.
- 새 파일을 stage한 뒤 `scripts/check-public.sh`: 통과. 이 clone에 `private-words`가 없어 로컬 실제 이름 목록 검사는 생략됐다.

새 개별 시험은 모두 5초 이내다. 웹·proto를 바꾸지 않아 웹 E2E는 실행하지 않았다. 전체 `ci-local`·다른 OS·실제 NAS의 flush/rename·운영 크기 DB 성능은 이 기록의 검증 범위에 포함하지 않는다. 운영 활성화 시 목적지 권한과 실제 키로 수동 백업·새 파일 복원을 먼저 확인한다.

## 수정한 시험 문제

초기 설정 fixture에 필수 `schema = 1`이 없어 설정 시험이 실패했다. 정상·오류 fixture 모두 schema를 넣어 백업 설정 자체를 검사하도록 고쳤다. 빌드 중 소스 변경으로 CLI와 구판 host 산출물이 어긋난 컴파일 오류도 있었으며, 소스 변경을 마친 뒤 다시 검증했다.

초기 알림 시험에서 프로젝트 root를 생략해 기본 프로젝트 위치에 시험용 `fixture/PROJECT.md`와 알림 파일이 생성됐다. 파일 이름·내용을 확인해 이번에 만든 두 파일과 빈 폴더만 정리했다. 모든 새 host 시험은 프로젝트 root를 실험실 아래로 지정한다. 재시도·시한 연장·기대값 완화로 실패를 덮지 않았다.


## 승인된 정규 착지

main `13928d57`에 후보 `c0d858e5`를 no-ff 병합한다. 문서 충돌은 두 검증 링크를 모두 보존해 해결하고 testing.md에는 이 변경의 링크 한 줄만 추가한다. 설계·wire·웹 E2E 번호 정본은 main 그대로이며 새 번호를 만들지 않는다.

Mac 전용 worktree에서 LLVM PATH·cargo-xwin 0.23.1·작업 폴더 안 Playwright 1.64.1과 공용 브라우저 캐시를 사용한다. nice15 전체 ci-local과 uptime, 공개 검사, Linux/Windows strict cross, 별도 Windows production, 개인정보 검사 뒤 ff push와 ls-remote를 대조한다. 실제 착지 SHA salt의 Windows 설치기를 별도로 빌드해 전달하고 hash·판을 확인한다. 전체 gate에는 salt를 넣지 않는다.

문서 충돌 해결은 착지 절차의 일부다. 첫 검증 실패는 원문을 보존하고 재시도·시한·기대값 변경 없이 중단한다. 백업 기본 꺼짐을 유지하며 키 생성·NAS 목적지·운영 설정·host 재시작과 이력 보존 정책은 착지·전달 뒤 별도 결정한다. 실행 원문과 최종 결과는 thread 보고와 library에 남긴다.
