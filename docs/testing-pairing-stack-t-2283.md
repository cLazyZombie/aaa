# Front API 요청의 스택 고갈 수리(t-2283)

## 이전 수리와 재발

Windows CI의 페어링 시험 `pairing_tests::the_web_keeps_its_last_controller_and_the_host_machine`이
`120451b1`과 `d47a54b3`에서 `0xc00000fd`로 종료됐다. t-1894의 수리 `bee92c75`는 기획 버리기와
thread 시작의 큰 future를 heap으로 분리했다. 수리 병합 `ed43c32e`의 Windows CI는 통과했고,
`bee92c75`와 `ed43c32e`는 직전 성공 `c972e9ff`와 이번 실패 `d47a54b3` 모두의 조상이다.
[이전 실패](https://github.com/cLazyZombie/aaa/actions/runs/43800872516/job/133080569669) ·
[이전 수리 CI](https://github.com/cLazyZombie/aaa/actions/runs/43806693954/job/133098994826) ·
[t-1894 분석](testing-design-docs-t-1894.md).

| 소스 | GitHub Windows CI | Windows 11 x86-64 실기, 대상 단독 실행 |
| --- | --- | --- |
| `120451b1` | 스택 고갈 | `0xc00000fd` |
| `c972e9ff` | 통과 | `0xc00000fd` |
| `d47a54b3` | 스택 고갈 | `0xc00000fd` |
| 수리 코드 `065cadd7` | 착지 창 대기 | 통과 |

실기는 Rust 1.100.0 MSVC의 `verify` profile로 `aaa-host --lib`를 빌드했다. CI와 같은 시험 thread 2개,
Tokio worker 2개를 사용했고 libtest 스택 기본값 2 MiB를 유지했다.
직전 성공 SHA도 같은 실기에서 단독으로 실패한다. 공용 호출 경계의 스택 여유가 다시 소진된 재발로
판정한다. 같은 CI 산출물에서 성공과 실패가 번갈아 나는지는 확인되지 않았다.
[직전 CI](https://github.com/cLazyZombie/aaa/actions/runs/46080701142/job/140778738697) ·
[이번 CI](https://github.com/cLazyZombie/aaa/actions/runs/46117507684/job/140894515639).

## 실제 예외와 프레임

디버거가 대상 시험 자식을 시작하고 첫 `EXCEPTION_STACK_OVERFLOW`에서 `GetThreadContext`와
`StackWalk64`로 스택 포인터와 호출 주소를 얻었다. TEB와 메모리 할당 범위에서 세 재현의 예약 크기
2,097,152바이트를 확인했다. 실행 파일에서 읽은 GUID·age에 맞는 PDB로 주소를 해석했고, 반환 주소를 포함한
프레임 크기는 PDB의 `S_FRAMEPROC`와 예외 시점의 인접 스택 포인터 차이로 맞췄다.
[Windows StackWalk64](https://learn.microsoft.com/windows/win32/api/dbghelp/nf-dbghelp-stackwalk64) ·
[Rust 기본 스택](https://doc.rust-lang.org/std/thread/#stack-size).

```text
페어링 시험 poll → 기기를 짝짓는 closure poll → mint poll
→ Host::call poll → Host::call_from poll → 요청 가림 → FrontRequest::serialize
```

`c972e9ff`는 직렬화가 문자열을 가리는 중에 멈췄다. `120451b1`과 `d47a54b3`는 직렬화 프레임을
확보하는 스택 probe에서 멈췄다. 셋 다 첫 `WebPair` 요청의 경로다. 모델 비활성화 시험
`settings_admin::tests::disabled_models_stay_listed_and_block_only_profile_model`도 `d47a54b3`의
단독 실행에서 같은 종료 코드로 실패했다. 별도 첫 예외에서 이 시험의 poll, 설정 시험 도우미,
`Host::call_from`, 요청 가림·직렬화가 중첩된 것을 확인했다.

| Windows 프레임(바이트) | `120451b1` | `c972e9ff` | `d47a54b3` | 수리 후보 |
| --- | ---: | ---: | ---: | ---: |
| 페어링 시험 poll | 510,048 | 465,984 | 471,744 | 16,464 |
| 페어링 closure poll | 49,968 | 45,552 | 46,128 | 608 |
| `mint` poll | 53,472 | 49,056 | 49,632 | 4,112 |
| `Host::call` poll | 50,480 | 46,064 | 46,640 | — |
| 공용 dispatch poll | 1,270,608 | 1,296,944 | 1,319,216 | 1,319,216 |
| 모델 비활성화 시험 poll | — | 425,536 | 430,720 | 21,008 |

## t-2273 변경의 영향과 수리

두 현행 SHA에서 `dispatch.rs`는 같다. 공용 poll 프레임은 22,272바이트, 페어링 시험 poll은
5,760바이트 커졌다. `FrontRequest::serialize` 프레임은 두 판 모두 94,832바이트다.

같은 Rust의 macOS arm64 컴파일러 타입 배치에서 검수 설정 추가 뒤 `ThreadRec`이 2,104 → 2,152바이트,
`StartArgs`가 1,056 → 1,104바이트가 됐다. `thread_park` future는 19,672 → 19,960바이트,
`thread_restart_with`를 기다리는 `thread_unpark`는 14,200 → 16,520바이트였다. 계정 시작 잠금 변경이
함께 들어간 `pool_submit`도 20,600 → 21,288바이트로 커졌다. 공용 `call_from` future의 저장 크기는
22,128 → 22,416바이트였다. 이 값은 Windows poll 프레임 크기와 별개다. 비최적화 빌드에서 각 요청
분기의 임시값이 공용 poll 프레임에 잡히고, 호출자도 `await`마다 큰 임시값을 저장한다.
페어링 요청에서도 이 공간이 겹친다.

`Host::call_from`에서 비동기 dispatch를 `Box::pin`하고 `Host::call`이 그 future를 직접 넘긴다.
요청마다 heap 할당 한 번에 dispatch 상태를 담는다. 기존 dispatch 본문에서 권한 판정·가림·정규화·처리를 수행한다.
수리 후 공용 poll은 1.26 MiB이며, 호출부의 인라인 future와 임시 공간이 줄어든다. 기존 시험의 기본 스택과 단언으로 확인했다.
Tokio(MIT)의 큰 future 처리 방식을 참고했다. [Tokio runtime 구현](https://github.com/tokio-rs/tokio/blob/master/tokio/src/runtime/runtime.rs).

## 관련 검증

- Windows 수리 전 PDB에서 큰 시험 poll을 찾아 페어링 외 8개를 같은 기본 스택에서 단독 실행했다.
  모델 비활성화 시험은 스택 초과, 나머지 7개는 통과했다. `installed_machine_check_and_login`은
  Unix에서만 등록돼 Windows 검사 수에서 뺐다.
- Windows 수리 후 기본 스택에서 페어링과 모델 비활성화 시험 통과. host 유닛 전체 972개 통과, 기존 3개 ignored. 네이티브 strict Clippy 통과.
- macOS 수리 전 대상 단독 실행은 스택 초과. 수리 후 host 유닛 1,003개 통과·기존 2개 ignored,
  doctest·fmt·strict Clippy, 역할 의존성 시험 2개 통과.
- Linux x86-64 및 Windows MSVC 대상의 host strict cross Clippy와 공개 저장소 검사 통과.
- Windows 첫 전체 실행의 네 실패는 로컬 `core.autocrlf`가 스키마와 CI/Git 예제 세 파일을 CRLF로
  바꿔 생겼다. 세 파일을 Git blob에서 바이트 그대로 복원한 뒤 전체 시험을 실행했다.

최종 통합 gate는 착지 창에서 [검증 규칙](verification.md)에 따라 실행한다.
