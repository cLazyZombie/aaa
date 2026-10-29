# 전체 gate의 재빌드 예산(t-1310 후속)

사용자가 후속 진행 중 목표를 「재빌드까지 5분 이내」로 변경했다. 전체 ci-local의 web·fmt·workspace build·Rust/Python test·웹 E2E·public과 종료 정리를 포함해 300초 이내로 확인한다. cross·Windows production·전달 시간은 착지의 별도 절차다.

## 179.7초와 착지 208초의 차이

| 단계 | 개발 d84623e8 | 착지 ee14b487 |
| --- | ---: | ---: |
| web / fmt(병렬) | 9 / 2초 | 9 / 2초 |
| Cargo build | 18.96초 | 47.39초 |
| Rust/Python test | 147초 | 148초 |
| 웹 E2E(test와 병렬) | 139초 | 142초 |
| public | 2초 | 2초 |
| 전체 외부 경과 | 179.6794초 | 208.818초 |

29.1386초 증가 중 build 증가가 28.43초다. 병렬 단계의 정수 시간을 합해 전체 경과로 쓰지 않는다.
개발은 aaa만 컴파일했고 착지는 common·host·session·doctor·agent·aaa를 컴파일했다.
common panic 소스와 시험은 두 SHA에서 같은 blob이지만 실제 mtime은 착지 전 main checkout 시각이었다.
Cargo의 dep-info/mtime freshness가 그 입력과 의존 crate를 dirty로 보았다. 병합된 host 3파일과 웹 변경은 실제로 달라 host 자산과 CLI 재빌드도 필요했다.
같은 내용이라도 파일 시각을 갱신한 작은 Cargo 의존 fixture에서 두 crate의 fresh가 true→false로 바뀌는 것도 확인했다.

Rust runner 경과는 146.482→146.980초로 거의 같았다. 첫 작업 load는 개발 4.77/3.71/4.58, 착지 5.34/2.96/2.74였다.
착지 시작 ps에는 다른 cargo/rustc/Playwright가 없었다. 개발 사전 ps 전체 원문은 없어 모든 외부 상태가 동일했다고 단정하지 않는다.
관측된 차이 대부분은 재빌드로 설명되며 외부 부하를 주원인으로 볼 근거가 없다.

## 실행과 관측

최신 main에서 시작하며 검증 전에 옛 main checkout으로 공통 소스를 되돌렸다 복원하지 않는다.
새 SHA의 CLI 재빌드를 포함하고, workspace 산출물을 비운 재빌드 조건도 별도로 표시한다. warm 반복 값으로 재빌드 비용을 대신하지 않는다.
기존 kache·외부 의존성 캐시·이 worktree만의 target을 사용한다. 전체 최초 의존성 다운로드·컴파일까지 보장하는 예산은 아니다.

build에 Cargo `--timings`를 추가했다. `target/cargo-timings/`에 unit별 컴파일·링크 시간과 rebuilt/fresh target 수를 보존한다.
기존 단계 로그·workspace-summary.tsv·Rust state/events·E2E timings와 함께 비교하며 캐시 판정·검증 범위·종료 코드는 바꾸지 않는다.
nice15, LLVM bin PATH, 작업 폴더 Playwright 1.64.1과 공용 브라우저 캐시, salt 없는 환경을 사용한다.
시작과 실패의 uptime/ps, 고정 SHA/tree, 단계 경과와 바깥 monotonic 전체 경과를 원문과 함께 남긴다.

기존 6단계, Rust 324작업·E2E 31작업, Rust6/browser4·libtest1·Tokio2를 유지한다. 시험 삭제·기대값·시한·재시도 변경은 없다.
검토한 aaa 검증용 심볼 제거는 철회했다. 모든 profile과 배포용 줄 정보·기호를 기존대로 유지한다.
대표 owner는 CPU 사용 비중이 높아 I/O 대기로 가정해 슬롯을 줄이는 안도 채택하지 않았다.

최종 개발 실측의 SHA·전체 경과·관련 owner 결과는 t-1310의 후속 보고와 원문 로그에 보존한다. 착지 gate는 다시 받은 창에서 최종 통합 SHA로 한 번 수행한다.

## 참고

- [Cargo freshness](https://doc.rust-lang.org/stable/nightly-rustc/cargo/compiler/fingerprint/index.html): dep-info·mtime와 의존 fingerprint의 무효화 조건을 대조했다.
- [Cargo build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html), [build timings](https://doc.rust-lang.org/cargo/reference/timings.html): 기존 캐시를 유지하고 Cargo가 제공하는 unit별 관측을 사용한다.
- [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html): 내부 심볼 축소를 검토했으나 최종 변경에는 포함하지 않았다.

외부 코드·새 의존성은 들여오지 않았다.
