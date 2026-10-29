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
- 추가 실제 산출물·Windows 검증과 시간은 확인 뒤 아래에 기록한다.

## 전후 시간의 기준

| 구분 | 변경 전 근거 | 변경 후 확인 |
| --- | --- | --- |
| 개발 전체 중복 | r-5206 전체 115회 중앙값 361초. 후보/착지 각각 358/402초인 관측 사례 | 개발 명령에 관련 owner/spec만 기록. 전체 반복 1회의 절감 후보가 약 6분이며 이번 동일 조건 실측 절감으로 주장하지 않는다 |
| 웹 착지 무관 검사 | build 59초·test 198초·clippy 49초 중앙값. 병렬 단계이므로 합쳐 절감 시간으로 계산하지 않는다 | 웹 gate의 단계는 web/e2e/public. 실제 내장 binary를 빌드하고 모든 웹 계약을 확인한다 |
| Windows production 두 번 | 두 관측의 무 salt/salt 빌드가 154/139초, 216/207초 | 최종 SHA+salt 빌드 한 번과 불변 exe/PDB를 소비하는 verify. 한 번 약 2~3.5분은 과거 관측 기반 절감 후보 |

변경 전 자료는 t-1308의 r-5206을 읽었다. 부하·cache가 다른 실행을 같은 조건의 성능 비교로 합치지 않는다.
전체 gate·cross는 착지 창의 최종 통합 SHA에서 수행하며 개발에서 미리 전수를 반복하지 않는다.
