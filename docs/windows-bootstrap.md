# Windows 한 줄 설치와 사용자 설치 시험

Windows x86_64, Windows PowerShell 5.1 또는 PowerShell 7의 자기 계정 외부 콘솔에서 시작한다.
이 입구가 포함된 새 release 게시 후에는 다음 한 줄을 쓴다. 0.1.4에는 `install.ps1`이 없다.

```powershell
irm https://github.com/cLazyZombie/aaa/releases/latest/download/install.ps1 | iex
```

script에는 게시 당시 정확한 판과 Windows exe SHA-256이 들어 있다. 이후 manifest/exe는 같은 고정 tag URL에서
받아 내장 SHA·PE x86_64·실제 `--version`을 대조한다. 설정·암호·승인은 같은 콘솔의 Rust 설치기가 묻는다.
script는 shell로 돌아오고 `$LASTEXITCODE`에 결과를 남기며 실행 정책·PATH를 영구 변경하지 않는다.
SCM·계정·UAC/LSA 변경은 검토한 설치 계획과 별도 명시 승인에 따른다. ARM64/32비트는 지원하지 않는다.

기존 `aaa.exe`와 같은 SHA면 재사용한다. 다른 판이면 구판을 덮지 않고
`%USERPROFILE%\.local\share\aaa\installers\<SHA 앞 16자리>\aaa.exe`에 새 executor를 검증해 놓는다.
이 경우 설치 계획의 bin target도 그 위치를 명시하여 구판을 쓰는 다른 instance를 보존한다.
설정·작업·hook·journal·identity를 삭제하고 시작하지 않는다. 처음 `aaa 이름`에는 이어갈 기존 이름을 고른다.
중단 기록이 있으면 같은 실행 파일은 원래 ID/계획의 재검토·새 승인을 통해 resume할 수 있다.
다른 실행 파일에 결속된 승인 또는 충돌 기록은 새 repair 계획을 검토하며, 다른 instance의 공유 hook은 바꾸지 않는다.

host 등록 창이 닫혔다면 host 웹 또는 host의 `aaa machine enroll --open 10m`으로 먼저 연다.
agent SAS를 확인한 뒤 같은 한 줄 입구에서 같은 이름/중단 ID를 선택한다. Controller passkey 등록과 agent 승인은 별개다.
새 repair는 원래 기록을 지우지 않는다. 외부 편집·공유 소유권·정책 문제는 설명과 함께 멈추며 권한으로 우회하지 않는다.

아래 저장 파일 방식은 판을 직접 고정하는 관리용 대안이다. 일반 사용자에게 여러 명령 입력을 요구하지 않는다.

## 준비물

- 관리자가 지정한 **이 기능이 들어간 새 release**의 정확한 판과 `aaa-install-windows.ps1`의 SHA-256.
  아직 게시하지 않은 파일이나 예전 판을 이 구현으로 간주하지 않는다. release 게시와 사용자 PC 인수는 별도 단계다.
- 자기 계정의 외부 Windows Terminal/PowerShell. aaa pane·SSH·자동화·`--yes`는 사용자 승인을 대신하지 않는다.
- 실행 계정의 실제 암호(Windows Hello PIN과 다름), SCM 생성 권한이 필요한 경우 사용자가 확인한 UAC 경로.
  다른 관리자 계정으로 전환해 SID가 바뀌면 같은 승인으로 진행하지 않는다.
- 기존 서비스·실행 파일의 판/SHA/설정/데이터 백업. 비어 있는 `install-check` instance와 별도 작업 폴더,
  쓰지 않는 포트, 연결할 host의 실제 주소/등록 창. 공개 예시의 주소는 실제 연결 대상이 아니다.
- 새 SCM 등록은 승인 범위에서 계정의 `SeServiceLogonRight`를 추가할 수 있다. 시험 전 로컬 보안 정책의
  「서비스로 로그온」 사용자/그룹과 조직 정책 상태를 기록한다. 정책을 확인할 수 없으면 새 서비스 적용 전에 멈춘다.
- 처음은 PATH/hook/Serve/프로젝트 변경을 끈 좁은 계획으로 시험한다. 실제 운영 설치와 대상이 겹치면 중단한다.

## 받기와 검증

관리자가 아래 두 값을 채워 준 뒤 실행한다. 판은 `x.y.z`, SHA는 해당 release script의 64자리 hex다.
이 관리용 방식은 받은 파일의 본문과 SHA를 직접 검토한다. 실행 정책을 전역으로 낮추지 않는다.

```powershell
$Release = '<새 검증 release의 x.y.z>'
$BootstrapSha256 = '<관리자가 확인한 script SHA-256>'
if ($Release -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' -or
    $BootstrapSha256 -notmatch '^[a-fA-F0-9]{64}$') { throw '먼저 release 판과 SHA를 받으세요' }
$Download = Join-Path $env:USERPROFILE ('Downloads\aaa-bootstrap-' + $Release)
New-Item -ItemType Directory -Path $Download -ErrorAction Stop | Out-Null
$Bootstrap = Join-Path $Download 'aaa-install-windows.ps1'
Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/cLazyZombie/aaa/releases/download/v$Release/aaa-install-windows.ps1" -OutFile $Bootstrap
if ((Get-FileHash -LiteralPath $Bootstrap -Algorithm SHA256).Hash -ine $BootstrapSha256) { throw 'script SHA 불일치' }
Get-Content -LiteralPath $Bootstrap
# 검토/해시 확인을 마친 이 파일의 인터넷 차단만 해제한다. 시스템 실행 정책 변경이 아니다.
Unblock-File -LiteralPath $Bootstrap
& $Bootstrap -Version $Release -InstallArgs @('--help')
if ($LASTEXITCODE -ne 0) { throw 'bootstrap 실패: 위 고정 오류 코드를 기록하세요' }
$Aaa = Join-Path $env:USERPROFILE '.local\bin\aaa.exe'
& $Aaa --version
Get-FileHash -LiteralPath $Aaa -Algorithm SHA256
```

bootstrap은 같은 판 manifest의 고정 Windows URL·SHA·PE x86_64·실제 `--version`을 모두 확인한다.
저장 파일의 `-Version`만 사용한 관리용 호출은 동일 bytes의 mtime을 유지하고 다른 파일을 덮지 않고 실패한다. 기존 설치의 업그레이드는
기존 업그레이드 절차를 쓴다. 다운로드/hash/경로 오류는 `bootstrap_*`, exit 2이며 설치기를 시작하지 않는다.
배치 후 CLI가 실패/취소해도 최초 실행 파일은 남는다. 서비스나 설치 완료를 뜻하지 않는다.

## 명령과 확인

아래는 기존 host에 agent를 더하는 좁은 예다. 먼저 자기 host 주소로 바꾼다. host 등록은 host에서
실제 등록 창을 열고 SAS를 양쪽에서 확인한다. 표시 ACK만으로 연결/등록 완료라 하지 않는다.
작업 폴더·서비스 계정·harness/login은 질문에서 검토한다. 암호/승인서/등록 코드를 argv나 일반 파일에 넣지 않는다.
`InstallArgs`는 PowerShell의 문자열 배열이다. `powershell -File ...`에 인자 배열을 문자열로 합치지 않는다.

```powershell
$HostAddress = '<사용할 host 주소:포트>'
if ($HostAddress.StartsWith('<')) { throw '실제 host 주소를 먼저 정하세요' }
$Work = Join-Path $env:USERPROFILE 'aaa-install-check-work'
$Choices = @('--instance', 'install-check', '--role', 'agent', '--name', 'windows-a',
    '--host', $HostAddress, '--network', 'direct', '--enroll', 'wait', '--work-root', $Work, '--work-storage', 'internal',
    '--harness', 'none', '--login', 'defer', '--hooks', 'skip', '--path', 'preserve',
    '--service', 'windows-scm', '--startup', 'auto', '--start', 'now', '--prompt', 'plain')
& $Bootstrap -Version $Release -InstallArgs ($Choices + @('--dry-run'))
# 대상·변경·필요 권한을 검토한 뒤 같은 외부 터미널에서 실행한다.
& $Bootstrap -Version $Release -InstallArgs $Choices
```

- dry-run 뒤 서비스/설정/작업 폴더가 만들어지지 않았는지 확인한다(bootstrap의 exe 배치는 별개).
- 거부/Ctrl+C/EOF 때 설치 변경 0, 승인 뒤 installation_id·exit code를 기록한다.
- `needs_user`면 표시한 조작만 하고 같은 ID로 재개한다. 다른 선택은 새 계획/승인이다.
- 등록/running/application_ready, 실제 SCM 계정/프로세스, host 연결을 따로 확인한다.
  `--login defer`/`--harness none`은 인증/hook의 실제 성공을 뜻하지 않는다.
- 같은 선택의 두 번째 실행에서 unchanged, 설정/작업/기존 instance와 exe mtime 보존을 확인한다.

```powershell
$Id = '<출력된 installation_id>'
& $Aaa --instance install-check install --status $Id --json
& $Aaa --instance install-check install --resume $Id --prompt plain
Get-CimInstance Win32_Service -Filter "Name='aaa-install-check-agent'" |
    Select-Object Name,State,StartName,PathName,ProcessId,StartMode
```

실기에서만 판정할 항목: 실제 SCM logon SID/Session 0, 로그인 전 재부팅·디스크 미연결,
SeServiceLogonRight/SeDenyServiceLogonRight·조직 GPO, UAC 거부, Windows Hello와 실제 암호,
서비스 문맥의 ConPTY/harness 인증 갱신. CI PE/argv/파일 검증이나 cross는 이를 대신하지 않는다.

## 되돌리기

공개 `aaa uninstall`은 아직 제공되지 않는다. 없는 옵션을 안내하거나 journal을 편집하지 않는다.
이 좁은 시험은 PATH/hook/Serve 변경을 끄므로, 아래 **해당 SCM 서비스만** 제거해 자동 실행을 되돌리고
설정·identity/pin·DB·repo/worktree·작업 폴더·내구 journal을 보존한다. 이는 최종 I7의 공유 참조/승인 기반
제거 기능을 대신하는 완료 판정이 아니다. 시험 전부터 있던 서비스면 아래 명령을 실행하지 않는다.

1. 설치 출력과 위 `Win32_Service` 결과의 정확한 이름·PathName·계정이 새 시험 instance 소유와 같은지 직접 확인한다.
   다르면 중단한다. 다른 instance/서비스의 이름을 패턴으로 골라 제거하지 않는다.
2. 같은 사용자 권한 또는 사용자가 확인한 필요한 UAC 창에서 다음을 실행한다.

```powershell
# 위의 소유 대조를 마친 새 시험 서비스에만 실행한다.
Stop-Service -Name 'aaa-install-check-agent' -ErrorAction Stop
(Get-Service -Name 'aaa-install-check-agent').WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
sc.exe delete aaa-install-check-agent
if ($LASTEXITCODE -ne 0) { throw '서비스 제거 실패: 오류를 보존하세요' }
```

이미 Stopped/미등록이면 상태를 확인하고 해당 명령만 생략한다. 시한 초과/marked for delete면 실패를 기록하고
임의 kill이나 강제 삭제로 덮지 않는다. 서비스 핸들을 연 관리 창을 닫은 뒤 상태를 확인한다.
서비스 삭제는 LSA 권한을 되돌리지 않는다. 시험에서 새로 추가한 자기 계정 권한이 있고 다른 서비스가
그 권한을 쓰지 않는 것을 확인한 경우에만, 사용자가 로컬 보안 정책의 「서비스로 로그온」에서 시험 전 목록으로
복원한다. 기존/그룹/공유 권한은 제거하지 않고 조직 GPO는 관리자 절차를 따른다. 계정·암호 자체는 바꾸지 않는다.
다른 서비스와 이전 instance의 ready, 사용자 데이터 보존을 비교한다. bootstrap은 PATH를 바꾸지 않았으므로
PATH 복원은 없다. `.local\bin\aaa.exe`는 다른 instance도 공유할 수 있어 기본적으로 남긴다.
처음에 없었고 이후 어떤 서비스/instance도 사용하지 않는 것을 직접 확인한 경우에만 기록한 SHA와 다시 대조해
그 실행 파일 하나를 사용자 스스로 삭제한다. 폴더 전체·사용자 데이터·journal·공유 참조는 삭제하지 않는다.
기존 운영 설치 복구는 백업한 판/검증 SHA와 원래 서비스 설정으로 관리자의 배포 절차를 따른다.

## 문제 때 남길 로그

- release 판·script/exe SHA, PowerShell 판·Windows/CPU, 실행한 비밀 없는 명령 형태, exit code,
  `bootstrap_*`/`needs_user` 코드, installation_id, 어디서 취소/중단했는지.
- 자기 `install --status $Id --json`의 checks·needs_user와 SCM의 위 다섯 필드, 재부팅/UAC 시점,
  다른 instance와 데이터에 영향이 있었는지. 경로/SID/주소/계정은 공유 전에 가린다.
- state의 해당 `install/jobs/<ID>/services/agent/service.log`는 자기 PC에서만 먼저 읽는다.
  전체 private context/ready/journal, 승인서·암호·SAS/pairing 코드·인증 파일은 첨부하지 않는다.
- 승인/암호 입력을 포함한 `Start-Transcript`는 하지 않는다. 준비 단계와 상태 조회 출력만 필요한 부분을 가려 남긴다.
