# Windows 설치 worker 사건 대기와 진단 보강(t-1487)

## 확인한 실패 경계

이번 자연 실패는 plain 승인 입력의 ReadConsoleW 정체와 다르다. 승인 발급은 끝났고, 승인된 파일 적용 CLI가 detached worker의 다음 메시지를 기다리고 있다. worker 내부 정체 원인은 현존 자료로 확정할 수 없다.

[실패 Windows job](https://github.com/cLazyZombie/aaa/actions/runs/41576224737/job/125720231322)의 전체 로그와 `rust-tests-windows-2025-41576224737-1` artifact 전체 `test.log`를 대조했다. 둘의 실패 경계·스택·공개 사건이 일치한다. artifact에는 `test.log`만 있다. 원문은 작업 전용 target에만 보존했다.

- 실행 SHA는 `3d018dd727ba5a13457e7490f7baef1c4a630fad`, attempt 1이다. 진단 변경 `af8f4b13`은 이 SHA의 조상이다.
- `closed-window phase=authorize`는 1.468초에 끝났다. 실패는 `recovery.rs:127`의 승인 파일 적용 호출이며, `phase=apply`다.
- `answered=0/0`, 입력 쓰기 기록 없음, `waiting=child_exit`, `child_exit=None`, `reader_done=false`다. 이 호출은 승인 질문이나 취소 입력을 요구하지 않는다.
- PTY 판정 상한은 60.0138519초다. 그 뒤 스냅샷 helper가 885ms에 성공했고 실제 스택 기록은 913ms에 완료됐다.
- 살아 있는 CLI의 주 thread는 `NtRemoveIoCompletionEx → GetQueuedCompletionStatusEx → mio → tokio current_thread → worker::launch`의 async 대기에 있다.
- 다른 thread는 `NtWaitForSingleObject → Child::wait → worker::launch`다. 이 thread는 worker의 종료를 관측하는 보조 thread다. 주 thread가 `completion.join()`에 막힌 모양이 아니다.
- 공개 사건은 `seq=1 Authorize/Completed`, `seq=2 Stage/Running`까지다. `Configure/Completed`, `Enroll/Running`, 최종 결과는 없다.
- 따라서 worker의 spawn·보호 입력/EOF·연결·Started·Stage 진행 메시지는 이미 통과했다. CLI는 그 뒤 `rx.control::<Reply>()`의 다음 메시지를 기다리는 것으로 좁혀진다.
- worker는 Stage 메시지를 보낸 뒤 `execution.files()`를 동기 실행하고 이후 파일 완료 메시지를 보낸다. 파일 내부 어느 호출에서 멈췄는지, 파일 처리가 끝난 뒤 메시지 전달이 멈췄는지는 기존 자료로 구분되지 않는다.
- 콘솔 호스트는 누적 kernel 156250×100ns, user 0, 다섯 thread의 WCT가 종류 8·상태 3·cycle false다. 이 누적 값과 한 번의 WCT로 순간 부하나 파일 I/O 원인을 추정하지 않는다.

PTY 출력에 제목 OSC만 있는 것은 worker 실행 이전에 멈췄다는 뜻이 아니다. 승인된 worker의 진행은 별도 IPC와 사건 파일로 전달되므로, 이번에는 공개 사건 파일과 실제 CLI 스택이 더 많은 정보를 준다.

## 기존 실패와의 관계

| 기록 | 입력·단계 | 확인한 대기 | 판단 |
| --- | --- | --- | --- |
| t-1303 | 승인 Ctrl+Z 전송 뒤 | console read 반환 미확인 | 이번 worker 사건 대기와 구분 |
| t-1365 | 승인 yes+CR 전송 뒤 | ReadConsoleW 미반환 | 이번과 다른 관측 경계 |
| t-1472 | wizard 머신 이름+CR 전송 뒤 | 실제 ReadConsoleW 스택 | 이번과 다른 관측 경계 |
| t-1246 | 같은 recovery의 승인 파일 적용 | 생존 CLI, 사건·worker 스택 없음 | 외형은 같으나 공통 근본 원인은 미확정 |
| 이번 | 승인 완료 뒤 Stage/Running | CLI의 worker 메시지 대기·worker 종료 관측 thread | worker 내부 증거가 빠짐 |

기존 Ctrl+Z 문제가 해결됐다고 쓰거나, 위 실패들이 모두 같은 원인이라고 묶지 않는다. 기존 thread를 되살리거나 다른 thread에 연락하지 않았다.

## 진단 후보

제품 코드는 바꾸지 않았다. 실패 확정 뒤 실행하는 기존 시험 수집기에 다음을 더했다.

- 호출자는 직접 띄운 CLI의 부모·전체 실행 이미지 경로를 검증한 뒤, 같은 이미지의 직계 자식 최대 4개의 실제 process handle을 보관한다.
- helper는 요청받은 PID마다 부모·전체 이미지 경로를 다시 확인하고 같은 handle로 thread의 함수 스택을 수집한다. detached worker에는 콘솔이 없으므로 worker의 콘솔 호스트 조회는 하지 않는다.
- DbgHelp는 프로세스별로 차례로 사용하고, CLI·worker의 thread마다 기존 suspend/resume 경계를 쓴다. 다른 작업의 PID·전체 시스템 스택·명령줄·메모리 원문은 내보내지 않는다.
- helper의 강제 종료·실패나 worker thread resume 오류 때는 호출자가 보관한 검증된 handle로만 해당 자식을 종료한다. 수집기가 중간 종료돼 detached worker가 중지 상태로 남는 것을 막는다. 정상 수집 때는 자식 수명을 바꾸지 않는다.
- 원래 PTY 실패 판정 60초와 helper 수집 5초를 유지한다. helper 실패 뒤 소유 자식 정리는 별도의 종료 확인이며, 성공 응답을 더 기다리지 않는다.
- 자식 4개가 추가될 수 있어 진단 진행 배열의 허용 개수만 72에서 360으로 맞췄다. 성공/실패 시한이나 시험의 기대값을 완화한 것이 아니다.
- 기존 worker 수명 owner에서 실제 detached worker PID의 스택 존재와 private 값·실험실 경로 비노출을 확인한다. 필요한 이유는 CLI 스택만으로 worker 원인을 구분하지 못한 이번 진단 누락을 막기 위해서다.
- 새 Windows owner는 실제 두 자식을 띄워 검증한 handle만 종료되고 다른 자식이 살아 있는지 확인한다. 필요한 이유는 새 실패 정리 경계가 무관한 프로세스에 닿지 않음을 OS에서 검증하기 위해서다.

입력 방식·승인 조건·뒤로 이동·같은 계획·private 파일·worker가 부모를 벗어나는 계약은 유지한다. 근본 원인 수리라고 주장하지 않는다.

## 검증

검증은 `aaa verify`를 통해 수행한다. Windows는 전용 폴더·TEMP를 사용하며 운영 설치·서비스·사용자 설정을 바꾸지 않는다.

- 원본 Windows recovery owner: 1개 통과, 6.06초. 정체 미재현이며 CI 회복 근거가 아니다.
- 후보 macOS install 전체: 35개 통과·기존 1개 ignored, 12.89초.
- macOS install strict Clippy와 fmt: 통과.
- Windows 후보 install 전체: 35개 통과·2개 실패·기존 helper 1개 ignored, 12.70초. 전체 성공이 아니다.
- 실제 detached worker 스택 단언, 검증한 handle 정리·다른 자식 보존, 스냅샷 세 계약·오류 정제, Ctrl+Z/C 취소, 두 wizard, recovery, 실제 TLS enrollment·부모 소멸 뒤 worker 결과는 모두 통과했다.
- worker를 포함한 helper 수집은 1241ms였고 기존 5초 상한 안에 끝났다. 원본 정체는 후보에서도 재현되지 않았다.
- Windows install native strict Clippy: 통과, 1분 4초.
- Windows의 검증 SHA는 코드 후보 `5bb81fea`이고 Git 작업 트리는 깨끗했다. Rust 네 파일은 Windows checkout의 CRLF 때문에 raw SHA-256이 달랐고, 로컬 LF를 CRLF로 바꾼 바이트의 SHA-256은 네 파일 모두 실기 파일과 일치했다. 소스 차이는 줄 끝뿐이다.
- 검증 전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽 기준이 모두 같았고 전용 폴더의 실행 중 프로그램은 0개였다.
- fmt·diff·공개 저장소 검사: 통과. 최종 문서 반영 뒤에도 공개 검사를 확인한다.

기존 원본 owner의 단독 실행도 개별 시험 5초 목표를 넘었다. 이번 작업에서 시나리오를 줄여 성능 목표를 통과시켰다고 쓰지 않는다. 웹 자산 없는 개발용 verify 실행 파일을 썼으므로 CI의 실행 파일과 바이트가 같은 재현이 아니다.

Windows 준비 첫 실행은 기억에 기록된 깨진 Git shim 때문에 `aaa verify`가 Git root를 찾지 못해 시험 전에 끝났다. 실제 Git 디렉터리를 그 검증 프로세스의 PATH 앞에만 둬 수정했다. 사용자 PATH는 바꾸지 않았다. 초기 fmt 검사에서 보인 들여쓰기는 formatter로 정리했다. 미커밋 상태의 검증 원장이 수집되지 않은 첫 fmt 실행은 최종 검증으로 세지 않는다.

## Windows 전수에서 남은 별도 실패

두 실패 모두 [t-1303의 원본 전수 기록](https://github.com/cLazyZombie/aaa/blob/main/docs/testing-win-auth-t-1303.md)에 같은 서명으로 기록돼 있고 변경한 네 파일의 계약과 다르다. 이번 실패를 반복 실행·기대값 변경으로 통과시키지 않았다.

- `recovery::one_line_entry_enters_setup_and_cancels_without_installation_changes`: `recovery.rs:244`의 `Command::new("python")`가 `NotFound`다. 해당 검증 프로세스에서 `python` 명령이 없고, `py -3`는 설치된 Python 3.9를 찾는 것을 별도 준비 조회로 확인했다. 제품이나 사용자 PATH를 바꾸지 않았다.
- `authorization::ssh_environment_and_helper_hash_flags_do_not_bootstrap_a_new_trust_root`: 기대 3, 실제 4다. 실기는 실제 SSH 자손이어서 `VerifiedSsh::current()`가 실제 OS 조상을 인정한 뒤 0 해시를 conflict/4로 거부하는 코드와 부합한다. 원래 CI의 plain/worker 정체와 다른 실패다.

## 참고와 한계

[Microsoft Terminal cooked read](https://github.com/microsoft/terminal/blob/main/src/host/readDataCooked.cpp) ([MIT](https://github.com/microsoft/terminal/blob/main/LICENSE))와 [Alacritty ConPTY](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/tty/windows/conpty.rs) ([Apache-2.0](https://github.com/alacritty/alacritty/blob/master/LICENSE-APACHE))의 입력·콘솔 수명 경계를 대조했다. 이번에는 console read가 정체된 스택이 아니므로 raw 입력 전환이나 콘솔 설정 변경을 수리로 택하지 않았다.

[Microsoft Toolhelp](https://learn.microsoft.com/en-us/windows/win32/toolhelp/taking-a-snapshot-and-viewing-processes)와 [StackWalk64](https://learn.microsoft.com/en-us/windows/win32/api/dbghelp/nf-dbghelp-stackwalk64)의 부모 PID·process handle·직렬 DbgHelp 계약을 따랐다. 외부 코드나 새 런타임 의존성은 추가하지 않았다.

worker 스택·파일 journal·실패 순간 자원 표본은 원 CI artifact에 없다. 사라진 상태를 소급 복원할 수 없으며, 실기 단독 성공으로 원본 CI가 회복됐다고 선언할 수 없다. 별도 승인한 착지에서 진단을 반영한 뒤 다음 자연 실패의 worker 스택을 확인해야 추가 원인 분류가 가능하다. 병합·push·새 CI 실행·production 빌드·배포는 하지 않았다.
