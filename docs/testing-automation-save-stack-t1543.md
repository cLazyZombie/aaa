# 자동화 저장 시험의 Windows 스택 고갈 수리(t-1543)

## 실패와 기존 수리의 관계

실패 기준 `a8fbe5058ba26020dbb7d49e10fbb96394ba0aba`는 t-1208의 병합
`b3fa9a08e72fbd878db915a65b0c0877306ab1e8`를 포함한다(`git merge-base --is-ancestor` 확인).
t-1208은 `idle_audit_is_read_only_on_web_and_local_cli_keeps_control`의 요청 future를
heap에 두었으며, 이번 저장 시험에는 그 경계가 없었다.

`automation::tests::save_writes_only_the_seen_revision_and_never_resurrects_a_deleted_rule`을
Windows x86_64의 verify profile·기본 libtest 스택에서 격리 실행해 같은 실패를 재현했다.
`RUST_MIN_STACK`은 설정하지 않았고 스택·profile·runtime·시한을 바꾸지 않았다.
외부 디버거가 직접 생성한 시험 자식의 Windows debug event에서 첫 stack overflow 예외를
읽고, `GetThreadContext`·`StackWalk64`와 같은 실행 파일의 PDB로 호출 위치를 확인했다.
프로세스 종료 코드는 `0xc00000fd`였고 TEB에서 본 스택 범위도 약 2MiB였다.
macOS의 같은 기본 조건 격리 실행도 stack overflow로 종료했다.

## 관측한 원인과 최소 수리

실패 스택은 시험 poll → `Host::call` → `Host::call_from` → 요청 가림 직렬화 →
`redact::text` → 가림 규칙의 `LazyLock` 초기화 → `_alloca_probe` 순서다.
재귀는 관측되지 않았다. 마지막 초기화 함수만으로 원인을 단정하지 않고,
호출자가 이미 소비한 큰 프레임의 중첩을 원인으로 보았다.

Windows PDB의 `S_FRAMEPROC`에서 지역 프레임·보존 레지스터·반환 주소를 합산했다.
수리 전 값은 예외 시점의 인접 stack pointer 차이와도 일치한다.
이 값은 future 객체 자체의 저장 크기가 아니라 실제 컴파일된 poll 프레임 크기다.

| 프레임 | 수리 전(bytes) | 수리 후(bytes) |
| --- | ---: | ---: |
| 저장 시험 poll | 639,568 | 31,792 |
| `Host::call` poll | 48,176 | 48,176 |
| `Host::call_from` poll | 1,222,752 | 1,222,752 |

저장 시험 안에 t-1208과 같은 `Box::pin(h.call(...))` closure를 두고 요청 13개의
await가 이를 거치게 했다. 제품 dispatcher·저장 구현은 바꾸지 않았다.
관측 revision 충돌 시 파일 불변, 삭제 뒤 낡은 저장의 NotFound·비복원,
Reader 쓰기 거부, 잘못된 글 거부, 꺼진 상태 복원·중복 복원 거부,
구형 규칙 읽기·쓰기 거부 단언을 모두 보존한다.
스택 증대·재시도·sleep·timeout 연장·기대값 변경·시험 삭제는 없다.

같은 모듈의 다른 요청 시험도 PDB 프레임을 확인했다. 큰 순서로 묶음 해제
390,256bytes, 변환 미리보기 343,760bytes, 수동 실행 246,832bytes,
본문 파일 저장 196,864bytes였다. 기존 idle-audit의 heap 경계도 그대로 있다.
추가 변경은 실제 실패가 재현된 저장 시험으로 한정한다. 새 시험은 추가하지 않고
실패했던 계약 시험을 그대로 회귀 owner로 사용한다.

## 검증

검증 명령은 모두 `aaa verify -- <명령>`으로 실행했다.

- macOS: 대상 시험 수리 전 stack overflow, 수리 후 1개 통과(0.40초).
- macOS: `scripts/verify.py develop --base a8fbe505 --package aaa-host` 통과.
  aaa-host 전체 849개 통과·보조 2개 ignored, doctest와 all-targets strict Clippy·공개 검사 통과.
- macOS: `cargo test --profile verify --locked -p aaa --test deps` 2개 통과.
- Windows: 대상 시험 수리 전 `0xc00000fd`, 수리 후 1개 통과(0.20초).
- Windows: `cargo test --profile verify --locked -p aaa-host --lib automation::`
  기본 스택·기본 병렬도에서 84개 통과(전체 4.39초).
- Windows: aaa-host all-targets strict Clippy 통과.
- fmt와 공개 저장소 검사 통과.

초기 재현은 기본 임시 경로에 abort fixture를 남겼다. 실행 시각과 fixture 구성을 대조해
이번 Windows 재현의 3개와 macOS 재현의 1개만 식별했고, 링크가 없음을 확인한 뒤 삭제했다.
Windows 서비스·예약 작업·Run·사용자 PATH·방화벽 기준은 전후가 모두 같았다.
격리 경로를 실행하는 프로세스 0개·작업 폴더 링크 0개를 확인하고 Windows 작업 폴더를 삭제했다.

착지 창은 아직 받지 않았다. [검증 규칙](verification.md)에 따라 최종 통합 SHA가 정해지는
착지 때 전체 ci-local·Linux/Windows strict cross gate를 한 번 실행한다.
현재 개발 후보 검사를 최종 통합 gate 통과로 기록하지 않는다. 배포 산출물은 만들지 않았다.
