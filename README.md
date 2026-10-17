# aaa

**aaa = Any Agent, Anywhere.** 어느 머신의 어떤 agent든 한곳에서 다룹니다.

여러 머신의 agent를 오케스트레이션하는 프로젝트입니다.
CLI를 중심으로 tmux 같은 세션 관리와 웹 접근을 제공합니다.

기획이 승인되어 구현을 진행하고 있습니다. [프로젝트 계획](docs/plan.md)을 참고하세요.

- **host**: 중심 머신 1대. 목록·layout·thread·권한의 정본을 갖고 TUI·웹에 화면을 보낸다.
- **agent**: 각 머신. 세션(PTY)을 띄우고 지킨다. host가 다시 시작해도 세션은 이어진다.
- **front**: TUI(`aaa attach`)와 웹. 상태를 갖지 않고 host하고만 말한다.

## 설치

macOS 기준 요약입니다. 순서·확인 기준은 [운영 설치 점검표](docs/install-runbook.md), 설계는 [계획 §10·§15](docs/plan.md)에 있습니다. 아래에서 머신 이름은 `mac-a`(host)·`mac-b`, 주소는 `example.ts.net` 예시입니다.

1. **빌드하고 고정 경로에 둔다(각 Mac).** 업그레이드도 같은 방법(rename으로 교체)입니다.

   ```bash
   cargo build --release -p aaa
   install -m 755 target/release/aaa ~/.local/bin/aaa.new && mv ~/.local/bin/aaa.new ~/.local/bin/aaa
   ```

2. **설정.** `aaa config example host`·`aaa config example agent`로 예시를 보고 `~/.config/aaa/`에 둡니다. `aaa config check`로 확인합니다.
   - `host.toml`(host만): agent link listen(`0.0.0.0:7430`), 웹 listen(`127.0.0.1:<port>`), 프로젝트 폴더, `[tui]` prefix.
   - `agent.toml`(각 Mac): host 주소(host Mac은 `"local"`, 다른 Mac은 `"mac-a.example.ts.net:7430"`), 작업 폴더, profile(`claude`·`codex` 등).
3. **감독자 설치(사람이 직접, 각 Mac).** launchd를 바꾸므로 자기 터미널에서 `yes`를 입력합니다. host Mac은 host를 agent보다 먼저 설치합니다.

   ```bash
   ~/.local/bin/aaa service install --role host  --aaa ~/.local/bin/aaa   # host Mac만
   ~/.local/bin/aaa service install --role agent --aaa ~/.local/bin/aaa   # 모든 Mac
   ```

4. **다른 Mac 등록.** host Mac에서 `aaa machine enroll --open 10m`을 실행합니다. 다른 Mac에서 `aaa agent enroll mac-a.example.ts.net:7430`을 실행하면 확인 코드가 나옵니다. host Mac에서 `aaa machine approve mac-b --code <코드>`로 승인합니다. 끝나면 `aaa machine list`에서 모두 「연결」인지 봅니다.
5. **웹 공개와 pairing.** `aaa web publish`가 tailnet 안 HTTPS로 공개합니다(기존 `tailscale serve` 항목은 덮지 않습니다). `aaa web pair`가 주는 코드를 브라우저에 입력합니다. 보기만 할 기기는 `aaa web pair --reader`입니다.

## 사용법

### 웹

- **목록**: 왼쪽에 Thread와 머신별 pane이 보입니다. 상태(작업 중·입력 대기·질문·검토 대기·사람 차례·쉼)와 질문 카드가 위에 뜹니다.
- **터미널**: pane을 누르면 터미널이 열립니다. 직접 쳐도 되고, 아래 **입력 막대(composer)** 를 써도 됩니다.
  - Enter는 보내기, Shift+Enter는 줄바꿈입니다. 휴대폰에서는 Enter가 줄바꿈이고 보내기는 단추입니다.
  - 키 줄은 Esc·Tab·Ctrl·Alt·방향키 등입니다. 그림은 붙여 넣거나 끌어 놓습니다.
- **크기와 입력(가져오기 없음)**: 보기 권한이 아니면 언제나 입력됩니다. PTY 크기는 「지금 쓰는 화면」이 정합니다. 입력하거나 터미널을 누르거나 창 focus를 얻은 화면이 자기 크기로 다시 겁니다. 탭이 1초 넘게 가려지면 놓습니다. 「화면에 맞추기」로 직접 맞출 수도 있습니다.
- **대화**: Claude·Codex pane은 「대화」로 기록을 읽기 전용으로 봅니다.
- **알림 켜기**: 목록 아래 단추입니다. 질문·보고가 기기 알림(Web Push)으로 옵니다. iPhone은 홈 화면 앱(Safari 공유 → 홈 화면에 추가)에서만 켤 수 있습니다.

### TUI(`aaa attach`)

host Mac 터미널에서 `aaa attach`(workspace 기본 `main`, 다른 것은 `-w <이름>`)로 엽니다. 탭·나누기 layout은 host에 저장되므로 나갔다 다시 붙어도 같습니다.

- **왼쪽 목록**: 웹처럼 Thread(상태 배지)와 머신별 pane, 「+ 새 셸」이 보입니다. 고르고 Enter면 그 pane을 탭으로 엽니다(이미 탭에 있으면 그 탭으로). 100칸보다 좁은 터미널에서는 접힌 채 시작합니다. 목록에서는 `↑↓`·`j/k`로 고르고 `Esc`로 pane에 돌아갑니다.
- 명령으로도 됩니다: 이미 있는 pane을 탭으로 넣기 `aaa ws add <pane> -w <이름>`, 새 pane을 새 탭으로 `aaa ws tab --machine mac-b -- <명령>`.
- 입력은 포커스 pane으로 바로 갑니다. 다른 화면(웹 등)이 쓰던 pane도 입력하면 이 화면 크기가 되고, 터미널 focus를 다시 얻어도 크기를 다시 겁니다.
- **prefix(기본 `Ctrl+B`) 다음 키**(3초 안에 누르지 않으면 취소):

| 키 | 하는 일 |
|---|---|
| `c` | 새 탭(포커스 pane 머신의 기본 셸) |
| `%` · `\|` | 좌우로 나누기 |
| `"` · `-` | 위아래로 나누기 |
| 화살표 · `o` | pane 이동 |
| `n` · `p` · `1`~`9` | 탭 이동 |
| `s` | 왼쪽 목록 펴기·접기(펴면 목록으로 포커스) |
| `Tab` | 목록 ↔ pane 포커스 |
| `t` | 이 화면 크기로(보통은 입력만으로 된다) |
| `r` | 끝난 pane 재개 |
| `x` | 닫기(세션도 끝냄) |
| `d` | 나가기(세션은 그대로) |
| `Ctrl+B` | `Ctrl+B`를 pane에 보냄 |

- `Ctrl+V`(prefix 없이): 이 기기 clipboard에 그림이 있으면 pane 머신에 올리고, 입력창이면 경로를 넣습니다. 그림이 없으면 원래 `Ctrl+V`입니다.
- 한글 입력기가 켜져 있으면 prefix 동안 영문 입력기(ABC 등)로 바꿨다가 되돌립니다. 자모로 들어와도 같은 자리 키로 읽습니다(ㅊ = `c`).
- 설정(`host.toml`):

  ```toml
  [tui]
  prefix = "C-b"        # tmux 안에서 쓰면 겹치지 않게(예: "C-g")
  prefix_ascii = true   # prefix 동안 영문 입력기로 바꾸기(macOS, ssh 밖)
  ```

### 자주 쓰는 CLI

| 명령 | 하는 일 |
|---|---|
| `aaa machine list` | 머신 연결·디스크·이상 |
| `aaa pane list` · `aaa pane read <pane>` | pane 목록 · 지금 화면 글자 |
| `aaa pane run --machine mac-b -- <명령>` | 새 pane |
| `aaa pane send <pane> --text "…" --enter` | 입력 보내기(사람의 attach에 막히지 않음) |
| `aaa question list` · `aaa question answer <id> yes` | 확인창 질문 보기·답하기 |
| `aaa project create <p>` · `aaa context <p>` | 프로젝트 만들기 · coordinator 요약(할 일·열린 thread·inbox) |
| `aaa thread start <p> --title "…" --machine mac-b --profile codex --kind dir --task-file task.md` | thread 시작 |
| `aaa thread list -p <p>` · `aaa thread show <p> <id>` | thread 목록 · 보고·Next 보기 |
| `aaa thread ack <p> <id>` · `aaa thread resolve <p> <id>` | 보고 확인 · 끝내기(worktree 정리) |
| `aaa inbox list <p>` · `aaa inbox done <p> <id>` | 보고·질문·끊김 같은 놓치면 안 되는 항목 |

모든 명령은 `--json`으로 기계가 읽는 출력을 줍니다. 자세한 것은 `aaa <명령> --help`입니다.

## 개발

CI(`.github/workflows/ci.yml`)와 같은 단계를 로컬에서 한 번에 돈다. 단계의 명령은 `scripts/ci-local.sh`에만 있고 CI도 이 스크립트를 단계별로 부른다(로컬과 CI의 명령이 어긋나지 않는다).

```sh
bash scripts/ci-local.sh                     # 전부: web fmt build clippy test clipboard e2e public
bash scripts/ci-local.sh --only test,e2e     # 고른 단계만
bash scripts/ci-local.sh --skip e2e          # 빼고 전부
bash scripts/ci-local.sh --native-clipboard  # clipboard 단계를 이 머신의 실제 clipboard로(덮어쓴다)
```

- 끝에 단계마다 시작·끝 시각, 걸린 시간, 결과를 요약한다. 실패한 단계는 로그 위치를 보인다(기본 `target/ci-local/<시각>/<단계>.log`). 하나라도 실패하면 1로 끝난다. 실패를 다시 돌려 통과시키지 않는다.
- `clipboard`는 사람의 clipboard를 덮어쓰므로 기본으로 건너뛴다(CI는 `AAA_CLIP_NATIVE=1`로 켠다).
- `e2e`는 웹 E2E(Playwright Chromium·WebKit, 한글 IME 기록 재생 포함)다. Unix만이고 playwright를 전역 npm 모듈로 찾는다: `npm install -g playwright@1.59.1 && npx playwright@1.59.1 install chromium webkit`.
- `public`은 공개 저장소 검사(`scripts/check-public.sh`)다. CI에서는 돌지 않는다(push 전 로컬 검사).

## 라이선스

aaa 코드는 [MIT](LICENSE)입니다.

웹 터미널에는 [Meslotendard](https://github.com/cLazyZombie/Meslotendard) 0.3.1 글꼴(Regular·Bold, WOFF2)을 함께 넣었습니다. 이 글꼴은 SIL Open Font License 1.1이고, 구성 원본 Meslo LG(Apache 2.0)·Nerd Fonts·Pretendard(OFL 1.1)의 고지는 [web/public/fonts/meslotendard-0.3.1/](web/public/fonts/meslotendard-0.3.1/)에 있습니다. host는 같은 고지를 웹 주소 `/fonts/meslotendard-0.3.1/`로도 보냅니다.
