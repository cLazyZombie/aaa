# 보고서 보존 설정 검증(t-1710)

## 바꾼 계약

기존 host.toml `[projects]`의 `library_max_days`·`library_max_mb`를 웹 설정 「보고서」에서 조회·수정한다.
기본 30일·20480MiB, 각각 0은 제한 끄기, 양수 최솟값은 1일·1MiB다. 음수·소수·변환 범위 초과는 거부한다.
저장 때 삭제하지 않고 다음 예정된 자동 회차에서 새 값을 읽는다. 미리보기·사용량도 같은 실효값을 쓴다.
보관함·진행 중 thread·끝난 thread의 최신 보고 보호, 기한 우선·오래된 순 용량 정리, 수동 prune의 명시 기한,
공유 링크의 기본 30일은 기존 동작이다. host.local.toml 우선순위를 존중하며 웹은 그 파일을 고치지 않는다.

## 필요한 시험

- `settings_admin::tests::library_retention_settings_validate_persist_and_reject_stale_or_shadowed_writes`: 실제 파일에 양수·0을 저장하고 주석·safety 보존, 즉시 재조회, stale 판·local 덮기·잘못된 값 거부를 확인한다.
- `settings_admin::tests::library_retention_settings_require_a_human_controller`: 보기 전용 기기·세션 내부 agent가 보존 정책을 대신 바꾸지 못하게 한다.
- `library_links::tests::changed_retention_limits_apply_on_the_next_pass_and_preserve_protected_reports`: 실제 사본과 DB로 한도 축소→저장 때 보존→다음 회차의 기한/용량 삭제·공유 링크 폐기를 확인한다. 진행 중·끝난 thread 최신·보관 표시 사본을 남기고 재시작 뒤 0도 유지한다. 시간을 자지 않고 `now`를 주입한다.
- 웹 E2E 132: PC Chromium·휴대폰 WebKit에서 음수·소수 거부, 저장, 사용량·정리 미리보기 반영, 새로고침 뒤 값 유지와 기본 복원을 확인한다.
- 기존 `aaa-config` 보존 설정 파서·`library_links` 정리·`report_ids` 과거 판 보호 owner를 함께 검사한다.

## 참고한 공개 구현

- [restic 보존 정책](https://restic.readthedocs.io/en/stable/060_forget.html), [BSD-2-Clause](https://github.com/restic/restic/blob/master/LICENSE): 보존 정책과 실제 제거를 구별하고 dry-run으로 후보를 확인하는 접근을 참고했다.
- [Borg prune](https://borgbackup.readthedocs.io/en/stable/usage/prune.html), [BSD-3-Clause](https://github.com/borgbackup/borg/blob/master/LICENSE): 명시적 기간·보호 대상·미리보기의 구분을 참고했다.

외부 코드를 가져오거나 런타임 의존성을 추가하지 않았다.

## 결과

모든 실행은 `aaa verify -- <기존 명령>`을 통했다.

| 검사 | 결과 |
| --- | --- |
| 관련 owner: aaa-config·aaa-proto·aaa-host | 시험 979건 통과, 기존 ignore 2건 유지 |
| 관련 owner strict clippy | 통과 |
| 정규 ci-local 전체 | web·fmt·build·test·e2e·public 모두 통과 |
| 웹 유닛 | 718건 통과 |
| Rust 전체 | 343개 owner 작업 모두 통과 |
| Python | 88건 성공, 기존 skip 2건 유지 |
| 웹 E2E 전체 | 40개 파일·51개 작업 모두 통과 |
| 변이 검사 | 저장한 설정 대신 시작 설정을 읽게 바꾸면 새 정리 회귀 시험이 실패하며, 원본 복원 뒤 통과 |

개발 때 새 DTO의 Eq/PartialEq·CLI 렌더링 누락, 새 fixture의 TOML bool·schema 오류,
기존 설정 거부 문구 변경을 찾아 수리했다. 시한 연장이나 기대값 완화로 넘긴 실패는 없다.
전체 검증은 미커밋 후보에서 실행해 `aaa verify`의 출처 원장은 수집되지 않았다. 각 단계의 성공 결과는 별도 로그로 확인했다.
착지 통합 SHA의 gate와 OS cross·배포 검증은 승인된 착지 창에서 별도로 수행한다.
