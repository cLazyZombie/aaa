# 검증 시간과 실행 범위

## 변경 전 이력

2026년 11월 11일 08시(KST)부터 18일 조사 시점까지 세 Mac의 Git 공용 원장에서 ci-local 전체 실행 85회를 확인했다. 성공 65회, 실패 20회다. `aaa verify`가 같은 실행을 바깥에서 기록한 요약은 중복 집계에서 뺐다. 아래는 머신별 성공 실행의 중앙값이며 단위는 초다.

| 머신 | 성공 / 전체 | 전체 경과 | web / fmt | build | Rust·Python | E2E | public |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Mac A | 17 / 23 | 311 | 9 / 2 | 62 | 177 | 242 | 2 |
| Mac B | 28 / 33 | 319.5 | 9 / 2 | 59.5 | 190 | 239 | 2 |
| Mac C | 20 / 29 | 172 | 6 / 2 | 35 | 96.5 | 127.5 | 2 |

Rust·Python과 E2E는 병렬이다. 빌드 종료부터 시험 시작까지 중앙값은 세 머신 모두 1초였다. 같은 머신·tree에서 전수를 두 번 실행한 11쌍 중 7쌍은 둘 다 성공했다. 실행 환경과 재검증 사유가 달라질 수 있어 과거 통과 결과를 새 gate로 대체하지 않는다.

- t-1998 첫 빌드가 SDK 입력을 찾지 못해 39초 만에 실패했다. 같은 SHA에서 컴파일러 환경을 바로잡은 뒤 전체 339초, build 62초, Rust·Python 200초, E2E 261초였다. E2E 23 한 파일은 76.9초였다.
- t-2002 첫 착지는 fmt가 2초 만에 실패했는데도 전체 373초를 썼다. build 82초, Rust·Python 228초, E2E 278초였다. 다음 판은 로컬 전수가 통과한 뒤 Linux strict cross에서 타입 복잡도 오류가 발견됐다. 타입을 정리한 최종 판은 build 56초, Rust·Python 226초, E2E 276초와 양 OS strict cross를 통과했다.
- main CI 10회 중 성공 8회, 실패 1회, 취소 1회였다. 성공 실행의 생성부터 완료까지 중앙값은 2,097.5초다. Intel Mac을 사용한 성공 7회의 Rust job 중앙값은 2,020초였다. 빌드 758초, 시험 619초, 실제 LaunchAgent 검사 306초가 주요 단계였다. 같은 job의 최초 배정까지 3~396초가 걸렸다. [표본 마지막 성공](https://github.com/cLazyZombie/aaa/actions/runs/44242254218) · [실패한 실행](https://github.com/cLazyZombie/aaa/actions/runs/44240774133)

## 방법

기준 `db07097c`의 첫 부모 변경 100건 중 기존 웹 전용 분류 16건 모두 E2E 파일 1~2개를 함께 고쳤다. 개발에서 그 파일의 전체 단계를 자동으로 선택하고 선택 근거와 이름을 원장에 기록한다. 직접 고른 범위는 `--spec`, 웹 전수는 `--all-specs`로 실행한다. 변경 spec이 없거나 삭제·이동 원본 등으로 파일을 확정할 수 없으면 전수다. 공통 harness·의존성·backend와 섞인 변경에는 자동 선택을 적용하지 않는다. 착지에서는 최종 SHA의 기존 범위, 공개 검사와 full의 양 OS strict cross를 수행한다.

포맷 오류는 지정한 Rust crate의 개발 검사 첫 단계에서 확인한다. 기본 전체 gate에서 fmt가 실패하면 web과 공개 검사의 결과를 보존하고 build·test·e2e는 시작하지 않는다. 같은 로그 경로에 이전 E2E·Rust 결과가 있어도 새 원장에는 이번에 실행한 단계의 요약만 넣는다. 빌드가 성공한 뒤 발견된 Rust·브라우저 실패의 원문과 종료 코드는 계속 보존한다.

GitHub Linux·macOS Rust job은 독립 CLI를 링크한 뒤 workspace 시험을 사전 컴파일한다. 두 Cargo 호출에 같은 verify profile을 주어 호환되는 의존 산출물을 공유한다. Windows 설치와 웹 브라우저 job은 각자 소비하는 e2e CLI를 빌드한다.

## 실제 웹 개발 검사

`bb9fc2d3`의 웹 변경을 격리 저장소에 재생했다. 두 명령 모두 같은 HEAD·기준 SHA·제품 소스·웹 번들·CLI 바이트를 썼다. 먼저 CLI를 준비한 뒤 `aaa verify -- nice -n 15 python3 scripts/verify.py develop --base HEAD^1`을 전수(`--all-specs`)와 자동 선택으로 각각 실행했다. 전체 경과는 `result.json`의 단조 시계 값이다.

| 실행 | 웹 유닛 | 브라우저 파일 / 작업 | E2E | 공개 검사 | 전체 경과 |
| --- | ---: | ---: | ---: | --- | ---: |
| 기존 전수 범위 | 747 | 45 / 56 | 266.0초 | 통과 | 279.840초 |
| 변경 파일 자동 선택 | 747 | 1 / 1 | 50.9초 | 통과 | 71.662초 |

한 변경에서 208.178초(74.4%)를 절감했다. 선택한 135는 Chromium·WebKit의 1280px·390px 조합을 모두 실행했다. 전수에서 가장 긴 개별 단계는 WebKit 390px의 셸 교체·재개 23.134초였다. 선택 실행의 WebKit 390px 문서·미디어 확인은 6.342초였다. 두 실행 모두 재확인 0회였다.

첫 비교에서는 양쪽 모두 브라우저 검사가 통과했으나 측정용 커밋 작성자 주소가 공개 규칙에 걸려 종료 코드 1이었다. 첫 기록을 보존하고, 측정용 커밋에 허용된 noreply 신원을 적용해 공개 검사를 먼저 확인했다. 표는 새 SHA에서 두 명령이 모두 성공한 결과다. 부하와 캐시가 다른 새 checkout에서는 시간이 달라질 수 있다.

## 포맷 실패 확인

격리한 실제 aaa-agent 소스의 공백을 틀리게 한 한 줄을 커밋했다. `develop --package aaa-agent`는 0.367초에 fmt 오류로 종료했다. 같은 SHA의 `landing`은 web과 공개 검사를 마친 뒤 22.636초에 종료 코드 1을 반환했다. 원장에 fmt 실패와 build·test·e2e를 시작하지 않은 이유가 남았다. t-2002에서 관측한 373초는 다른 작업·머신의 이력이다.

관련 Python 검사 35개와 Node 검사 19개가 통과했다. 실제 Git의 미추적 spec, 명시한 선택, 이름 변경, 착지 전수, 실행 시점에 사라진 파일, 포맷 실패 뒤 공개 검사, 로그 경로 재사용과 독립 CLI·workspace 빌드 순서를 확인했다.

## 실제 빌드 비교

10 CPU Mac에서 `bb9fc2d3`의 같은 제품 소스와 웹 번들, Rust 1.100.0, 기존 kache를 사용했다. CI의 두 빌드 구성을 각각 빈 worktree 전용 target에서 실행했다.

| 빌드 구성 | 독립 CLI | workspace 시험 사전 컴파일 | 빌드 단계 | 두 호출에서 중복 컴파일한 패키지·판 |
| --- | ---: | ---: | ---: | ---: |
| e2e CLI + verify 시험 | 64초 | 98초 | 163초 | 297 |
| verify CLI + verify 시험 | 81초 | 14.92초 | 96초 | 21 |

빌드 단계가 67초(41%) 줄었다. Cargo 로그의 두 번째 호출에서 컴파일한 패키지는 298개에서 22개로 줄었다. 외부 컴파일 캐시와 머신 부하는 실행 순서에 따라 달라질 수 있다. GitHub 호스팅 러너의 변경 후 시간은 승인된 병합 뒤 CI에서 측정한다.

## 참고한 구현

herdr의 OS별 owner와 명시적 시험 선택(Apache-2.0), herdr-projects의 빌드 성공 뒤 진행하는 배포 단계(MIT), herdr-web-ui의 unit·integration 분리와 독립 시험 실행(MIT)을 참고했다. 구현은 기존 Cargo·ci-local·브라우저 실행기를 사용한다. [herdr](https://github.com/herdrdev/herdr/blob/v0.9.5/.github/workflows/ci.yml) · [herdr-projects](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/.github/workflows/release.yml) · [herdr-web-ui](https://github.com/devswha/ovis/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/scripts/ci-tests.ts)
