# macOS 자식 실행과 pipe 상속(t-1104)

host·agent·CLI가 여러 자식을 함께 실행할 때, 한 명령의 출력 수집이 무관한 장수 자식 때문에 끝나지 않는 결함을 막는다. 운영 프로세스·계정·설정은 바꾸지 않고 실험실에서 조사했다.

## 원인과 수리

macOS Rust std는 `pipe()` 뒤 각 FD에 `fcntl(FD_CLOEXEC)`를 호출한다. 그 사이 다른 스레드가 spawn하면 해당 자식은 pipe의 쓰기 끝을 받을 수 있다. 원래 명령이 종료해도 그 자식이 쓰기 끝을 닫기 전에는 EOF가 오지 않는다. Tokio도 std Command로 자식을 생성하므로 같은 창이 있다. 출력에 둔 시한이 끝났어도 별도 reader의 `join()`을 기다리는 host 사용량 경로는 계속 막힐 수 있다. stdin 쓰기 끝의 누출도 대상 자식의 EOF를 늦출 수 있다.

`aaa_common::child_process`의 macOS 프로세스별 잠금으로 pipe 생성부터 spawn 반환까지 보호한다. std·Tokio Command, PTY 생성/실행, 진단의 직접 `posix_spawn`, worker의 `exec`가 함께 사용한다. `wait`·출력 읽기·stdin 전달·비동기 대기에서는 잠금을 잡지 않는다. 같은 프로세스에 새 실행 경로를 더할 때에도 반드시 이 경계를 사용한다. `pre_exec` 안에서는 잠금을 다시 잡지 않는다.

- std의 `spawn_guarded`는 설정한 stdio·환경·flags를 유지한다. `status_guarded`는 생성 뒤 잠금을 풀고 기다린다.
- std의 `capture_output`은 닫힌 stdin과 stdout/stderr 전체 수집을 명시한다. 바꾼 기존 std `output()` 호출들은 모두 이 계약이었다. 일부 출력만 읽거나 stdin을 전달하는 경로는 `spawn_guarded`를 쓴다.
- Tokio의 `output_guarded`는 원래 `output()`처럼 stdout/stderr만 pipe로 바꾸고 stdin·`kill_on_drop`을 유지한다. Future를 만들 때 spawn하고 잠금을 푼 뒤 출력을 기다린다.
- 출력 크기 제한·실행 시한·오류 문맥·종료 코드·정리 정책을 유지한다. 파일 시스템 수집·재시도·시한 연장은 추가하지 않는다.

## 제품 경로 전수 조사

아래는 제품 소스의 `output()`·piped stdin/stdout/stderr와 모든 자식 생성 경로다. 시험 전용 호출, thread/task spawn, HTTP 응답의 `status`, 세션 화면의 `output`은 제외한다. 수리 뒤 일반 Command의 직접 spawn/output/status는 공통 adapter 안에만 남는다.

| 역할·소스 | 수집/생성 경로 | 함께 실행될 수 있는 자식·영향 | 적용 |
| --- | --- | --- | --- |
| agent `threads.rs` | worktree 준비·정리·adopt의 git output | 새 session host·배포 guard·routine; EOF 지연/시간 초과 | Tokio output |
| agent `pr.rs` | git origin output·published status·gh output | session host·guard; 조회 지연 | Tokio output/status |
| agent `git_watch.rs` | git stdout, 4 MiB 제한 | session host·guard; 조회 지연 | Tokio spawn |
| agent `diff.rs` | git stdin/stdout, 크기 제한·임시 index | session host·guard; 입력/출력 EOF 지연 | Tokio spawn |
| agent `ci.rs` | gh stdout, API 상한·실패 로그 꼬리 | session host·guard; 조회 지연 | Tokio spawn |
| agent `command.rs` | routine stdout/stderr, 병렬 drain | session host·다른 routine; 출력 누락/reader 대기 | Tokio spawn |
| agent `health.rs` | 도구 output | session host·guard; 진단 시간 초과 | Tokio output |
| agent `workspace_metrics/lines.rs` | git listing stdout·cat-file 양방향 pipe | session host·guard; 수집 스레드 대기 | std spawn |
| agent `lib.rs` | 장수 session host, startup stdin pipe·로그 파일 | 위 수집 pipe의 상속자; startup EOF도 지연 가능 | std spawn |
| agent `deploy/guard.rs` | setsid의 독립 복구 guard, 로그 파일 | 위 수집 pipe의 상속자 | std spawn |
| host `machine_setup.rs` | SSH·도구 output | 로컬 agent·다른 SSH·RPC; EOF 지연/시간 초과 | Tokio output |
| host `machine_setup/release.rs` | curl output | 위와 같음 | Tokio output |
| host `machine_setup/install.rs` | SSH stdin/stdout/stderr·NDJSON stream·wait_with_output | 장수 SSH·로컬 agent·RPC; 양방향 EOF 지연 | Tokio spawn |
| host `usage.rs` | 명령 stdout 별도 reader, 자식 종료 뒤 join | 로컬 agent·SSH·RPC; 자식 시한 뒤에도 join 대기 | std spawn |
| host `tailscale.rs` | Tailscale output | 로컬 agent·SSH·RPC; 조회 시간 초과 | Tokio output |
| host `quota_rpc.rs` | Codex helper stdin/stdout NDJSON, stderr null | 로컬 agent·SSH·이미지 helper; 응답/EOF 지연 | Tokio spawn |
| host `image_rpc.rs` | Codex 이미지 helper stdin/stdout NDJSON | 로컬 agent·SSH·다른 helper; 응답/EOF 지연 | Tokio spawn |
| host `account_login.rs` | 공식 로그인 PTY 생성·자식 실행 | 다른 수집과 병행; 아래 PTY 예외 참조 | 공통 잠금 |
| host `lib.rs` | 개발 모드 로컬 agent, setsid·로그 파일 | host 수집 pipe의 장수 상속자 | std spawn |
| 공통 `harness_version.rs` | harness --version stdout/stderr, 4097바이트 제한 | session host 안 실행; 다른 진단/자식과 병행 시 조회 시간 초과 | Tokio spawn |
| session `host.rs` | pane PTY 생성·harness 실행 | 직접 pipe 상속은 portable-pty가 정리; 생성 구간 자체는 함께 보호 | 공통 잠금 |
| CLI `update.rs` | binary 확인 stdout/stderr·별도 reader | setup backend·다른 검사 자식과 병행 가능 | std spawn |
| CLI `update/release.rs` | manifest·binary curl output | 위와 같음 | std capture |
| CLI `install/detect.rs` | git rev-parse output | 설치 도우미·setup backend와 같은 실행 파일 경계 | Tokio output |
| CLI `install/engine/worker.rs` | 분리 worker, stdin pipe | 다른 수집의 상속자·입력 EOF 지연 | std spawn |
| CLI `restart.rs` | worker 자기 교체 exec | 프로세스 교체 직전 pipe가 있으면 상속 가능 | 공통 잠금 |
| CLI `setup/unix/service.rs` | sudo -v status | 출력 수집은 없음; 다른 pipe의 상속자 가능성 | std status |
| CLI `install/services.rs` | systemctl output, Linux 전용 | macOS 실행 대상 아님; 새 직접 호출을 만들지 않는 공통 경계 | Tokio output |
| doctor `lib.rs`, `macos.rs` | true status·codesign output/status·ps output·직접 posix_spawn | agent 자가 점검과 session host 생성이 병행 | std capture/status·공통 잠금 |

`portable-pty 0.9.0`은 `pre_exec`에서 `/dev/fd`를 열어 3 이상 FD를 닫으므로, 정상적으로 정리가 된 pane/harness를 pipe 누출의 장수 상속자로 단정하지 않는다. 반면 표준 Command로 실행하는 session host·개발 로컬 agent·guard·SSH/RPC는 이 정리가 없다. PTY 내부의 std spawn 오류 pipe와 `openpty()`의 비원자적 CLOEXEC 설정도 있어 PTY 생성·실행을 같은 잠금에 넣었다. 비동기 SSH/RPC 자체가 장수 상속자가 될 수 있으므로 장수 daemon만 고쳐서는 부족하다.

## 재현과 확인 경계

macOS 전용 회귀 하나가 실험실 자식에만 dyld interpose를 넣어 실제 Rust std의 `pipe()` 반환 직전을 붙잡는다. FD의 CLOEXEC가 아직 없음을 확인하고, 수정 전 조합은 그 창 안에서 입력을 기다리는 무관한 자식을 exec한다. 해당 자식의 FD 상속과 캡처 대상의 실제 종료를 kqueue NOTE_EXIT로 확인한 뒤에도 output은 미완료다. 무관한 자식을 종료하면 반환한다.

수정 조합은 같은 창에서 공통 생성 잠금을 확인하고 다른 자식 실행을 시작한다. pipe 창을 풀면 동기 수집·Tokio 수집·실제 `harness_version::observe` 모두 무관한 자식에 FD가 없고 그 자식이 살아 있는 동안 반환한다. stdout/stderr와 실패 종료 코드 7도 보존한다. 사건으로 순서를 정하며 30초는 실패 상한이다. 고정 sleep·반복 성공 확률·재시도는 없다. DYLD 환경과 hook은 별도 시험 프로세스에만 있고 제품에는 들어가지 않는다.

별도 일회성 제품 실험은 실제 `UsageContext::system().run`과 `threads::adopt`의 git 수집을 호출했다. host의 로그인·네트워크 함수는 호출하지 않고 명령 runner만 썼으며, agent에는 격리 폴더의 가짜 git만 제공했다. 상속자 쪽을 원래의 직접 spawn으로 두면 두 제품 경로 모두 캡처 자식 종료 후까지 막히고 상속자 종료 뒤에만 반환했다. 양쪽이 공통 경계를 쓰면 상속자가 살아 있는 동안 반환했다. 이 대조는 실행 중인 운영 host·agent에서 결함을 관측했다는 뜻이 아니다.

Windows에는 macOS의 `pipe()` 후 `fcntl()` 창이 없다. Rust std는 `CREATE_PROCESS_LOCK`으로 상속 가능 handle 준비와 CreateProcess를 보호하며 Tokio도 이를 사용한다. 이번 공통 잠금은 Windows/Linux에서 아무것도 하지 않는다. Windows 설치·ConPTY·SCM의 고유 동작은 바꾸지 않으며, 다른 라이브러리가 직접 CreateProcess를 호출하는 별도 위험을 이번 수리로 해결했다고 주장하지 않는다.

기준 main에는 t-1097의 `lab::captured()`·해당 회귀가 없었다. 파일 수집은 단발 output을 EOF 의존에서 떼지만 RPC/NDJSON·cat-file·stdin 전달에는 그대로 적용할 수 없어 제품은 공통 생성 잠금을 선택했다. 실험실 helper를 임의로 가져오지 않는다.

## OSS 비교·라이선스

- [Rust std pipe 구현](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/pipe/unix.rs)·[Windows 생성 잠금](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/process/windows.rs): MIT OR Apache-2.0. OS별 CLOEXEC·생성 동작을 확인했다. 관련 [std FD 상속 경쟁 이슈 #114554](https://github.com/rust-lang/rust/issues/114554)는 실행 파일 FD의 별도 경쟁 사례이며, 이번 pipe EOF 결함과 같은 이슈로 세지 않는다.
- [Duct의 macOS 상속 설명](https://github.com/oconnor663/duct.py/blob/master/gotchas.md#preventing-pipe-inheritance-races-on-macos)·[구현](https://github.com/oconnor663/duct.rs/blob/master/src/lib.rs)·[MIT](https://github.com/oconnor663/duct.rs/blob/master/LICENSE): pipe 생성과 모든 spawn을 같은 전역 mutex로 묶는 방식을 따른다. Duct의 잠금은 Duct 밖 호출을 보호하지 않으므로 의존성 추가만으로는 해결되지 않는다.
- [Tokio process](https://github.com/tokio-rs/tokio/blob/master/tokio/src/process/mod.rs)·[MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE): output의 즉시 spawn·stdin·kill_on_drop·출력 설정 의미를 유지한다.
- [portable-pty Unix](https://github.com/wez/wezterm/blob/main/pty/src/unix.rs)·[MIT](https://github.com/wez/wezterm/blob/main/pty/Cargo.toml): fork 뒤 FD 정리와 PTY CLOEXEC 창을 구분한다.

외부 코드를 복사하거나 의존성을 추가하지 않았다. 전체 ci-local·착지·push는 coordinator가 배정한 묶음 착지에서 수행한다.

## 74회차 lab 통합

t-1097의 captured는 출력 파일 정책으로 유지하되 status_guarded와 lab의 나머지 spawn/status/capture도 같은 aaa-common 경계를 사용한다. 제품과 시험이 서로 다른 생성 잠금을 갖지 않는다. lab 회귀는 자식이 의도적으로 출력을 손자에게 물려주는 경우를 검사하므로 공통 잠금만 적용한 pipe output으로 바꾸면 의미가 달라진다. 파일 수집은 이 시험 정책만 담당하고 제품 RPC·NDJSON의 출력을 파일로 바꾸지 않는다. 기존 두 회귀와 출력·종료 단언을 모두 전체 gate에서 검증한다.
