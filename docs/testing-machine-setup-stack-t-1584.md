# Ubuntu 머신 설정 시험의 스택 고갈 수리(t-1584)

## 실패와 이전 수리의 관계

[CI 실행](https://github.com/cLazyZombie/aaa/actions/runs/42316855981)의 Ubuntu 전체 job 로그와
`rust-tests-ubuntu-24.04` artifact를 확인했다. `289989e7`의
`machine_setup::tests::a_lab_machine_is_reached_trusted_detected_and_a_changed_key_stops`가
stack overflow를 출력하고 `aaa_host-55`가 SIGABRT(`exit=-6`)로 끝났다.
원문 artifact에도 호출 스택은 없었다. CI의 Rust는 1.99.1이었다.

기본 스택 재현은 격리한 Ubuntu 24.04 ARM VM 안의 Ubuntu 24.04 x86_64 사용자 환경에서
QEMU 11.1.3으로 실행했다. Ubuntu의 Rust 1.99.1과 GCC 13.3으로 x86_64 GNU 시험을 빌드했다.
verify profile, 기존 current-thread runtime과 단언을 유지했고 `RUST_MIN_STACK`을 설정하지 않았다.
QEMU의 스택 크기 옵션이나 환경 변수도 사용하지 않았다. CPU 실행은 에뮬레이션이며 x86 실기 결과로 쓰지 않는다.

| 소스 | 같은 기본 스택의 독립 실행 |
| --- | --- |
| t-1578 직전 main `5523c2d9` | stack overflow, SIGABRT |
| 실패한 main `289989e7` | stack overflow, SIGABRT |
| 이번 수리 | 기존 대상 시험 통과 |

직전 main도 같은 도구·실행 조건에서 실패하므로 t-1578의 백업 변경에만 생긴 회귀로 분류하지 않는다.
이 대조가 과거 CI 실행의 모든 환경과 실행 순서를 복원했다는 뜻은 아니다.

기존 수리 `bdaaa91b`는 현재 기준의 조상이다. [t-1543 기록](testing-automation-save-stack-t1543.md)의
자동화 저장 시험에 둔 heap 경계는 유지되어 있다. 이번 실패 시험에는 그 경계가 없었다.

## 관측한 원인과 수정

별도 GDB 실행에서 첫 SIGSEGV를 잡았다. 호출 경로는 다음과 같다.

```text
머신 설정 시험 poll → start poll → Host::call poll → Host::call_from poll
→ 요청 가림 직렬화 → redact::text → Rules의 LazyLock 초기화 → 스택 probe
```

중단 위치는 `aaa_common::redact::Rules::default`의 첫 스택 probe다. 반복 재귀는 관측되지 않았다.
실험실의 SSH 접속·키 감지 처리에 들어가기 전, 큰 호출자 poll 프레임에 정상적인 직렬화·초기화 프레임이
겹쳐 스택이 고갈됐다. t-1543의 Windows 관측과 같은 종류의 원인이다.

동일한 Ubuntu GNU 산출물의 함수 진입부에서 잰 프레임이다. 반환 주소·보존 레지스터를 포함하며
GDB의 인접 프레임 stack pointer 차이와 일치한다. future 객체 저장 크기와는 구별한다.

| 프레임 | 수정 전(bytes) | 수정 후(bytes) |
| --- | ---: | ---: |
| 대상 시험 poll | 522,528 | 232,224 |
| `start` poll | 52,208 | 52,208 |
| `Host::call` poll | 49,776 | 49,776 |
| `Host::call_from` poll | 1,230,992 | 1,230,992 |

실패 시험 안에 `Box::pin(h.call(...))` closure를 두고 직접 요청 여섯 개가 이를 거치게 했다.
시험 poll 프레임을 290,304바이트 줄였다. 실제 접속·새 키 승인·OS/CPU 감지·아는 키 재접속·
닫힌 포트 오류·바뀐 키 중단·역할별 거부·목록과 취소·runtime 종료 후 Host 해제 단언을 모두 보존한다.
새 시험을 추가하지 않고 실제 실패한 기존 계약 시험을 회귀 owner로 사용한다.

제품 dispatcher와 머신 설정 구현은 변경하지 않았다. 수정 함수는 `#[cfg(unix)]` 시험이므로
Windows 제품 경로 변경에 필요한 native 검사는 이번 범위에 해당하지 않는다.
스택 한도·시험 시한·기대값은 바꾸지 않았다. sleep·재시도 로직·시험 삭제도 추가하지 않았다.

## 검증

검증 명령은 `aaa verify -- <명령>`으로 실행했다.

- macOS: `scripts/verify.py develop --base 289989e7 --package aaa-host` 통과.
  aaa-host 전체 862개 통과·기존 2개 ignored, doctest와 all-targets strict Clippy·공개 검사 통과.
- Ubuntu x86_64 사용자 환경: 수리 전 대상 시험의 스택 고갈과 GDB 호출 경로 확인.
- 같은 Ubuntu 산출 조건의 수리 후 대상 시험 1개 통과(11.47초).
- 같은 수리 후 산출물의 `machine_setup::` owner 29개 통과(32.89초, CI와 같은 시험 thread 8개).
  이 시간은 QEMU 실행 시간이므로 native 성능 수치로 쓰지 않는다.
- 읽기 전용 fmt와 공개 저장소 검사 통과.

격리 VM과 조사에 쓴 별도 worktree를 삭제했다. abort로 남은 SSH fixture와 실행기는 VM과 함께 회수했다.

전체 ci-local·양 OS cross·Windows production은 이번 개발 후보에서 반복하지 않는다.
착지 창에서 [검증 규칙](verification.md)에 따라 최종 통합 SHA의 gate를 한 번 수행한다.

## 재현 환경의 실패와 구분

- 최초 macOS Zig 교차 링크 산출물에는 약 256KiB의 Zig signal-stack TLS가 추가됐다.
  그 조건에서는 수리 후에도 SQLite 호출에서 스택이 고갈됐다. GNU 산출물의 TLS는 672바이트로,
  이 초기 산출물을 CI 조건의 수리 판정에 사용하지 않았다.
- 교차 빌드에서 macOS archive 도구가 ELF 오브젝트를 담지 못했다. LLVM archive 도구를 지정하고,
  과거 build-script 캐시를 구분할 작업 전용 compiler 경로로 다시 만들었다. 기존 kache는 유지했다.
- Ubuntu의 QEMU 8.2.2는 독립적인 Rust hello에서도 내부 SIGSEGV를 냈다. 격리 VM의 작업 폴더에
  Debian QEMU 11.1.3을 풀어 실행했으며 host 설정은 변경하지 않았다.
- 최소 x86 사용자 환경에 curl·Python이 없어 owner 네 개가 fixture 단계에서 실패했다.
  누락 도구를 설치한 뒤 같은 바이너리·단언·시한으로 owner 29개가 통과했다.

## 참고한 공개 구현

- [Rust thread 문서](https://doc.rust-lang.org/std/thread/#stack-size)와
  [표준 구현](https://github.com/rust-lang/rust/blob/main/library/std/src/thread/mod.rs)
  (MIT 또는 Apache-2.0): 기본 thread 스택과 heap 저장의 구분을 확인했다.
- [Tokio runtime 구현](https://github.com/tokio-rs/tokio/blob/master/tokio/src/runtime/runtime.rs)
  ([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE)): 큰 future를 `Box::pin`으로 분리하는 방식을 참고했다.
  전체 future가 이미 heap에 있어도 요청을 만드는 시험 poll의 큰 프레임은 별도로 남는 것을 GDB로 확인했다.

외부 코드와 새 의존성은 추가하지 않았다.
