# Windows 승인 PTY 정지의 일회성 진단(t-1159)

사용자 q-1553 1A가 허용한 진단 branch다. 기준은 원본 실패와 같은
`b61c276dc4faeda97bd4feada31f9688ed900665`다. 운영 수리는 원인이 확인된 뒤 별도로 다룬다.

기존 승인 시험의 정확한 yes·외부 TTY·같은 계획·private 발급 단언과 60초 실패 상한을 유지한다.
plain 읽기 진입/반환, 외부 TTY/계획 재검사, 발급자 초기화/잠금/키 생성/저장,
서명 완료, 출력 파일 생성/동기화에 고정 단계 이름을 기록한다.
부모의 trace는 청크 크기·단계·응답 순번/크기·자식 종료·reader 완료만 저장한다.
입력·키·서명·환경 원문과 메모리 dump는 추가 진단에 기록하지 않는다.

`.github/workflows/diagnostic-windows-approval.yml`은 이 진단 branch의 push에서만 실행한다.
Windows Server 2025에서 원본 CI와 같은 웹 자산을 준비하고 실제 install/aaa 실행 파일과
PDB의 SHA-256을 기록한다. 빌드 뒤 같은 실행 파일을 다시 만들지 않고 다음을 수행한다.

1. 자체 검증 fixture를 단독 실행해 실제 콘솔 대기와 Rust 함수가 스택에 보이는지 확인한다.
2. 각 독립 process에서 기존 승인 시험 한 건을 `--exact`로 선택한다.
3. 8개 process 동시 실행을 미리 정한 4차례 수행한다. 첫 실패 차례 뒤 중단한다.

이 반복은 원인 재현 실험이며 실패를 다시 돌려 성공으로 바꾸는 재시도가 아니다.
정상 결과만 얻으면 원인 미확정을 유지한다. CI 재실행과 추가 push는 새 승인을 받아야 한다.

`AAA_PTY_NATIVE_STACK`은 위 단독 진단 process에서만 켠다. DbgHelp API는 단일 thread 사용이
필요하므로 다른 panic/backtrace와 섞지 않는다. 기존 상한에서 아직 살아 있는 소유 자식만
함수 스택을 수집한 뒤 원래 실패로 끝난다. 스택 수집은 PID와 실제 handle을 대조하며
잠시 중단한 소유 thread를 재개한다. 전체 suite에 이 환경 변수를 켜지 않는다.

artifact에는 proof/trial stdout·stderr, 단계 trace, 실험 결과, OS/기준/head SHA,
실제 실행 파일/PDB의 상대 경로·크기·SHA를 보존한다. 취소된 실험실의 private 파일이
남을 수 있는 tmp 폴더는 업로드하지 않는다. 원본 실패 당시의 스택이 소급 확보되는 것은 아니다.

참고: [WezTerm portable-pty](https://github.com/wezterm/wezterm/tree/main/pty/src/win)
([MIT](https://github.com/wezterm/wezterm/blob/main/LICENSE.md)),
[Microsoft Terminal cooked read](https://github.com/microsoft/terminal/blob/main/src/host/readDataCooked.cpp)
([MIT](https://github.com/microsoft/terminal/blob/main/LICENSE)),
[ReadConsoleW](https://learn.microsoft.com/en-us/windows/console/readconsole),
[StackWalk64](https://learn.microsoft.com/en-us/windows/win32/api/dbghelp/nf-dbghelp-stackwalk64),
[SymInitializeW](https://learn.microsoft.com/en-us/windows/win32/api/dbghelp/nf-dbghelp-syminitializew).
구현·수명 경계를 참고했으며 외부 코드나 의존성은 추가하지 않는다.
