# 운영 설치 점검표(macOS, 결정 14)

launcher 등록([P0 절차서](p0-runbook.md))과 L1 측정이 끝난 뒤 aaa를 운영 자리에 올리는 순서다. 설계는 기획서 [§10](plan.md)·[§15](plan.md#15-현재-운영에서-이전)에 있다. 이 문서는 명령과 확인만 적는다.

- 머신 이름은 일반 이름이다. mac-a·mac-b·mac-c 중 host를 둘 Mac을 **host Mac**이라 부른다. 주소 예시는 `example.ts.net`이다.
- **launchd·시스템 설정을 바꾸는 단계(✋)는 사람이 자기 터미널에서 직접 한다.** `aaa service install·uninstall·stop·start`와 일반 restart는 aaa 세션 안이거나 터미널이 아니면 거부하고 `yes` 입력을 받는다. 아래 coordinator 배포 safety를 켠 경우에만 coordinator의 restart를 허용한다.
- **명령은 전체 경로 `~/.local/bin/aaa`로 친다.** 본문의 `aaa …`는 모두 `~/.local/bin/aaa …`를 뜻한다. launchd가 주는 PATH나 셸 PATH 순서에 기대지 않는다.
- **이름 변경(결정 18) 전 `aa` 설치에서 옮기기:** 아래 「10. aa에서 옮기기」를 먼저 한다.
- 지금 운영(herdr·herdr-projects·herdr-web-ui·WezTerm)과 그 설정·socket·port는 건드리지 않는다. aaa는 다른 port·state·작업 폴더를 쓴다.

## 0. 전제

| 확인 | 명령·기준 |
|---|---|
| launcher 등록 | `~/Applications/aaa-launcher.app`이 개발자 도구·전체 디스크 접근에 켜져 있다 |
| L1 합격 | `p0/launcher/measure.sh after` 판정 통과(median ≤10ms, p95 ≤20ms, FDA ok, 책임 프로세스 `aaa-launcher`) |
| 각 Mac 같은 aaa | 같은 release 판으로 맞춘다(`aaa --version`). 같은 판의 로컬 빌드까지 구분하려면 `shasum -a 256 ~/.local/bin/aaa` 또는 `aaa update list`로 본다 |

## 1. aaa를 고정 경로에 둔다(각 Mac)

```bash
cd <저장소>/web && npm ci --no-audit --no-fund && npm run build
cd .. && cargo build --locked --release -p aaa
install -m 755 target/release/aaa ~/.local/bin/aaa.new && mv ~/.local/bin/aaa.new ~/.local/bin/aaa   # rename으로 교체
~/.local/bin/aaa --version
```

- 업그레이드는 9번(`aaa update`)이다. launcher 등록은 다시 하지 않는다(L1이 확인하는 것).
- 공개 [GitHub release](https://github.com/cLazyZombie/aaa/releases)의 자기 OS·CPU 파일을 써도 된다. 받은 파일의 SHA-256을 같은 release의 `manifest.json`·`SHA256SUMS`와 맞춘 뒤 설치한다. 기존 설치의 갱신·최초 release 전환은 9번을 따른다.
- 로컬 빌드를 옮길 때는 같은 macOS·CPU의 다른 Mac에서 빌드해 `ssh <mac> 'cat > ~/aaa.new' < target/release/aaa`로 옮긴 뒤 그 Mac에서 `~/.local/bin/aaa update --from ~/aaa.new`로 바꾼다(처음 설치면 `install -m 755 ~/aaa.new ~/.local/bin/aaa.new && mv ~/.local/bin/aaa.new ~/.local/bin/aaa`). 옮긴 뒤 양쪽 `shasum -a 256`이 같은지 본다(`aaa update`가 앞 12자리를 보인다).
- Claude hook은 실행마다 준다. Codex는 전역 hook을 설치한다(§8.201). **배포 뒤 각 Mac에서** 아래를 실행한다.
  ```bash
  ~/.local/bin/aaa integration install codex
  ~/.local/bin/aaa doctor
  ```
  빠졌거나 실행 경로가 바뀌었으면 `~/.local/bin/aaa doctor --fix`도 같은 설치를 한다.
  `CODEX_HOME`을 쓰는 머신은 Codex·설치 명령에 같은 값을 준다. 새 workspace에는 추가 설정이 없다.
  새 Codex 세션의 Hooks 화면에서 aaa hook을 검토하고 신뢰한 뒤 진행한다. 설치·doctor는 신뢰를 대신하지 않는다.
  시작 세션 ID·진행 안내·사람 답 뒤 기다림 정리·기존 사용자 notify를 새 pilot에서 확인한다(G5).
- 백업은 Codex 설정 폴더의 `<파일>.before-aaa-<시각ms>-<PID>-<순번>`이다. 설치 명령이 경로를 보인다.
  원래 없던 파일에는 백업이 없다. `aaa-codex-integration.json`은 설치한 명령 소유 기록이므로 보관한다.
  제거는 `~/.local/bin/aaa integration uninstall codex`다. 기존 hook·사용자 notify·공용 hooks 기능은 남긴다.
  완전 복원은 Codex와 설정 편집을 멈춘 뒤 해당 작업 직전 백업을 원래 파일에 임시 복사→rename으로 돌린다.
  백업 뒤의 사용자 변경도 되돌리므로 새 변경이 있으면 uninstall을 먼저 쓰고 필요한 항목만 수동 복원한다.
  실제 운영 설정 설치·복원은 구현 thread가 수행하지 않는다.

## 2. 설정

host Mac `~/.config/aaa/host.toml`(예시: `aaa config example host`)

```toml
schema = 1
[host]
agent_listen = "0.0.0.0:7430"      # 다른 Mac의 agent가 붙는다(tailnet 안)
thread_prepare_timeout_secs = 600  # 큰 checkout·LFS 준비: 기본 10분
thread_cleanup_timeout_secs = 600  # worktree 정리: 기본 10분
file_request_timeout_secs = 120    # 보고 목록·파일 조각 읽기: 기본 2분
[web]
listen = "127.0.0.1:7431"          # herdr-web-ui(7317)과 겹치지 않는 port
[local_agent]
mode = "launchd"                   # 운영: host가 agent를 띄우지 않는다
[projects]
root = ""                          # 기본 ~/.aaa/projects. herdr-projects 폴더와 다른 곳
```

- 위 요청 상한은 1~86400초이며 기존 `schema = 1` 설정과 `host.local.toml` 덮어쓰기를 따른다. `aaa config check`로 검사하고 다음 host 시작 때 적용한다. 현재 웹 설정은 UI 환경설정이며 host.toml 편집 화면은 없다.
- coordinator는 `aaa thread start <p> … --background`로 바로 thread ID와 「준비 중」을 받는다. 준비·실패 결과는 `aaa thread show <p> <id>`·`aaa events <p> --follow`로 확인한다. 기본 호출은 예전처럼 세션 시작 결과까지 기다린다.
- 연결이 끊겨도 agent가 준비를 마쳐 `<work_root>/.preparations/`에 기록한다. `thread restart`는 같은 repo·경로·branch·기준 commit의 변경 없는 자리를 이어 쓴다. 변경·다른 branch·소유권이 불명확한 자리면 보존하고 사람의 확인을 요구한다.
- 정리 중에는 `thread show`에 「정리 중」이 보인다. 정리 응답 전에 끊겼다면 같은 `thread resolve`로 이어서 한다(처음의 보존·사본 선택을 유지). 완료 후에만 완료 사건·inbox를 남긴다. agent의 상태 폴더 `thread-cleanup/`에도 결과가 남는다. 미완료 정리를 마치기 전에는 `--reopen`을 거부한다.

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
  Codex 실행·resume에는 aaa가 `--no-daemon`을 붙여 hook을 pane의 환경·프로세스 계보 안에서 실행한다.
  직접 `aaa tab create -- codex`도 같다. 이미 도는 공유 daemon이나 대화는 바꾸지 않는다.
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

## 6. shadow(§15 2)

- aaa 프로젝트는 aaa projects root에 새로 만든다(`aaa project create <p>`). herdr-projects 폴더를 가리키지 않는다.
- 기존 herdr thread는 aaa로 제어하지 않는다. 같은 성격의 새 low-impact 일을 aaa thread로 하나 맡기고 상태·보고를 비교한다.
- agent 안내 파일은 AGENTS.md만 쓴다. CLAUDE.md는 쓰지 않는다.
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

1. 각 Mac의 일반 터미널(aaa pane 밖)에서 `aaa update --version 0.1.1 --dry-run`으로 받을 판을 먼저 검사한다. 자기 OS·CPU asset을 공개 GitHub release에서 받고 SHA-256을 확인한 뒤 기존 self-check(`--version`·`config check`)와 `[update] version` 고정 검사를 한다. `--dry-run`은 누구나 가능하며 내려받기·검사까지 하고 staging을 지운다.
2. ✋ host Mac에서 `aaa update --version 0.1.1`(`yes`)로 바꾸고 `aaa service restart --role host` → `aaa service restart --role agent` 순으로 다시 띄운다(각각 `yes`). 이어 원격 Mac마다 같은 update → `aaa service restart --role agent`를 하나씩 한다. 각 단계에서 `aaa status`·`aaa machine list`로 연결과 판을 확인한 뒤 다음 머신으로 간다. session host의 세션은 이어진다. 바꾸기만 하면 도는 host·agent는 옛 이미지로 계속 돈다.
3. `aaa --version`으로 설치판을 보고, `aaa update list`의 SHA-256으로 실제 파일을 구분한다. 보관은 `~/.local/bin/.aaa-old/`에 `[update] keep`개(기본 3)다. 판을 주지 않은 `aaa update`는 최신 안정 release를 고르며 같거나 옛 판이면 실행 파일을 받지 않는다. `--version v0.1.1`도 된다. 명시한 옛 판은 고정 검사를 거쳐 받을 수 있다. major 변경은 기존대로 사람이 TOML에 해당 판을 고정해야 한다.
4. 문제가 있으면 ✋ `aaa update rollback`(지금과 다른 가장 새 보관, 이름을 주면 그것, 목록은 `aaa update list`)으로 되돌리고 2번의 재시작·확인을 한다. DB는 되돌리지 않는다(plan §11 「DB 백업」은 아직 없다).

- **최초 전환(기존 0.1.0).** 옛 `aaa update`에는 내려받기가 없다. 첫 release `v0.1.1`에서 `aaa-macos-aarch64`(Intel은 `aaa-macos-x86_64`)와 `SHA256SUMS`를 받아 `shasum -a 256 <받은 파일>` 결과를 맞춘다. 옛 실행 파일로 `aaa update --from <받은 파일> --dry-run` → ✋ `aaa update --from <받은 파일>`을 한 번 한다. 이후 위 원격 update를 쓴다. Windows도 11.4의 `--from`으로 한 번 전환한다.
- **로컬 빌드 유지.** `aaa update --from target/release/aaa [--dry-run]`은 그대로다. 같은 version이어도 SHA-256이 다른 빌드는 바꿀 수 있다. `--from`과 `--version`은 함께 쓰지 않는다.
- **판 고정.** 기존 host.toml·agent.toml에 `[update] version = "0.1.0"`처럼 고정했다면 사람이 목표 판으로 고쳐야 한다. updater는 설정을 저절로 바꾸지 않는다. 보관 이름은 UTC·밀리초·순번·판을 담아 같은 시각의 upgrade·downgrade도 생성 순서대로 되돌린다. 옛 이름의 보관도 계속 읽는다.
- **네트워크와 실패.** curl이 필요하다(macOS·Windows 기본 제공). 개인 `.curlrc`·GitHub token은 쓰지 않는다. manifest는 `https://github.com/cLazyZombie/aaa/releases/latest/download/manifest.json`, 지정판은 `/releases/download/v<판>/manifest.json`이다. asset은 그 manifest 판의 같은 공개 저장소 release URL만 받는다. 전송 중단·checksum·self-check 실패면 기존 실행 파일과 보관은 건드리지 않고 staging을 지운다. 자동 재시도·우회 설치는 없다.
- **macOS 키체인.** 기존 Mac 빌드 배포와 같이 ad-hoc 서명을 쓴다. 서로 다른 빌드의 cdhash는 이미 배포마다 바뀌었으며 GitHub release로 받아도 같다. `keychain_service` 기본값은 빈 값이어서 기본 자가 점검은 잠금 상태만 본다. doctor의 선택적 실제 항목 조회는 기존 ACL에 따라 새 접근 승인이 필요하거나 UI 생략으로 조회 실패가 날 수 있다. 고정 서명·ACL 변경·키체인 승인 이전은 하지 않는다. 구독 조회의 `security` 자식과 직접 조회는 다른 경로다. [Apple DR 설명](https://developer.apple.com/library/archive/technotes/tn2206/_index.html), [ACL 설명](https://developer.apple.com/documentation/security/access-control-lists).

### 9.1 release 만들기(coordinator, 착지 뒤)

현재 첫 release는 `v0.1.1`이다. 이후에는 `Cargo.toml`의 `[workspace.package] version`을 올리고 `cargo update --offline --workspace`로 lockfile의 workspace 판을 맞춘 변경을 검증·착지한다. 버전 숫자를 workflow에서 덮어쓰지 않는다(herdr처럼 tag와 Cargo 판이 같아야 빌드한다). main이 push되고 CI가 통과한 commit에서 coordinator만 실행한다:

```bash
python3 scripts/release.py check-tag --tag v0.1.1
git tag v0.1.1
git push origin refs/tags/v0.1.1
```

`.github/workflows/release.yml`은 웹을 먼저 빌드해 실행 파일 안에 넣고, macOS arm64·Intel, Windows x86_64, Linux x86_64·arm64(musl)를 각각 빌드한다. 자기 runner에서 `aaa --version`까지 확인한 뒤 실행 파일 다섯 개·`SHA256SUMS`·`manifest.json`을 같은 공개 release에 올린다. 최초에는 Actions 다섯 빌드 및 게시 완료, asset 이름·checksum·웹 포함을 확인하고 시험 머신의 dry-run·실제 update·rollback 뒤 운영에 순차 배포한다. release workflow를 만드는 thread는 tag나 branch를 push하지 않는다.

| 항목 | herdr 참고본 | aaa와 다른 이유 |
| --- | --- | --- |
| 최신 manifest | `herdr.dev/latest.json`, release 뒤 소스 `distribution/latest.json` 갱신 | 같은 release의 `manifest.json` asset. 별도 웹 배포·branch 쓰기·배포 token 없이 공개 release만으로 설치하기 위해서다. |
| 지정판 manifest | 안정 최신 manifest와 별도 이력 metadata | `/releases/download/v<판>/manifest.json`. 요청판의 asset·checksum을 그 tag에 고정한다. |
| manifest 형태 | `version`·`assets[os-arch]`의 URL·SHA-256, 옛 문자열 asset도 지원 | 동일 object 형태만 낸다. aaa의 첫 release라 옛 문자열 manifest 호환은 필요 없다. |
| Windows | ConPTY DLL이 든 zip·PowerShell installer·판별 release 폴더 | 정적 CRT의 `aaa.exe` 하나·기존 고정 자리 rename. 기존 `--from`·보관·rollback과 설치 경로를 유지한다. |
| 빌드 도구·runner | ghostty용 Zig·Linux arm64 교차 빌드 | aaa에는 ghostty 빌드가 없다. 같은 target을 native runner에서 빌드해 각 산출물의 판을 실행해 확인한다. |
| 부가 배포 | Nix 갱신·docs 게시·issue 종료·preview 검증 | aaa에는 대응하는 배포가 없다. release 만들기와 배포 계약 시험만 둔다. |

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

## 11. Windows에서 웹으로 접속

2026-10-19 TUI를 삭제했다. agent 없는 PC는 aaa 실행 파일·서비스·예약 작업을 설치하지 않고 브라우저로 접속한다.

1. host에서 `aaa web status`로 웹 주소를 확인한다.
2. Windows 브라우저에서 그 주소를 열고 host에서 사람이 `aaa web pair`로 낸 코드를 넣는다(보기만이면 `--reader`).
3. 기기 확인·폐기는 host의 `aaa web devices`·`aaa web revoke <기기>`로 한다.

옛 PC 바이너리의 `attach --remote`도 새 host가 「TUI는 삭제됐다」로 거절한다. 보존 tag는 `tui-last`이며 [삭제 기록](removed-tui.md)에 복구 방법을 남긴다. 운영 host·agent 교체와 재시작은 배포 담당이 한다.

### coordinator가 요청받은 배포를 실행하기(10-19)

기본은 차단한다. 먼저 사람이 이 API를 아는 host·agent를 설치한 뒤, aaa 밖 host 터미널에서 한 번 켠다.

```sh
aaa safety set demo coordinator_deploy on
```

`demo`는 프로젝트 이름이다. `[safety.demo] coordinator_deploy = true`를 host 설정에 기록한다. yolo나 project.toml은 이 허용을 켜지 않는다. 끄려면 `aaa safety set demo coordinator_deploy off`다. 변경은 다음 요청부터 적용하며, 이전 배포 기록 조회는 계속된다. 같은 OS 계정의 파일 접근 자체를 격리하는 sandbox는 아니다.

사용자가 **채팅에서 배포를 지시했을 때만** 그 프로젝트의 host 머신 coordinator가 실행한다. thread·평가자·임의 셸 pane·웹 Controller에는 배포 권한을 주지 않는다. host는 socket peer PID로 coordinator 소속을 정하므로 세션 환경변수를 지우거나 coordinator ID를 신고해도 허용되지 않는다.

```sh
aaa update --machine mac-a --version 0.1.2
# 또는: aaa update --machine mac-a --from /opt/aaa/candidate/aaa
aaa update status <출력된-ID> --wait
aaa service restart --role host --machine mac-a
aaa update status <재시작-ID> --wait
```

`--from`은 대상 머신에 이미 있는 파일이다. 배포 순서는 host 파일 교체·판/SHA 확인 → host restart·연결 확인 → host 머신 agent → 나머지 agent들이다. agent마다 update → ID의 완료/판/SHA 확인 → `service restart --role agent --machine <머신>` → `machine list`의 재연결과 `update status <id>`의 running_software·running_sha256 확인을 끝낸 뒤 다음으로 간다. 세션은 별도 session host가 소유해 backend 재시작 동안 이어진다. macOS의 기존 launchd 감독자만 재시작하며 설치·해제·start·stop은 계속 사람만 한다.

시작 명령은 실행 전 ID와 조회 명령을 stderr에 출력한다. host 응답이나 agent 연결이 끊겨도 시작 명령을 재전송하지 않고 같은 ID를 조회한다. `status`의 machine 생략은 host에 기록된 대상을 따른다. 작업 기록은 agent 상태 폴더의 `deployments/<id>.json`에 원자 저장한다. 같은 ID·같은 작업은 다시 실행하지 않고, 다른 작업은 거부한다. 머신마다 동시에 하나만 실행한다.

failed·interrupted·판/SHA 불일치면 진행을 멈추고 원인을 본다. `aaa update rollback --machine <문제 머신>`의 완료 → 해당 서비스 restart → 판/SHA·재연결을 확인한다. 중단된 update는 자동 재실행하지 않는다. agent restart 중 종료는 OS 잠금으로 확인한 이전 실행기 종료와 기대 SHA가 맞을 때만 복구 완료로 기록한다. 구판 host/agent는 새 API를 사용하지 않고 사람의 최초 반영 또는 터미널 복구를 안내한다.

생성된 coordinator 안내는 binary 반영 뒤 `aaa doctor files --fix`로 갱신한다. 운영 설정·서비스의 변경은 이 코드의 시험·착지와 별도다.
