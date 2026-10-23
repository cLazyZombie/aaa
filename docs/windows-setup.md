# Windows 설치기(`aaa setup`)

Windows PC 하나를 aaa agent로 설치하고, Windows 서비스로 띄워 host에 연결하는 데까지 **한 번 실행으로** 끝낸다.
실행 파일 하나(`aaa-setup.exe`, 내용은 `aaa.exe`와 같다)를 실행하면 된다. 중간에 멈춰 「같은 ID로 재개」를 시키지 않는다.
사람 입력(host 승인·Windows 암호)은 그 자리에서 받고 이어 간다. 설계 기록은 [plan §8.466](plan.md).

## 실행

| 어디서 | 명령 |
| --- | --- |
| 관리자 계정의 SSH(대화형) | `ssh -t <PC> "C:\경로\aaa-setup.exe"` 또는 접속한 셸에서 `aaa-setup.exe` |
| 관리자 PowerShell | `.\aaa-setup.exe` |
| 탐색기 | 더블클릭 → 관리자 창을 열지 묻고 UAC로 같은 실행을 이어 간다. 끝나면 Enter로 창을 닫는다 |

- 관리자 권한이 필요하다(SCM 서비스 등록·「서비스로 로그온」 권한). 관리자가 아닌 SSH 세션이면 바꾸기 전에 멈추고 할 일을 알린다.
- 질문과 숨김 암호 입력이 있어 대화형 콘솔이 필요하다. `ssh`에 `-t`가 없으면 그렇게 알리고 끝낸다(종료 코드 2).
- 이름이 `aaa-setup`으로 시작하는 실행 파일은 인자 없이 또는 옵션만 줘도(`aaa-setup.exe --uninstall`) 이 설치기다.
  설치한 뒤에는 `aaa setup …`으로 같은 일을 한다.

미리 해 두면 빠른 것: host 웹 › 설정 › 머신 › 「등록 창 열기」. 안 했어도 설치기가 그 자리에서 안내하고 Enter로 이어 간다.

## 질문

바꾸기 전에 질문과 점검을 모두 끝낸다. `[ ]` 안은 기본값이라 Enter만 누르면 된다.

| 질문 | 기본값 | 점검 |
| --- | --- | --- |
| host 주소 | 이 인스턴스·이 PC의 다른 aaa 설정의 host | TCP로 붙어 본다. 포트가 없으면 7430을 붙인다. 안 되면 다시 묻는다 |
| 이 PC 이름 | 설정의 이름 → 다른 aaa 설정의 이름 → 소문자 DNS 이름 | 영문·숫자·`.`·`-`·`_` 1~64자 |
| 작업 폴더 | `%USERPROFILE%\aaa-work` | 전체 경로 |
| Windows 암호 | 없음(숨김 입력) | `LogonUser`로 바로 확인, 3번까지 다시 묻는다 |

암호는 서비스를 이 사용자 계정으로 띄우는 데 쓴다. 그래야 그 사람의 codex·claude 로그인과 사용자 PATH를 그대로 쓴다.
PIN이 아니라 로그인 암호다(Microsoft 계정이면 그 계정 암호). 메모리에서만 쓰고 지우며 Windows 서비스 설정 밖에는
파일·로그·명령줄에 남기지 않는다. 마지막으로 바꿀 것 전부(실행 파일·설정·서비스·권한·PATH)를 보여 주고 「진행할까요?」를 묻는다.

## 단계

1. **host 등록.** 확인 코드 6자리를 보이고 승인을 기다린다(최대 15분). host 웹 › 설정 › 머신 › 「승인을 기다리는 머신」의
   「승인…」에 코드를 넣는다(host 터미널은 `aaa machine approve <이름> --code <코드>`). 그 자리에서 고치는 것:
   등록 창 닫힘(열고 Enter), 이름 충돌(host에서 옛 등록을 폐기하고 Enter, 또는 새 이름), 승인 거절(다시 요청).
   이미 같은 host에 등록돼 있으면 건너뛴다.
2. **실행 파일.** 기본 인스턴스는 `%USERPROFILE%\.local\bin\aaa.exe`. 다른 파일이 있으면 `.aaa-old\`에 보관한다.
3. **설정.** `agent.toml`의 `[agent]` host·name·work_root만 바꾸고 다른 값·주석은 둔다. 기본 인스턴스는
   실행 파일 폴더가 사용자 PATH에 없을 때만 더한다.
4. **서비스.** 「서비스로 로그온」 권한이 없으면 넣고, 서비스 로그온(`LOGON32_LOGON_SERVICE`)을 미리 해 본다.
   그 뒤 서비스를 등록한다. 이름은 `aaa-agent`, 인스턴스는 `aaa-<이름>-agent`다. 자동 시작(지연)이고, 멈추면 5·15·60초 뒤 다시 띄운다.
5. **시작과 연결 확인.** 서비스가 host에 붙을 때까지 기다린다(최대 90초). 안 붙으면 서비스 로그 끝을 보이고,
   서비스를 둘지(기본) 모두 되돌릴지 묻는다.

## 실패·취소

Ctrl+C(질문·승인 대기 어디서든)나 실패 때는 **이번 실행이 바꾼 것만** 거꾸로 되돌린다. 그리고 `되돌림:`·`남음:` 줄로 보인다.
바꾸기 전이면 「아무것도 바꾸지 않았습니다」로 끝낸다. 취소는 종료 코드 130이다.

- 되돌리는 것: 서비스, 넣은 권한, PATH 원문, 실행 파일(보관본 복원), 설정 파일 원문, 새로 만든 빈 폴더.
  승인 전이면 새 장치 키도 지운다.
- 남기는 것: 승인까지 끝난 host 등록과 장치 키. 다시 실행하면 승인 없이 이어 쓴다. 지우려면 host 웹에서 폐기한다.

## 다시 실행·상태·제거

| 명령 | 하는 일 |
| --- | --- |
| `aaa-setup.exe`(새 판) | 이미 설치돼 있으면 서비스를 멈추고 실행 파일만 바꾼 뒤 다시 띄워 연결을 확인한다. 암호·승인은 다시 묻지 않는다 |
| `aaa setup --status` | 서비스 상태·계정·host 연결·설치 판·로그 위치 |
| `aaa setup --uninstall` | 서비스를 지우고 이 설치기가 넣은 권한·PATH만 뺀다. 같은 계정의 다른 서비스가 있으면 권한은 남긴다 |
| `aaa setup --uninstall --purge` | 위와 함께 이 인스턴스의 설정·상태 폴더를 지운다 |

이 설치기가 만들지 않은 같은 이름의 서비스(예전 `aaa install` 등)는 건드리지 않고 멈춘다. 다른 aaa 서비스가 있으면 계획 화면에 참고로 보인다.
무엇을 바꿨는지는 상태 폴더의 `setup.json`에 남고 제거는 그 기록만 따른다. 실행 파일은 다른 aaa 명령이 쓰므로 남긴다.

## 서비스 모양

- 명령: `"<aaa.exe>" [--instance <이름>] setup-service`(SCM만 실행한다. 콘솔에서 직접 실행하면 거절한다).
- 계정: 설치한 사람의 계정(`.\<사용자>`), 세션 0. 서비스 token으로 `CreateEnvironmentBlock`을 불러 사용자 환경(레지스트리 PATH 포함)을 만든다.
- 로그: 상태 폴더의 `agent-service.log`(8MB를 넘으면 시작할 때 `.1`로 돌린다). 연결 상태: `setup-service.json`(PID·연결 여부).

## 참고한 오픈소스

코드는 가져오지 않았고 아래의 모양과 순서만 따랐다.

| 프로젝트 | 라이선스 | 따른 것 |
| --- | --- | --- |
| [rustup](https://github.com/rust-lang/rustup) `rustup-init` | MIT OR Apache-2.0 | 바꿀 것 전부를 먼저 보이고 Enter로 진행, 끝의 Enter 대기, PATH 처리 |
| [dist](https://github.com/axodotdev/cargo-dist) `installer.ps1` | MIT OR Apache-2.0 | HKCU PATH(ExpandString)·설정 변경 알림, 설치 기록 |
| [windows-service-rs](https://github.com/mullvad/windows-service-rs) | MIT OR Apache-2.0 | SCM 생성·실패 동작·지움 표시 대기, 서비스 entry 수명 |
| [Tailscale](https://github.com/tailscale/tailscale) `up`·`install_windows.go` | BSD-3-Clause | 코드와 승인 안내 뒤 기다렸다 성공 한 줄, 실패 시 다시 띄우기 |
| [SyncthingWindowsSetup](https://github.com/Bill-Stewart/SyncthingWindowsSetup) | MPL-2.0 | 넣은 「서비스로 로그온」 권한만 제거 때 회수 |

## 남은 것

- host 배포(rollout)의 Windows 재시작은 아직 launchd만 안다. 이 서비스는 설치기 재실행이나 `sc stop/start`로 바꾼다.
- 탐색기 더블클릭(UAC) 경로와 도메인·Microsoft 계정은 실기로 확인하지 않았다(시험은 [testing](testing.md)의 「Windows 설치기」).
