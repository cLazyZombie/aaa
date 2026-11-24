# 작업 트리 준비 시험의 Git 진입 진단

main `0b595a53`의 macOS Intel CI에서 준비 시험 `configured_prepare_timeout_recovers_only_unchanged_worktree`가
`m13-53`에서 55.51초 만에 종료 코드 101로 끝났다. shard 원문의 스택은 1초 준비 시한과 변경 파일 보존을
처음 검사한 뒤, journal 유무를 바꾸는 반복문의 `gate.take("add")`에 도달했음을 보여 준다.
두 반복 중 어느 것인지는 원문에 없다. 처음 누락이 관측된 사건은 시험 측 TCP accept다.
[macOS Intel CI](https://github.com/cLazyZombie/aaa/actions/runs/45308370753/job/138157656953)

`rust/events.jsonl`에서는 Rust runner 시작을 기준으로 4.738초에 시험 자식이 시작되고 60.251초에 종료됐다.
원본 shard와 업로드 목록에는 실험실 보존 경로만 있다. 해당 경로의 host·agent 로그와 Git 자식 및 소켓
기록이 업로드되지 않아, 준비 요청 미수신·Git 오류·Git 실행 중 시한·소켓 실패를 소급해서 가를 수 없다.
예전에 보고된 15분 단계 중단은 시험 내부 위치를 확인할 자료가 없다.

같은 SHA의 Ubuntu에서는 해당 시험이 11.78초, 직전 SHA `b1a080c1`의 Intel에서는 58.27초에 통과했다.
실패한 shard가 시작하고 끝날 때 러너의 1분 부하 평균은 각각 5.35와 7.66이었고 CPU 예산은 4였다.
이 수치와 전체 시험 시간으로 1초 준비 구간의 지연 위치를 특정할 수 없다.
[실패한 실행의 OS별 결과](https://github.com/cLazyZombie/aaa/actions/runs/45308370753) ·
[직전 실행](https://github.com/cLazyZombie/aaa/actions/runs/45307182933)

## 사건 기록

- 실험실에서 `aaa::thread_prepare` debug를 켠다. host의 요청과 응답 오류, agent의 요청 수신, 연결 등록을
  통과한 실행 시작과 종료를 남긴다. 요청 수신과 실행에 기록한 시한으로 두 단계 사이의 차이를 볼 수 있다.
- Git 래퍼는 gate가 열린 동안 준비에 앞선 Git 명령부터 기동 시도·자식 PID·종료 코드·접속 시도·연결·
  신호 송신을 UTC epoch ms로 기록한다. OS 오류는 단계·종류·오류 번호를 함께 남긴다.
- 시험은 TCP accept와 동작 신호 읽기를 따로 기록한다. 진입 실패 시 해당 thread ID와 실험실의 관련
  host·agent 로그를 가린 뒤 기존 shard 출력에 남긴다. 시험 러너가 panic 출력 전에 강제 종료되면 그 출력은
  확보되지 않을 수 있다.

제품의 준비·회수 처리, 1초 설정, Git 진입의 30초 안전 상한과 기존 단언은 그대로 적용한다.
Git 자식 수명과 출력 수집은 [Python subprocess](https://docs.python.org/3/library/subprocess.html)
([PSF License](https://github.com/python/cpython/blob/main/LICENSE))와
[Tokio process](https://docs.rs/tokio/1.53.3/tokio/process/struct.Command.html)
([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE))의 기존 API를 사용한다.

## 대조 검증

macOS arm64에서 기준판의 지정 시험은 11.75초에 통과했다. 진단 후보의 m13 준비 owner 세 건은
16.84초에 통과했다. 격리한 실험실에서 세 번째 준비에만 아래 조건을 주입했고 두 경우 모두
원래와 같은 TCP accept 시한에서 실패했다. 추가 사건으로 정지 위치를 구분할 수 있었다.

| 주입 조건 | 진단에서 확인한 순서 |
| --- | --- |
| Git 자식이 종료 코드 19 반환 | 준비 요청 수신 → Git 자식 시작과 종료 19 → host에 하위 오류 반환 → TCP accept 시한 |
| 실제 Git 성공 뒤 수신하지 않는 loopback 소켓으로 접속 | 준비 요청 수신 → Git 종료 0 → 접속 시도 → host의 1초 응답 시한 → agent의 Git 시한 → TCP accept 시한 |

둘째 대조에서 agent 요청 수신을 기준으로 Git 자식은 약 476ms에 시작하고 518ms에 종료했다.
같은 시각에 접속을 시도했고 host는 약 1,003ms에, agent는 약 1,441ms에 각각 시한을 기록했다.
Git 성공 뒤 연결이 끝나지 않은 경우와 Git 자체가 오류로 끝난 경우를 구별할 수 있다.
대조용 주입 코드는 실험실 밖으로 전달하지 않았다. 원래 CI의 누락 원인은 미확정이다.

포맷, 세 관련 Rust 패키지의 strict Clippy, 공개 저장소 검사도 통과했다.
