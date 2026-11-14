# Ubuntu 실험실 sshd 포트 인계·실패 진단 수리

## 확인한 실패와 한계

기준은 `9e1c6a3a`다. [CI 실행](https://github.com/cLazyZombie/aaa/actions/runs/43615221724)의
Ubuntu 24.04 `rust-tests-ubuntu-24.04-43615221724-1` artifact에서
`rust/14-aaa_host-57.log` 전체를 확인했다. launcher package 바인딩·개발 host 준비 안내 시험이
`실험실 sshd가 뜨지 않았다: Ok(Err(()))`로 실패했고 같은 shard의 나머지 28건은 통과했다.
이 오류는 stderr가 끝났다는 뜻이다. 기존 helper는 모든 stderr를 버렸으므로
그 실행의 실제 종료 코드·신호·bind 오류를 복원할 수 없다. 과거 같은 증상의 실패와
같은 원인이라고 단정하거나 기존 Ubuntu 스택 고갈과 합치지 않는다.

코드에는 별도로 확인 가능한 경쟁 구간이 있다. `TcpListener::bind(127.0.0.1:0)`로
포트를 고른 직후 listener를 닫고, 설정 파일·클라이언트 설정을 만든 뒤 sshd가 다시 bind한다.
그 사이 다른 시험의 빈 포트 할당이나 SSH 클라이언트의 출발 포트 할당이 주소를 가져갈 수 있다.
실제 sshd에 이미 점유된 포트를 주면 설정 검사는 통과하고 bind 오류·종료 255·ready 전 EOF로 끝난다.
이 재현은 가능한 실패 경로의 증거이며 과거 CI의 원인 확정은 아니다.

## 수리

- Linux는 `SO_REUSEADDR`를 켠, listen하지 않는 `TcpSocket`으로 OS 할당 포트를 유지한다.
  sshd도 `SO_REUSEADDR`를 쓰므로 같은 주소를 bind하고 listen할 수 있다. 주소가 맞는
  `Server listening` 줄을 받은 뒤 예약 소켓을 해제한다. 포트 선택과 sshd bind 사이에
  주소를 비우지 않는다. 같은 포트를 명시해 재사용하는 별도 서버를 막는 독점 잠금은 아니다.
- macOS는 위 공유 bind를 허용하지 않아 기존 포트 인계를 유지한다. 이번 포트 수리의 범위는 Linux다.
- `sshd -t -f <lab>/sshd_config` 결과, 자식 시작 PID, 준비 사건, 종료 코드·신호,
  stderr를 보존한다. 실패 메시지에 사건·stderr를 함께 넣고 실험실 경로를 남긴다.
  기록 파일은 `sshd.events`, `sshd.stderr`다. 설정·포트 충돌·신호 종료를 구별할 수 있다.
- 준비는 요청한 loopback 주소·포트가 일치하는 출력 사건으로 끝난다. 기존 20초 안전 상한과
  launcher package·전송 SHA·개발 host 안내의 단언은 유지한다. sleep·재시도는 추가하지 않았다.

## 검증 방법

모든 검사 진입점은 `aaa verify -- <명령>`이다. Ubuntu 24.04는 격리된 Lima VM에서
Rust 1.100.0·OpenSSH 9.6p1로 실행하고, macOS도 관련 owner를 실행한다.
VM은 별도 소스·target을 사용하며 운영 sshd·설정·계정은 바꾸지 않는다.

| 회귀 시험 | 확인하는 사건·값 |
| --- | --- |
| `an_occupied_port_reports_bind_stderr_and_the_child_exit_status` | 실제 점유 포트, 설정 검사 0, `Address already in use`, 종료 255, ready 전 EOF |
| `a_bad_config_reports_the_check_error_without_starting_a_daemon` | 잘못된 설정, 검사 255, 설정 오류 본문, 자식 시작 없음 |
| `an_early_signal_reports_stderr_and_the_termination_signal` | 검사 성공 뒤 stderr를 남기고 SIGTERM(15)으로 끝나는 자식 |
| `a_reserved_port_rejects_competitors_until_the_real_sshd_is_listening` | Linux 예약 중 비공유 bind의 EADDRINUSE(98), listen 전 ECONNREFUSED(111), 실제 sshd 준비·SSH 배너 |

첫 실기에서 잘못된 fixture 두 가지를 고쳤다. Unix의 `std::TcpListener`는 SO_REUSEADDR를 켜므로
비공유 경쟁자 fixture는 이를 명시적으로 끈 `TcpSocket`을 쓴다. 실패 기대값 98은 그대로다.
macOS의 listen하지 않는 예약 소켓은 연결 거부 대신 대기하게 하므로 `lab-down`에는 적용하지 않는다.
기존 닫힌 포트·거부 단언은 보존했다.

## 확인 결과

- Ubuntu 24.04: `cargo test --profile verify --locked -p aaa-host --lib machine_setup::tests:: -- --test-threads=8`
  33건 통과, 실행 3.41초. 기존 launcher package 바인딩·개발 host 안내와 실제 SSH 설치 대본을 포함한다.
- macOS: 같은 owner·동시 수로 32건 통과, 실행 1.44초. Linux 전용 예약 시험 한 건의 차이다.
- Linux 예약 소켓을 반환 전에 닫는 변형은 경쟁 bind가 성공해 회귀 시험에서 exit 101로 실패했다.
  예약을 유지하는 원본으로 바이트 복원하고 해당 회귀 시험 1건 통과를 확인했다.
  확률적 충돌을 기다리거나 재시도로 통과시킨 것이 아니다.
- Ubuntu·macOS의 `cargo clippy --profile verify --locked -p aaa-host --all-targets -- -D warnings` 통과.
- `cargo fmt --all --check`와 `scripts/check-public.sh` 통과.

착지 창에서 최종 통합 SHA의 전체 gate를 별도로 한 번 실행한다. 이 결과는 후보의 관련 owner 검증이며,
GitHub CI 회복·착지·배포 완료를 뜻하지 않는다.

## 참고한 공개 구현

- [OpenSSH sshd.c](https://github.com/openssh/openssh-portable/blob/master/sshd.c)와
  [라이선스](https://github.com/openssh/openssh-portable/blob/master/LICENCE): BSD 또는 그보다 자유로운 조건.
  bind 전 `set_reuseaddr`와 listen 성공 뒤 준비 로그를 확인했다.
  [sshd 설명서](https://man.openbsd.org/sshd.8)의 `-t` 설정·키 검사를 사용한다.
- [Testcontainers Rust 준비 사건](https://rust.testcontainers.org/features/wait_strategies/)와
  [MIT 라이선스](https://github.com/testcontainers/testcontainers-rs/blob/main/LICENSE-MIT): stderr 준비 메시지를
  기다리는 접근을 참고했다. 컨테이너·새 의존성·외부 코드를 도입하지 않았다.
- [Tokio TcpSocket](https://docs.rs/tokio/latest/tokio/net/struct.TcpSocket.html): 기존 Tokio 의존성의
  bind·소켓 수명 API를 사용한다.
