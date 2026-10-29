# 검증 중복 축소 확인(t-1309)

## 변경과 범위

- 개발의 관련 검사와 착지의 gate를 [verification.md](verification.md)로 구분했다. `verify.py`는 Git 변경 범위를 기록하고 기존 ci-local을 호출한다.
- 웹만 바뀐 착지는 웹 전체와 실제 host/agent E2E 전체를 유지하며 Rust unit 사전 컴파일·전수·무관한 OS cross를 반복하지 않는다.
  backend·protocol·의존성·harness·CI가 함께 바뀌면 기존 전체 gate다. GitHub OS/브라우저 전체 행렬은 유지한다.
- Windows 최종 SHA의 production release는 salt 포함 한 번 빌드하고 검사한 exe/PDB를 보존한다. 별도 Cargo 호출,
  PE/PDB GUID·age·debug·줄 표·SHA·실제 Windows 판/config 검사와 전달 후 SHA 대조를 유지한다.
- 개별 제품 시험·시한·기대값·ci-local 병렬도는 바꾸지 않았다. 기존 제품 시험을 삭제하지 않았다.

## 꼭 필요한 자동 회귀

| owner | 필요한 이유 |
| --- | --- |
| `scripts.test_verify` | 실제 Git의 rename 원본·미추적/backend 경로를 놓쳐 검사를 웹으로 축소하는 오류, dirty 착지, 부분 검사를 전체로 오인하는 오류 방지 |
| `scripts.test_ci_local`의 관련 spec 회귀 | 선택한 spec을 실제 runner에 전달하면서 새 binary 사본·기존 실패 전파를 유지하는지 확인 |
| `scripts.test_windows_production` | 한 번 빌드·salt·독립 Cargo 호출·다른 SHA/바이트/기호/줄 정보/native 영수증 누락 거부·재빌드 없는 검증 계약 보존 |

가짜 컴파일러를 쓴 Python 계약 검사와 실제 Windows production/native 검증은 구분한다.

## 검증 기록

- 첫 계약 검사에서 macOS 기본 Bash 3의 `set -u`가 빈 배열 확장을 거부했다. E2E 선택 인자를 함수 위치 인자로 바꿔 빈 선택에서도 기존 전체 실행을 보존했다.
- Windows fixture의 임시 폴더 경로는 macOS canonical 경로와 같게 고쳤다. 실제 target 격리 단언을 유지했다.
- 수리 후 관련 Python 계약 23개가 9.896초에 통과했다. 개별 신규 시험은 모두 5초 미만이다.
- 웹 타입·번들 및 유닛 656개, 공개 검사가 통과했다. web 단계 8초, public 2초이며 전체 11초다.
- 배포 빌드·원장·cross 진입점·계측을 포함한 관련 Python 23개도 13.593초에 통과했다. 앞의 23개와 공통 8개가 포함된 별도 묶음이다.
- 실제 새 선택 경로 `ci-local --only e2e --spec 23,109,114`가 통과했다. 내부 실행 파일 준비 36.68초,
  E2E 전체 75.7초, 명령 경과 113초다. E23 63.7초·E109 30.5초·E114 75.7초는 파일 전체이며 개별 시험 시간이 아니다.
  Playwright 1.64.1 npm 패키지만 작업 폴더에 두고 공용 브라우저 캐시를 썼다. 기존 Chromium/WebKit·iPhone·history/PTY·대량 처리 단언을 유지했다.
- 구현 후보 `a930891b`에서 Windows production을 salt 포함 한 번 빌드했다. 228.141초에 빌드·PE/PDB GUID/age·debug·줄 정보 검사가 통과했다.
  새 target에서 기존 kache를 사용한 관측이며 과거 표본과 같은 cache/부하 조건의 개선율로 비교하지 않는다.
- Windows Python 3.9 SSH pipe에서 실제 exe `--version`·격리 config 검사가 성공한 뒤, Python의 마지막 한국어 성공 출력이 ANSI 인코딩 오류로 실패했다.
  도구의 stdout/stderr만 UTF-8로 고치고 CP1252 pipe 회귀를 기존 계약 검사에 넣었다. exe/PDB를 다시 빌드하지 않고 같은 파일로 native를 재검증해 영수증을 회수했다.
  수리 후 Windows 계약 3개는 2.113초, 검증 계획 계약 5개는 1.512초에 각각 통과했다.
- 같은 후보의 전달 전 `verify`가 0.575초에 통과했다. 검사 전후 exe/PDB·출처·native 영수증 4개 파일의 SHA-256이 모두 같았다.
  exe SHA-256은 `5b25c542fbdcad6b2487a33537ae82dda4706524be22828359d9c2806741401f`,
  PDB SHA-256은 `618c0eab58b47d36f2066e785687887c182c84c9bfdc3442dee6c3b20419f371`이다.
- Windows 작업 폴더를 지우고 서비스·예약 작업·Run 키·사용자 PATH·방화벽 규칙의 기준 목록 해시가 전후 일치함을 확인했다. 운영 설치·서비스는 바꾸지 않았다.
- 이 후보는 착지 전의 도구 검증용이다. 실제 최종 통합 SHA가 정해지면 그 SHA의 production을 한 번 만들고 동일 산출물을 전달한다.
  전체 ci-local·양 OS strict cross·병합·push는 착지 창에 남겨 개발 중 전체 중복을 만들지 않았다.

## 전후 시간의 기준

| 구분 | 변경 전 근거 | 변경 후 확인 |
| --- | --- | --- |
| 개발 전체 중복 | r-5206 전체 115회 중앙값 361초. 후보/착지 각각 358/402초인 관측 사례 | 개발 명령에 관련 owner/spec만 기록. 전체 반복 1회의 절감 후보가 약 6분이며 이번 동일 조건 실측 절감으로 주장하지 않는다 |
| 웹 착지 무관 검사 | build 59초·test 198초·clippy 49초 중앙값. 병렬 단계이므로 합쳐 절감 시간으로 계산하지 않는다 | 웹 gate의 단계는 web/e2e/public. 실제 내장 binary를 빌드하고 모든 웹 계약을 확인한다 |
| Windows production 두 번 | 두 관측의 무 salt/salt 빌드가 154/139초, 216/207초 | 실제 한 번 빌드 228.141초, 같은 파일의 verify 0.575초. 중복 빌드 한 번 약 2~3.5분 절감은 과거 관측 기반 후보 |

변경 전 자료는 t-1308의 r-5206을 읽었다. 부하·cache가 다른 실행을 같은 조건의 성능 비교로 합치지 않는다.
전체 gate·cross는 착지 창의 최종 통합 SHA에서 수행하며 개발에서 미리 전수를 반복하지 않는다.
