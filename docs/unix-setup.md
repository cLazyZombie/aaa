# macOS·Linux agent 설치

`aaa setup`은 host 주소·머신 이름·작업 폴더를 확인하고, 등록 → 실행 파일 → 설정 → 서비스 → host 연결을 한 실행에서 진행한다. Windows 암호 질문은 Unix에서 나오지 않는다. host 웹에서 등록 창을 열고 설치기에 표시된 확인 코드로 승인한다.

사용자가 일반 터미널에서 실행한다. SSH는 `ssh -t`로 터미널을 붙인다. aaa pane 안이거나 표준 입력이 터미널이 아니면 설치·제거를 거부한다. `--status`는 읽기 전용이라 터미널 없이도 쓸 수 있다. `-y`는 질문 기본값을 사용하며 첫 설치의 최종 계획 확인과 host 등록 승인을 생략하지 않는다.

## macOS

GUI 로그인한 계정에서 실행한다. 기존 `~/Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher`를 보존해 사용하며 `--launcher <절대 경로>`로 다른 launcher를 고를 수 있다. 새 Mac에는 같은 release의 `aaa-launcher-macos-aarch64.json` 또는 `aaa-launcher-macos-x86_64.json`을 설치기 옆에 둔다. 실행 파일 SHA·판·CPU·고정 파일 목록이 맞는 package만 새 app으로 풀고, 기존 app은 교체하지 않는다. package가 없으면 변경 전에 준비 방법을 안내한다.

```sh
./aaa setup --host host.example.ts.net --name mac-a
```

서비스는 사용자 LaunchAgent → 불변 launcher → agent 순서다. 로그인 뒤 자동 실행하며 잠긴 키체인·개발자 도구·전체 디스크 접근 권한은 사용자가 직접 설정한다. 설치기는 시스템 권한·키체인·기존 launcher 서명을 바꾸지 않는다. 같은 instance의 기존 `aaa service` LaunchAgent가 있으면 중복 감독자를 만들지 않고 별도 instance를 안내한다. `setup`의 label은 기본 `aaa-agent`, 인스턴스는 `aaa-<이름>-agent`다.

## Linux

systemd가 있는 glibc 환경에서 일반 사용자로 실행한다. 기본은 사용자 서비스다.

```sh
./aaa setup --host host.example.ts.net --name linux-a
```

사용자 systemd manager가 실행 중이어야 한다. `systemctl --user`가 연결되지 않는 SSH 세션이면 로그인 환경부터 확인한다. 설치기는 linger를 켜지 않는다. linger가 없는 계정은 로그아웃 뒤 서비스가 계속 실행된다고 보장하지 않는다.

부팅 때 실행할 서비스는 명시적으로 고른다. `sudo aaa setup`으로 전체 설치기를 root로 실행하지 않는다.

```sh
./aaa setup --service systemd-system --host host.example.ts.net --name linux-a
```

설정·장치 키·실행 파일·서비스 로그는 일반 사용자 소유다. 시스템 unit의 `User=`도 그 계정이며, root 권한은 unit 등록·시작·제거에만 `sudo`로 쓴다. 필요한 암호는 sudo가 터미널에서 직접 받는다. aaa가 암호를 읽거나 기록하지 않는다.

사용자 unit은 `$XDG_CONFIG_HOME/systemd/user`(없으면 `~/.config/systemd/user`), 시스템 unit은 `/etc/systemd/system`에 둔다. 서비스에는 설치 당시의 PATH·언어 환경과 명시적 HOME·설정·상태 경로만 전달한다. 셸로 명령을 해석하지 않으며 셸 rc 파일과 사용자 PATH 설정은 고치지 않는다.

## 상태·갱신·제거

기본 실행 파일은 `~/.local/bin/aaa`다. 별도 instance나 설정·상태 경로를 지정하면 `<상태 폴더>-bin/aaa`에 격리한다. 같은 instance로 설치기를 다시 실행하면 등록을 유지하고 실행 파일 갱신·서비스 재시작을 안내한다. Windows와 같이 host·이름 변경은 먼저 서비스를 제거한 뒤 설치한다.

```sh
~/.local/bin/aaa setup --status
~/.local/bin/aaa setup --uninstall
```

`--instance`·`--config-dir`·`--state-dir`를 지정해 설치했으면 조회·제거에도 같은 값을 쓴다. 서비스 종류는 설치 기록에서 읽으므로 보통 다시 적지 않아도 된다. 제거는 자기 서비스 파일과 완료 기록 `setup.json`만 지운다. 서비스 환경 기록 `setup-unix.json`·설정·host 등록·장치 키·실행 파일·launcher는 남긴다. 시스템 서비스를 다시 설치할 때도 `--service systemd-system`을 지정한다. `--purge`는 해당 설정·상태 폴더까지 지운다. 실행 파일과 launcher는 보존한다.

서비스 파일 내용·경로·계정·설치 기록이 다르거나 같은 이름의 다른 서비스가 있으면 변경을 거부한다. 실패·취소하면 이번에 바꾼 파일·서비스를 되돌리고 남은 항목과 하위 오류를 표시한다. host에서 이미 승인한 등록은 유지한다. 연결 확인을 끝내지 못했을 때 서비스를 남길지는 같은 실행에서 묻는다.

서비스 로그는 상태 폴더의 `agent-service.log`, 설치 기록은 `setup.json`과 `setup-unix.json`이다. `setup-service.json`의 PID가 실제 감독자의 PID와 같을 때만 연결 상태를 채택한다. 공개 보고에는 실제 경로·주소·계정·지문을 가린다.

## 참고와 범위

[rustup-init](https://github.com/rust-lang/rustup/blob/master/src/cli/self_update.rs)(MIT 또는 Apache-2.0)의 자기 실행 파일 배치와 OS별 경계, [cargo-dist installer](https://github.com/axodotdev/cargo-dist/blob/main/cargo-dist/templates/installer/installer.sh.j2)(해당 템플릿 MIT)의 설치 위치와 PATH 변경 구분, [Tailscale installer](https://github.com/tailscale/tailscale/blob/main/scripts/installer.sh)(BSD-3-Clause)의 OS 사전 점검을 참고했다. 외부 코드·의존성을 가져오지 않는다. systemd 명령 인자와 실행 환경은 [systemd.service](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html)·[systemd.exec](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html)의 규칙을 따른다.

이 변경은 `setup`의 Unix 실행 경로다. release 게시·macOS/Linux 다운로드 bootstrap·기존 `aaa install`의 Linux I4·운영 서비스 이관을 완료했다는 뜻은 아니다.
