# host DB 정기 백업

host 내부 ticker가 SQLite 온라인 백업 API로 일관된 사본을 만들고 gzip 압축·age X25519 암호화 후 보관한다. 인증 표를 포함한 DB 전체를 보존한다. 설정 전에는 꺼져 있다. 별도 launchd·예약 작업·외부 실행 파일은 필요 없다.

## 설정

1. 복원 담당자가 별도로 보관할 개인키를 만든다. 상위 폴더는 미리 만든다. 기존 파일은 덮어쓰지 않는다. 출력은 공개키 하나이며 개인키는 사용자 전용 파일에만 쓴다.

   ```sh
   aaa db-backup keygen --identity /secure/aaa-db-identity.txt
   ```

2. 출력한 `age1…` 공개키를 host 설정 폴더의 `host.local.toml`에 넣는다. 개인키는 host 설정·NAS 백업 폴더·git에 넣지 않고 host 장애 때도 찾을 수 있는 별도 장소에 보관한다. 개인키를 잃으면 백업을 복원할 수 없다.

   ```toml
   schema = 1

   [backup]
   enabled = true
   directory = "/Volumes/<disk>/aaa-db-backups"
   recipient = "age1…공개키로 바꾼다…"
   daily_utc = "19:45"
   keep = 7
   ```

3. `directory`는 미리 존재하는 절대 경로다. NAS를 연결한 뒤 폴더를 준비한다. host는 이 경로 자체를 만들지 않는다. NAS가 없을 때 mount 자리 아래에 폴더를 대신 만드는 일을 피한다. Windows는 `directory = 'D:\backups\aaa'`처럼 TOML literal 문자열을 쓸 수 있다.
4. 사용자가 host를 재시작한다. 실행 중 설정을 다시 읽지 않는다. 시스템 서비스·launchd 설정은 바꾸지 않는다.

`daily_utc`는 정확한 `HH:MM`이며 기본 `19:45`는 한국 시각 다음 날 04:45다. `keep`은 1~365개, 기본 7개다. host별 하위 폴더 `aaa-host-<host_id>`에서만 회전한다. 여러 host가 같은 목적지를 써도 서로의 사본을 지우지 않는다.

host가 꺼져 있었으면 다음 tick에 가장 최근 차례를 한 번 따라잡는다. 실행 시도를 원본 DB에 먼저 적어 같은 날 재시작해도 중복 실행하지 않는다. 실패한 차례를 자동으로 되풀이하지 않으며 다음 날 차례에 실행한다. 마지막 성공 이후 변경은 장애 때 잃을 수 있다.

## 수동 실행과 확인

host 머신에서 같은 `--instance`·`--config-dir`·`--state-dir`를 지정해 실행한다. `--state-dir`는 `host/host.db`의 상위 상태 root다. 운영 중인 host를 띄우거나 재시작하지 않고 별도 읽기 연결로 백업한다.

```sh
aaa db-backup run
aaa --json db-backup status
```

`run`은 `enabled = false`여도 목적지·공개키가 있으면 한 번 실행하며 정기 차례의 시도·성공 시각에는 영향을 주지 않는다. 같은 state의 실행은 파일 잠금으로 겹치지 않게 한다. `status`는 정기 실행의 `last_attempt_ms`·`last_success_ms`·`last_error`를 읽는다. 시각은 UTC Unix 밀리초이며 아직 기록이 없으면 `null`이다.

정기 실행 실패는 host 로그와 DB 상태에 원인 사슬을 남기고 프로젝트 inbox에 `system / host-db-backup / backup failed` 항목을 만든다. 같은 열린 실패 항목은 중복 생성하지 않는다. 전체 원인은 inbox 본문에 남긴다. 이후 오류의 최신 내용은 `status`와 host 로그에서 확인한다. 프로젝트가 아직 없거나 DB 자체가 쓰기 불가능하면 host 로그가 진단 근거다. 수동 명령의 실패는 종료 코드와 표준 오류로 반환한다.

목적지 파일은 `host-<시각>-<ULID>.db.gz.age`다. 암호문을 `.partial`에 쓰고 내구 쓰기·이름 교체가 끝난 뒤에만 옛 사본을 지운다. 시계가 과거로 돌아가도 방금 완성한 사본은 보존한다. 회전 실패도 실패로 알리며 이때 새 사본과 보관 수를 넘는 옛 사본이 함께 남을 수 있다. 규칙 밖 파일·폴더·symlink·미완성 파일은 회전 대상이 아니다. 비정상 종료의 `.partial`은 자동 삭제하지 않는다.

평문 snapshot은 host state의 사용자 전용 `db-backup-staging` 안에만 만든다. 정상·오류 반환 때 지우며 비정상 종료의 잔여물은 다음 백업에서 잠금을 얻은 뒤 지운다. NAS에는 암호문만 쓴다. snapshot의 `PRAGMA integrity_check`와 host schema를 검사한다. snapshot을 고정한 동안 원본 쓰기는 가능하지만 WAL 회수는 지연될 수 있다. 온라인 page 복사는 300초 작업 상한을 두고 BUSY·LOCKED를 즉시 실패로 남긴다. 파일 시스템 호출 자체를 강제로 중단하는 시한은 아니다.

로컬에는 DB 크기 이상의 snapshot 여유가, NAS에는 기존 사본과 새 암호문 하나를 함께 둘 여유가 필요하다. NAS의 rename·flush 지원과 서비스 계정의 권한은 실제 목적지에서 수동 백업·복원으로 확인한다. NAS 자체의 snapshot·다른 장소 사본은 별도 운영 정책이다.

## 복원

복원은 새 파일에만 쓴다. 기존 파일이나 같은 이름의 SQLite sidecar가 있으면 거부한다. 잘못된 개인키·잘린 암호문·인증 실패·gzip 오류·SQLite 무결성 오류·지원하지 않는 schema는 실패이며 이번에 만든 불완전한 복원 파일을 지운다.

```sh
aaa db-backup restore /backup/host-example.db.gz.age \
  --identity /secure/aaa-db-identity.txt \
  --output /secure/restored-host.db
```

1. 운영 DB와 다른 새 파일에 복원하고 명령의 성공을 확인한다. 무결성과 현재 실행 파일이 지원하는 schema를 검사한다. 개인키는 같은 사용자만 읽을 수 있어야 한다.
2. 사용자가 host를 멈추고 종료를 확인한다. 실행 중인 `host.db` 위로 복사하지 않는다.
3. 기존 `host.db`와 존재하는 `host.db-wal`·`host.db-shm`·`host.db-journal`을 복구 전 보관 폴더로 함께 옮겨 보존한다. 옛 WAL을 복원 DB 옆에 남기지 않는다.
4. 검증한 복원 파일을 같은 인스턴스의 `host/host.db`로 놓는다. host 서비스 사용자 전용 권한을 유지한다. 사용자가 host를 시작하고 할 일·예약·thread·inbox·기기 연결을 확인한다.
5. 문제 있으면 host를 멈춘 뒤 복원 DB와 새 sidecar를 따로 보관하고 복구 전 파일 묶음을 원래 위치에 되돌린다.

이 기능은 DB 백업이다. host TLS 개인키·인증서, 계정 암호화 키, Web Push 개인키, library 서명 키, 설정, 프로젝트 파일·보고서·첨부물과 agent session 파일은 포함하지 않는다. 원래 host state가 남은 같은 인스턴스에서 DB를 되돌릴 수 있다. 머신 전체를 잃었을 때 같은 인증 신원까지 복구하려면 이 자료도 별도의 암호화된 state·파일 백업으로 보관해야 한다. DB의 등록 정보만 복원해 새 TLS 신원으로 기존 agent에 바로 연결되는 것은 보장하지 않는다.

개인키를 바꾸면 새 공개키로 설정을 바꾸고 host를 재시작한다. 옛 사본은 옛 개인키가 필요하므로 해당 사본이 회전될 때까지 옛 개인키도 보존한다. 월 1회 정도 새 파일 복원으로 실제 목적지·개인키의 사용 가능 여부를 점검한다.

## 이력 보존 조사

이번 변경은 운영 이력을 삭제하거나 VACUUM하지 않는다. brief의 DB 약 411MB·`task_queue_log` 약 286MB는 전달받은 수치이며 이번에 운영 DB에서 직접 측정한 값은 아니다.

| 자료 | 현재 보존 | 정리 시 영향 |
| --- | --- | --- |
| `task_queue_log` | 자동 삭제 없음. 예약 변경·기동 선점마다 목록 전체의 before/after JSON 저장 | 과거 예약 목록과 판단 근거 소실. 현재 `task_queue`와는 별도 |
| `task_log` | 자동 만료 없음. 명시적 `task_log_prune` API 있음 | 과거 변경·되돌리기·검색 원문에 영향 |
| `automation_runs` | 자동 만료 없음 | 실행 원장·중복 처리·완료 판단에 쓰이므로 날짜만 보고 삭제하면 안 됨 |
| routine 관측 이력 | routine마다 최근 50개·마지막 결과·마지막 건너뜀 보존 | 실행 원장과 구분해 기존 정책 유지 |
| `input_events` | 저장 때 30일·최근 10,000건·20MiB 한도 | 기존 정책 유지 |
| 끝낸 inbox | `projects.inbox_done_days` 이후 정리 | 기존 정책 유지 |

`db/task_queue.rs`의 `queue_change`·`queue_claim_tx`는 바뀐 항목만이 아니라 모든 예약의 직렬화 결과를 로그에 넣는다. 예약 수·본문·변경 횟수가 늘면 같은 내용이 반복된다. `task_queue_log`를 읽어 운영 상태를 복구하는 경로는 현재 코드 검색에서 찾지 못했다. 조사에 필요할 수 있으므로 임의로 지우지 않는다.

권장 정책안은 `task_queue_log` 최근 30일 유지다. 적용 전 실제 행 수·최초/최근 시각·before/after 총 바이트를 읽기 전용으로 집계하고 검증된 전체 암호화 백업을 먼저 확보한다. 30일 이전 이력의 장기 보관 필요와 백업 7개 보관의 관계도 함께 결정해야 한다. 일별 백업 7개만으로는 30일 이전 이력을 계속 보존하지 못한다. 장기 이력이 필요하면 월별 백업을 별도 보관한 뒤 삭제한다.

삭제 승인 뒤 별도 변경에서 작은 transaction으로 오래된 행을 정리하고 시간 index와 변경 항목만 남기는 형식을 검토한다. 대량 DELETE만으로 파일 크기가 바로 줄지는 않는다. 공간 반환용 VACUUM은 추가 디스크·잠금·운영 시간을 따로 검토한다. `task_log`·실행 원장까지 함께 지우지 않는다.

## 참고와 라이선스

- [SQLite Online Backup API](https://sqlite.org/backup.html)와 [저작권](https://sqlite.org/copyright.html): public domain. `.backup`과 같은 API를 `rusqlite` backup 기능으로 호출한다. WAL 파일을 따로 복사하지 않는다.
- [Litestream 설정](https://litestream.io/reference/config/)과 [라이선스](https://github.com/benbjohnson/litestream/blob/main/LICENSE): Apache-2.0. snapshot·보관·복원 구조를 참고했다. 현재 v0.5의 age 클라이언트 암호화는 지원되지 않아 직접 의존하지 않는다.
- [restic 보관 정리](https://restic.readthedocs.io/en/stable/060_forget.html)와 [라이선스](https://github.com/restic/restic/blob/master/LICENSE): BSD-2-Clause. 완성된 백업과 보관 정리를 나누는 원칙을 참고했다. 별도 repository·비밀번호·실행 파일 도입은 하지 않았다.
- [age Rust API](https://docs.rs/age/0.11.5/age/)와 [소스](https://github.com/str4d/rage): MIT OR Apache-2.0. 이 라이브러리의 age v1 스트리밍 암호화를 사용한다. 자체 암호 파일 형식은 만들지 않는다.
- [SQLite VACUUM](https://sqlite.org/lang_vacuum.html): DELETE 뒤 공간 회수와 추가 디스크 요구의 근거다.
