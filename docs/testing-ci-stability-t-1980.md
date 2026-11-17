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
| route 수명 경합 | 136 `Route is already handled!` | t-1978 선행 수리 필요. route 구간은 그대로 두고 아래 칩 비교 구간만 따로 수리 |
| 같은 상태의 시점 차이 | 로컬 전수 136에서 블록 칩은 작업 중, 뒤에 읽은 글 속 칩은 검수 대기. 두 상태 모두 허용된 전이 | 한 번의 page.evaluate로 두 칩을 읽고 같은 순간의 일치를 엄격히 단언. 기다림·허용 값·시한은 유지 |
| 실제 경과 단언 | Windows 질문 guard 5초 단언 실패 | t-1967에서 전달 완료 단언으로 이미 수리됨. 추가 완화 없음 |
| Windows 설치 PTY 정체 | 3개 실행에서 60초. Enter 관측 후 `ReadConsoleW`가 끝나지 않는 경우와 입력 전 자식 종료 대기를 구분 | 병렬도와 runtime 수만 줄임. 시한 확대·Rust 재시도·ignore 없음. 근본 원인은 아직 미확정이므로 Windows 반복 실행으로 재관찰 |
| 과거 실제 결함·관찰 시점 | Windows 스택 고갈, Ubuntu sshd 준비·후속 처리, 웹 복귀·애니메이션 단언 | 원래 실패를 이력에 남김. 광범위한 무시·continue-on-error를 추가하지 않음 |

## 실행 정책과 보존 범위

| 항목 | GitHub CI | 로컬 착지 gate |
| --- | --- | --- |
| 웹 spec·기기 조합·단언 | 전부 유지, spec 안 상태를 이어서 실행 | 전부 유지 |
| 웹 병렬도 | 3 shard × 러너별 worker 1 | 기존 CPU 예산 |
| 실패 재확인 | 최초 전수가 끝난 뒤 실패 작업만 직렬로 1회, 새 child·host·agent·브라우저 | 0회 |
| 재확인도 실패 | job 실패 | 최초 실패로 실패 |
| 최초 실패 뒤 통과 | warning, step summary와 시도별 JSON에 별도 표시 | 해당 없음 |
| 시간 제한 | locator 30초·단계 120초 유지, job 40분 유지. E2E step 30분으로 끝내 진단 업로드 여유 확보 | 변경 없음 |
| Rust 선택·시한·단언 | 제외 없음, 기존 OS 전수·native 설치·SSH owner 유지 | 변경 없음 |
| 제거·ignore·CI 제외 | 없음 | 없음 |

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
전체 ci를 반복 실행할 수 있게 했다. 새 workflow가 main에 등록되기 전에는 승인된
`hp/**` branch의 최초 push와 run rerun으로 같은 후보를 반복 확인한다.

`ci-local.sh`는 CI 환경 없이 분할·재확인 설정을 받으면 실패한다. `verify.py landing`은
CI 환경에서도 `AAA_E2E_ONLY`·`AAA_E2E_SHARD`·0이 아닌 `AAA_E2E_RETRIES`를 거부한다.
따라서 CI에서 허용한 예외가 착지 전수 검사를 줄이지 않는다.

## 참고한 공개 구현

- [Playwright CI](https://playwright.dev/docs/ci)·[재시도](https://playwright.dev/docs/test-retries)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)):
  CI worker 1과 러너 간 sharding, 실패 worker를 버리고 새 worker에서 준비부터 실행하는 방식을 참고했다.
- [Cypress 재시도](https://docs.cypress.io/app/guides/test-retries)
  ([MIT](https://github.com/cypress-io/cypress/blob/develop/LICENSE)):
  개발 기본과 CI 재시도 정책을 분리하고 최초 실패와 최종 통과를 구분하는 방식을 비교했다.

코드를 복사하거나 새 실행 의존성을 추가하지 않았다. 기존 독립 프로세스 runner에 최소한의
spec 분배·재확인 기록을 더했다. 시험 삭제·기기 조합 축소·전역 timeout 확대는 채택하지 않았다.

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

원문·화면은 작업 전용 target 아래에 보존한다. GitHub CI 반복 실행과 Windows의 변경 후
결과는 아직 없다. t-1978은 조사 종료 시 main에 없으며, 먼저 반영한 뒤 같은 후보 SHA의
전체 GitHub CI를 세 번 실행해 최초 실패 수·재확인 회복 수·Windows 설치 PTY·전체 시간을 비교한다.
진단 branch push는 착지 창 승인 뒤 수행한다.

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
