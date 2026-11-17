# GitHub CI 반복 실패 안정화(t-1980)

## 범위와 관찰

2026-11-17에 최근 ci 실행 50개의 메타데이터를 조회하고, 그중 최근 32개(11-14~11-17)의
job·step 결과와 실패 로그를 조사했다. 18개 성공, 14개 실패다. 실패 job은 웹 8건,
Windows 5건, Ubuntu 2건이며 웹·Windows 동시 실패가 두 번 있다. 나머지 한 실행은
web·macOS가 cancelled이고 실패 step이 없어 시험 실패로 분류하지 않았다.

이 수치는 변경 전 표본이며 제품 결함·시험 결함·러너 장애를 합친 것이다. 로컬 통과로
GitHub 러너의 개선율을 대신하지 않는다. 최초 실패율과 재확인 통과율을 따로 비교한다.

| 실행(UTC) | SHA | 결과 | 실패 단계·spec·증상 |
| --- | --- | --- | --- |
| [2026-11-17 12:37:31](https://github.com/cLazyZombie/aaa/actions/runs/44084891588) | `26d703c9` | 실패 | 웹 E2E / 136 / route.continue 중복 처리 |
| [2026-11-17 11:18:59](https://github.com/cLazyZombie/aaa/actions/runs/44072898718) | `5ba5cd77` | 성공 | — |
| [2026-11-17 10:52:50](https://github.com/cLazyZombie/aaa/actions/runs/44068875658) | `e94cb512` | 성공 | — |
| [2026-11-17 09:53:10](https://github.com/cLazyZombie/aaa/actions/runs/44059668753) | `813f6af9` | 성공 | — |
| [2026-11-17 08:47:00](https://github.com/cLazyZombie/aaa/actions/runs/44049673196) | `a9ad1f3e` | 실패 | 웹 E2E / 133 휴대폰 WebKit waitFor 30초; Windows test / 질문 guard 실제 경과 5초 단언 |
| [2026-11-17 07:58:44](https://github.com/cLazyZombie/aaa/actions/runs/44042860898) | `d43b80f7` | 실패 | 웹 E2E / 138 휴대폰 reload ERR_ABORTED |
| [2026-11-17 07:39:35](https://github.com/cLazyZombie/aaa/actions/runs/44040406136) | `e2462985` | 성공 | — |
| [2026-11-17 07:28:30](https://github.com/cLazyZombie/aaa/actions/runs/44038980693) | `e82cf487` | 실패 | 웹 E2E / 02 새 pane waitFor 30초 |
| [2026-11-17 06:35:58](https://github.com/cLazyZombie/aaa/actions/runs/44032403020) | `64e8eb3a` | 실패 | Windows test / install recovery PTY 60초, 입력 전 자식 종료 대기 |
| [2026-11-17 03:33:08](https://github.com/cLazyZombie/aaa/actions/runs/44011008060) | `df62707e` | 성공 | — |
| [2026-11-17 02:41:00](https://github.com/cLazyZombie/aaa/actions/runs/44004774599) | `204b1338` | 성공 | — |
| [2026-11-16 23:10:49](https://github.com/cLazyZombie/aaa/actions/runs/43976977936) | `ed396285` | 성공 | — |
| [2026-11-16 22:34:06](https://github.com/cLazyZombie/aaa/actions/runs/43971294699) | `1e0dc214` | 성공 | — |
| [2026-11-16 20:59:23](https://github.com/cLazyZombie/aaa/actions/runs/43955405926) | `f9ca9f13` | 실패 | web·macOS job cancelled, 실패 step 없음(원인 미분류) |
| [2026-11-16 20:35:03](https://github.com/cLazyZombie/aaa/actions/runs/43951288399) | `feae1eb6` | 실패 | Windows test / install wizard PTY 60초, Enter 관측 뒤 다음 prompt 없음 |
| [2026-11-16 20:23:09](https://github.com/cLazyZombie/aaa/actions/runs/43949223552) | `b10c039c` | 성공 | — |
| [2026-11-16 20:10:29](https://github.com/cLazyZombie/aaa/actions/runs/43946995158) | `dfc4acaf` | 실패 | 웹 E2E / 134 WebKit fill 30초 |
| [2026-11-16 12:08:04](https://github.com/cLazyZombie/aaa/actions/runs/43863802717) | `d76f60a1` | 성공 | — |
| [2026-11-16 06:24:41](https://github.com/cLazyZombie/aaa/actions/runs/43816556117) | `8c27303f` | 실패 | Windows test / install wizard·worker_lifetime PTY 60초 |
| [2026-11-16 04:52:41](https://github.com/cLazyZombie/aaa/actions/runs/43806693954) | `ed43c32e` | 성공 | — |
| [2026-11-16 03:54:16](https://github.com/cLazyZombie/aaa/actions/runs/43800872516) | `120451b1` | 실패 | 웹 E2E / 109 coordinator 본문 20초; Windows test / aaa-host 스택 고갈 0xc00000fd |
| [2026-11-16 02:07:20](https://github.com/cLazyZombie/aaa/actions/runs/43790218035) | `e1742763` | 성공 | — |
| [2026-11-15 23:28:17](https://github.com/cLazyZombie/aaa/actions/runs/43774982698) | `8e8cb2c4` | 성공 | — |
| [2026-11-15 23:14:46](https://github.com/cLazyZombie/aaa/actions/runs/43773749470) | `8ebc07f0` | 성공 | — |
| [2026-11-15 00:23:01](https://github.com/cLazyZombie/aaa/actions/runs/43657893825) | `c999ccab` | 성공 | — |
| [2026-11-14 23:56:57](https://github.com/cLazyZombie/aaa/actions/runs/43655738672) | `1cad4151` | 성공 | — |
| [2026-11-14 23:45:25](https://github.com/cLazyZombie/aaa/actions/runs/43654811095) | `f04267bb` | 실패 | 웹 E2E / 118 휴대폰 인증 창 애니메이션 중 위치 단언 |
| [2026-11-14 17:16:00](https://github.com/cLazyZombie/aaa/actions/runs/43620151649) | `977dbacd` | 실패 | 웹 E2E / 109 화면 복귀 중 거짓 읽기 전용·빈 본문 |
| [2026-11-14 16:22:06](https://github.com/cLazyZombie/aaa/actions/runs/43615221724) | `9e1c6a3a` | 실패 | Ubuntu test / machine_setup 실험실 sshd 준비 오류 |
| [2026-11-14 03:33:21](https://github.com/cLazyZombie/aaa/actions/runs/43552238839) | `14698f3e` | 성공 | — |
| [2026-11-14 02:15:20](https://github.com/cLazyZombie/aaa/actions/runs/43545251543) | `a448941f` | 실패 | Ubuntu test / thread_park 후속 남음·task_dependencies 중간 상태 단언 |
| [2026-11-14 00:40:34](https://github.com/cLazyZombie/aaa/actions/runs/43535776603) | `ad9ffa9a` | 성공 | — |

## 원인 묶음과 대책

| 묶음 | 근거 | 대책·남는 한계 |
| --- | --- | --- |
| 웹 러너 자원 중복 | 성공 실행 `44072898718`도 3 CPU·worker 2에서 load 15.35→15.58. 45 spec·56 작업에 568초 | 세 독립 macOS 러너로 spec을 나눠 러너마다 worker 1. load가 반드시 낮아진다고 단정하지 않고 변경 후 기록과 비교 |
| Rust 중첩 병렬도 | Unix owner 실행기는 CPU 슬롯만큼 binary를 실행하지만 workflow가 binary마다 8 thread를 지정 | Unix libtest 1, Windows 2. CI test 단계의 자식 Tokio runtime도 2 thread. Cargo 컴파일 병렬도는 유지 |
| 조작 준비·기록 복귀 경합 | 02·133·134의 요소 기다림과 138 reload 실패 | t-1967의 사건 대기·sheet 기록 복귀 수리를 유지. 재시도로 이 수리를 대체하지 않음 |
| route 수명 경합 | 136 `Route is already handled!` | 착지 main의 수리 뒤에도 처음·재확인 모두 실패. HTTP route를 제거하고 실제 읽기 응답의 전달만 사건으로 보류 |
| 같은 상태의 시점 차이 | 로컬 전수 136에서 블록 칩은 작업 중, 뒤에 읽은 글 속 칩은 검수 대기. 두 상태 모두 허용된 전이 | 한 번의 page.evaluate로 두 칩을 읽고 같은 순간의 일치를 엄격히 단언. 기다림·허용 값·시한은 유지 |
| 실제 경과 단언 | Windows 질문 guard 5초 단언 실패 | t-1967에서 전달 완료 단언으로 이미 수리됨. 추가 완화 없음 |
| Windows 설치 PTY 정체 | 3개 실행에서 60초. Enter 관측 후 `ReadConsoleW`가 끝나지 않는 경우와 입력 전 자식 종료 대기를 구분 | 부하를 줄여도 재발. Enter 도착·ReadConsoleW 대기 서명이 확인된 세 owner만 새 프로세스에서 1회 재확인. 원문 보존·경고·실패 전파. 원래 콘솔 정체의 원인은 아직 미확정 |
| 과거 실제 결함·관찰 시점 | Windows 스택 고갈, Ubuntu sshd 준비·후속 처리, 웹 복귀·애니메이션 단언 | 원래 실패를 이력에 남김. 광범위한 무시·continue-on-error를 추가하지 않음 |

## 실행 정책과 보존 범위

| 항목 | GitHub CI | 로컬 착지 gate |
| --- | --- | --- |
| 웹 spec·기기 조합·단언 | 전부 유지, spec 안 상태를 이어서 실행 | 전부 유지 |
| 웹 병렬도 | 3 shard × 러너별 worker 1 | 기존 CPU 예산 |
| 실패 재확인 | 웹은 실패 작업 1회. Windows는 아래의 확인된 ConPTY 서명·세 owner만 새 프로세스 1회 | 0회 |
| 재확인도 실패 | job 실패 | 최초 실패로 실패 |
| 최초 실패 뒤 통과 | warning, step summary와 시도별 JSON에 별도 표시 | 해당 없음 |
| 시간 제한 | locator 30초·단계 120초 유지. 웹 job 40분·E2E step 30분. Rust는 Intel Mac job 60분·test 25분, 나머지 job 40분·test 15분 | 변경 없음 |
| Rust 선택·시한·단언 | 시험 제외 없음. macOS Intel·Linux·Windows 전수와 native 설치·SSH 유지 | 변경 없음 |
| 제거·ignore·CI 제외 | 시험 삭제·ignore 없음. macOS arm64 자동 실행은 명시적 진단으로 분리 | 없음 |

`AAA_E2E_SHARD`는 같은 spec 목록과 시간 원장으로 긴 spec부터 세 묶음에 분배한다.
준비와 결과를 이어 쓰는 단계를 다른 러너로 찢지 않는다. 알려지지 않은 spec은 기존 최장
시간으로 예약한다. 원장은 성공한 위 실행의 45 spec 점유 시간(분할 spec은 작업 합계)을
올림한 초로 갱신했다. 세 묶음의 원장 합계는 385·382·385초다(벽시계 예측이 아니다).
시간 값은 선택·성공 단언에 사용하지 않는다. 한 workflow의 macOS job은 웹 셋·Rust 하나로
늘어나므로 빌드와 러너 비용은 늘 수 있다. 부하와 전체 종료 시간을 함께 비교한다.

CI 최초 전수가 끝난 뒤에만 실패 작업을 다시 실행하므로 재확인 때 다른 작업이 자원을
점유하지 않는다. 기존 독립 단계 분할을 사용한 직접 실행에서는 같은 단계 필터를 다시 쓰고,
CI worker 1에서는 spec 전체와 그 준비를 새 실험실에서 실행한다.

실패 child의 원문은 e2e.log에, 화면·trace 등 기존 진단은 PID별 폴더에 남는다.
재확인 성공이어도 지우지 않는다. `run-timings.jsonl`에 shard 선택·시도 번호·시작/끝·부하를,
`run-summary.json`에 최초/재확인 종료 코드·최종 결과를 기록한다. artifact 이름에는 shard를
포함하여 세 업로드가 충돌하지 않는다. `workflow_dispatch`를 추가해 승인된 branch에서
전체 ci를 반복 실행할 수 있게 했다. main에 파일이 이미 등록된 workflow라 branch 판의
workflow_dispatch도 실제 동작함을 확인했다. 최초 push 뒤 같은 SHA를 두 번 명시적으로 dispatch한다.

`ci-local.sh`는 CI 환경 없이 분할·재확인 설정을 받으면 실패한다. `verify.py landing`은
CI 환경에서도 `AAA_E2E_ONLY`·`AAA_E2E_SHARD`·0이 아닌 `AAA_E2E_RETRIES`와
`AAA_CI_WINDOWS_INSTALL_RECHECK`를 거부한다.
따라서 CI에서 허용한 예외가 착지 전수 검사를 줄이지 않는다.

## 참고한 공개 구현

- [Playwright CI](https://playwright.dev/docs/ci)·[재시도](https://playwright.dev/docs/test-retries)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)):
  CI worker 1과 러너 간 sharding, 실패 worker를 버리고 새 worker에서 준비부터 실행하는 방식을 참고했다.
- [Cypress 재시도](https://docs.cypress.io/app/guides/test-retries)
  ([MIT](https://github.com/cypress-io/cypress/blob/develop/LICENSE)):
  개발 기본과 CI 재시도 정책을 분리하고 최초 실패와 최종 통과를 구분하는 방식을 비교했다.

코드를 복사하거나 새 실행 의존성을 추가하지 않았다. 기존 독립 프로세스 runner에 최소한의
spec 분배·재확인 기록을 더했다. 시험 삭제·기기 조합 축소·개별 시험의 전역 timeout 확대는 채택하지 않았다.

## 검증

- 실제 runner와 가짜 child를 실행하는 검사: 실패만 1회 재확인, 서로 다른 PID, 최초 산출물 보존,
  두 번 실패한 작업의 종료 코드 1, 기본 재확인 0, 모든 spec의 shard 합집합·중복 없음, 잘못된 설정 거부.
- 재확인 경로를 제거한 임시 변이에서 원래 회복할 사례가 종료 코드 1로 바뀌는 것도 확인했다.
- Python 관련 owner: 로컬 ci-local 분할·재확인 거부, CI 환경에서도 착지 축소 거부와 기존 실행·잠금 계약.
- 웹 타입·번들·유닛 전체와 실제 host·agent·Chromium·WebKit E2E 02·133·134·138 통과.
- 전체 shard 최초 실행에서 136의 칩 상태 비교 경합을 발견했다. 블록의 `작업 중 d-0001 · t-0004`를
  읽은 뒤 글 속 칩을 읽을 때 `검수 대기 d-0001 · t-0004`로 바뀌었다. 두 표시를 같은 DOM
  차례에서 읽도록 수리했고, 최초 실패 원문·화면은 작업 전용 산출물에 보존했다.

| 개발 확인 | 결과 |
| --- | --- |
| runner·배치·수집 관련 Node 검사 | 18개 통과. 최종 종료 코드 기록 보강 뒤 runner 4개 재확인 통과 |
| CI 실행·착지 범위·자원 Python owner | 25개 통과 |
| 웹 타입·번들·유닛 전체 | 750개 통과 |
| 실제 E2E 02·133·134·138 | 모두 통과 |
| 실제 shard 1/3, worker 1, 재확인 0 | 15 spec 통과, 246.6초 |
| 실제 shard 2/3, worker 1, 재확인 0 | 14 spec 중 136 한 건 실패, 280.1초. 최초 실패 종료 코드 1 보존 |
| 실제 shard 3/3, worker 1, 재확인 0 | 16 spec 통과, 275.3초 |
| 칩 비교 수리 뒤 E2E 136 전체, 새 실험실 3회 | 3/3 통과, 40.3·41.3·40.5초, 재확인 0 |
| workflow YAML 읽기·diff 공백·공개 검사 | 통과 |

45개 spec을 전부 실제 실행했고 136에서 발견한 경합을 수리한 뒤 해당 spec 전체를 세 번 확인했다.
최초 전수의 실패를 삭제하거나 재확인 성공으로 바꾸지 않았다. 마지막 136 수리만 별도 확인한
결과이므로 최초 전수 실행을 단일 최종 SHA의 gate 통과로 기록하지 않는다.

원문·화면은 작업 전용 target 아래에 보존한다. 위 표는 GitHub push 전의 개발 확인이다.
이후 q-4171 1A로 진단 push·CI 3회를 승인받았으며, 착지 main 포함과 반복 CI의 결과는 아래에 기록한다.

착지 창의 최종 통합 SHA에서 전체 ci-local과 양 OS strict cross를 수행하는 절차는 유지한다.
이 개발 단계의 부분 검사를 최종 착지 gate 통과로 기록하지 않는다.


## 진단 후보 선행 통합과 추가 수리

q-4171 1A로 진단 branch push·전체 CI 3회 실행을 승인받았다. t-1978은 E2E 53의
brief 준비 오류로 착지 gate를 중단해 main에 없었다. main을 변경하지 않고 승인된
수리 후보 `6a29be9f`를 이 후보의 부모로 포함했다. 소스의 route 변경과 통계 대기를
검토했고 기존 칩 비교 수리도 유지했다. testing.md 링크 충돌은 두 문서를 모두 남겼다.

E2E 53은 첫 report_hash만 확인한 뒤 답을 보냈다. `orch/report_tests.rs`는 보고 회수가
brief 전달 확인보다 먼저 올 수 있음을 두 순서로 검사하며, `harness.rs`의 thread_message는
brief가 delivered가 아니면 답을 거부한다. 따라서 첫 보고와 delivered를 함께 기다리도록
E2E 53의 네 준비와 공용 question-reply 시나리오를 고쳤다. 기존 시한·답 내용·전달 단언은 유지한다.
이것은 t-1978 착지 실패의 API 오류와 일치하는 준비 조건 수리이며 부하만 원인으로 단정하지 않는다.

선행 후보 통합과 brief 준비 수리 뒤 `ci-local --only web,e2e,public --spec 53,136`을
`aaa verify`로 실행했다. 웹 타입·번들·750개 유닛·실제 E2E 53·136·공개 검사가 모두 통과했다.


## 첫 진단 CI에서 발견한 Windows 목록 검사 오류

[첫 실행](https://github.com/cLazyZombie/aaa/actions/runs/44100000910)의 Windows 웹 검사에서
이번에 추가한 shard 합집합 검사가 실패했다. 파일 URL의 pathname을 CLI 인자로 써
Windows에서 드라이브 이름이 중복된 MODULE_NOT_FOUND가 났다. OS 경로 변환은
`fileURLToPath`로 고쳤으며 해당 runner 검사 4개를 로컬에서 다시 확인했다.
첫 실패 원문은 보존하고 수리한 새 SHA의 CI 결과와 구분한다. 시험 제외·재시도·시한 변경은 없다.


## 착지 main 통합

coordinator 지시에 따라 main `35ebd550`을 진단 branch에 통합했다. t-1978의
route 수리와 brief 전달 수리가 포함된다. 독립적으로 같은 준비 조건을 고친 두 파일의
충돌은 main 판을 사용해 해소했다. 조건은 같고 대기 이름·주석 차이만 있었다.
testing.md는 양쪽 검증 문서 링크를 모두 남겼다. 칩 비교·CI 분산·Windows URL 수리는 유지한다.
이전 두 후보의 CI는 별도 기록으로 보존하고, 이 통합 뒤 같은 SHA에서 전체 CI 3회 성공을 확인한다.


## Windows SCM 준비 검사의 추가 실패

최종 후보 `d01e3aab`의 [세 번째 실행](https://github.com/cLazyZombie/aaa/actions/runs/44104967730)은
Windows 기본 시험을 통과했으나, SYSTEM SSH 실제 owner에서 `service_ready_unverified`로 실패했다.
실제 owner 37.43초, 원격 install은 exit 3·NeedsUser였고 시험은 Installed를 요구해 실패했다.
시한 만료 코드와는 다르다. 첫 두 Windows 전체는 성공했으며 이 실패를 성공 횟수에 넣지 않는다.

기존 오류는 준비 증명의 PID·context·account·profile·binary·session·boot와 연결·프로세스 관측을
한 코드로 뭉쳤다. 추가로 worker가 NeedsUser 문구를 다시 쓰며 하위 원인을 지웠다.
실패한 고정 검사 이름만 Cause에 보존하고 worker에서도 유지하도록 진단을 보강한다.
실제 계정·경로·해시 값은 출력하지 않는다. 모든 거부 조건·시한·실패 종료는 유지한다.

관련 실제 backend/영수증 owner 7개가 통과했다. 변조한 필드가 해당 원인 이름으로 거부되는지 검사했다.
원래 실패 VM의 proof는 보존되어 있지 않아 어느 필드가 실제로 달랐는지는 아직 모른다.
Windows만 실행하는 명시적 진단 모드에서는 서로 다른 일회 사용자·실험실로 실제 SYSTEM SSH owner를
최대 12회 수행하되 첫 실패에서 중단한다. 이 부분 진단을 전체 CI 성공으로 집계하지 않는다.

Windows 서비스 진단은 [WinSW 문제 해결](https://github.com/winsw/winsw/blob/v3/docs/troubleshooting.md)
([MIT](https://github.com/winsw/winsw/blob/v3/LICENSE.txt))의 wrapper/backend 관찰 분리와
[windows-service-rs](https://github.com/mullvad/windows-service-rs)
([MIT/Apache-2.0](https://github.com/mullvad/windows-service-rs/blob/master/README.md))의
SCM 상태 전이 구분을 대조했다. 코드를 가져오거나 준비 증명 검사를 약화하지 않는다.


## 러너 배정 실패 분리

통합 후보의 첫 전체 실행에서 macOS Rust와 웹 shard 하나는 runner 이름·단계·로그 없이 취소됐다.
GitHub check annotation은 hosted runner를 여러 번 시도해도 얻지 못했다고 명시하고
macOS arm64 용량 부족을 알렸다. 시험 실패나 사용자의 취소로 집계하지 않는다.
처음에는 웹 세 shard만 Intel로 옮기고 Rust arm64를 유지했다. 이후 같은 후보의 Rust arm64 세 작업이
20분 넘게 한 번도 배정되지 않아 기본 Rust macOS도 Intel로 옮겼다. 실제 LaunchAgent·SSH와
웹의 Chromium·WebKit·기기 조합은 유지한다. arm64 전수·native owner는 명시적
`macos_arm_diagnostic` 실행으로 남기고 착지 기기(arm64)의 전수 gate도 유지한다.
기본 CI에서 arm64 실행 결과를 얻지 못한다는 차이를 명시한다.
Cargo 캐시에 runner.arch를 넣어 두 아키텍처의 산출물이 같은 키를 차지하지 않게 한다.

[GitHub의 공식 러너 표](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)는
macos-15-intel을 Intel 4 CPU·14 GB RAM, macos-15를 arm64 3 CPU·7 GB RAM으로 명시한다.
Intel에서도 같은 전체 CI 후보의 실제 결과를 확인하며 대기 실패가 없어졌다고 미리 단정하지 않는다.


## E2E 136의 HTTP route 의존 제거

착지 main을 포함한 후보는 처음과 허용된 재확인 모두 `route.continue`에서 실패했다.
추적만 추가한 GitHub arm64 독립 진단 8회와 로컬 Node 24·26 각각 5회는 성공했으므로
자연 실패의 정확한 내부 사건 순서는 아직 입증하지 못했다.

화면에서 필요한 계약은 읽기 중 그림 붙이기·통계 미도착 시 버리기 거부·초기 branch 선택이다.
이를 위해 HTTP interceptor를 쓰지 않고, 실제 fetch를 원래 인자로 한 번 실행한 뒤
실제 응답의 화면 전달만 exposeBinding 사건으로 보류한다. 원본 응답의 본문·상태·헤더를 그대로 반환한다.
배경 갱신은 응답 도착 때가 아니라 요청 시작 때의 실제 pointerdown 표지로 가려 교착을 막는다.
136의 두 페이지에서 route 등록·해제·continue를 모두 제거했다. 제품·시한·화면 단언은 바꾸지 않는다.

실제 loopback HTTP·Chromium·WebKit으로 인증 cookie·추가 헤더·본문·201 상태·응답 헤더가 유지되고,
서버가 처리한 응답만 화면에 보류되며, 다른 API는 바로 끝나고 요청이 중복되지 않음을 확인했다.
요청 시작 뒤 pointer 표지가 바뀌어도 배경 요청은 막지 않는 순서도 사건으로 고정했다.
이 변경의 실제 host·agent E2E 136 전체는 Node 24에서 새 실험실 3/3 통과했다(재확인 0).

이 방식은 기존 E2E의 panel-scroll-capabilities와 composer-attachments에서 실제 응답 전달을
보류하는 구조를 따른다. 별도 라이브러리나 mock 응답을 추가하지 않았다.


## Windows 반복 진단의 자원 경계

준비 원인 진단의 실제 SYSTEM SSH·SCM owner는 새 계정 6회 성공했다. 일곱 번째는
서비스 검증 전에 `work_space_insufficient`로 실패했다(20GB 필요, 19.79GB 남음).
사용자·서비스를 지워도 일회 사용자 프로필의 설치 사본은 VM 종료까지 남는다.
성공 프로필 삭제를 추가한 후보는 설치 검사 자체 3/3 성공 뒤 새 정리 guard로 실패했다.
이 추가 정리는 철회했다. 정상 CI는 원래처럼 설치 owner 1회이고, 선택한 준비 진단만
같은 VM 자원 안에서 3회 수행한다. 기본 20GB 조건·설치 단언·실패 중단은 유지한다.
원래 `service_ready_unverified`는 이후 실제 owner 9회에서 재현되지 않아 안전한 필드 원인 진단을 남긴다.

## Windows ConPTY의 제한된 재확인

부하를 줄인 뒤에도 승인 입력 6/6을 쓴 상태에서 Enter가 원래 입력 큐에 있고,
원래 stdin 읽기 thread가 `ReadConsoleW` I/O pending인 정체를 다시 관찰했다.
시한을 늘리거나 제품의 입력·승인 처리를 추측으로 바꾸지 않는다. 사용자 승인 범위에서
GitHub Windows에만 다음 세 owner의 같은 서명이 있을 때 1회 재확인한다.

| owner | 유지하는 계약 |
| --- | --- |
| `worker_lifetime::approved_worker_outlives_its_cli_and_records_the_same_id_after_private_display_disappears` | 실제 부모 Job 종료·breakaway 허용/거부·동일 ID 내구 기록 |
| `wizard::both_wizards_back_from_host_choices_then_submit_the_same_agent_plan_as_explicit_cli` | 두 prompt의 뒤로 이동·같은 planner/hash·변경 없음 |
| `recovery::closed_enrollment_window_keeps_the_id_and_action_after_files_are_applied` | 실제 적용 파일 보존·같은 설치 ID·재개 작업 |

첫 Cargo workspace 실행은 전부 끝낸다. 실패 target이 install 하나이고 실패 이름이 모두 위 목록에 있으며,
각 실패 본문에 PTY 사건 상한·ReadConsoleW·원래 reader pending·Enter 도착이 모두 있어야 한다.
다른 단언·다른 owner·다른 target·컴파일 오류는 재확인하지 않고 원래 실패를 반환한다.
확인된 owner는 Cargo의 exact 선택으로 새 테스트 프로세스·새 Lab에서 딱 한 번 다시 실행한다.
한 번 더 실패하면 job도 실패한다. 최초 원문·재확인별 원문·JSON 요약·GitHub 경고·step summary를 남긴다.

로컬과 착지 gate는 이 경로를 쓰지 않는다. 로컬 ci-local과 CI 환경의 landing도 해당 flag를 거부한다.
시험 삭제·ignore·Windows 고유 Job 사례 제외는 없다. 재확인 성공을 최초 성공으로 집계하지 않는다.

실제 자식 실행 회귀는 새 PID·정확한 단일 owner·원래 실패 보존·재확인 실패 종료·
다른 오류 거부·자식 종료 전 줄바꿈 없는 원문 보존을 사건으로 확인했다.
실제 실패 CI의 원본 test.log도 정확히 해당 worker owner 하나로 분류됨을 대조했다.

## 종료 장벽 경합의 결정적 확인

수리 전 Intel 진단은 버리기 종료에서 같은 예외를 냈다. 단일 route handler의 두 요청을 보류하고
unrouteAll(wait) 뒤 빠른 콜백만 먼저 완료시키면, Page._onRoute가 빈 client routes를 보고
서버 interceptor를 먼저 제거해 느린 요청도 통과시킨다. 그 뒤 느린 콜백의 continue가 실패한다.
공개 API·실제 HTTP·Promise 사건만으로 Chromium·WebKit에서 같은 예외를 재현했다.
새 응답 보류기는 route 등록·제거를 사용하지 않는다.
후보의 첫 GitHub 웹 전수는 세 shard 모두 성공했다.


Intel 웹의 실제 전수는 shard별 599·643·750초였고 첫 codegen을 포함한 job은 24~29분이었다.
새 Intel Rust는 첫 verify codegen·전수·native 설치를 한 job에서 수행하므로 집계 예산을
60분, test 단계를 25분으로 둔다. 개별 owner·설치 준비·SSH·브라우저의 실패 시한은 늘리지 않았다.
새 아키텍처의 전체 비용을 수용하는 집계 예산이며 원인이 있는 개별 실패를 기다려 통과시키는 변경은 아니다.


## 긴 줄 파서의 벽시계 단언 교체

후보 72a9888b의 두 번째 실행은 웹 유닛 designModel의 긴 줄 검사에서 실패했다.
기존 검사는 네 입력을 처리한 합계가 1초 미만이어야 했고 Intel 러너의 실측은 2.027초였다.
정규식은 이미 되돌기를 제한했지만 CPU 배정 시간을 포함한 벽시계 값이 성공 조건이었다.

20만 공백·10만 겹친 그림 시작·긴 제목·CRLF 표지의 입력 크기는 유지했다.
각각의 표지·제목·본문·그림 상태·절·덩어리 결과를 정확히 단언하고, 해당 네 해석을
별도 Node 프로세스에서 실행한다. 자식 완료로 즉시 끝나며 실제 되돌기 정체만
30초의 소유한 자식 종료 상한으로 차단한다. 동기 정규식이 부모 검사 timer를 막지 못한다.
제품 파서·시나리오·시험 수는 바꾸지 않았다.

관련 22개와 웹 750개가 통과했다. 제목 읽기에 과도한 되돌기를 되살린 별도 변이는
동일한 30초 자식 종료 상한에서 실제로 차단됐다. 정상 완료와 이 의도된 변이 거부를 구분해 보존한다.
[Node execFileSync](https://nodejs.org/docs/latest-v24.x/api/child_process.html#child_processexecfilesyncfile-args-options)의
자식 실행·timeout·killSignal 경계와 기존 E2E runner의 독립 자식 방식을 따른다.
