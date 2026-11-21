# macOS Intel 지정 계정 예약 재연결 수리

두 Intel CI에서 같은 `m13-81` 시험이 실패했다. 두 번째 실행에서는 새 host의 첫 front 응답은 5초 안에 왔고, 실제 agent 연결은 시험이 실패를 선언한 뒤에 왔다. 출처는 `artifact/rust/251-m13-81.log`, `artifact/rust/summary.json`, 전체 `job.log`의 실패 요약에 실린 `host-pinned-account.log`다. [ae1f77d5 CI](https://github.com/cLazyZombie/aaa/actions/runs/44915853117/job/136881176935)

## CI 시간축

두 번째 실행의 재시작 요청 `15:18:36.239 UTC`를 기준으로 삼았다. `machine list`의 경과는 첫 조회 시작 기준이라 아래 표에서 3ms를 더했다. GitHub의 로그 수신 시각 대신 시험과 backend가 적은 시각을 비교했다.

| 재시작 요청 기준 | 관측 |
| --- | --- |
| −5ms | 지정 계정·선행 조건이 담긴 예약 저장을 확인했다. 이전 host를 종료하고 금고 키를 옮겼다. |
| +0ms | 새 host spawn을 요청했다. agent는 같은 밀리초에 이전 host 연결 닫힘을 기록했다. |
| +3ms | spawn 호출이 반환했고 첫 `machine list`를 시작했다. 새 자식이 사용자 코드에 들어간 정확한 시각은 기록되지 않았다. |
| +3.233~4.667초 | 남아 있는 조회 8회가 front 소켓 연결 거부로 끝났다. |
| +4.700초 / +4.703초 | 새 프로세스가 `host 시작`과 TLS listener 열림을 기록했다. 시작 로그는 `Host::open` 반환 뒤, front bind는 TLS listener 뒤다. |
| +4.856초 | 첫 성공 조회를 시작했고 47ms 뒤 `labA=disconnected`를 받았다. |
| +5.146초 | 두 번째 성공 조회를 시작했고 34ms 뒤에도 `disconnected`였다. |
| +5.182초 | 시험이 5초 재연결 대기에서 실패했다. |
| +5.810초 | 새 host가 `labA` 연결을 기록했다. TLS listener 로그에서 약 1.107초 뒤다. |

예약은 host가 여는 SQLite의 `task_queue`에 들어 있고, `task queue` 목록 요청에서 읽는다. 위 실행은 재연결 단언에서 끝나 새 host의 예약 값을 조회하지 못했다. 금고 키를 옮긴 뒤의 오류는 전체 로그에서 agent 연결 다음에 보인다. 러너는 4 CPU 슬롯을 사용했고 실패 시점 1분 부하는 10.48이었다. CI 로그는 자식의 실행 진입·DB 열기·파일 해시 각각의 시간이나 agent backoff와 스케줄링 지연을 재는 표본을 담고 있지 않다.

첫 실패에서도 새 host의 TLS listener 로그 `02:41:07.789 UTC` 뒤 `02:41:09.194 UTC`에 agent가 연결됐다. 재시작 요청과 front 응답 시각은 당시 진단에 없었다. [fabc2557 CI](https://github.com/cLazyZombie/aaa/actions/runs/44854273377/job/136692649812) · [t-2119 진단](testing-macos-intel-reserved-account-t-2119.md)

## 수리와 검증

- 시험은 실험실의 기존 `wait_host_ready`로 새 host가 실제 front `status`에 응답할 때까지 기다린다. host가 먼저 종료되면 그 상태와 로그로 실패한다. 이후 기존 5초 안에 `machine list`의 `labA=connected`를 관측한다. 같은 예약·선행 조건 복원, 인증 잠김 대기, 사용 가능한 대체 계정이 있어도 미기동, 지정 인증의 agent 도착 단언을 유지한다.
- 호스트 시작은 공용 파일 SHA-256 구현을 재사용한다. 실제 실행 파일 전체를 읽고 프로세스가 처음 계산한 결과를 보관한다. macOS arm64의 같은 verify 실행 파일 약 236MiB로 계측했을 때 재시작 중 DB 열기는 두 조건에서 각각 3ms, 기존 SHA-256은 813ms, 공용 구현은 121ms였다. 재시작 요청에서 front bind까지는 각각 829ms와 136ms였다. 이 측정은 로컬 병목을 보여 주며 Intel CI의 4.7초를 구간별로 분해하는 근거는 없다.
- 로컬 진단에서 FIFO로 새 host 실행을 6초 보류했다. 기존 시험은 front 연결 거부를 기록하고 재연결 단계에서 종료 101로 끝났다. 같은 해시 경로에서 front 응답을 기다리도록 고치자 준비 뒤 11ms에 연결을 관측했고 마지막 인증 단언까지 통과했다. 공용 해시를 함께 쓴 경우에도 통과했다. 보류 장치와 임시 계측은 후보에서 제거했다.

준비를 별도로 관측하는 Kubernetes의 startup probe와 systemd의 `READY=1` 방식을 비교하고, 실험실에 이미 있는 front 응답 대기를 사용했다. [Kubernetes probe](https://kubernetes.io/docs/concepts/workloads/pods/probes/) · [Apache-2.0](https://github.com/kubernetes/kubernetes/blob/master/LICENSE) · [systemd 서비스 준비](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html) · [LGPL-2.1-or-later](https://github.com/systemd/systemd/blob/main/src/core/service.c)
