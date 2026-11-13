# Rust 1.100 CI의 Cargo 경로와 Clippy 수리 (t-1814)

## 원인과 범위

main `cc31e14c`의 [실패 CI](https://github.com/cLazyZombie/aaa/actions/runs/43341759970)는
Ubuntu와 macOS의 Python 시험에서 `test_build_watch` 두 건이 실패했다.
[직전 통과 CI](https://github.com/cLazyZombie/aaa/actions/runs/43192767307)의 Rust는 1.99.1,
실패 CI의 Rust는 1.100.0이다. 두 run 모두 `rustup`의 stable toolchain을 사용한다.
CI는 `cargo --version`을 직접 출력하지 않으므로 Cargo의 실제 실행 버전 문자열은 로그에 없다.
공식 배포 manifest의 Cargo 패키지 판은
[1.99.1](https://static.rust-lang.org/dist/channel-rust-1.99.1.toml)의
`0.100.0 (5f94df478 2026-08-27)`에서
[1.100.0](https://static.rust-lang.org/dist/channel-rust-1.100.0.toml)의
`0.101.0 (3acc79c0c 2026-11-05)`로 바뀐다. manifest는 내부 패키지 판을 쓰며,
로컬에서 직접 실행한 사용자 표시 판은 `cargo 1.100.0 (3acc79c0c 2026-11-05)`이다.
CI Cargo 판은 같은 stable bundle을 설치했다는 근거로 비교하며 직접 실행 출력과 구별한다.

로컬 rustc 1.100.0 / cargo 1.100.0에서 수정 전 두 시험 모두 같은 `ValueError`가 재현됐다.
Cargo 자체의 check는 성공했고 생성 파일도 있었다. 실제 상대 경로는
`target/debug/build/aaa-build-watch-probe/<hash>/out/panic_build.rs`였지만,
시험은 `target/debug/build/*/out/panic_build.rs`만 찾아 0개를 받았다.

[Cargo 1.100 변경 기록](https://doc.rust-lang.org/cargo/CHANGELOG.html#cargo-1100-2026-11-12)은
중간 산출물 배치가 바뀌었으며 JSON 메시지를 쓰라고 명시한다.
[build cache 문서](https://doc.rust-lang.org/cargo/reference/build-cache.html)도
중간 산출물 배치를 내부 구현으로 규정한다.
제품의 `crates/aaa/build.rs`와 include 위치는 이미 `OUT_DIR`을 사용하며 같은 결함이 없다.
별도의 `scripts/build_watch` 도구는 없고, 해당 owner는 `scripts/test_build_watch.py`다.

## 세 OS의 Clippy 실패

같은 실패 run에서 Ubuntu·macOS·Windows 모두 `task_edit.rs:160`의
`clippy::needless_range_loop`로 `aaa-host` lib·lib test가 실패했다.
직전 run의 세 OS Clippy는 모두 통과했다. 각 run의 세 OS Rust는 각각 1.99.1과 1.100.0이다.
따라서 Windows만의 결함이 아니며 새 toolchain에서 진단되는 공통 순회 코드다.
로그에는 독립 `clippy-driver --version` 출력이 없어 정확한 Clippy 문자열을 직접 비교할 수는 없다.
로컬 Clippy는 `0.1.100 (0ef1bc3860 2026-11-09)`이다.

`idx + 1..len`으로 인덱싱하던 순회를 `iter().enumerate().skip(idx + 1)`으로 바꿨다.
`enumerate`를 먼저 적용해 원래 절대 줄 번호 `j`를 보존한다. 빈 줄 건너뛰기, 제목·같은 깊이에서
멈추기, 메모 블록 끝과 하위 task 묶음 끝을 정하는 규칙은 그대로다. lint allow는 추가하지 않았다.
기존 `task_edit::tests`의 원문 바이트·CRLF·하위 묶음 이동/삭제·복구 owner로 확인한다.
Clippy의 [공식 lint 설명](https://rust-lang.github.io/rust-clippy/rust-1.100.0/index.html#needless_range_loop)과
같은 iterator 접근을 따른다.

## 전체 검사에서 추가로 드러난 종료 코드 진단

순회 lint 수리 뒤 Rust 1.100.0의 workspace/all-targets strict Clippy를
네이티브·Linux cross·Windows cross에서 실행하자 세 대상 모두 동일한
`unused_must_use` 다섯 곳을 진단했다. 기존 host 컴파일 실패 뒤 가려져 있던 CLI 코드다.
대상은 `deployment`의 rollback/상태 출력, `rollout`의 상태 출력,
`update`의 설치 마무리/복구 마무리다.

내부 `finish`는 모든 성공 경로에서 `ExitCode::SUCCESS`만 반환하므로 `Result<()>`로 바꿨다.
그 값을 그대로 전달하던 내부 `rollback_approved`도 `Result<()>`로 맞추고,
실제 CLI 경계인 `rollback`에서 성공 종료 코드로 변환한다. 오류는 기존 `?`로 계속 전달한다.
배포와 rollout 상태 출력은 출력 결과의 `Result`를 상태별 종료 코드로 `map`해 바로 반환한다.
직렬화 오류와 실패/중단 상태의 종료 코드 1을 보존한다. lint allow나 반환값 무시는 추가하지 않았다.

## Cargo 수리와 회귀 계약

기존 `--message-format=json` 출력을 한 번 파싱하고 probe의 라이브러리 artifact와
`package_id`가 같은 `build-script-executed` 메시지의 `out_dir`에서 생성 파일을 읽는다.
[Cargo 메시지 계약](https://doc.rust-lang.org/cargo/reference/external-tools.html#build-script-output)에 따르면
이 메시지는 build script를 다시 실행하지 않은 fresh 빌드에서도 나온다. 실제 연속 두 빌드에서도 확인했다.

- 기존 두 시험의 Git SHA·index·새 소스·packed ref·새 loose ref·detached HEAD 단언을 유지했다.
- probe의 fresh 여부와 공통 의존성의 캐시 보존 단언을 유지했다.
- 공통 의존성에도 빈 build script를 넣어 출력 메시지가 여러 개일 때 probe를 고르는지 확인한다.
- 별도 `CARGO_BUILD_BUILD_DIR`에서도 최초 빌드와 fresh 빌드를 검사하는 시험을 추가했다.
- skip·재시도·시한 연장·기대값 완화·toolchain 고정 변경은 없다.

## 관련 owner 확인

검증 진입점은 모두 `aaa verify -- <명령>`이다.

| 실행 | 결과 |
| --- | --- |
| 수정 전 Rust/Cargo 1.100.0, `python3 -m unittest scripts.test_build_watch` | 2건 모두 같은 오류 재현 |
| 수정 후 Rust/Cargo 1.100.0, 같은 owner | 3건 통과 |
| 설치돼 있던 Rust/Cargo 1.92.0, 같은 owner | 3건 통과, 이전 배치 호환 확인 |
| 전체 Python owner | 89건 성공, 기존 OS 조건부 skip 2건 |
| `cargo test --profile verify --locked -p aaa-host --lib task_edit::tests::` | 기존 13건 통과 |

1.92.0 대조는 검증 명령에만 `RUSTUP_TOOLCHAIN`을 지정했다. 기본 stable 설정은 바꾸지 않았다.
최종 후보의 전체 gate·cross 결과와 출처는 t-1814 보고의 상세 기록에 남긴다.
착지 창에서 통합 SHA가 달라지면 `docs/verification.md`의 최종 통합 검증 기준을 따른다.

Python 출력의 `module::failed`와 owner 실패 흉내는 fixture의 자식 출력이다.
바깥 unittest의 실제 결과는 `Ran 89 tests`, `OK (skipped=2)`로 판단했다.
원본 CI의 실제 Python 오류는 build_watch 두 건이며 Clippy 오류와 구별했다.

## 참고한 공개 구현

- [Cargo](https://github.com/rust-lang/cargo) ([MIT](https://github.com/rust-lang/cargo/blob/master/LICENSE-MIT)
  또는 Apache-2.0): 공식 JSON 메시지와 package ID 연결을 따른다.
- [cargo_metadata](https://github.com/oli-obk/cargo_metadata/blob/main/src/messages.rs)
  ([MIT](https://github.com/oli-obk/cargo_metadata/blob/main/LICENSE-MIT) 또는 Apache-2.0):
  `BuildScript`의 `package_id`·`out_dir` 모델과 같은 계약을 확인했다.

외부 코드를 복사하거나 새 의존성을 추가하지 않고 기존 Python JSON 처리만 고쳤다.
