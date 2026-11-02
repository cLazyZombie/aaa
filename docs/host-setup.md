# host 설치(macOS)

host는 웹과 프로젝트 상태를 맡는 Mac 한 대다. 다른 머신의 agent는 host에 연결하고, 브라우저와 휴대폰은 host 웹에만 붙는다.
이 안내는 지금 운영 절차([운영 점검표](install-runbook.md) 1~5절)에서 새 host에 필요한 단계만 뽑았다.
머신 이름·주소는 예시(`mac-a`, `example.ts.net`)다. ✋는 사용자가 직접 하는 단계다.

## 준비

- GUI로 로그인한 macOS 사용자 계정. 서비스는 그 사용자의 LaunchAgent로 돈다.
- [Tailscale](https://tailscale.com) 로그인. 다른 머신의 연결과 휴대폰의 웹 접속이 tailnet으로 들어온다.
- 쓸 agent CLI(Claude Code·Codex)의 설치와 로그인.
- launcher를 빌드할 Xcode Command Line Tools(`cc`)와 이 저장소의 clone.

## 1. 실행 파일

[GitHub releases](https://github.com/cLazyZombie/aaa/releases/latest)에서 `aaa-macos-aarch64`(Apple Silicon) 또는 `aaa-macos-x86_64`(Intel)와
같은 release의 `SHA256SUMS`를 받아 `shasum -a 256 <파일>` 값이 맞는지 확인한다. 그 파일을 고정 경로에 둔다.

```sh
install -m 755 aaa-macos-aarch64 ~/.local/bin/aaa
~/.local/bin/aaa --version
```

명령은 PATH에 기대지 않고 전체 경로 `~/.local/bin/aaa`로 친다. 소스로 빌드하려면 [CONTRIBUTING](../CONTRIBUTING.md)을 따른다.

## 2. launcher 등록 ✋(한 번)

서비스는 한 번 만들어 두는 작은 launcher(`aaa-launcher.app`)가 띄운다. 권한은 launcher에 주므로 `aaa`를 바꿔도 다시 등록하지 않는다.

```sh
bash p0/launcher/install.sh      # 저장소 루트에서. 이미 있으면 그대로 둔다
```

**시스템 설정 → 개인정보 보호 및 보안**의 **개발자 도구**와 **전체 디스크 접근 권한**에 `~/Applications/aaa-launcher.app`을 넣고 켠다.
자세한 내용은 [launcher 절차](p0-runbook.md#사용자-작업-launcher-등록-한-번-mac-b-화면에서-결정-15)에 있다.

## 3. 설정

`~/.config/aaa/host.toml`:

```toml
schema = 1
[host]
agent_listen = "0.0.0.0:7430"   # 다른 머신의 agent가 연결한다(tailnet 안)
[web]
listen = "127.0.0.1:7431"       # 웹은 loopback에만 열고 공개는 5단계에서 한다
[local_agent]
mode = "launchd"                # host Mac의 agent도 서비스로 띄운다
```

`~/.config/aaa/agent.toml`:

```toml
schema = 1
[agent]
host = "local"
work_root = ""                  # 비우면 ~/aaa-work
```

모든 설정 항목은 `aaa config example host`·`aaa config example agent`로 본다. `~/.local/bin/aaa config check`가 오류 없이 끝나야 한다.

## 4. 서비스 ✋

```sh
~/.local/bin/aaa service install --role host --aaa ~/.local/bin/aaa
~/.local/bin/aaa service install --role agent --aaa ~/.local/bin/aaa
~/.local/bin/aaa status
```

host를 agent보다 먼저 설치한다. macOS가 「백그라운드 항목이 추가됨」을 알리면 허용한다. `aaa status`에 로컬 머신이 연결로 보이면 된다.
로그는 `~/Library/Logs/aaa/<role>.log`다. Codex를 쓰면 `~/.local/bin/aaa integration install codex`로 hook을 넣고, 새 Codex 세션의 Hooks 화면에서 신뢰한다.

## 5. 웹과 휴대폰 ✋

```sh
~/.local/bin/aaa web publish     # tailscale serve로 HTTPS 공개
~/.local/bin/aaa web pair        # 1회용 짝짓기 코드(보기 권한만 줄 때는 --reader)
```

표시된 HTTPS 주소를 브라우저에서 열고 코드를 넣는다. 휴대폰도 같은 tailnet에 로그인한다. iPhone은 Safari의 **공유 → 홈 화면에 추가**로 연 앱에서
다시 짝짓고 알림을 켠다. 공개 주소로 짝지은 기기는 짝지은 뒤 10분 안에 **passkey 등록**을 해야 머신 승인 같은 동작을 할 수 있다.

## 다음

- 다른 머신 연결: [머신 설치](install.md)
- 첫 프로젝트: [README 빠른 시작](../README.md#4-첫-프로젝트)
- 갱신 ✋: `~/.local/bin/aaa update --dry-run`으로 받을 판을 검사한 뒤 `aaa update`로 바꾸고 `aaa service restart --role host` → `--role agent` 순으로 다시 띄운다. 되돌리기는 `aaa update rollback`이다([운영 점검표 9절](install-runbook.md#9-업그레이드)).
