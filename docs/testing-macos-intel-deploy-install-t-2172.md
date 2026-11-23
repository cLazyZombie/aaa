# macOS Intel 배포·설치 시한 초과와 진단 보강

[15d05bf8의 Intel CI](https://github.com/cLazyZombie/aaa/actions/runs/45118727931/job/137507377103)에서 배포와 설치 시험이 서로 다른 구간에서 멈췄다. 역할 로그 회전은 두 실패 경로에서 실행되지 않았다. 같은 판의 Ubuntu·Windows job은 통과했다. Intel 두 실패 모두 정확히 어느 연산에서 지연됐는지는 원본 기록으로 확정할 수 없다.

## 배포

`05-m13-37.log`의 화면에는 1~9번의 완료와 10번 명령 입력이 있었다. 10번은 의도적으로 실패시킨 agent 재시작의 최종 상태를 `deploy status --wait`로 조회한다. 제출 응답 뒤 시작한 출력 대기가 30,011ms에 끝났다. 이후의 host·로컬 agent 재시작 검사는 실행되지 않았다. 원본 CI artifact에는 분할 시험 로그와 프로세스 시작·종료 기록이 있었고, 당시 실험실의 CLI 결과 파일과 복구 감시 기록은 보존되지 않았다. 따라서 10번의 실행 완료·PTY 출력·host의 복구 결과 중 어느 사건이 처음 빠졌는지는 확인할 수 없다.

이 시험은 모의 `launchctl`을 거치며 host와 agent의 stdout·stderr를 실험실 자체 파일에 연결한다. 운영 로그 경로와 열린 fd의 inode 대조에서 회전 진입이 제외된다. 회전 잠금도 `LOCK_NB`로 연다. t-2167의 계측은 rollout 상태 전이·조회 경로를 바꾸지 않았다. 완료 inbox와 coordinator 알림은 같은 host에서 처리하므로 원본 한 건에서 모든 간접 영향을 배제할 수는 없다.

시험은 probe 번호별 제출·PTY 입력 확인·화면 관측을 기록한다. 셸 시작·CLI 종료·PTY 쓰기는 실험실 표지 파일의 수정 시각으로 남긴다. 실패 순간에는 같은 shard 로그에 감독자 호출 수와 마지막 시각, host·agent의 연결 기록, host의 rollout 상태, agent의 독립 감시 단계와 재시작 시각을 싣는다. DB는 읽기 전용으로 열고 잠금을 기다리지 않는다. 출력 본문과 감시 오류 원문은 새 로그에 싣지 않는다.

## 설치

`08-install-13.log`의 실패 지점은 필수 인증 작업 뒤의 두 번째 `install authorize`다. 첫 작업은 이미 `FilesApplied`와 exit 3을 기록했다. 두 번째 CLI는 시작 17.091초 뒤 `yes`를 받았고 입력 읽기 반환과 승인 통과를 기록했다. 60.138초 상한에서 CLI와 PTY reader가 살아 있었고 직계 자식은 없었다. CLI의 누적 CPU는 사용자 25.592초와 시스템 7.143초였다. 러너의 1분 load는 10.37이었고 PTY 실행 구간 CPU tick 중 idle은 약 2%였다. 이 한 표본은 러너 포화와 승인 후 작업 지연을 뒷받침한다. CPU 시간은 승인 전 구간도 포함한다. 당시 호출 스택과 발급 단계별 시각은 없다.

macOS 설치 승인 CLI는 역할 로그를 회전시키지 않는다. 승인은 CLI 안에서 실행되고 승인한 뒤 현지 계획·범위를 재계산하고 발급 기록과 승인 파일을 쓰므로, 다음 실패에서 이 경계들을 확인할 수 있도록 기존 opt-in PTY 로그에 작업별 완료 표지를 추가했다. 시험 드라이버는 자식 종료 사건과 reader 완료 사건의 경과 시각을 따로 남기고, 시험이 지정한 승인 파일의 유무와 크기만 기록한다. t-2114는 입력 반환·승인 통과와 실패 시점의 OS 상태를 기록했다. 이번 보강으로 그 뒤 계획 재검사·발급·파일 동기화 중 어느 작업까지 돌아왔는지 구분할 수 있다. [t-2114 검증](testing-macos-intel-install-pty-t-2114.md)

## 비교 기록

직전 main CI 22회의 Intel artifact에서 `rust/plan.json`의 정확한 시험명과 `rust/events.jsonl`의 시작·종료 사건을 맞췄다. 아래 시간은 시험 전체 시간으로, 개별 30초·60초 대기의 분포는 아니다.

| 대상 | 변경 전 22회 최저·중앙·최고 | 직전 `36c72bde` | `15d05bf8` 실패 |
| --- | ---: | ---: | ---: |
| 배포 | 50.05 · 77.15 · 135.91초 | 76.00초 | 124.87초 |
| 설치 | 58.39 · 80.49 · 170.40초 | 71.13초 | 162.26초 |

두 시험은 직전 22회에서 모두 통과했다. 앞선 CI의 m13 job 중단 당시 이 시험이 미완료였다는 인계 기록이 있으나 10번 probe의 재발로 셀 수 있는 원본은 없었다. `e17a0bd4`에서는 다른 설치 owner 세 개가 입력을 보낸 뒤 60초 상한에 닿았다. 해당 run에서 이번 설치 시험은 160.66초에 통과했다. [직전 CI](https://github.com/cLazyZombie/aaa/actions/runs/45107175582) · [오래 걸린 통과 CI](https://github.com/cLazyZombie/aaa/actions/runs/45082839160) · [기존 PTY 실패 CI](https://github.com/cLazyZombie/aaa/actions/runs/44832774284/job/136621593363)

운영에서는 14:41~15:34 KST의 활성 로그 948줄과 회전 사본 마지막 한 줄을 읽어 배포·설치 재시작 지연과 로그 회전 오류 표지를 확인했다. 표본에서 같은 증상이나 비교할 승인·배포 실행은 관측되지 않았다. 운영 설치 CLI의 개별 PTY는 host 로그의 관측 범위 밖이다.

다음 자연 실패에서 배포 CLI가 이미 종료했으면 출력 표지 전달을, rollout이 진행 중이면 감시 단계와 감독자 호출을 먼저 확인한다. 설치는 마지막 작업 표지에서 멈춘 구간의 비용을 재고, 반복 검사 비용이 원인일 때 실제 승인·파일 재사용 계약을 유지한 채 그 경로를 줄인다. 이번 후보의 시한과 단언은 원래 값을 유지한다.

## 개발 검증

| 실행 환경과 판 | 배포 시험 | 설치 시험 |
| --- | ---: | ---: |
| macOS arm64 · 변경 전 `36c72bde` | 10.58초 통과 | 11.82초 통과 |
| macOS arm64 · `15d05bf8` | 9.36초 통과 | 10.56초 통과 |
| macOS Rosetta x86_64 · `15d05bf8` | 95.51초 통과 | 63.02초 통과 |
| macOS Rosetta x86_64 · 진단 후보 | 94.53초 통과 | 60.76초 통과 |

Rosetta는 별도 격리 사본에서 실행했다. x86_64 `xcrun`이 arm64 전용 CLI Tools 라이브러리를 열지 못해, 그 사본의 PATH에서 직접 확인한 네이티브 `strip`을 사용했다. Intel CI의 OS·동시 부하를 재현하는 환경은 아니다.

후보의 macOS 설치 owner 36개와 의존성 경계 2개가 통과했다. 기존 실제 LaunchAgent 등록 owner 한 개는 CI 전용 ignored였다. macOS native 및 Linux·Windows cross strict Clippy, Rust 포맷을 확인했다.

격리 변형에서 10번 probe의 출력만 보류하자 기존 30초에 실패했다. 셸·CLI 종료 기록과 감독자 호출 세 번, host의 `RecoveryFailed`, agent 감시의 `Failed`를 보존해 출력 대기에서 실패했음을 확인했다. 두 번째 승인 발급에서 범위 대조 직후 실행을 보류한 변형은 기존 60초에 실패했다. 마지막 작업 표지와 승인 파일 미생성, CLI 종료·reader 완료의 미관측을 함께 보존했다. 변형은 진단 확인에만 사용했다.

## 참고한 방식

cargo-nextest의 실패 시 시험 출력 보존과 pytest의 시한 시점 진단을 비교했다. 기존 shard 출력에 필요한 자료를 싣고, 프로세스를 정리하기 전에 상태를 수집하는 방식을 적용했다. 외부 코드는 가져오지 않았다. [cargo-nextest 출력](https://www.nexte.st/docs/reporting/) · [cargo-nextest MIT 라이선스](https://github.com/nextest-rs/nextest/blob/main/LICENSE-MIT) · [pytest 실패 진단](https://docs.pytest.org/en/stable/how-to/failures.html) · [pytest MIT 라이선스](https://github.com/pytest-dev/pytest/blob/main/LICENSE)
