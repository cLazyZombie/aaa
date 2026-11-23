# 운영 로그

macOS에서 `aaa service`가 연결한 `host.log`와 `agent.log`는 기존 역할별 경로에 있다. `aaa service show --role host` 또는 `--role agent`에서 경로를 확인한다. 승인 설치의 macOS `service.log`, macOS·Linux의 Unix setup `agent-service.log`도 각 감독자가 연결한 기존 경로에서 회전한다. Linux에서 직접 실행한 역할을 상태 폴더의 `host.log` 또는 `agent.log`로 stdout·stderr 모두 연결한 경우도 해당한다.

`host.toml`·`agent.toml`의 정책은 각 역할이 시작할 때 읽는다. 머신별로 덮어쓰려면 `host.local.toml`·`agent.local.toml`에 둔다.

```toml
[logs]
max_file_mib = 64
keep_files = 4
keep_days = 60
```

- `max_file_mib`는 활성 파일 하나의 회전 크기다. 최소값은 1 MiB다. 새 줄이 크기 기준을 넘기기 전에 회전하고 줄을 새 활성 파일에 쓴다.
- `keep_files`는 활성 파일을 제외한 사본 수다. 0이면 사본을 보관하지 않는다. 기본값은 역할당 활성 파일 포함 최대 320 MiB다.
- `keep_days`는 회전 사본의 최대 보관 일수다. 0이면 사본 개수만 적용한다. 시작·회전과 실행 중 매시간 확인하며, 개수와 기한 중 먼저 닿은 조건을 적용한다. 활성 파일의 줄은 개별 시각으로 삭제하지 않는다.

회전 뒤에도 활성 파일명은 같고 사본 이름에는 회전 시각이 붙는다. 첫 적용에서 이미 파일별 상한을 넘은 로그는 원본을 그대로 사본으로 보관한다. 이 사본이 정리되기까지 합계가 평소 상한을 넘을 수 있다. 새 파일은 사용자 전용이다. 256 KiB보다 긴 한 줄은 끝부분을 잘랐다는 표시를 붙인다. 회전에 실패하면 로그에 원인을 기록하고 열려 있는 파일로 출력을 이어 간다.

운영 로그의 새 줄을 계속 보려면 파일명 교체를 따라가는 `tail -F host.log`를 사용한다. 카드 답과 연결의 지연·실패를 찾는 검색어는 [운영 계측 검증](testing-operational-logs-t-2167.md#hostlog-검색)에 있다.

터미널·파이프로 실행한 역할과 별도 파일로 리다이렉트한 일반 CLI는 기존 출력을 쓴다. Windows SCM의 `agent-service.log`는 기존 시작 시 회전 정책을 따른다.
