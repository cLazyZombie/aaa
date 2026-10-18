# 운영 설치 점검표(macOS, 결정 14)

launcher 등록([P0 절차서](p0-runbook.md))과 L1 측정이 끝난 뒤 aaa를 운영 자리에 올리는 순서다. 설계는 기획서 [§10](plan.md)·[§15](plan.md#15-현재-운영에서-이전)에 있다. 이 문서는 명령과 확인만 적는다.

- 머신 이름은 일반 이름이다. mac-a·mac-b·mac-c 중 host를 둘 Mac을 **host Mac**이라 부른다. 주소 예시는 `example.ts.net`이다.
- **launchd·시스템 설정을 바꾸는 단계(✋)는 사람이 자기 터미널에서 직접 한다.** `aaa service install·uninstall·restart·stop·start`는 aaa 세션 안이거나 터미널이 아니면 거부하고 `yes` 입력을 받는다.
- **명령은 전체 경로 `~/.local/bin/aaa`로 친다.** 본문의 `aaa …`는 모두 `~/.local/bin/aaa …`를 뜻한다. launchd가 주는 PATH나 셸 PATH 순서에 기대지 않는다.
- **이름 변경(결정 18) 전 `aa` 설치에서 옮기기:** 아래 「10. aa에서 옮기기」를 먼저 한다.
- 지금 운영(herdr·herdr-projects·herdr-web-ui·WezTerm)과 그 설정·socket·port는 건드리지 않는다. aaa는 다른 port·state·작업 폴더를 쓴다.

## 0. 전제

| 확인 | 명령·기준 |
|---|---|
| launcher 등록 | `~/Applications/aaa-launcher.app`이 개발자 도구·전체 디스크 접근에 켜져 있다 |
| L1 합격 | `p0/launcher/measure.sh after` 판정 통과(median ≤10ms, p95 ≤20ms, FDA ok, 책임 프로세스 `aaa-launcher`) |
| 각 Mac 같은 aaa | 같은 commit으로 빌드한다. version 번호는 빌드마다 같아(0.1.0) `shasum -a 256 ~/.local/bin/aaa`(또는 `aaa update list`)로 본다 |

## 1. aaa를 고정 경로에 둔다(각 Mac)

```bash
cd <저장소> && cargo build --release -p aaa
install -m 755 target/release/aaa ~/.local/bin/aaa.new && mv ~/.local/bin/aaa.new ~/.local/bin/aaa   # rename으로 교체
~/.local/bin/aaa --version
```

- 업그레이드는 9번(`aaa update`)이다. launcher 등록은 다시 하지 않는다(L1이 확인하는 것).
- 운영 Mac에 빌드 부하를 주지 않으려면 같은 macOS·CPU의 다른 Mac에서 빌드해 `ssh <mac> 'cat > ~/aaa.new' < target/release/aaa`로 옮긴 뒤 그 Mac에서 `~/.local/bin/aaa update --from ~/aaa.new`로 바꾼다(처음 설치면 `install -m 755 ~/aaa.new ~/.local/bin/aaa.new && mv ~/.local/bin/aaa.new ~/.local/bin/aaa`). 옮긴 뒤 양쪽 `shasum -a 256`이 같은지 본다(`aaa update`가 앞 12자리를 보인다).
- hook 명령(`aaa hook --agent …`)은 이 경로로 실행마다 주므로 Claude·Codex 사용자 설정은 고치지 않는다.

## 2. 설정

host Mac `~/.config/aaa/host.toml`(예시: `aaa config example host`)

```toml
schema = 1
[host]
agent_listen = "0.0.0.0:7430"      # 다른 Mac의 agent가 붙는다(tailnet 안)
[web]
listen = "127.0.0.1:7431"          # herdr-web-ui(7317)과 겹치지 않는 port
[local_agent]
mode = "launchd"                   # 운영: host가 agent를 띄우지 않는다
[projects]
root = ""                          # 기본 ~/.aaa/projects. herdr-projects 폴더와 다른 곳
```

- thread가 쓸 profile·yolo·신뢰 창·routine 명령은 사람만 바꾸는 safety 설정이다. 파일을 고치지 말고 aaa 밖 터미널에서 명령으로 바꾼다(herdr-projects와 같다, plan §8.170):
  - `aaa safety show --global`·`aaa safety show <프로젝트>`: 지금 값과 출처
  - `aaa profile allow threads claude codex [--project <프로젝트>]`: thread가 쓸 수 있는 profile(없으면 모두)
  - `aaa profile default threads claude`: `thread start`에 profile이 없을 때(프로젝트 `thread_profile`이 먼저)
  - `aaa safety yolo <프로젝트|--global> on|off|default`: 묻지 않고 thread를 시작하고 claude·codex를 권한 건너뛰기 인자로 띄운다(y로 확인)
  - `aaa safety set <프로젝트|--global> routine_commands on`: routine 명령 실행(명령마다 `aaa routine approve`도 필요)
  - 옛 `[projects] allow_profiles`·`routine_commands`는 그대로 읽는다(safety 표에 값이 없을 때).

- 알림(선택): 봇 token이 든 주소는 `host.local.toml`의 `[notify]`에 둔다(커밋·공유하지 않는다).

각 Mac `~/.config/aaa/agent.toml`(예시: `aaa config example agent`)

```toml
schema = 1
[agent]
host = "local"                     # host Mac. 다른 Mac은 "mac-b.example.ts.net:7430"
work_root = ""                     # 기본 ~/aaa-work. herdr worktree 접두사와 다른 곳
[health]
min_free_gb = 20
[health.toolchain]
cargo = ["cargo", "--version"]
```

- profile: claude·codex는 정의하지 않아도 같은 이름의 기본 profile(인자 없음)이다. model·effort·인자는 **host Mac 터미널**에서 한 번 더한다(host.toml `[profiles.<이름>]`, 참고와 같은 `agent`·`model`·`effort`·`description`·`args`). host·agent를 다시 띄우지 않아도 다음 thread부터 쓴다.
  ```bash
  ~/.local/bin/aaa profile add opus --agent claude --model opus --effort high --description "깊게 볼 일"
  ~/.local/bin/aaa profile list --machine mac-a     # host 정본 + 그 Mac에 설치·로그인한 기본 profile
  ```
  권한 건너뛰기는 profile `--arg` 대신 `aaa safety yolo`(host Mac)로 켠다.
- 머신마다 CLI 경로가 다르면 그 Mac의 agent.toml에만 둔다:
  ```toml
  [harnesses.codex]
  command = ["/opt/tools/codex"]
  env = { CODEX_SSH_SKIP_APP_SERVER_BOOT = "true" }
  ```
  model·effort·args는 agent에서 덮어쓰지 않는다. 실행 파일 없음·실행 권한 없음은 이유와 함께 거절한다.
- 옛 agent.toml `[profiles]`는 읽지만 「host로 옮기라」 경고가 난다. 새 host는 그 정의를 쓰지 않는다.
  agent가 `host_profiles.v1`을 지원하지 않으면 연결은 유지하고 새 시작·새 대화 restart만 갱신 안내와 함께 거부한다.
- `aaa profile list`·`resolve`는 연결된 host 정본을 읽는다. add·edit·remove는 host 머신의 사람 터미널에서 한다.
  도는 대화의 model은 harness `/model`로 바꾼다. aaa는 실행 중 profile을 다시 적용하지 않는다.


확인: 각 Mac에서 `aaa config check`가 오류 없이 끝난다.

## 3. 감독자 설치 ✋(각 Mac, 사람이 직접)

```bash
~/.local/bin/aaa service show --role agent --aaa ~/.local/bin/aaa      # 바꾸지 않고 plist·경로·상태를 본다
~/.local/bin/aaa service install --role host --aaa ~/.local/bin/aaa    # host Mac만, agent보다 먼저
~/.local/bin/aaa service install --role agent --aaa ~/.local/bin/aaa   # 모든 Mac
```

- plist는 `~/Library/LaunchAgents/dev.aaa.<role>.plist`, 로그는 `~/Library/Logs/aaa/<role>.log`다.
- ProgramArguments는 `aaa-launcher <aaa> <role>`다. 설치하는 셸의 `PATH`·`LANG`을 plist에 적는다(launchd 기본 PATH로는 `claude`·`codex`를 못 찾는다). PATH가 바뀌면 `--replace`로 다시 설치한다.
- macOS가 「백그라운드 항목이 추가됨」 알림을 띄우면 허용한다.

| 확인 | 기준 |
|---|---|
| `aaa service show --role agent` | `launchd: 올라가 있다 · state = running` |
| `aaa doctor lineage --match "aaa agent"`(그 Mac) | agent의 책임 프로세스가 `aaa-launcher` |
| `aaa status`(host Mac) | 로컬 머신이 연결, 운영 모드 |

## 4. 다른 Mac 등록

```bash
~/.local/bin/aaa machine enroll --open 10m          # host Mac: 등록 창
~/.local/bin/aaa agent enroll mac-b.example.ts.net:7430   # 다른 Mac: 화면에 확인 코드가 나온다
~/.local/bin/aaa machine approve mac-a --code <코드>  # host Mac: 그 코드로 승인
```

확인: `aaa machine list`에 세 Mac이 「연결」이고 `이상:`이 없다(디스크·toolchain·시계).

## 5. 웹·휴대폰

```bash
tailscale serve status                # 기존 항목 기록(바꾸지 않는다)
~/.local/bin/aaa web publish                        # 겹치지 않는 port를 골라 HTTPS로 공개
~/.local/bin/aaa web pair                           # 휴대폰에서 주소를 열고 이 코드 입력(보기 권한은 --reader)
```

확인: 휴대폰에서 세 Mac의 pane 목록과 터미널이 보이고 입력된다. 질문 카드에 답할 수 있다.

- 홈 화면 앱(PWA): Android Chrome은 목록 아래 「홈 화면에 설치」, iPhone은 Safari 공유 → 「홈 화면에 추가」. iPhone 홈 화면 앱은 Safari와 cookie가 따로라 처음 열 때 다시 pairing한다.
- 알림(Web Push): 목록 아래 「알림 켜기」를 누르면 시험 알림이 온다. iPhone은 홈 화면 앱에서만 켤 수 있다(iOS 16.4 이상).
- 다른 기기 TUI: 그 기기(aaa를 둔 노트북 등)에서 `aaa attach --remote <위 publish 주소>` → host에서 `aaa web pair`로 낸 코드 입력. 확인: 같은 탭·pane이 보이고 입력되며, host `aaa web devices`에 「aaa TUI · …」가 있다. 그 기기를 `aaa web revoke`하면 열린 TUI가 「취소됐다」로 끝난다.

## 6. shadow(§15 2)

- aaa 프로젝트는 aaa projects root에 새로 만든다(`aaa project create <p>`). herdr-projects 폴더를 가리키지 않는다.
- 기존 herdr thread는 aaa로 제어하지 않는다. 같은 성격의 새 low-impact 일을 aaa thread로 하나 맡기고 상태·보고를 비교한다.
- coordinator: `aaa project open <p>`가 host 머신에서 프로젝트 폴더를 cwd로 띄운다(profile은 `--profile`, 없으면 project.toml `coordinator_profile` → host.toml `[defaults] coordinator_profile` → claude). 이미 돌면 그 pane을 알린다. AGENTS.md 안내대로 매 턴 `aaa context <p>`를 본다.

| 비교 | 기준 |
|---|---|
| 상태 | 질문·입력 대기·작업 중이 실제 화면과 맞는다(polling 없음) |
| 보고 | 보고(`## Report`·`## Next`)와 결과물이 host로 오고, 웹에서 열린다 |
| 알림 | 질문·보고 준비가 coordinator(nudge)와 webhook으로 온다 |
| 단절 | 한 Mac의 agent를 내렸다 올려도 세션이 이어지고 thread가 실패로 바뀌지 않는다 |

## 7. pilot → 전환

- 새 일부터 aaa에서 시작한다. 기존 herdr thread는 herdr에서 마무리한다(live takeover는 하지 않는다).
- active thread마다 소유 도구는 하나다.
- 전환 gate: 사용자가 정한 기간 동안 shadow·pilot에서 잃은 보고·잘못된 입력·멈춘 감독자가 없다.

## 8. 되돌리기

```bash
~/.local/bin/aaa web publish off                    # aaa가 만든 serve 항목만 끈다
~/.local/bin/aaa service uninstall --role agent     # ✋ 각 Mac(host Mac은 --role host도)
```

잠깐 멈추기만 하려면 ✋ `aaa service stop --role agent`(plist는 남아 다음 로그인 때 다시 뜬다), 다시 올리려면 ✋ `aaa service start --role agent`다. launchd가 KeepAlive로 감독하므로 프로세스를 직접 끝내면 곧 다시 뜬다.

- 세션은 session host가 소유하므로 agent를 내려도 도는 harness는 바로 죽지 않는다. 필요하면 `aaa pane kill`로 먼저 끝낸다.
- herdr 쪽 설치·설정은 처음부터 건드리지 않았으므로 되돌릴 것이 없다.
- 보고·결과물은 aaa projects root에 남는다. 기존 coordinator가 읽을 수 있는 Markdown이다.

## 9. 업그레이드

1. ✋ 새 `aaa`를 빌드하고 `~/.local/bin/aaa update --from target/release/aaa`로 바꾼다(`yes`). 같은 폴더에 두고 self-check(`--version`·`config check`)와 `[update] version` 고정 검사를 한 뒤 rename한다. 옛것은 `~/.local/bin/.aaa-old/`에 `[update] keep`개(기본 3) 남는다. 먼저 `--dry-run`으로 검사만 해 볼 수 있다.
2. ✋ `aaa service restart --role agent`(host Mac은 `--role host`도, 각각 `yes`)로 다시 띄운다(`launchctl kickstart -k`를 감싼다). 세션은 이어진다. 바꾸기만 하고 다시 띄우지 않으면 도는 host·agent는 옛 실행 파일로 계속 돈다.
3. `aaa status`·`aaa machine list`로 같은 버전·연결을 확인한다. version 번호는 아직 빌드마다 같다(0.1.0): 같은 빌드인지는 `aaa update list`의 sha256으로 본다.
4. 문제가 있으면 ✋ `aaa update rollback`(지금과 다른 가장 새 보관, 이름을 주면 그것, 목록은 `aaa update list`)으로 되돌리고 2번을 다시 한다. DB는 되돌리지 않는다(plan §11 「DB 백업」은 아직 없다).

## 10. aa에서 옮기기(결정 18)

이름이 `aa`에서 `aaa`로 바뀌었다. 옛 설치(`dev.aa.*` 감독자, `~/.config/aa`, `~/Library/Application Support/aa`)가 있는 Mac만 한다.

1. ✋ 옛 감독자를 내린다(옛 실행 파일로, 각각 `yes`).
   ```bash
   ~/.local/bin/aa service uninstall --role agent
   ~/.local/bin/aa service uninstall --role host     # host Mac만
   ```
2. `ps -axo pid,ppid,command`로 `aa host`·`aa agent`·`aa-launcher`가 남지 않았는지 본다.
3. 설정·상태 폴더를 옮긴다. 장치 키·등록·DB가 그대로 이어진다. 로그 `~/Library/Logs/aa`는 기록으로 남겨 둔다.
   ```bash
   mv ~/.config/aa ~/.config/aaa
   mv ~/Library/Application\ Support/aa ~/Library/Application\ Support/aaa
   ```
   - `aaa host`·`aaa agent`·`aaa config check`는 옛 폴더만 있고 새 폴더가 없으면 시작하지 않고 이 명령을 보인다(새 장치 키로 떠서 등록이 끊기지 않게).
   - 설정 파일 안에 `~/aa-work`·`~/.aa/projects`를 직접 적었다면 그대로 쓰인다. 기본값은 `~/aaa-work`·`~/.aaa/projects`다.
4. 새 `aaa`를 1번처럼 `~/.local/bin/aaa`에 둔다.
5. launcher: `aaa-launcher.app`은 새 앱이라 개발자 도구·전체 디스크 접근에 다시 등록해야 한다([P0 절차서](p0-runbook.md)). 둘 중 하나를 고른다.
   - A. `bash <저장소>/p0/launcher/install.sh`로 `~/Applications/aaa-launcher.app`을 만들고 ✋ 두 목록에 넣는다. 3번처럼 설치한다.
   - B. 등록을 미룬다. 옛 launcher를 그대로 쓴다: 3번 명령에 `--launcher ~/Applications/aa-launcher.app/Contents/MacOS/aa-launcher`를 붙인다.
6. ✋ 3번(감독자 설치)과 확인을 다시 한다. `aaa service install`은 옛 `dev.aa.*` plist가 남아 있으면 거부한다.
7. 웹 pairing cookie 이름이 바뀌어(`__Host-aaa`) 휴대폰은 다시 pairing한다. 옛 `~/.local/bin/aa`는 확인 뒤 지워도 된다.

## 11. Windows에서 접속만(agent 없는 PC)

agent·host·감독자 없이 `aaa attach --remote`로 붙기만 하는 Windows PC(x86_64, Tailscale 설치됨)다. 그 PC에는 aaa 서비스·예약 작업을 두지 않는다. 실행 파일 자리는 Mac의 `~/.local/bin/aaa`와 같은 `%USERPROFILE%\.local\bin\aaa.exe`이고, 명령은 그 전체 경로로 친다(PATH에 넣는 것은 사람이 사용자 환경 변수에서 직접, 선택). 설계는 기획서 §8.181·§8.186.

### 11.1 빌드(Mac에서)

Windows용 실행 파일은 Mac에서 교차 빌드한다. 사용자 수준 도구만 쓴다(sudo·시스템 설정 없음).

```bash
rustup target add x86_64-pc-windows-msvc          # 한 번
cargo binstall cargo-xwin                         # 한 번(또는 cargo install cargo-xwin)
cd <저장소>
bash scripts/ci-local.sh --only web               # host 실행 파일에 들어가는 웹 front(같은 파일 하나라 함께 빌드한다)
cargo xwin build --release --locked -p aaa --target x86_64-pc-windows-msvc
file target/x86_64-pc-windows-msvc/release/aaa.exe   # PE32+ executable (console) x86-64
shasum -a 256 target/x86_64-pc-windows-msvc/release/aaa.exe
```

- cargo-xwin은 처음 빌드할 때 Microsoft CRT·Windows SDK 파일을 사용자 캐시(`~/Library/Caches/cargo-xwin`)에 받는다. 그 파일에는 Microsoft 라이선스가 붙는다(cargo-xwin이 받으며 동의한 것으로 친다).
- cargo-xwin이 링커를 Rust에 든 `rust-lld`(`lld-link`)로, C 코드(ring·SQLite)를 `clang-cl`로 잡는다(`cargo xwin env --target x86_64-pc-windows-msvc`로 본다). 확인한 Mac은 Homebrew `llvm`의 clang을 썼다.
- C 런타임은 정적으로 넣는다(저장소 `.cargo/config.toml`의 `crt-static`, herdr와 같다): VC++ 재배포 패키지가 없는 PC에서도 `aaa.exe` 하나로 돈다. `llvm-readobj --coff-imports aaa.exe`에 `VCRUNTIME140.dll`·`api-ms-win-crt-*`가 없어야 한다.

### 11.2 그 PC로 보내기

- ssh가 열린 PC: `scp target/x86_64-pc-windows-msvc/release/aaa.exe <pc>:Downloads/aaa.exe`
- Taildrop(같은 tailnet, 같은 사용자의 기기): Mac에서 `tailscale file cp target/x86_64-pc-windows-msvc/release/aaa.exe <pc 이름>:` → Windows에서 `"C:\Program Files\Tailscale\tailscale.exe" file get --wait "%USERPROFILE%\Downloads"`(받은 파일함에서 꺼낸다. 같은 이름이 있으면 `--conflict=overwrite`). 명령 꼴은 두 쪽 `--help`로 맞췄고, 실제로 보내 보지는 않았다.
- 받은 뒤 같은 파일인지 본다: `certutil -hashfile "%USERPROFILE%\Downloads\aaa.exe" SHA256`이 Mac의 `shasum -a 256`과 같다.

### 11.3 처음 두기와 붙기

```bat
mkdir "%USERPROFILE%\.local\bin"
move "%USERPROFILE%\Downloads\aaa.exe" "%USERPROFILE%\.local\bin\aaa.exe"
"%USERPROFILE%\.local\bin\aaa.exe" --version
"%USERPROFILE%\.local\bin\aaa.exe" attach --remote https://mac-a.example.ts.net:8443
```

- 처음 붙으면 pairing 코드를 묻는다. host Mac에서 사람이 `aaa web pair`(보기만이면 `--reader`)로 낸 코드를 넣는다. host `aaa web devices`에 「aaa TUI · <그 PC 이름>」이 생긴다.
- 장치 token은 `%LOCALAPPDATA%\aaa\state\remote\https-<이름>-<포트>.token`이다. 폴더와 파일은 상속을 끊은 DACL에 그 계정(폴더는 소유자)과 SYSTEM만 있다(Unix의 0700·0600): `icacls "%LOCALAPPDATA%\aaa\state\remote"`로 본다.
- 키 설정(`[tui]`)은 그 PC의 `%LOCALAPPDATA%\aaa\config\host.toml`이다. 없으면 기본값이다(prefix `ctrl+b`).
- 터미널은 Windows Terminal을 권한다(conhost도 된다). 그 PC 터미널에서 다른 점:
  - 붙여넣기(Windows Terminal의 Ctrl+V·오른쪽 클릭)는 낱 키로 간다(Windows 콘솔 입력에는 bracketed paste가 없다): 여러 줄은 줄마다 Enter가 된다. 그림 붙여넣기(Ctrl+V)는 Windows Terminal이 Ctrl+V를 먼저 가져가 되지 않는다.
  - 한글은 터미널(IME)이 조합을 보여 주고 확정된 음절만 TUI로 온다. prefix 뒤 명령 키는 한국어 IME가 한글 상태면 영문으로 바꿨다가 되돌린다(`[tui] switch_ascii_input_source_in_prefix`).
  - AltGr 글자(유럽 배치 `@`·`{` 등)는 글자로 간다. 진짜 Ctrl+Alt 조합은 영문 글자와 Space만 남는다(Windows가 AltGr를 Ctrl+Alt로 알린다).
- 기기를 지우려면 host에서 `aaa web revoke <기기>`. 그 PC의 열린 TUI는 「취소됐다」로 끝나고, 다음에 붙으면 token을 지우고 코드를 다시 묻는다.

### 11.4 바꾸기

새 `aaa.exe`를 11.1·11.2처럼 받아 둔 뒤(받은 것과 지금 것이 다른 이름·자리여야 한다):

```bat
"%USERPROFILE%\.local\bin\aaa.exe" update --from "%USERPROFILE%\Downloads\aaa.exe" --dry-run
"%USERPROFILE%\.local\bin\aaa.exe" update --from "%USERPROFILE%\Downloads\aaa.exe"
"%USERPROFILE%\.local\bin\aaa.exe" update list
```

- self-check(`--version`·`config check`)를 한 뒤 `yes`를 받고 바꾼다. Mac과 같은 순서다: 지금 것을 `%USERPROFILE%\.local\bin\.aaa-old\`에 hard link로 보관하고(`[update] keep`개, 기본 3) 새것을 그 자리로 rename한다. 열린 TUI가 있어도 바꿀 수 있고(Windows CI 시험이 도는 exe를 바꿔 본다), 그 TUI는 끝날 때까지 옛것으로 돈다.
- 되돌리기는 `update rollback`이다. 이 PC에는 host·agent가 없으니 `aaa service restart`는 하지 않는다.
