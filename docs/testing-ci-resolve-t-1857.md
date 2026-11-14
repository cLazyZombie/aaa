# Ubuntu CI의 보고 후속·예약 시험 수리(t-1857)

## 원인과 변경

기준 main `a448941f`의 GitHub run `43545251543`, Ubuntu job `132294956697`에서 실패한 두 시험을 구분했다.
같은 run의 macOS·Windows·웹 job은 성공했다. 두 시험은 `m13.rs`의 `cfg(unix)` 아래 있으며 Linux 전용이 아니다.
로컬과 CI는 같은 ci-local·독립 owner 실행기를 사용한다. macOS 단독 최초 실행에서도 두 시험이 통과했으므로,
로컬에서 빠진 시험이 아니라 아래 관측 순서에 따른 경합이다. OS 자체가 원인이라는 증거는 없다.

- `thread_park::park_moves_between_machines_through_the_pushed_branch`: 가짜 agent는 이어 하기 brief의
  마지막 보고를 새 보고의 Report에 싣는다. 제품의 `section_lines`가 코드 울타리를 구분하지 않아 예전 보고의
  `## Next`와 닫는 울타리도 후속으로 읽었다. 별도로 시험의 `report_hash.is_string()`은 이전 판에도 참이어서,
  helper가 처리한 뒤 최종 수집에서 새 후속이 나타날 수 있었다.
  파서는 울타리의 문자·최소 길이·닫는 줄을 구분한다. 짧은 안쪽 울타리·다른 문자·글이 붙은 닫기는 끝으로 보지 않는다.
  공통 fixture helper는 디스크의 최신 보고 SHA와 host의 수집 hash가 같은 사건을 기다린 뒤 실제 후속을 명시적으로 처리한다.
- `task_dependencies::multiple_predecessors_start_once_after_all_archives_and_capacity_across_restart`:
  CI 실제값은 `연결된 할 일이 없다 · 예약을 풀거나 할 일을 복원한다`, 기대값은 `자리 대기`다. resolve 이전의 단언이다.
  `std::fs::write`의 truncate와 본문 쓰기 사이에 ticker가 빈 TASKS.md를 읽으면 예약을 확인 필요로 바꾼다.
  둘째 완료 표시 직전에 파일을 truncate하고 tick 완료를 기다린 진단 변형에서 CI와 같은 단언·값으로 실패했다.
  완료 표시는 기존 `task edit <d-ID> --done`의 잠금·원자 저장 경로로 바꿨다. 같은 파일의 지정 계정 시험도 맞췄다.
  선행 두 건의 완료 보관·자리 대기·한 번만 기동·재시작 뒤 pane 보존·thread 두 개 단언은 유지한다.

CLI resolve 전체 호출은 `rg`로 조사했다. 공통 fake 보고를 끝내는 기존 helper 호출은 최신 판까지 기다리도록 함께 고쳤다.
그 밖은 보고 없음·진행 보고·빈 Next·정리 전용·자동화 출처·의도적인 차단 owner다. 제품 차단을 우회하는 옵션은 추가하지 않았다.
기존 후속 원장은 자동 삭제하지 않는다. 새 보고 파싱과 시험 준비를 고쳤다.

## 재현과 회귀

- 최초 macOS 개별 실행: 기존 두 시험 모두 통과. CI 실패를 회복으로 표시하지 않는다.
- 수리 전 새 proto 시험: `이어 하기`, 울타리 문자 등이 실제 결과에 섞여 실패했다.
- 수리 전 둘째 완료의 truncate 경계를 고정한 기존 예약 시험: CI와 같은 실제값·기대값으로 실패했다.
  진단 변형은 제거했고 고정 sleep·재시도·시한 증가·기대값 완화는 없다.
- proto 전체 55개 통과. 새 시험은 이전 보고의 긴 울타리 속 Next/Decisions, 실제 절 안의 예시,
  backtick·tilde·들여쓰기·더 긴 닫기·닫히지 않은 울타리를 확인한다.
- 실제 CLI 회귀 owner는 예시의 Next를 원장에 넣지 않으면서 실제 후속 하나의 resolve 차단·명시 처리 뒤 종료를 확인한다.

- macOS 관련 통합 owner 5개 모두 통과: 기존 두 실패 owner, 지정 계정 예약, 기존 후속 차단, 새 코드 울타리 회귀.
- Ubuntu 24.04 aarch64 VM, Rust 1.100에서 기준 main을 별도로 native 빌드했다.
  `thread_park`는 별도 강제 순서 없이 CI와 같은 「이어 하기」·울타리·실제 후속의 conflict로 실패했다.
  `task_dependencies`는 둘째 완료의 truncate 경계를 고정한 진단 변형에서 CI와 같은 실제값·기대값으로 실패했다.

- 수리 뒤 같은 Ubuntu VM의 m13 전체 98개 통과(8 threads). 변경 Rust 소스 4개는 macOS와 SHA-256으로 대조했다.
  시험 뒤 VM을 정지·삭제했다. Ubuntu CI의 x86_64와 이번 VM의 aarch64는 구분한다.
- 관련 `aaa-proto`·`aaa` all-targets strict Clippy, fmt, diff 공백 검사가 통과했다.
- 정규 macOS ci-local 전체 통과: web·fmt·build·test·e2e·public, 총 317초.
  Rust 실행 단위 349개 모두 성공, Python 89개 성공(환경별 기존 무시 2개), Chromium·WebKit E2E 전체 성공이다.
  Playwright 1.64.1 패키지만 작업 폴더에 두고 기존 공용 브라우저 캐시를 썼다.
- 모든 검증은 `aaa verify -- <명령>`으로 실행했다. 미커밋 상태였으므로 자동 workspace finish·verify 원장 집계는
  수집되지 않았다. 단계 로그와 Rust 상태·사건 기록은 `target/ci-local/20261114-114625/`에 보존했다.
  실제 단계 종료 코드는 모두 0이다. 이 개발 검증을 최종 착지 SHA 검증이나 GitHub CI 회복으로 쓰지 않는다.

처음 시도한 Linux cross 실행 준비는 Zig 0.17 linker의 미지원 Cortex-A53 옵션, 이어 SQLite symbol 링크 오류로
완료하지 못했다. cross 산출물은 시험에 쓰지 않고, 일회성 Ubuntu VM 안에 Rust 1.100·native C 도구를 준비해
기준 재현과 수리 뒤 전체 m13을 직접 실행했다. 호스트 시스템 설정은 바꾸지 않았다.
착지 창의 최종 통합 SHA gate·병합·push는 별도다.

## 참고한 공개 구현

- [pulldown-cmark의 fenced code 처리](https://github.com/pulldown-cmark/pulldown-cmark/blob/main/pulldown-cmark/src/firstpass.rs),
  [MIT](https://github.com/pulldown-cmark/pulldown-cmark/blob/main/LICENSE): 울타리의 문자·길이를 기억하는 경계를 비교했다.
- [cmark의 block 처리](https://github.com/commonmark/cmark/blob/master/src/blocks.c),
  [BSD-2-Clause](https://github.com/commonmark/cmark/blob/master/COPYING): 같은 문자이며 열 때 이상 길이인 닫기를 확인하는 접근을 따랐다.
- [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/#fenced-code-blocks)의 울타리 규칙을 기준으로 삼았다.
  명세는 CC-BY-SA-4.0이다. 외부 코드·문구를 복사하거나 새 의존성을 넣지 않고 기존 줄 파서의 경계만 보완했다.
