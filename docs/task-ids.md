# 할 일 ID

웹 할 일 화면과 `aaa task list`, `aaa context`는 제목 옆에 `d-0001` 같은 ID를 보인다.
프로젝트마다 1부터 증가하며 네 자리를 넘으면 자릿수가 늘어난다. 제목·목록·순서가 바뀌어도 같은 ID다.
삭제·완료 보관·기록 정리 뒤에도 번호를 재사용하지 않는다. 삭제 복원과 완료 보관 복원은 원래 번호를 쓴다.

```sh
aaa task edit d-0001 --note '완료 범위'
aaa task move d-0001 --list Backlog --before d-0002
aaa task rm d-0001
aaa task log d-0001
aaa task restore d-0001
aaa thread start --from-task d-0001 --link-task
aaa task reserve d-0001
aaa task unreserve d-0001
aaa task queue-move d-0001 1
aaa task archive --restore d-0001
```

기존 제목과 내부 ULID 입력도 계속 받는다. 같은 문자열이 표시 ID로 이미 발급돼 있으면 ID를 우선한다.
`restore`는 종전의 삭제 기록 번호도 받는다. ID로 복원하면 최근의 아직 복원하지 않은 삭제 기록을 찾고,
삭제 기록이 없으면 완료 보관을 찾는다. 완료 보관 복원은 실행 연결을 풀고 예약하지 않는다. `--list`는 삭제 기록 복원에서만 받으며,
완료 보관은 원래 목록에 복원한 뒤 `task move`로 옮긴다.

내부 연결 키는 기존 `<!-- aaa-task:ULID -->` 주석이다. host는 할 일 목록을 처음 읽을 때
없는 주석을 추가하고, 웹·CLI 편집으로 더한 할 일에도 주석을 붙인다. 메모·들여쓰기·줄 끝 문자와
기존 ULID는 유지한다. 중복 ULID는 오류로 처리한다. 직접 파일을 고칠 때도 주석을 함께 유지하고,
새 할 일을 만들 때 다른 할 일의 주석을 복사하지 않는다.

짧은 번호와 ULID의 대응은 host SQLite의 `task_ids` 표에 영속 저장한다. 삭제된 할 일의 대응도 남긴다.
프로젝트 이름 변경·휴지통 내보내기·복원에는 이 표도 포함된다. TASKS.md만 다른 host에 복사하면
ULID는 유지되지만 표시 번호는 그 host에서 새로 배정되므로, 번호까지 보존할 때는 host DB도 보존한다.

Front API 1.86에서 `TaskView.short_id`와 `TaskLog.task` 필터를 추가했다. 새 CLI는 옛 host에
기록 필터를 보내지 않고 갱신 안내를 낸다. 옛 CLI의 제목 입력과 ULID 기반 예약·보관 연결은 유지한다.
