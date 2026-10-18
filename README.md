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

- **왼쪽 목록**: herdr처럼 위는 workspace(` spaces`, 고르면 그 workspace로), 아래 ` agents`에 웹처럼 머신마다 「+ 새 셸」, 프로젝트별 thread(「t-0001 · 제목」과 둘째 줄 상태 말, 사람 차례 → 검토 대기 → 작업 중 → 쉼 차례), thread에 딸리지 않은 pane이 보입니다. `prefix w`(탐색)에서 `↑↓`로 고르고 Enter면 그 pane을 탭으로 엽니다(이미 탭에 있으면 그 탭으로). `prefix b`로 접으면 번호와 상태 점만 든 4칸 열이 됩니다(`[tui] sidebar_start_collapsed`로 접은 채 시작).
- 명령으로도 됩니다: 새 pane을 새 탭으로 `aaa tab create --machine mac-b -- <명령>`, 있는 pane을 다른 workspace의 새 탭으로 `aaa pane move <pane> --new-tab -w <이름>`, 나누기 `aaa pane split <pane> --direction right`.
- 입력은 포커스 pane으로 바로 갑니다. 다른 화면(웹 등)이 쓰던 pane도 입력하면 이 화면 크기가 되고, 터미널 focus를 다시 얻어도 크기를 다시 겁니다.
- **키는 herdr와 같습니다.** prefix(기본 `ctrl+b`) 다음 키 하나(시간 제한 없음, `Esc`나 모르는 키는 취소). `prefix ?`로 단축키 창(`/`로 검색)이 뜹니다.

| 키 | 하는 일 |
|---|---|
| `c` | 새 탭: 이름 창(Enter면 번호 탭), 포커스 pane 머신의 기본 셸을 그 pane의 지금 폴더에서 |
| `Shift+T` · `Shift+X` | 탭 이름 바꾸기 · 탭 닫기(세션도 끝냄, 마지막 탭은 확인) |
| `v` · `-` | 오른쪽에 · 아래에 나누기(새 pane은 나눈 pane의 지금 폴더에서) |
| `h` `j` `k` `l` · `Tab` · `Shift+Tab` | pane 포커스 옮기기 · 차례로 |
| `n` · `p` · `1`~`9` | 탭 이동 |
| `x` | pane 닫기(세션도 끝냄) |
| `Shift+P` | pane 이름 바꾸기(비우고 Enter면 띄울 때 이름으로) |
| `[` | copy 모드: `h/j/k/l`·`w/b/e`·`W/B/E`·`{`/`}`·`PageUp`/`PageDown`·`ctrl+u/d`·`g`/`G`·`0`/`$`/`^`로 커서(기록까지 넘김), `/`·`?` 찾기·`n`/`N` 다시, `v`·`V`로 고르고 `y`·`Enter` 복사, `q`·`Esc` 끝(보던 자리로) |
| `z` | 포커스 pane 크게 보기(다시 누르면 원래대로) |
| `r` | 크기 모드: `h/j/k/l`로 가장자리 옮기기, `Esc` 끝 |
| `Shift+H/J/K/L` | 그 방향 pane과 자리 바꾸기 |
| `b` | 왼쪽 사이드바 펴기·접기 |
| `w` | 목록 탐색(`↑↓` 고르기, `Enter` 열기, `1`~`9` n번째 열기, `h/j/k/l`·`Tab` pane, `Esc` 끝) |
| `o` | 질문이 열린 pane으로 |
| `?` | 단축키 창 |
| `Shift+R` | `[tui]` 설정 다시 읽기 |
| `q` | 나가기(세션은 그대로) |
| prefix 한 번 더 | prefix 키를 pane에 보냄 |

- **마우스(herdr와 같음)**: 탭 이름표를 누르면 그 탭, 탭 막대 휠은 앞·뒤 탭, ` + `는 새 탭, 넘치면 ` < ` ` > `로 넘깁니다. 사이드바 줄을 누르면 엽니다. `«`·`»`는 접기·펴기, 사이드바 오른쪽 경계를 끌면 폭(18~36)이 바뀝니다. pane을 누르면 포커스, 끌면 글을 골라 떼는 때 이 기기 clipboard에 복사합니다(두 번 누르면 낱말, ssh 셸이면 OSC 52). 끄는 중 pane 위·아래 밖으로 나가거나 휠을 굴리면 기록을 넘기며 고르고, 화면에 없는 기록 줄까지 복사합니다. vim·htop처럼 마우스를 켠 프로그램에는 누르기·끌기·휠이 그대로 가고, less처럼 alt 화면을 쓰는 프로그램에는 휠이 위·아래 화살표로 갑니다. 보통 셸 화면에서 휠은 그 화면의 기록을 3줄씩 넘기고(`[tui] mouse_scroll_lines`), 글자를 치면 맨 아래로 돌아옵니다. 다른 기기의 화면은 따라 움직이지 않습니다. 보통 화면 pane의 오른쪽 1칸은 기록 scrollbar라 누르면 그 자리로 뛰고 thumb를 끌어 넘깁니다(alt 화면에서는 그 칸도 프로그램이 씁니다). 나눈 pane 사이 테두리를 끌면 크기가, 탭 이름표를 끌어 놓으면 탭 차례가 바뀝니다. 오른쪽 누름은 탭 이름표에서 탭 메뉴(새 탭·이름 바꾸기·닫기), pane에서 pane 메뉴(이름 바꾸기·지우기·포커스 pane과 자리 바꾸기·나누기·크게 보기·닫기)를 엽니다. 마우스를 켠 프로그램에 오른쪽 누름을 보내려면 pane 메뉴의 「오른쪽 누름을 pane에 보내기」를 켭니다. 창과 메뉴는 밖을 누르면 닫힙니다. 터미널 자체의 끌어 고르기는 `[tui] mouse_capture = false`로 돌아옵니다(켠 채로도 WezTerm은 shift, iTerm2는 option을 누른 채 끌면 터미널이 고릅니다).
- `Ctrl+V`(prefix 없이): 이 기기 clipboard에 그림이 있으면 pane 머신에 올리고, 입력창이면 경로를 넣습니다. 그림이 없으면 원래 `Ctrl+V`입니다.
- 재부팅 등으로 중단된 pane에는 다시 띄우는 명령(`aaa pane resume <id>`)이 보입니다.
- 설정(`host.toml`):

  ```toml
  [tui]
  prefix = "ctrl+b"                             # 여러 개면 ["ctrl+b", "ctrl+a"]. tmux 안에서 쓰면 겹치지 않게(예: "ctrl+g")
  switch_ascii_input_source_in_prefix = true    # prefix·탐색 동안 한글 입력기를 영문으로(macOS·Windows 한국어 IME, ssh 밖)
  prefix_jamo_keys = true                       # prefix 뒤 두벌식 자모(ㅊ)를 같은 자리 키(c)로 읽기
  prompt_new_tab_name = true                    # prefix c에서 이름 창을 띄우기
  confirm_close = true                          # 마지막 탭·마지막 pane을 닫을 때 묻기
  sidebar_width = 26                            # 사이드바 폭(18~36)
  sidebar_start_collapsed = false               # 접힌 채 시작
  sidebar_collapsed_mode = "compact"            # 접으면 번호·상태 점 4칸(compact) 또는 없음(hidden)
  mobile_width_threshold = 64                   # 이 폭 이하면 사이드바·탭 막대 대신 두 줄 머리
  theme = "catppuccin"                          # herdr 테마 이름(terminal·tokyo-night·dracula·nord 등 18개)
  pane_borders = "auto"                         # pane 테두리: auto(나눈 탭만) · always · off
  mouse_capture = true                          # 마우스: 탭·사이드바·pane 누르기, 휠, 끌어 고르기
  copy_on_select = true                         # 끌어 고른 글을 떼는 때 복사(끄면 ctrl+c·cmd+c)
  mouse_scroll_lines = 3                        # 보통 화면 휠 한 칸에 넘기는 기록 줄 수
  pane_scrollbars = true                        # 보통 화면 pane 오른쪽 1칸에 기록 scrollbar(그만큼 PTY가 좁다)

  [terminal]
  new_cwd = "follow"                            # 새 탭·나누기의 폴더: follow(포커스 pane의 지금 폴더) · home · 절대 경로
  ```

### 자주 쓰는 CLI

| 명령 | 하는 일 |
|---|---|
| `aaa machine list` | 머신 연결·디스크·이상 |
| `aaa pane list` · `aaa pane read <pane>` | pane 목록 · 최근 출력 글(`--source visible`이면 지금 화면) |
| `aaa tab create --machine mac-b -- <명령>` | 새 탭에 새 pane |
| `aaa pane run <pane> "…"` | 명령 실행(글 + Enter, 사람의 attach에 막히지 않음) |
| `aaa pane send-text <pane> "…"` · `aaa pane send-keys <pane> enter` | 글만 · 키만 보내기 |
| `aaa pane wait-output <pane> --match "…" --timeout 30000` | 출력에 글이 나올 때까지 기다리기 |
| `aaa question list` · `aaa question answer <id> yes` | 확인창 질문 보기·답하기 |
| `aaa project create <p>` · `aaa context <p>` | 프로젝트 만들기 · coordinator 요약(할 일·열린 thread·inbox) |
| `aaa thread start <p> --title "…" --machine mac-b --profile codex --kind dir --task-file task.md` | thread 시작 |
| `aaa thread list <p>` · `aaa thread show <p> <id>` | thread 목록 · 보고·Next 보기 |
| `aaa thread ack <p> <id>` · `aaa thread resolve <p> <id>` | 보고 확인 · 끝내기(worktree 정리) |
| `aaa inbox list <p>` · `aaa inbox done <p> <id>` | 보고·질문·끊김 같은 놓치면 안 되는 항목 |
| `aaa routine list <p>` · `aaa routine toggle <p> <이름> --off` | 일정 작업과 PR 사건 routine(`on = "pr"`: thread PR의 check 실패·리뷰에 thread로 글을 보낸다) |

모든 명령은 `--json`으로 기계가 읽는 출력을 줍니다. `pane`·`tab`·`workspace`는 늘 JSON 한 줄이고, 실패하면 stderr에 `{"error":…}`를 쓰고 종료 코드 1입니다(herdr와 같다). 셸 완성은 `aaa completion zsh`(bash·fish·elvish·powershell)입니다. 자세한 것은 `aaa <명령> --help`입니다.

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
