# Windows workspace_metrics None panic 수리

## 실패와 확인 범위

main `a816c942e0e08bbbc68fea2339dff29e5713e18a`의 Windows CI에서
`legacy_runs_need_main_checkout_but_confirmed_records_and_code_follow_pushed_objects`가
첫 `saved.code_lines.as_ref().unwrap()`에서 실패했다. 원본 job 로그와 보존된 Rust 로그를 확인했다.
실행 결과는 aaa-agent 172 통과·1 실패이며, 해당 실행의 회복 증거는 없다.

`collect`는 코드 집계를 보조 지표로 취급한다. Git/자료 오류나 비용 상한이면 실행 기록을 보존하고
`code_lines=None`과 대기 메모를 돌려준다. 이 시험은 그 정상적인 보류 가능성을 둔 채 실제 Git 실행을
포함하는 750ms 예산 안에서 코드 집계가 반드시 성공한다고 가정했다.

원본 CI에는 코드 집계 하위 오류 원문이 없어 당시 실제 상한 초과인지 Git/파일 오류인지는 확정하지 않는다.
Windows 실기에서 실패 SHA의 수정 전 시험을 기본 상한으로 단독 실행하면 1개가 통과했다(4.35초).
따라서 기준판의 단독 통과를 CI 회복으로 세지 않는다. 동일 집계 경로에 750ms를 주입하면 기존의
main·push 단언이 같은 Option unwrap 실패를 낸다. 확인된 수리 대상은 이 fixture의 시간 의존성이다.
제품의 750ms 상한, 실패 커밋 재스캔 방지, Git 객체 선택 정책을 완화하지 않는다.
입력 owner나 앞선 설치 입력 수리의 담당 관계로 원인을 추정하지 않았다.

## 수정

- 공개 `collect`는 기존 실제 시계·Git 수집기를 호출한다. 내부 수집기 경계와 `origin/main` 경과 주입 입구만 분리했다.
- 코드 값을 단언하는 기존 main/legacy 및 CI archive 시험 2개는 실제 Git·캐시·잠금·push 참조 검사를 계속 쓴다.
  경과 출처만 고정해 CPU 부하와 프로세스 시작 시간으로 계약 시험이 실패하지 않게 했다. 기존 단언은 유지했다.
- 새 회귀 시험 `code_budget_exhaustion_keeps_confirmed_runs_without_inventing_code_or_rescanning`은
  750ms에서 실제 집계기가 실패하는 경우를 상위 수집 경로까지 확인한다. 확정 실행 보존, 코드 값 없음,
  대기 메모, 성공 스냅샷 미저장, 같은 커밋 재스캔 방지를 함께 본다.
  이 시험은 정상적인 선택 지표 보류와 main 실행 보존의 조합을 검증하기 위해 필요하다.
- 기존 749,999,999ns·750ms·751ms 경계, 부분 캐시 보존과 다음 커밋 회복 시험은 유지했다.
- sleep, 재시도, 시한 연장, 기대값 완화, 시험 삭제는 없다.

## 개발 검증

검증은 `aaa verify -- <명령>`으로 실행했다.

| 검사 | 결과 |
| --- | --- |
| 수정 전 macOS workspace_metrics owner | 25 통과 |
| 실패 SHA Windows 실기 단독 원본 시험 | 1 통과; CI 회복 증거로 간주하지 않음 |
| 기존 main/legacy 단언에 750ms 주입 | 의도대로 같은 None unwrap 실패 |
| 수정 후 macOS workspace_metrics owner | 26 통과 |
| 수정 후 Windows 실기 owner, 기존 thread 8 | 22 통과 |
| macOS aaa-agent 전체 target strict Clippy | 통과 |
| Windows aaa-agent 전체 target strict Clippy | 통과 |
| push 참조를 HEAD로 바꾼 결함 주입 | stale checkout 시험이 1줄/2줄 불일치로 실패 |

macOS/Windows 시험 수 차이는 Unix 전용 시험의 cfg에서 온다.
Windows 실기는 격리한 작업 폴더 및 TEMP/TMP에서 실행했다. 사용자 화면·운영 설치·서비스는 건드리지 않았다.
시험 전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽 규칙 목록은 같았다.
결함 주입 원복 뒤 macOS owner 26개가 다시 통과했다. fmt와 공개 검사를 완료했다.
Windows에서 실행한 변경 소스 3개의 SHA-256은 최종 후보와 일치했다.

착지 창은 아직 받지 않았다. 최종 통합 SHA gate·양 OS cross·main CI 회복은 아직 확인하지 않았다.
실기 owner 결과를 main CI 전체 통과로 확대하지 않는다. 배포 산출물은 만들지 않았다.

## 참고한 공개 구현·문서

- [Rust std::time::Instant](https://doc.rust-lang.org/std/time/struct.Instant.html) (Rust: MIT/Apache-2.0):
  실제 경과 측정은 제품 경로에 유지한다.
- [Tokio의 pause](https://docs.rs/tokio/latest/tokio/time/fn.pause.html) (MIT):
  Tokio 시계 정지는 std::time::Instant에 영향을 주지 않으므로 이 동기 Git 경로에는 적용하지 않았다.
- [Git gitattributes](https://git-scm.com/docs/gitattributes) (Git: GPL-2.0):
  줄 끝 변환 후보를 검토했다. 집계는 checkout 바이트 대신 Git blob을 읽으며, 상한 경계만으로 실패가 재현된다.

기존 저장소의 `collect_with_elapsed` 방식을 `origin/main` 계약 시험에 확장했다.
외부 코드 복사·의존성 추가는 없다.
