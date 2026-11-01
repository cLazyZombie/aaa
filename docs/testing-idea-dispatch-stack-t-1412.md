# 아이디어 API 분리와 스택 회귀 수리(t-1412)

아이디어 비동기 경로를 추가한 뒤 자동화 저장 회귀 시험이 stack overflow로 중단됐다. 시작 main의 같은 묶음은 통과했다. 공용 `Host::call_from`의 큰 poll 스택 프레임이 다른 요청의 가림·직렬화에도 겹쳐 기본 시험 스택을 소진한 회귀였다.

## 원인과 수정

실제 crash는 `automation::tests::save_writes_only_the_seen_revision_and_never_resurrects_a_deleted_rule`의 앞부분 `AutomationValidate` 요청에서 발생했다. `Host::call` → `call_from` → `redact::json` → `FrontRequest` 직렬화 → 정상적인 JSON form 순회 경로다. 자동화 저장 구현이나 무한 재귀의 문제가 아니었다.

`lib.rs`의 아이디어 요청 아홉 분기를 `idea_thread.rs`의 별도 비동기 함수 `call_idea`로 옮겼다. 공용 입구에는 도메인 호출 하나만 남겼다. 권한·프로젝트 해석·요청 가림 순서와 각 아이디어 요청의 처리는 유지했다. 제품 코드 변경은 이 host 두 파일이다. 자동화 구현·시험 코드·스택 크기·시한·단언·의존성·protocol은 바꾸지 않았다.

## 같은 조건의 대조

workspace 전체 `cargo test --no-run`이 만든 verify(opt-level 0) artifact를 사용했다. 당시 자동화 owner의 정확한 86개 이름을 `--exact`, `RUST_TEST_THREADS=1`, `TOKIO_WORKER_THREADS=2`로 실행했다. `RUST_MIN_STACK`과 production salt는 넣지 않았다. 시작 main은 별도 worktree·자기 target으로 준비했고 기존 kache는 유지했다.

| 대상 | 같은 86개 묶음 |
| --- | --- |
| 시작 main `0af6fcc8` | 통과 |
| 수정 전 통합 후보 `d761d605` | 같은 시험에서 stack overflow, exit -6 |
| 아이디어 분기 분리 후 | 86개 통과 |

이는 실패를 재실행해 우연히 통과시키는 절차가 아니라 승인된 시작 main 대조와 원인 수리 전후 비교다. 최초 실패 원문과 수정 전 binary는 별도로 보존했다.

## 생성 코드의 스택 감소

macOS arm64 verify binary의 함수 진입부(stack probe·SP 감소·저장 레지스터)로 확인했다. future 객체 크기가 아니라 poll 실행 때 잡는 임시 스택 프레임이다.

| 프레임(bytes) | 시작 main | 수정 전 | 분리 후 |
| --- | ---: | ---: | ---: |
| 시험 async poll | 639,984 | 639,984 | 639,568 |
| 시험 wrapper | 50,432 | 50,432 | 50,400 |
| `Host::call` poll | 48,160 | 48,160 | 48,128 |
| `Host::call_from` poll | 1,202,880 | 1,254,880 | 1,191,024 |
| `FrontRequest::serialize<Guard>` | 98,960 | 99,408 | 99,408 |
| 위 다섯 프레임 합계 | 2,040,416 | 2,092,864 | 2,028,528 |

공용 poll 프레임은 수정 전보다 63,856바이트 줄어 시작 main보다 작아졌다. 아이디어 처리 전용 poll 프레임은 80,704바이트이고 자동화 요청의 공용 가림 단계에는 호출되지 않는다.

기본 Rust thread 스택은 Tier-1에서 현재 2MiB다. 다섯 큰 프레임 외에도 Tokio·libtest·정상 직렬화·가림 프레임이 필요하다. 수정 전에는 나머지 공간이 4,288바이트뿐이었고, 분리 후에는 68,624바이트가 남는다. [Rust 공식 thread 문서](https://doc.rust-lang.org/std/thread/)

## 관련 검증

새 시험을 추가하거나 기존 시험을 고치지 않았다. 실패한 기존 owner가 회귀 검증을 맡는다. 공용 API의 추출 범위 때문에 같은 workspace artifact의 host 유닛 전체와 아이디어 E2E를 함께 확인한다.

- 실패 owner: 기본 스택에서 86개 통과.
- 같은 workspace artifact의 host 유닛 전체: 807개 통과, 기존 ignored 2개, 실패 0.
- host strict Clippy·읽기 전용 fmt: 통과.
- 아이디어 E2E 120(Chromium·WebKit): 통과.
- 공개 저장소 검사: 통과.

전체 착지 gate·양 OS cross·Windows production·전달·push는 새 착지 창에서 수행한다. 이 수리 단계에서는 착지를 재시도하지 않는다.
