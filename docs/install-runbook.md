# 운영 설치 점검표(macOS, 결정 14)

launcher 등록([P0 절차서](p0-runbook.md))과 L1 측정이 끝난 뒤 aa를 운영 자리에 올리는 순서다. 설계는 기획서 [§10](plan.md)·[§15](plan.md#15-현재-운영에서-이전)에 있다. 이 문서는 명령과 확인만 적는다.

- 머신 이름은 일반 이름이다. mac-a·mac-b·mac-c 중 host를 둘 Mac을 **host Mac**이라 부른다. 주소 예시는 `example.ts.net`이다.
- **launchd·시스템 설정을 바꾸는 단계(✋)는 사람이 자기 터미널에서 직접 한다.** `aa service install·uninstall`은 aa 세션 안이거나 터미널이 아니면 거부하고 `yes` 입력을 받는다.
- **명령은 전체 경로 `~/.local/bin/aa`로 친다.** macOS에는 Apple Archive 도구 `/usr/bin/aa`가 기본으로 있어, PATH에서 `~/.local/bin`이 뒤면 Apple `aa`가 실행된다. 본문의 `aa …`는 모두 `~/.local/bin/aa …`를 뜻한다(이름 변경은 사용자 결정, 기획서 §16 나중 후보).
- 지금 운영(herdr·herdr-projects·herdr-web-ui·WezTerm)과 그 설정·socket·port는 건드리지 않는다. aa는 다른 port·state·작업 폴더를 쓴다.

## 0. 전제

| 확인 | 명령·기준 |
|---|---|
| launcher 등록 | `~/Applications/aa-launcher.app`이 개발자 도구·전체 디스크 접근에 켜져 있다 |
| L1 합격 | `p0/launcher/measure.sh after` 판정 통과(median ≤10ms, p95 ≤20ms, FDA ok, 책임 프로세스 `aa-launcher`) |
| 각 Mac 같은 aa | 같은 commit으로 빌드한다(`aa --version`) |

## 1. aa를 고정 경로에 둔다(각 Mac)

```bash
cd <저장소> && cargo build --release -p aa
install -m 755 target/release/aa ~/.local/bin/aa.new && mv ~/.local/bin/aa.new ~/.local/bin/aa   # rename으로 교체
~/.local/bin/aa --version
```

- 업그레이드도 같은 방법이다. launcher 등록은 다시 하지 않는다(L1이 확인하는 것).
- 운영 Mac에 빌드 부하를 주지 않으려면 같은 macOS·CPU의 다른 Mac에서 빌드해 `ssh <mac> 'cat > ~/.local/bin/aa.new && chmod 755 ~/.local/bin/aa.new && mv ~/.local/bin/aa.new ~/.local/bin/aa' < target/release/aa`로 옮겨도 된다. 옮긴 뒤 양쪽 `shasum -a 256`이 같은지 본다.
- hook 명령(`aa hook --agent …`)은 이 경로로 실행마다 주므로 Claude·Codex 사용자 설정은 고치지 않는다.

## 2. 설정

host Mac `~/.config/aa/host.toml`(예시: `aa config example host`)

```toml
schema = 1
[host]
agent_listen = "0.0.0.0:7430"      # 다른 Mac의 agent가 붙는다(tailnet 안)
[web]
listen = "127.0.0.1:7431"          # herdr-web-ui(7317)과 겹치지 않는 port
[local_agent]
mode = "launchd"                   # 운영: host가 agent를 띄우지 않는다
[projects]
root = ""                          # 기본 ~/.aa/projects. herdr-projects 폴더와 다른 곳
allow_profiles = ["*"]
```

- 알림(선택): 봇 token이 든 주소는 `host.local.toml`의 `[notify]`에 둔다(커밋·공유하지 않는다).

각 Mac `~/.config/aa/agent.toml`(예시: `aa config example agent`)

```toml
schema = 1
[agent]
host = "local"                     # host Mac. 다른 Mac은 "mac-b.example.ts.net:7430"
work_root = ""                     # 기본 ~/aa-work. herdr worktree 접두사와 다른 곳
[health]
min_free_gb = 20
[health.toolchain]
cargo = ["cargo", "--version"]
[profiles.claude]
harness = "claude"
model = "opus"
effort = "high"
args = []                          # 권한 건너뛰기를 쓰려면 여기(그 머신 사용자만 고친다)
[profiles.codex]
harness = "codex"
args = []
```

확인: 각 Mac에서 `aa config check`가 오류 없이 끝난다.

## 3. 감독자 설치 ✋(각 Mac, 사람이 직접)

```bash
~/.local/bin/aa service show --role agent --aa ~/.local/bin/aa      # 바꾸지 않고 plist·경로·상태를 본다
~/.local/bin/aa service install --role host --aa ~/.local/bin/aa    # host Mac만, agent보다 먼저
~/.local/bin/aa service install --role agent --aa ~/.local/bin/aa   # 모든 Mac
```

- plist는 `~/Library/LaunchAgents/dev.aa.<role>.plist`, 로그는 `~/Library/Logs/aa/<role>.log`다.
- ProgramArguments는 `aa-launcher <aa> <role>`다. 설치하는 셸의 `PATH`·`LANG`을 plist에 적는다(launchd 기본 PATH로는 `claude`·`codex`를 못 찾는다). PATH가 바뀌면 `--replace`로 다시 설치한다.
- macOS가 「백그라운드 항목이 추가됨」 알림을 띄우면 허용한다.

| 확인 | 기준 |
|---|---|
| `aa service show --role agent` | `launchd: 올라가 있다 · state = running` |
| `aa doctor lineage --match "aa agent"`(그 Mac) | agent의 책임 프로세스가 `aa-launcher` |
| `aa status`(host Mac) | 로컬 머신이 연결, 운영 모드 |

## 4. 다른 Mac 등록

```bash
~/.local/bin/aa machine enroll --open 10m          # host Mac: 등록 창
~/.local/bin/aa agent enroll mac-b.example.ts.net:7430   # 다른 Mac: 화면에 확인 코드가 나온다
~/.local/bin/aa machine approve mac-a --code <코드>  # host Mac: 그 코드로 승인
```

확인: `aa machine list`에 세 Mac이 「연결」이고 `이상:`이 없다(디스크·toolchain·시계).

## 5. 웹·휴대폰

```bash
tailscale serve status                # 기존 항목 기록(바꾸지 않는다)
~/.local/bin/aa web publish                        # 겹치지 않는 port를 골라 HTTPS로 공개
~/.local/bin/aa web pair                           # 휴대폰에서 주소를 열고 이 코드 입력(보기 권한은 --reader)
```

확인: 휴대폰에서 세 Mac의 pane 목록과 터미널이 보이고 입력된다. 질문 카드에 답할 수 있다.

## 6. shadow(§15 2)

- aa 프로젝트는 aa projects root에 새로 만든다(`aa project create <p>`). herdr-projects 폴더를 가리키지 않는다.
- 기존 herdr thread는 aa로 제어하지 않는다. 같은 성격의 새 low-impact 일을 aa thread로 하나 맡기고 상태·보고를 비교한다.
- coordinator: aa 프로젝트 폴더를 cwd로 Claude를 띄운다(`aa pane run --cwd <프로젝트 폴더> -- claude`). AGENTS.md 안내대로 매 턴 `aa context <p>`를 본다.

| 비교 | 기준 |
|---|---|
| 상태 | 질문·입력 대기·작업 중이 실제 화면과 맞는다(polling 없음) |
| 보고 | `[완료]` 보고와 결과물이 host로 오고, 웹에서 열린다 |
| 알림 | 질문·보고 준비가 coordinator(nudge)와 webhook으로 온다 |
| 단절 | 한 Mac의 agent를 내렸다 올려도 세션이 이어지고 thread가 실패로 바뀌지 않는다 |

## 7. pilot → 전환

- 새 일부터 aa에서 시작한다. 기존 herdr thread는 herdr에서 마무리한다(live takeover는 하지 않는다).
- active thread마다 소유 도구는 하나다.
- 전환 gate: 사용자가 정한 기간 동안 shadow·pilot에서 잃은 보고·잘못된 입력·멈춘 감독자가 없다.

## 8. 되돌리기

```bash
~/.local/bin/aa web publish off                    # aa가 만든 serve 항목만 끈다
~/.local/bin/aa service uninstall --role agent     # ✋ 각 Mac(host Mac은 --role host도)
```

- 세션은 session host가 소유하므로 agent를 내려도 도는 harness는 바로 죽지 않는다. 필요하면 `aa pane kill`로 먼저 끝낸다.
- herdr 쪽 설치·설정은 처음부터 건드리지 않았으므로 되돌릴 것이 없다.
- 보고·결과물은 aa projects root에 남는다. 기존 coordinator가 읽을 수 있는 Markdown이다.

## 9. 업그레이드

1. 1번처럼 새 `aa`를 빌드해 rename으로 교체한다.
2. ✋ `launchctl kickstart -k gui/$(id -u)/dev.aa.agent`(host Mac은 `dev.aa.host`도)로 다시 띄운다. 세션은 이어진다.
3. `aa status`·`aa machine list`로 같은 버전·연결을 확인한다.
