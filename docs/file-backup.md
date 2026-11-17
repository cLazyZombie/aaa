# 첨부 파일 백업

`aaa file-backup`은 프로젝트의 `PROJECT.md`와 `uploads/` 전체(할 일·대화 첨부)를 하나의 tar 복원 단위로 묶는다. 옛 아이디어 노트(`ideas/`)는 지웠고 묶지 않는다(t-1959). gzip 압축·age X25519 암호화·내구 쓰기·완성 후 회전·실패 알림은 [기존 DB 백업](host-db-backup.md)의 구현을 공유한다. 별도 백업 프로그램이나 OS 예약 작업은 필요 없다.

## 설정과 실행

목적지 `directory`와 공개키 `recipient`는 기존 `[backup]` 설정에서 가져온다. 폴더는 미리 존재하는 절대 경로여야 한다. 개인키 생성과 별도 보관은 `aaa db-backup keygen`을 그대로 쓴다. 파일 백업만 켜도 DB 백업은 켜지지 않는다.

```toml
schema = 1

[backup]
enabled = false
directory = "/Volumes/<disk>/aaa-backups"
recipient = "age1…공개키로 바꾼다…"

[file_backup]
enabled = true
daily_utc = "19:45"
keep = 30
```

`file_backup.enabled`의 기본은 false다. 켜면 매일 UTC 19:45, 한국 시각 다음 날 04:45에 실행한다. 시각은 `HH:MM`, 보관 수는 1~365개이며 파일 백업 기본은 30개다. 사용자가 설정한 뒤 host를 재시작한다. 이 기능을 추가하거나 수동 실행해도 운영 설정·coordinator Git 허용 목록을 바꾸지 않는다.

host 머신에서 동일한 인스턴스·설정·상태 root를 지정한다. host를 새로 띄우지 않고 수동 백업할 수 있다.

```sh
aaa file-backup run
aaa --json file-backup status
```

수동 실행은 정기 백업이 꺼져 있어도 목적지·공개키가 있으면 실행한다. 정기 실행의 기록에는 영향을 주지 않는다. 정기 차례는 가장 최근 차례만 한 번 따라잡고, 시도 시각을 먼저 기록해 재시작해도 같은 차례를 다시 실행하지 않는다. 실패한 차례는 자동 재시도하지 않는다.

`status`는 파일 백업만의 `last_attempt_ms`·`last_success_ms`·`last_error`를 읽는다. DB 백업 기록과 분리하며 아직 없으면 null이다. 정기 실패는 host 로그·상태에 원인을 남기고 프로젝트 inbox에 `system / host-file-backup / backup failed`를 만든다. 같은 열린 실패 알림은 중복 생성하지 않는다. 수동 실패는 종료 코드·표준 오류로 반환한다.

## 일관성과 보관

프로젝트 첨부 저장과 snapshot은 같은 프로세스 사이 파일 잠금(host state `files.lock`)을 쓴다. 첨부를 모두 복사하고 나서 잠금을 푼다. 저장이 진행 중이면 별도 CLI snapshot은 실패한다. snapshot 중 새 저장은 잠금 해제를 기다린다. 기존 host 프로세스를 새 실행 파일로 재시작한 뒤 사용해야 이 잠금 계약이 적용된다. 파일을 외부 편집기로 직접 수정하는 작업은 이 잠금에 참여하지 않는다.

첨부는 내용 hash 이름으로 보존하며 이미 있는 다른 바이트는 덮어쓰지 않는다. 압축·암호화·목적지 쓰기 동안에는 저장 잠금을 보유하지 않는다.

평문 tar는 host state의 사용자 전용 `file-backup-staging/snapshot.tar`에만 만든다. 정상·오류 반환 때 지우며 비정상 종료 잔여물은 다음 백업이 자기 중복 실행 잠금을 얻은 뒤 정리한다. 목적지에는 암호문만 쓴다. 기존 DB 사본과 같은 `aaa-host-<host_id>` 폴더에 `files-<시각>-<ULID>.tar.gz.age`로 쓴다. `.partial`의 내구 쓰기·rename이 완료된 뒤 파일 백업 사본만 회전한다. DB 사본·다른 host·다른 이름·symlink·미완성 파일은 지우지 않는다. 방금 완성한 사본은 시계가 뒤로 가도 보존한다.

기존 백업과 같이 회전 실패도 실패로 알린다. 이때 새 사본과 보관 수를 넘긴 옛 사본이 함께 남을 수 있다. NAS가 빠지면 상위 목적지를 대신 만들지 않는다. 실제 NAS의 권한·flush·rename은 운영자가 수동 백업과 새 폴더 복원으로 확인한다. 로컬에는 원본 파일 합계만큼의 tar 공간이, 목적지에는 기존 사본과 새 암호문 하나를 함께 둘 공간이 필요하다. 오래 남은 저장 임시 파일(`.`으로 시작)은 묶지 않는다. 파일당 32MiB를 넘거나 일반 파일·폴더가 아닌 원본은 실패로 알린다.

## 새 폴더 복원

```sh
aaa file-backup restore /backup/files-example.tar.gz.age \
  --identity /secure/aaa-db-identity.txt \
  --output /secure/aaa-recovery
```

`output`은 없는 새 폴더여야 하고 부모 폴더는 미리 준비한다. 기존 파일·폴더·symlink를 덮어쓰지 않는다. 잘못된 키·잘린 암호문·인증 실패·gzip 오류·중복 경로·폴더 밖 경로·tar 링크 항목은 실패하며 이번에 만든 불완전한 복원 폴더를 지운다. 복호화된 tar의 전체 age 인증을 확인한 뒤 파일을 풀며 소유권·권한·링크를 tar에서 복원하지 않는다. 복원 폴더와 파일은 사용자 전용이다.

복원 결과는 `projects/<프로젝트>/PROJECT.md`와 `uploads/`이고 원래 상대 위치와 바이트를 보존한다. 옛 판이 만든 백업에 든 아이디어 노트(`projects/<프로젝트>/ideas/…`)와 번호 원장(`idea-index.json`)은 경로 검사만 하고 복원하지 않는다. host DB는 만들지 않는다.

운영 자료를 복구할 때는 먼저 새 폴더에서 확인하고, 사용자가 host를 멈춘 뒤 기존 프로젝트 파일·DB를 별도로 보존하고 필요한 자료를 옮긴다. 전체 host DB를 복구하려면 별도 DB 백업을 사용한다. 두 백업은 같은 순간의 통합 snapshot이 아니므로 전체 장애 복구에서는 시점을 대조해야 한다.

외부 URL의 자료·agent 세션·thread library·host 인증키·설정·할 일 목록·Git 이력은 이 파일 백업 범위에 들지 않는다. 이를 복구하려면 각각의 기존 백업이 필요하다. 프로젝트 파일만으로 TLS 신원·등록 agent 연결까지 복원되지는 않는다.

## 참고한 구현과 라이선스

- [tar-rs Archive](https://docs.rs/tar/0.4.46/tar/struct.Archive.html), [MIT](https://github.com/composefs/tar-rs/blob/main/LICENSE-MIT) / Apache-2.0: 표준 tar 작성·순차 읽기에 사용한다. `unpack`은 호출하지 않고 허용 경로의 일반 파일만 직접 만든다.
- [age Rust API](https://docs.rs/age/0.11.5/age/), [MIT / Apache-2.0](https://github.com/str4d/rage): 기존 age v1 스트리밍 암호화·복호화를 그대로 공유한다. 암호 형식을 새로 만들지 않는다.
- [restic 보관 정리](https://restic.readthedocs.io/en/stable/060_forget.html), [BSD-2-Clause](https://github.com/restic/restic/blob/master/LICENSE): snapshot 보관과 정리를 구분하는 방식을 비교했고 기존 aaa의 완성 후 회전을 유지했다. 외부 실행 파일·repository·비밀번호를 추가하지 않는다.
