# Windows 승인 콘솔의 입력·스택 관측 후보(t-1365)

## 판단

세 경합 수리는 main `ca99fcc4`에 착지했다. 이 문서는 그 뒤 별도 후보의 개발 검증 기록이다.
기존 60초 승인 PTY 정체의 원인은 아직 미확정이다. 계측을 더했으며 정체를 수리했다고 판정하지 않는다.

기존 기록은 콘솔 모드 준비·ReadConsoleW 호출·반환, PTY 입력 쓰기 완료까지만 구분했다.
입력이 실제 콘솔 큐에서 관측됐는지와 살아 있는 자식의 스택이 없어 다음 단계가 막혔다.

## 더한 관측

Windows plain prompt의 기존 API·모드·wakeup mask·입력 판정은 그대로다.

- 각 read에 ID와 호출 thread ID를 붙인다. 반환 경과는 ReadConsoleW 직후에 저장하므로 관측 thread 정리 시간과 섞지 않는다.
- `AAA_TEST_INSTALL_CONSOLE_TRACE=1`이고 해당 debug 로그가 켜졌을 때만 읽기 전용 관측 thread를 만든다.
  원래 승인 PTY owner들이 이 옵션을 주며, 일반 실행의 기본값은 꺼짐이다.
- 입력 handle의 준비 사건을 기다리고 PeekConsoleInputW로 관측한다. 기록하는 것은 read ID·관측 시점의 경과 마이크로초·
  key-down 존재 여부뿐이다. 문자·키 코드·scan code·입력 길이를 출력하지 않고, 입력을 소비하거나 다시 쓰지 않는다.
- ReadConsoleW가 먼저 소비한 입력은 관측하지 못할 수 있다. 기록된 시간은 큐에서 **관측한 시각**이며 커널의 정확한
  최초 도착 시각이나 입력 전체 기록이 아니다. 관측이 없었다는 사실을 미도착 증거로 쓰지 않는다.
- 호출이 끝나면 자기 stop 사건을 깨워 관측 thread를 종료한다. 생성 실패나 관측 API 실패는 단계와 OS 오류 번호로 남긴다.
- PTY 부모는 입력 step 번호와 write 전·flush 뒤의 경과 시각, write 전 UTC 시각을 보관한다.
  보낸 문자열·키 값은 이 시간 기록에 넣지 않는다. 기존 입력 순서는 바꾸지 않았다.

## 실패 뒤 스택 수집

기존 60초 recv deadline이 먼저 실패를 확정하고 그 시점과 입력 시간 기록을 출력한다.
자식이 살아 있으면 종료하기 전에 별도 PowerShell helper로 최대 5초 동안 증거만 수집한다.
그 사이 응답이 오더라도 성공으로 바꾸거나 명령을 다시 보내지 않는다. 원래 panic의 elapsed도 실패 결정 시각으로 고정한다.
**통과 한도는 60초 그대로다.** 수집 비용 때문에 실패 프로세스의 실제 종료는 그보다 늦을 수 있다.

helper는 직접 띄운 자식의 PID·부모 PID·실행 파일을 대조한 뒤 읽기 권한으로 연다.
AMD64 context를 대상으로 잠깐 suspend → GetThreadContext/StackWalk64 → resume하며, 자기 helper 프로세스 안에서만
DbgHelp를 사용한다. 테스트 부모의 Rust backtrace와 심볼 엔진 상태를 공유하지 않는다.
새 debugger 설치·debug privilege 활성화·서비스·사용자 설정 변경은 없다.

저장하는 frame은 모듈 파일 이름·모듈 내 상대 위치·함수 이름뿐이다. 전체 경로·인자·변수·레지스터·절대 주소·
메모리 dump를 내보내지 않는다. 로컬 실행 파일/PDB 폴더를 검색하고 symbol server는 지정하지 않는다.
Rust 부모가 typed schema·크기·허용 문자 범위를 다시 확인한 값만 test.log로 옮긴다.
생성·대기·schema 오류는 분류를 남기고, helper가 실패하면 종료 코드·단계·오류 종류·OS 번호도 남긴다.

직계 CLI를 대상으로 한다. 다른 입구 프로그램이나 사라진 자식은 소유권/실행 파일 대조에서 거부된다.
AMD64가 아닌 helper에서는 stack 미지원 분류가 남으며 일반 입력 동작은 바꾸지 않는다.

## 검증

- macOS 기존 authorization owner 11개 통과(11.52초). Unix 입력/종료 경로는 바꾸지 않았다.
- Windows 기존 승인·취소·TLS owner와 새 실제 reader-stack owner: 4개 통과(8.13초).
  새 owner는 입력을 보내기 전에 살아 있는 CLI의 read thread를 스냅샷에서 찾고, frame이 실제 aaa.exe까지 이어지는지 확인한다.
  부모가 보낸 승인 입력으로 그 자식이 정상 완료하므로 suspend 뒤 resume도 검사한다.
  시험 폴더 경로·canary가 snapshot에 포함되지 않는지도 확인한다.
- 최종 marker/반환 경과 분리 뒤 새 owner 단독 통과(2.24초). syscall 앞 message가 여러 출력 조각으로 와도
  thread ID 뒤의 완전한 marker를 받은 뒤에만 snapshot과 입력을 실행한다.
- 필요한 새 시험인 이유: cross build만으로 실제 외부 프로세스의 thread/context·stack 수집과 재개를 확인할 수 없기 때문이다.
- Windows 별도 ConPTY probe에서 ReadConsoleW를 아직 시작하지 않은 상태로 opaque 입력을 보냈다.
  실제 큐의 key-down 관측 사건을 받은 뒤 read를 놓았고, 원래 입력 전체를 그대로 받았다.
  출력에 opaque 입력은 없었다(`observer_event=true input_kept=true private_input_logged=false`).
  이 probe는 검증용 임시 도구이며 제품이나 시험 목록에 더하지 않았다.
- macOS strict Clippy, Windows strict cross Clippy 통과. Windows native strict Clippy도 통과했다.
- 기존 60초 정체는 이번 일반 승인·취소·TLS 실행에서 재현하지 못했다. 위 결과를 원래 실패의 회복으로 쓰지 않는다.
- 새 후보의 개발 owner 검사다. 새 착지 창 전에는 전체 gate·production 재빌드·병합·push를 하지 않는다.
  이미 전달한 main production exe/PDB는 그대로 보존한다.

처음 만든 보조 probe는 ConPTY 초기 커서 질의에 답하기 전에 TCP 울타리를 기다려 멈췄다.
자기 probe의 실제 스택에서 TcpListener::accept를 확인하고 직접 확인한 자기 PID 둘만 종료했다.
질의 응답을 먼저 처리하는 사건 루프로 고쳤다. 제품 정체의 재현이나 제품 시험 실패로 세지 않았다.
또한 계측 개발 중 strict cross가 새 reader-ID 검사의 누락을 unused variable로 잡았고 실제 ID 대조를 완성해 통과했다. 관측 thread 정리의 중첩 if도 native lint 지적대로 합쳤다.
제품 단언·시한·기대값을 완화하지 않았다.

## 참고

- [PeekConsoleInput 공식 문서](https://learn.microsoft.com/en-us/windows/console/peekconsoleinput):
  입력을 제거하지 않는 API와 빈 큐에서 즉시 반환하는 조건을 확인했다.
- [GetThreadContext](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadcontext)·
  [StackWalk64](https://learn.microsoft.com/en-us/windows/win32/api/dbghelp/nf-dbghelp-stackwalk64):
  안정된 context와 frame 수집·심볼 엔진의 동시 호출 제한을 대조했다.
- [StackWalker](https://github.com/JochenKalmbach/StackWalker/blob/master/Main/StackWalker/StackWalker.cpp):
  외부 thread context와 bounded stack walk를 비교했다. 저장소 [LICENSE](https://github.com/JochenKalmbach/StackWalker/blob/master/LICENSE)는
  BSD-2-Clause이고 소스 머리에는 추가 endorsement 제한이 있어 원문도 확인했다. 코드는 가져오지 않았다.
- [Boost.Stacktrace](https://github.com/boostorg/stacktrace/blob/develop/include/boost/stacktrace/detail/frame_msvc.ipp)
  ([BSL-1.0](https://www.boost.org/LICENSE_1_0.txt)): 심볼 엔진 상태와 모듈/함수 이름의 경계를 비교했다.

외부 소스·새 라이브러리 의존성은 추가하지 않았다. Windows interop helper는 이 저장소에서 검토한 정적 코드만 실행한다.

## 사후 전달 위치 재확인에서 중단

착지 때 전달한 production은 native/verify·SHA·크기·판 대조를 모두 통과했다.
계측 개발 뒤 정리 전에 같은 전달 파일의 보존 여부를 확인하려 했으나 전달 폴더가 없었다.
뒤따른 읽기 전용 목록/존재 조회에서도 폴더와 exe가 없음을 확인했다. 삭제 원인은 미확정이다.
정리 명령은 이 검사에서 멈춰 뒤의 삭제 단계로 진행하지 않았다.

원격의 계측 시험 폴더는 보존했고, 재전송·재검증·재빌드·추가 push 없이 중단했다.
로컬의 검사한 원본 exe/PDB·출처·native 영수증은 남아 있다. 계측 소스와 개발 검증 결과도 로컬 후보로 보존한다.
원격 계측 전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽은 같았고, 계측 폴더의 실행 중인 프로그램은 0개였다.
