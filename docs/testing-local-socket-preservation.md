# 연결 오류 때 로컬 소켓 보존 검증(t-1363)

## 변경 계약

기존 Unix IPC는 경로가 있으면 연결 성공 여부만 보고 모든 실패에서 경로를 삭제했다.
실제 listener가 살아 있어도 클라이언트 FD 부족 등의 오류로 경로를 잃을 수 있었다.
LocalListener 종료도 경로만 보고 삭제해 같은 이름의 새 listener를 지울 수 있었다.

- `ECONNREFUSED`에서만 기존 stream socket의 회수를 시도한다.
- `ENOENT`는 삭제 없이 다음 bind에 맡긴다. 경로가 다시 생기면 커널의 중복 bind 거부로 보존한다.
- 권한·자원·중단·혼잡·timeout·알 수 없는 오류는 원래 `io::Error`와 errno를 반환한다.
- 최초 조회는 `symlink_metadata`로 소켓 종류와 현재 실효 UID를 검사한다. 일반 파일·symlink는 회수하지 않는다.
- 회수 직전에는 장치·inode·UID·소켓 종류를 다시 확인하고, 달라졌으면 `AddrInUse`로 거부한다.
- 정상 종료와 권한 설정 실패 정리는 bind 뒤 기록한 identity와 같은 소켓만 삭제한다.
- Windows Named Pipe와 외부 공개 API·의존성은 변경하지 않았다.

## 실행한 확인

macOS 네이티브에서 다음 관련 owner를 확인했다. 전체 gate·양 OS cross·Windows production은 착지 절차에 남긴다.

```sh
nice -n 15 cargo test --profile verify --locked -p aaa-common --lib ipc::unix_tests::
nice -n 15 python3 scripts/verify.py develop --base ce7f29df --package aaa-common
cargo fmt --all -- --check
git diff --check
```

수정 전 일반 파일 삭제와 종료 시 새 listener 삭제가 각각 재현됐다.
별도 자식의 FD만 고갈시킨 실제 `EMFILE`에서도 기존 listener 경로가 삭제됐다.
수정 뒤 신규 실행 시험 8개가 통과했고, aaa-common 전체는 72개 통과·2개 ignored였다.
ignored 둘은 부모가 직접 실행하는 기존 child_process fixture와 신규 IPC fixture다.
strict Clippy(`--all-targets -- -D warnings`)와 공개 저장소 검사도 통과했다.

| 신규 시험 | 필요한 이유와 확인한 사건 |
| --- | --- |
| `bind_preserves_listener_when_client_fds_are_exhausted` | 실제 listener를 열고 격리 자식의 FD만 고갈시킨다. bind의 EMFILE 원문과 경로 보존, FD 해제 뒤 실제 connect/accept를 확인한다. |
| `non_terminal_probe_errors_preserve_listener_and_errno` | 열린 listener에 EACCES·EPERM·EMFILE·ENFILE·EINTR·EAGAIN·EINPROGRESS·ETIMEDOUT·ECONNRESET·ENOBUFS·EIO·EPROTOTYPE를 주입해 inode와 errno 보존을 확인한다. errno 없는 오류도 그대로 반환하며 끝에 실제 연결을 확인한다. |
| `bind_reclaims_stale_socket_transports_bytes_and_cleans_own_path` | 종료한 자식이 남긴 실제 소켓의 ECONNREFUSED, 회수 뒤 0600·peer PID·바이트 전송·종료 정리·재bind를 확인한다. |
| `bind_preserves_live_listener` | 실제 살아 있는 listener의 중복 bind를 거부하고 원래 listener가 계속 연결을 받는지 확인한다. |
| `stale_probe_preserves_replacement_listener` | 연결 검사와 회수 사이에 새 실제 listener를 놓고 ECONNREFUSED·ENOENT 각각에서 새 경로와 연결이 유지되는지 확인한다. 옛 경로는 rename해 inode 재사용에 기대지 않는다. |
| `drop_preserves_replacement_listener` | 옛 LocalListener가 종료돼도 같은 이름의 새 listener가 연결을 받는지 확인한다. |
| `bind_preserves_non_socket_entry` | 일반 파일을 죽은 소켓으로 오인해 지우지 않는지 확인한다. |
| `bind_and_drop_preserve_symlinks_files_and_missing_paths` | symlink·dangling symlink를 회수하지 않고, 종료 때 파일·symlink로 교체되거나 사라진 경로도 보존하는지 확인한다. |

고정 sleep·재시도·시한 연장·기대값 완화는 추가하지 않았다. 정상 통신은 read/accept로 끝나고
3초는 바이트 전송 사건이 오지 않을 때의 안전 상한이다. 신규 시험 실행은 전체 병렬 실행에서도 5초 미만이었다.

## 첫 전체 검사에서 발견한 fixture 문제

처음 IPC 단독 8개는 통과했으나 전체 병렬 실행에서는 stale 생성 직후 connect가 성공해
두 시험이 제품 코드 진입 전 준비 단계에서 실패했다. 전체 결과는 70개 통과·2개 실패·1개 ignored였다.
부모에서 listener를 drop한 것만으로 모든 프로세스의 FD가 닫혔다고 가정했던 fixture를 고쳤다.

Unix의 fork 자식은 exec/종료 전 부모의 listener FD를 보유할 수 있다. 별도 작은 재현에서
부모 close 뒤 연결 성공, FD를 보유한 자식의 종료 뒤 ECONNREFUSED를 사건 순서로 확인했다.
첫 전체 실패에서 실제로 FD를 보유했던 자식 PID를 수집한 것은 아니므로 그 실행의 보유 주체까지 단정하지 않는다.

stale 소켓은 전용 시험 자식에서 만들고, 부모가 그 자식의 종료·출력 수집을 끝낸 뒤 사용하도록 바꿨다.
부모 프로세스에는 그 listener FD가 없어서 다른 병렬 시험의 fork에 상속되지 않는다.
기존 `StdCommandExt::capture_output`으로 spawn 구간의 stdio FD 누출도 방지한다.
변경 뒤 관련 owner 전체·Clippy·공개 검사가 통과했다. 시험 삭제나 직렬화로 덮지 않았다.

## 한계

운영 장애를 재현한 것은 아니다. EMFILE과 실제 listener로 기존 삭제 결함을 시험에서 재현했다.
Linux 네이티브 실행은 이번 개발 검사에 포함하지 않았다.

Unix의 마지막 metadata 조회와 unlink는 원자적 조건부 삭제가 아니다. bind와 identity 조회·권한 설정 사이도
같은 UID의 외부 프로세스가 경로를 임의로 바꾸는 것까지 원자적으로 배제하지 않는다.
이번 변경은 연결 검사 동안의 교체와 종료 전 교체를 identity 재검사로 보존하며,
같은 UID의 임의 경로 조작에 대한 완전한 잠금·보안 경계라고 주장하지 않는다.

## 먼저 검토한 공개 구현

- [herdr IPC](https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/src/ipc.rs),
  [Apache-2.0](https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/LICENSE):
  연결 오류를 분류하고 identity가 같은 경로만 정리하는 접근을 참고했다. 이 변경은 symlink를 따라가지 않고 Unix raw errno를 엄격하게 분류한다.
- [Tokio UnixListener](https://github.com/tokio-rs/tokio/blob/master/tokio/src/net/unix/listener.rs),
  [MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE):
  기존 bind·accept와 OS 오류 반환을 유지하며, 경로 회수 정책을 aaa-common에 둔다.
- [tmux server.c](https://github.com/tmux/tmux/blob/master/server.c)의 파일 머리 ISC 허용문과 socket 생성 경로도 검토했다.
  무조건 unlink하는 부분은 이 작업의 보존 계약에 쓰지 않았다.

외부 소스는 설계 참고로만 읽었고 코드·새 의존성은 가져오지 않았다.
