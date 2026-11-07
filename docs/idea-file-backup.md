# 아이디어·첨부 파일 백업

`aaa file-backup`은 프로젝트의 `ideas/` 전체(노트·판 이력·메타데이터·그림·영상·겹친 글)와 `uploads/` 전체를 하나의 tar 복원 단위로 묶는다. 아이디어에서 할 일로 넘긴 그림 사본도 포함한다. gzip 압축·age X25519 암호화·내구 쓰기·완성 후 회전·실패 알림은 [기존 DB 백업](host-db-backup.md)의 구현을 공유한다. 별도 백업 프로그램이나 OS 예약 작업은 필요 없다.

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

노트 저장·메타데이터 변경·그림 저장·프로젝트 첨부 저장과 snapshot은 같은 프로세스 사이 파일 잠금을 쓴다. 노트 본문·현재 판·판 이력을 모두 복사하고 나서 잠금을 푼다. 저장이 진행 중이면 별도 CLI snapshot은 실패한다. snapshot 중 새 저장은 잠금 해제를 기다린다. 기존 host 프로세스를 새 실행 파일로 재시작한 뒤 사용해야 이 잠금 계약이 적용된다. 파일을 외부 편집기로 직접 수정하는 작업은 이 잠금에 참여하지 않는다.

복사한 아이디어도 그림이 준비된 뒤 노트·메타데이터와 함께 공개한다. 그림과 첨부는 내용 hash 이름으로 보존하며 이미 있는 다른 바이트는 덮어쓰지 않는다. 압축·암호화·목적지 쓰기 동안에는 저장 잠금을 보유하지 않는다.

평문 tar는 host state의 사용자 전용 `file-backup-staging/snapshot.tar`에만 만든다. 정상·오류 반환 때 지우며 비정상 종료 잔여물은 다음 백업이 자기 중복 실행 잠금을 얻은 뒤 정리한다. 목적지에는 암호문만 쓴다. 기존 DB 사본과 같은 `aaa-host-<host_id>` 폴더에 `files-<시각>-<ULID>.tar.gz.age`로 쓴다. `.partial`의 내구 쓰기·rename이 완료된 뒤 파일 백업 사본만 회전한다. DB 사본·다른 host·다른 이름·symlink·미완성 파일은 지우지 않는다. 방금 완성한 사본은 시계가 뒤로 가도 보존한다.

기존 백업과 같이 회전 실패도 실패로 알린다. 이때 새 사본과 보관 수를 넘긴 옛 사본이 함께 남을 수 있다. NAS가 빠지면 상위 목적지를 대신 만들지 않는다. 실제 NAS의 권한·flush·rename은 운영자가 수동 백업과 새 폴더 복원으로 확인한다. 로컬에는 원본 파일 합계만큼의 tar 공간이, 목적지에는 기존 사본과 새 암호문 하나를 함께 둘 공간이 필요하다. 아직 노트 밖 `library/`·`uploads/`·로컬 파일을 가리키는 옛 판은 해당 판을 열어 기존 미디어 이행을 완료한 뒤 백업한다. 이런 링크나 누락된 그림은 성공으로 처리하지 않고 실패로 알린다. 외부 URL은 내려받지 않는다. 오래 남은 저장 임시 파일(`.`으로 시작)은 묶지 않는다. 파일당 32MiB를 넘거나 일반 파일·폴더가 아닌 원본은 실패로 알린다.

## 새 폴더 복원

```sh
aaa file-backup restore /backup/files-example.tar.gz.age \
  --identity /secure/aaa-db-identity.txt \
  --output /secure/aaa-recovery
```

`output`은 없는 새 폴더여야 하고 부모 폴더는 미리 준비한다. 기존 파일·폴더·symlink를 덮어쓰지 않는다. 잘못된 키·잘린 암호문·인증 실패·gzip 오류·지원하지 않는 schema·중복 경로·폴더 밖 경로·tar 링크 항목은 실패하며 이번에 만든 불완전한 복원 폴더를 지운다. 복호화된 tar의 전체 age 인증을 확인한 뒤 파일을 풀며 소유권·권한·링크를 tar에서 복원하지 않는다. 복원 폴더와 파일은 사용자 전용이다.

복원 결과는 다음과 같다.

- `projects/<프로젝트>/PROJECT.md`, `ideas/`, `uploads/`: 원래 상대 위치와 바이트를 보존한다.
- `idea-index.json`: 짧은 아이디어 ID 번호 원장. 삭제된 노트의 예약 번호도 포함한다.
- `host/host.db`: 새 복원용 DB. 아이디어 번호 원장만 넣어 `n-NNNN` 링크와 다음 번호를 보존한다. 원래 host DB의 인증·할 일·thread·예약은 담지 않는다.

별도 시험 인스턴스에서 `--state-dir /secure/aaa-recovery`와 `[projects] root = '/secure/aaa-recovery/projects'`를 지정하면 복원된 노트를 원래 짧은 ID로 열 수 있다. 노트 안 그림·영상과 `uploads/<이름>` 첨부는 같은 상대 위치에 있다. 현재 노트와 현재 판 기록이 같은지, 이력의 각 판과 노트가 가리키는 로컬 미디어가 있는지 검사한다.

운영 자료를 복구할 때는 먼저 새 폴더에서 확인하고, 사용자가 host를 멈춘 뒤 기존 프로젝트 파일·DB를 별도로 보존하고 필요한 자료를 옮긴다. 기존 운영 DB 위에 복원용 최소 DB를 덮어쓰지 않는다. 전체 host DB를 복구하려면 별도 DB 백업을 사용한다. 두 백업은 같은 순간의 통합 snapshot이 아니므로 전체 장애 복구에서는 시점과 번호 원장을 대조해야 한다.

외부 URL의 자료·agent 세션·thread library·host 인증키·설정·할 일 목록·Git 이력은 이 파일 백업 범위에 들지 않는다. 이를 복구하려면 각각의 기존 백업이 필요하다. 프로젝트 파일만으로 TLS 신원·등록 agent 연결까지 복원되지는 않는다.

## 참고한 구현과 라이선스

- [tar-rs Archive](https://docs.rs/tar/0.4.46/tar/struct.Archive.html), [MIT](https://github.com/composefs/tar-rs/blob/main/LICENSE-MIT) / Apache-2.0: 표준 tar 작성·순차 읽기에 사용한다. `unpack`은 호출하지 않고 허용 경로의 일반 파일만 직접 만든다.
- [age Rust API](https://docs.rs/age/0.11.5/age/), [MIT / Apache-2.0](https://github.com/str4d/rage): 기존 age v1 스트리밍 암호화·복호화를 그대로 공유한다. 암호 형식을 새로 만들지 않는다.
- [restic 보관 정리](https://restic.readthedocs.io/en/stable/060_forget.html), [BSD-2-Clause](https://github.com/restic/restic/blob/master/LICENSE): snapshot 보관과 정리를 구분하는 방식을 비교했고 기존 aaa의 완성 후 회전을 유지했다. 외부 실행 파일·repository·비밀번호를 추가하지 않는다.
