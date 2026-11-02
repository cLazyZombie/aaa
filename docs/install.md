# 머신 설치

host에 연결할 머신(macOS·Linux·Windows)마다 하는 설치다. host는 먼저 [host 설치](host-setup.md)대로 준비한다.
머신 이름·주소는 예시(`mac-a`, `example.ts.net`)다. ✋는 사용자가 직접 하는 단계다.

## 1. 한 줄로 시작 ✋

스크립트가 OS·CPU에 맞는 공개 release를 받고 SHA-256을 검증한 뒤 `aaa setup`을 연다.

**macOS · Linux**

```sh
curl -fsSL https://cLazyZombie.github.io/aaa/install.sh | sh
```

**Windows — 관리자 PowerShell**

```powershell
irm https://cLazyZombie.github.io/aaa/install.ps1 | iex
```

macOS·Linux는 arm64와 x86_64, Windows는 x86_64를 지원한다. 설치할 사용자 계정의 일반 터미널에서 실행하고, aaa pane 안에서는 실행하지 않는다.
SSH는 `ssh -t`로 터미널을 붙인다. macOS·Linux에서 명령 전체를 `sudo`로 실행하지 않는다.

## 2. setup 질문에 답하기 ✋

1. **host 주소·머신 이름·작업 폴더**를 확인한다. host 주소는 스크립트에 넣지 않고 setup에서 입력한다.
2. 설치기가 보여 주는 **변경 계획**을 확인한다. 실행 파일·설정·등록·서비스 처리는 setup이 맡는다.
3. host 웹의 **설정 → 머신 → 등록 창 열기**를 누르고, 설치기에 나온 확인 코드를 **승인을 기다리는 머신 → 승인**에 넣는다.
   host 터미널에서는 `aaa machine enroll --open 10m`으로 등록 창을 열 수도 있다.
4. 같은 설치 화면에서 서비스 시작과 host 연결 결과를 확인한다.

| OS | setup이 준비하는 것 | 사용자가 준비할 것 |
| --- | --- | --- |
| macOS | 사용자 LaunchAgent·새 설치의 launcher | GUI 로그인, 개발자 도구·전체 디스크 접근 권한, 잠긴 키체인 해제 |
| Linux | 기본 systemd 사용자 서비스 | glibc·systemd와 실행 중인 사용자 manager |
| Windows | 사용자 계정의 Windows 서비스 | 관리자 콘솔, Windows 로그인 암호(PIN과 다름) |

macOS 입구는 같은 release의 launcher package도 검증해 setup에 넘기고, 기존 launcher는 보존한다. Linux의 로그아웃 뒤 실행은
manager·linger 설정에 달려 있으며 setup은 linger를 켜지 않는다. 시스템 서비스는 [Linux 설치 안내](unix-setup.md#linux)를 따른다.

**Windows 로그인 암호는 설치기의 숨김 입력칸에만 입력한다.** 채팅·명령 인자·보고서에 넣지 않는다. 쓸 agent CLI의 설치와 로그인은
머신마다 따로 한다. 상세 절차는 [macOS·Linux setup](unix-setup.md)과 [Windows setup](windows-setup.md)에 있다.

## 3. 완료 확인

설치한 머신에서:

```sh
aaa --version
aaa setup --status
```

실행 파일은 macOS·Linux의 `~/.local/bin/aaa`, Windows의 `%USERPROFILE%\.local\bin\aaa.exe`다. PATH에서 찾지 못하면 전체 경로로 실행한다.

host에서:

```sh
aaa machine list
```

**서비스가 돌고 host 목록에서 새 머신이 「연결」이면 끝난 것이다.** 연결에 실패한 뒤 서비스만 남았다면 완료가 아니다. 설치기가 보인 로그 위치와 원인을 확인한다.

Codex를 쓰는 머신은 hook을 설치하고 ✋ 새 Codex 세션의 Hooks 화면에서 aaa hook을 검토·신뢰한다.

```sh
aaa integration install codex
aaa doctor
```

마지막으로 웹의 머신 정보에서 **자동 배정할 workspace**를 고른다. 새 머신은 자동 배정이 꺼진 채 시작한다. 저장소와 작업 폴더가 준비된 머신만
켠다([workspace 설정](workspace-access.md)).

## 수동 다운로드

[GitHub releases](https://github.com/cLazyZombie/aaa/releases/latest)에서 실행 파일과 같은 release의 `SHA256SUMS`를 받는다. 세 OS의 setup은 v0.1.6부터 있다.

| OS·CPU | 실행 파일 |
| --- | --- |
| macOS Apple Silicon | `aaa-macos-aarch64` |
| macOS Intel | `aaa-macos-x86_64` |
| Linux arm64 | `aaa-linux-aarch64` |
| Linux x86_64 | `aaa-linux-x86_64` |
| Windows x86_64 | `aaa-windows-x86_64.exe` |

macOS·Linux는 `shasum -a 256 <파일>` 또는 `sha256sum <파일>`, Windows는 `Get-FileHash <파일> -Algorithm SHA256`의 값을 `SHA256SUMS`의 해당 행과 비교한다.
맞는 파일을 Unix에서는 `aaa`, Windows에서는 `aaa.exe`로 이름을 바꾼다. 새 Mac은 같은 CPU의 `aaa-launcher-macos-….json`도 받아 SHA를 확인하고 실행 파일 옆에 둔다.

```sh
chmod +x aaa
./aaa setup
```

```powershell
.\aaa.exe setup
```

Windows는 release에 붙은 한 줄 입구도 쓸 수 있다.

```powershell
irm https://github.com/cLazyZombie/aaa/releases/latest/download/install.ps1 | iex
```

## 갱신·제거

갱신은 새 실행 파일로 같은 instance의 setup을 다시 실행한다. 위 한 줄 명령은 Pages에 게시된 판의 setup을 실행하며, 등록을 유지하고 서비스를 갱신·재시작해
연결을 확인한다. host·이름을 바꾸거나 예전 설치 방식에서 옮길 때는 OS별 안내를 따른다.

```sh
aaa setup --status
aaa setup --uninstall
```

제거해도 실행 파일은 남는다. `--purge`와 별도 instance의 조회·제거는 [Unix](unix-setup.md#상태갱신제거)·[Windows](windows-setup.md#다시-실행상태제거) 문서를 먼저 본다.
host의 갱신은 [host 설치](host-setup.md#다음)에 있다.
