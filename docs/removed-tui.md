# 삭제된 기능: TUI

2026-10-19 사용자 결정으로 터미널 front를 삭제했다. 웹과 TUI의 동시 개발·유지보수 부담과 컴파일 시간을 줄이고 화면을 웹 UI로 모은다. host의 `aaa web status`에서 주소를 확인한다.

보존점은 **annotated tag `tui-last`**다. 태그 객체는 `6e0ab176bf5daf9740821e560b094c0a1296be57`, 대상은 삭제 직전 main `2aafd4b8c9af6ff3682a8caf17126f721e539877`이다. 메시지는 「TUI 삭제 직전 마지막 판, 2026-10-19, 되살리려면 이 tag에서 가져온다」다. 착지 검증 뒤 main과 이 tag 하나만 명시적으로 push한다. 별도 보관 branch는 만들지 않는다. 핵심 삭제 커밋은 `5c00d6db747f98071a3041d85980ca68c5a6bce1`, 잔여 견본·안내 정리는 `55d431d3770a0db733ef5194ace302979f0cc2aa`다.

## 삭제 범위

- `crates/aaa-tui` 전체: 화면·키/마우스·복사·테마·IME·기기 표시 상태.
- `vendor/crossterm`과 patch, `aaa-config::tui_edit`·TUI 설정 schema, `aaa-common::remote` 원격 TUI client.
- host의 TUI cell grid 구독·스크롤·선택/검색·링크 열기·OSC 52 전달·TUI presence·설정 키. TUI 기록 편집기와 agent 임시 파일/편집기 모듈.
- TUI 전용 Front 요청·응답·grid 타입. 공통 Front API·workspace·lease·pane 입출력·Ctrl+Enter 판정과 웹 터미널은 유지한다.
- 직접 의존 `ratatui`, `crossterm`, `arboard`, `png`, `unicode-segmentation`, TUI 시험용 `vte`·`unicode-width`. 원격 TUI용 직접 `tokio-tungstenite` 의존도 제거했다(웹 서버 axum의 전이 의존은 유지).
- `tests/tui.rs`의 PTY/E2E 전체, TUI crate·설정·copy/link·grid·원격 client·기록 편집기의 전용 시험. host/agent의 공통 감지 fixture와 `tests/fixtures/prompt_box`는 웹 Ctrl+Enter에도 필요해 남겼다.
- ci-local/GitHub CI의 native clipboard 단계·Xvfb 설치와 vendored crossterm 시험. 웹 E2E·CLI·host 시험과 전체 workspace build/clippy는 계속 돈다.
- README의 TUI 사용법, Windows 원격 TUI 설치 안내, 기획서의 TUI 전용 절과 시험 설명.

옛 `aaa attach`·`aaa tui`·`aaa evaluator open`은 실패 코드와 「TUI는 삭제됐다. 웹 UI를 쓴다」를 출력한다. `mission open`은 TUI 대신 기존 Front 응답을 출력하는 CLI로 남는다. 옛 바이너리의 로컬 `tui`·`tui@…` hello와 인증한 원격 `/api/v1/front`는 명시적인 Reject를 받는다. 일반 웹 API·터미널·CLI 연결은 계속 받는다.

## 삭제 전후 측정

macOS arm64, Rust 1.99.0에서 `cargo build --workspace --all-targets --locked`를 각각 없는 target 폴더에 실행했다. 양쪽 모두 `RUSTC_WRAPPER`를 비워 kache를 사용하지 않았다. 웹 번들 준비는 측정에서 제외했다.

| 항목 | 삭제 전 | 삭제 후 |
|---|---:|---:|
| workspace crate | 10 | 9 |
| lock 전체 해결 패키지(서로 다른 버전을 각각 계산) | 381 | 251 |
| macOS 활성 패키지(workspace 포함, normal/build/dev) | 254 | 188 |
| clean build | 23.09초 | 22.22초 |

측정 기준은 삭제 전 `c594f12b`, 삭제 후 `5c00d6db`에 담긴 삭제 구현(착지 기준 `2aafd4b8` 통합 전)이다. 각 한 번의 측정이며 파일시스템·OS 캐시까지 비운 통계적 벤치마크는 아니다. 의존 그래프 축소와 한 번의 빌드 시간 차이를 구분한다. 85개 파일과 TUI 전용 Rust 시험 286개를 삭제했고, 공통 cwd 시험 하나는 TUI 부분만 걷어 이름을 바꿨다.

## 복구

이 tag는 과거 소스 보존점이며 실행 옵션이 아니다. 환경 변수로 다시 켜는 우회는 없다. 일부 경로를 가져오려면 별도 작업에서 `git restore --source tui-last -- crates/aaa-tui`처럼 복원한다. crate만으로 실행되지 않으므로 Cargo manifest·lock·설정·protocol·host·시험도 함께 복원·통합하고 당시 이후의 보안/웹 변경을 재검토한다. 핵심 삭제를 되돌릴 때는 `git revert 5c00d6db747f98071a3041d85980ca68c5a6bce1`하고 같은 검증을 거친다. 견본·안내까지 되살릴 때는 잔여 정리 커밋 `55d431d3770a0db733ef5194ace302979f0cc2aa`도 함께 되돌린다. 현재 main에 통째로 과거 파일을 덮어쓰지 않는다.

운영 host·agent는 이 소스 변경으로 자동 교체·재시작하지 않는다. 새 host 반영 전에는 옛 바이너리의 접속 차단이 적용되지 않는다. tag의 생성·공개와 host 배포는 착지·배포 절차에서 각각 확인한다.

## 보존 방식의 근거

[herdr CHANGELOG 0.9.2](https://github.com/herdrdev/herdr/blob/v0.9.5/CHANGELOG.md)는 graphics API 제거와 표준 Kitty graphics로의 대체를 Breaking Changes에 남긴다. [t3code의 변경 기록 원칙](https://github.com/pingdotgg/t3code/blob/main/AGENTS.md)은 병합한 PR을 구현 기록으로 삼고 바뀐 결정의 문서를 고치거나 없애도록 한다. 별도 보관 branch보다 설명 있는 tag와 삭제 커밋·짧은 복구 문서를 함께 두면 사용 시점과 되돌릴 범위를 바로 알 수 있어 이 방식을 택했다.
