# 계정 Codex hook 검증(t-1229)

## 범위와 필요한 회귀

- `aaa-session`의 기존 `codex_trust_uses_native_cwd_without_trusting_repository_or_neighbours`를
  확장했다. 실제 계정 홈의 세 hook·기능·전용 파일 권한, 재시작의 동일 설정과 대화 보존,
  실행 파일 이동의 명령·신뢰 해시 갱신을 검사한다. 기존 cwd만 신뢰하는 계약은 유지한다.
- 새 `codex_hook_write_failure_keeps_cause_and_removes_auth_before_launch`는
  hook 대상이 디렉터리여서 쓰기에 실패하는 경우를 만든다. 필요한 이유는 부분 설치 뒤
  인증을 남기거나 안내 없는 계정 pane을 조용히 시작하는 회귀를 막기 위해서다.
  하위 OS 오류와 인증 삭제·기존 대상 보존을 검사한다.
- 기존 `windows_command_preserves_special_path_and_utf8_stdin_stdout`를
  agent에서 common으로 이동했다. 특수 경로·한글·UTF-8 입출력 계약은 그대로다.
- 기존 `crates/aaa/tests/codex_integration.rs` 전체를 유지한다. 실제 CLI→agent→host의
  startup·compact·resume 응답과 재독 문맥, 위조 token/peer·중첩 사건 거부,
  사용자 hook·notify·백업 보존과 설치·제거를 계속 확인한다.

새 unit은 하나이며 시험 삭제·새 E2E 번호·실패 재시도·고정 sleep·시한 확대는 없다.
전체 ci-local·cross와 main 병합·push는 coordinator의 정규 착지 차례에 진행한다.

## 실제 Codex 인수 도구

`scripts/test-account-codex-hooks.py`는 실제 aaa host·agent·계정 Codex thread와
loopback 모델 응답 fixture를 쓴다. 외부 모델·운영 인증·서비스·사용자 Codex 설정은 쓰지 않는다.
시험 root는 새 폴더여야 하며 기존 폴더면 즉시 거부한다. 모델 요청 원문은 저장하지 않고
hook developer 문맥 수와 기억 재독 표지의 존재만 기록한다.

```sh
python3 scripts/test-account-codex-hooks.py \
  --aaa target/verify/aaa --codex /path/to/codex \
  --root /tmp/aaa-account-hooks-new \
  --canary scripts/fixtures/secret-canary.txt
```

Windows는 `py -3`, `.exe` 경로와 자기 전용 `C:\aaa-test\t-NNNN` 아래의 새 root를 쓴다.
startup 응답 뒤 저장된 세션 명세에 시험용 재독 문맥을 넣고 실제 `/compact`를 보낸다.
압축 완료 화면 뒤 다음 요청의 developer 문맥에 그 표지가 있어야 한다.
이것은 실제 자동화를 새로 승인·실행하는 시험이 아니라 저장된 재독 문맥의 hook 전달 시험이다.
대기는 프로세스 로그·턴 완료·입력 표시·출력 일치 사건으로 끝나고 생성한 pane과 직접 띄운 프로세스만 정리한다.

## 결과

- macOS 계정 유닛 8개: 0.09초에 통과. 새 실패 경계 포함 개별 5초 미만이다.
- macOS 기존 Codex 통합 10개: 1.07초에 통과. 의존 경계 2개·이동한 common keeper도 통과했다.
- macOS 실제 Codex 0.164.0: startup hook developer 문맥 1개,
  `/compact` 뒤 다음 요청의 hook developer 문맥 1개와 시험용 기억 재독 표지를 확인했다.
- Windows 계정 유닛 7개: 0.10초에 통과. common keeper도 통과했다.
- Windows 기존 Codex 통합 9개: 파일 전체 8.33초에 통과했다(개별 시험 시간과 구별한다).
  기존 owner의 compact·하위 compact 요청은 PowerShell을 거친다.
- Windows 실제 Codex 0.164.0: startup 문맥 1개와 `/compact` 뒤 다음 요청의
  hook 문맥 1개·시험용 기억 재독 표지를 확인했다.
- common·session·agent의 all-targets strict clippy와 공개 검사가 통과했다.

첫 Mac 실제 실행은 기능·hook만 설치한 상태에서 `Hooks need review`로 멈췄다.
이 실패 화면을 보존하고 생성 내용 해시의 제한된 신뢰 설정을 구현한 뒤 새 실험실에서 확인했다.
이전 실패를 통과로 바꾸지 않는다.

처음 보강했던 m13 계정 fixture는 상속된 trace 로그를 오류로 취급해 실패했고,
로그 환경을 고친 실행은 6.43초였다. 새 hook 호출을 각 계정에 반복하는 보강은 제거하고
원래 m13 소스를 그대로 유지했다. 계정 hook 실제 연결은 위 실제 Codex 도구와 기존
Codex 통합 owner가 확인하므로 중복 비용을 정규 회귀에 추가하지 않는다.

Windows의 첫 실제 Codex는 hook 종료1로 안내를 받지 못했다. PowerShell에 기존
`if defined` 명령을 직접 주면 MissingOpenParenthesisInIfStatement로 종료1,
명시적 cmd로 감싸면 종료0·무출력인 대조를 확인했다. Codex 0.164.0의
[실행 shell 선택](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/core/src/session/mod.rs)과
[command runner](https://github.com/openai/codex/blob/rust-v0.164.0/codex-rs/hooks/src/engine/command_runner.rs)를
대조해 공통 명령에 cmd 경계를 넣었다. 기존 통합 owner의 compact 호출도 Windows PowerShell로
실행하도록 보강해 직접 cmd 호출만 통과하던 누락을 막는다.

전달 tar·pyc와 외부 조사 소스 사본은 보고 library에서 제거했다. 묶음 구성원을
추적 소스와 검토한 신규 소스 목록에 대조했으며 운영 토큰·개인 키 파일은 포함하지 않았다.
인수 도구는 저장소의 공통 canary만 읽는다. 보고 증거에는 로그·결과·해시만 남긴다.

Windows 인수 도구의 첫 명령 전송은 본문 표시를 턴 완료로 여겨 `/compact`가 입력칸에
남았고, 이어지는 검사에서는 압축은 끝났으나 다음 `pane run`의 글만 입력칸에 남았다.
두 실패 화면과 요청 통계를 보존했다. 각 입력에 턴 완료와 실제 입력 표시를 확인한 뒤
Enter를 보내도록 바꿨다. 새로운 입력 표시 사건 상한은 5초이며 원래 출력 시한은 유지했다.
같은 제품 빌드의 최종 실행에서 두 hook 문맥과 기억 재독 표지가 모두 확인됐다.

검증 로그·결과를 회수한 뒤 자기 Windows 프로세스가 0개임을 확인하고 전용 시험 폴더를 지웠다.
Mac의 자기 임시 실험실 4개도 정리했다. 시스템·운영 설정은 변경하지 않았다.

## 승인된 정규 착지

q-1664 1A에 따라 최신 main 3b1716c3과 후보 68377112를 no-ff로 통합한다.
계획은 §8.534이며 wire 변경이 없어 FRONT_API1.145·AGENT_LINK1.11을 유지한다.
E30·35의 기존 네 파일 분할을 보존하고 testing.md에는 링크 한 줄만 더한다.
전용 착지 worktree의 Playwright1.64.1·공용 브라우저를 쓰며 기기를 페어링하지 않는다.
이전 착지의 release 프로세스 종료를 ps로 확인한 뒤 nice15 전체 ci-local을 시작한다.
시작·실패 uptime과 첫 실패 원문을 보존한다. main 유래 다른 E2E 실패는 수리 전에 보고한다.
통과 뒤 공개 검사·Linux/Windows strict cross·별도 Windows production·개인정보 grep,
ff push·ls-remote 대조, 착지 SHA salt의 설치기·심볼 전달과 SHA·--version을 확인한다.
