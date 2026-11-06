# 아이디어·첨부 파일 백업 검증(t-1578)

## 범위와 결정

사용자 답 1A·2A·3B에 따라 목적지·age 공개키는 `[backup]`과 공유하고 파일 백업만 별도로 켠다. 기본은 꺼짐, 정기 시각은 UTC 19:45(한국 04:45), 파일 사본 보관 수는 30개다. 기존 DB 백업 기본 7개와 켜짐·시도·성공·실패 기록은 독립이다. 운영 설정·coordinator `.gitignore`·인증 자료는 바꾸지 않았다. 보류된 DB 정기 백업 활성화 작업은 진행하지 않았다.

기존 `backup.rs`에서 gzip·age·내구 쓰기·완성 후 회전·인증 전체 확인과 정기 실행·실패 알림 절차를 공유한다. tar-rs로 표준 묶음을 만들며 외부 백업 프로그램이나 별도 암호 형식을 만들지 않았다. [설정·복원과 참고 구현](idea-file-backup.md)에 범위와 라이선스를 적었다.

## 새 시험과 필요한 이유

| owner | 지키는 계약 |
| --- | --- |
| `file_backup::tests::restores_revisions_metadata_media_handoff_uploads_and_short_ids_in_a_new_host` | 실제 노트 저장·그림·할 일 넘기기·복사 뒤 새 폴더에서 원래 판·메타 바이트·그림·첨부·번호와 다음 번호를 복원한다. 기존 경로 덮어쓰기도 거부한다. |
| `file_backup::tests::keeps_thirty_completed_file_archives_and_never_rotates_db_other_hosts_or_partials` | 실제 새 암호문 공개 뒤 정확히 30개만 남긴다. DB·다른 host·미완성 파일을 보존하고 시계가 뒤로 가도 새 사본을 지우지 않는다. |
| `file_backup::tests::rejects_incomplete_snapshots_wrong_keys_and_corrupt_ciphertext_without_losing_a_good_copy` | 노트와 판의 불일치·잘못된 개인키·잘린 암호문·없는 목적지에서 실패하며 정상 사본을 보존하고 불완전한 평문·복원 폴더를 지운다. |
| `file_backup::tests::snapshot_cannot_enter_a_multi_file_write_and_releases_its_lock_before_encryption` | 저장 중 독립 FD 잠금이 거부되고 snapshot 반환 뒤 다시 저장할 수 있다. 시간 경과가 아닌 실제 잠금 상태로 확인한다. |
| `file_backup::tests::daily_file_backup_is_independent_of_disabled_db_backup_and_remembers_failures_and_success` | DB 백업이 꺼져 있어도 파일 정기 백업이 실행된다. 재시작 후 중복 실행·열린 실패 알림 중복을 막고 마지막 성공을 보존한다. |
| `file_backup::tests::encrypted_archive_rejects_traversal_links_duplicates_and_unknown_schema` | 암호화된 악성 tar의 schema·링크·중복 항목·폴더 밖 경로를 거부하고 생성한 복원 폴더를 지운다. Windows 예약 이름·구분자도 거부한다. |
| `aaa-config::tests::file_backup_shares_destination_but_has_independent_opt_in_schedule_and_retention` | 독립 켜기와 04:45·30개 기본, 잘못된 시각·보관 수·공유 설정 누락을 검사한다. |
| `aaa --test file_backup` | 실제 host·CLI 사이 파일 잠금을 검사하고, 실행 중 host에서 수동 백업한 노트를 새 host에서 같은 짧은 ID로 읽는다. 수동 실행이 정기 기록을 바꾸지 않는다. |

실제 노트·사진·운영 키 대신 임시 합성 자료와 시험 중 만든 age 키만 썼다. 정한 시간을 자거나 재시도·시한 연장으로 통과시키지 않았다. 정기 worker 완료는 host 상태 변경 사건으로 기다린다.

## 개발 확인

모든 검증 명령은 `aaa verify -- <명령>`으로 실행한다. 후보 단계에서는 관련 owner만 확인하며 착지 전체 gate는 착지 창의 최종 통합 SHA에서 별도로 실행한다.

```sh
nice -n 15 cargo test --profile verify --locked -p aaa-config -p aaa-host --lib backup
nice -n 15 cargo test --profile verify --locked -p aaa-host --lib ideas::
nice -n 15 cargo test --profile verify --locked -p aaa --test file_backup --test db_backup --test deps
nice -n 15 cargo clippy --profile verify --locked -p aaa-common -p aaa-config -p aaa-host -p aaa --all-targets -- -D warnings
cargo fmt --all -- --check
bash scripts/check-public.sh
```

- 설정 전체 41개와 host 백업 관련 19개가 통과했다. 기존 DB 백업 7개·새 파일 백업 6개가 포함된다.
- 아이디어 저장·판·첨부·복사 기존 owner 8개가 통과했다.
- 실제 CLI 파일 백업 1개, 기존 DB 백업 CLI·ticker 2개, crate 의존성 2개가 통과했다. 파일 백업 CLI 시험 실행 시간은 약 1.9초였다.
- 첫 구현의 판 파일 이름이 기존 네 자리 형식과 달라 snapshot 시험이 실패했다. 기존 저장 형식으로 고쳤고 판별 바이트 대조를 유지해 통과했다.
- strict clippy가 공통 암호화 함수의 불필요한 경로 참조를 발견해 제거했다. 수정한 최종 후보의 관련 4개 crate all-targets strict clippy가 통과했다.

fmt와 공개 저장소 검사도 통과했다. 전체 착지 gate·배포용 release 빌드·병합·push는 이번 후보 단계에서 실행하지 않았다.

## Windows 실기

Windows용 host 시험 실행 파일을 자체 target에서 교차 빌드해 격리한 시험 폴더로 복사하고 SHA-256을 대조했다. TEMP·TMP와 사용량 홈도 시험 폴더 아래로 한정한다. 서비스·예약 작업·Run 키·사용자 PATH·방화벽 기준 목록을 시험 전에 보관한다. 실제 서비스나 GUI는 띄우지 않는다.

첫 실기에서 백업 12개 중 새 폴더 복원 1개가 private 파일 검증에 실패했다. 복원용 번호 DB를 SQLite가 기본 생성해 Windows 승격 token의 기본 소유권을 사용한 것이 원인이다. 기존 평문 DB snapshot과 같이 private 파일을 먼저 생성한 뒤 SQLite로 여는 방식으로 고쳤다. 시한·기대값은 바꾸지 않는다.

수정 뒤 같은 단언으로 Windows 백업 12개·아이디어 8개·아이디어 thread 2개·첨부 3개, 총 25개가 통과했다. 각 묶음은 1.68초·0.69초·0.17초·0.21초에 끝났다. 서비스·예약 작업·Run 키·사용자 PATH·방화벽은 전후 차이가 없었다. reparse point가 없음을 확인하고 시험 폴더·exe·임시 키·합성 자료·원격 로그를 모두 회수·삭제했다. 원본 기준 목록과 로그는 library 밖 작업 target에만 보관한다.

수정한 같은 소스의 macOS 관련 host owner 26개도 1.57초에 통과했다. Windows 시험 빌드에는 외부 CRT PDB 부재와 PE에 대한 macOS debug-map 진단이 있었으나 빌드 종료 코드는 0이며 실제 PE 실행 파일의 SHA 대조와 실기 실행이 통과했다. production release나 배포 산출물 검증으로 주장하지 않는다.
