# aaa

**aaa = Any Agent, Anywhere.** 어느 머신의 어떤 agent든 한곳에서 다룹니다.

여러 머신의 agent를 오케스트레이션하는 프로젝트입니다.
CLI를 중심으로 tmux 같은 세션 관리와 웹 접근을 제공합니다.

보고서 올리기·실시간 폴더·일괄 이전·정리는 [보고서 운영](docs/library.md)을 참고하세요.

기획이 승인되어 구현을 진행하고 있습니다. [프로젝트 계획](docs/plan.md)을 참고하세요.

- **host**: 중심 머신 1대. 목록·layout·thread·권한의 정본을 갖고 웹에 화면을 보낸다.
- **agent**: 각 머신. 세션(PTY)을 띄우고 지킨다. host가 다시 시작해도 세션은 이어진다.
- **front**: 웹. 상태를 갖지 않고 host하고만 말한다.

## 설치

macOS 기준 요약입니다. 순서·확인 기준은 [운영 설치 점검표](docs/install-runbook.md), 설계는 [계획 §10·§15](docs/plan.md)에 있습니다. 아래에서 머신 이름은 `mac-a`(host)·`mac-b`, 주소는 `example.ts.net` 예시입니다.

1. **빌드하고 고정 경로에 둔다(각 Mac).** 처음에는 rename으로 둡니다. 업그레이드는 `aaa update`가 같은 폴더에서 새것을 검사한 뒤 바꾸고 옛것을 보관합니다(사람이 터미널에서 `yes`, 되돌리기는 `aaa update rollback`).

   ```bash
   cargo build --release -p aaa
   install -m 755 target/release/aaa ~/.local/bin/aaa.new && mv ~/.local/bin/aaa.new ~/.local/bin/aaa   # 처음
   ~/.local/bin/aaa update --from target/release/aaa                                                     # 업그레이드
   ```

2. **설정.** `aaa config example host`·`aaa config example agent`로 예시를 보고 `~/.config/aaa/`에 둡니다. `aaa config check`로 확인합니다.
   - `host.toml`(host만): agent link listen(`0.0.0.0:7430`), 웹 listen(`127.0.0.1:<port>`), 프로젝트 폴더.
   - `agent.toml`(각 Mac): host 주소(host Mac은 `"local"`, 다른 Mac은 `"mac-a.example.ts.net:7430"`), 작업 폴더와 필요한 `[harnesses.<종류>] command/env` 덮어쓰기. profile 정의는 host Mac의 `host.toml [profiles.<이름>]` 한 곳(`agent`·`model`·`effort`·`description`·`args`)에 둡니다. `claude`·`codex`는 기본 profile이며 더하려면 host 터미널에서 `aaa profile add`를 씁니다. yolo·쓸 수 있는 profile은 host에서 사람이 `aaa safety`·`aaa profile allow`로 정합니다(plan §8.170).
3. **감독자 설치(사람이 직접, 각 Mac).** launchd를 바꾸므로 자기 터미널에서 `yes`를 입력합니다. host Mac은 host를 agent보다 먼저 설치합니다.

   ```bash
   ~/.local/bin/aaa service install --role host  --aaa ~/.local/bin/aaa   # host Mac만
   ~/.local/bin/aaa service install --role agent --aaa ~/.local/bin/aaa   # 모든 Mac
   ```

4. **다른 Mac 등록.** host Mac에서 `aaa machine enroll --open 10m`을 실행합니다. 다른 Mac에서 `aaa agent enroll mac-a.example.ts.net:7430`을 실행하면 확인 코드가 나옵니다. host Mac에서 `aaa machine approve mac-b --code <코드>`로 승인합니다. 끝나면 `aaa machine list`에서 모두 「연결」인지 봅니다.
5. **웹 공개와 pairing.** `aaa web publish`가 tailnet 안 HTTPS로 공개합니다(기존 `tailscale serve` 항목은 덮지 않습니다). `aaa web pair`가 주는 코드를 브라우저에 입력합니다. 보기만 할 기기는 `aaa web pair --reader`입니다.

## 사용법

### 웹

- **목록**: 왼쪽에 프로젝트별 thread(모든 머신의 thread를 한 묶음에, 둘째 줄에 상태 말과 머신 이름)와 thread에 딸리지 않은 pane의 「기타」 묶음(줄마다 머신 이름), 그 아래 머신 칸(연결·끊김·새 pane)이 보입니다. 상태(작업 중·입력 대기·질문·검토 대기·확인 필요·쉼)와 질문 카드가 위에 뜹니다. 아래 「Agents」는 설정 → 모양 「에이전트 정렬」에서 활동순(질문 먼저, 그다음 최근 바뀐 것)으로 바꿀 수 있습니다.
- **새 pane**: 머신 칸의 머신 이름 옆 `+`나 `⌘⇧O`(Windows·Linux `Ctrl+Shift+O`)로 창을 엽니다. 셸이나 그 머신 `agent.toml`의 profile, 폴더(절대 경로나 `~/…`, 최근 폴더), 이름을 고르고 「시작」합니다.
- **터미널**: pane을 누르면 터미널이 열립니다. 직접 쳐도 되고, 아래 **입력 막대(composer)** 를 써도 됩니다.
  - Enter는 보내기, Shift+Enter는 줄바꿈입니다. 휴대폰에서는 Enter가 줄바꿈이고 보내기는 단추입니다.
  - 키 줄은 Esc·Tab·Ctrl·Alt·방향키 등입니다. 그림은 붙여 넣거나 끌어 놓습니다.
- **크기와 입력(가져오기 없음)**: 보기 권한이 아니면 언제나 입력됩니다. PTY 크기는 「지금 쓰는 화면」이 정합니다. 입력하거나 터미널을 누르거나 창 focus를 얻은 화면이 자기 크기로 다시 겁니다. 탭이 1초 넘게 가려지면 놓습니다. 「화면에 맞추기」로 직접 맞출 수도 있습니다.
- **대화**: Claude·Codex pane은 「대화」로 기록을 읽기 전용으로 봅니다. 휴대폰은 agent pane을 처음부터 대화로 엽니다(설정 → 채팅 「패널 기본 화면」). 「생각 표시」를 켜면 agent의 생각도 접힌 줄로 보입니다. 대화 아래 입력창에서 바로 보내고, 「대기 메시지 자동 전송」을 끄면 작업 중에 쓴 글은 「지금 보내기」까지 붙잡아 둡니다. 빠른 답장 칩도 켤 수 있습니다.
- **알림 켜기**: 목록 아래 단추입니다. 기기 알림(Web Push)은 coordinator가 보낸 「확인 필요」·「전체 완료」 post만 받습니다. thread 질문·보고는 앱 안 알림으로 확인합니다. iPhone은 홈 화면 앱(Safari 공유 → 홈 화면에 추가)에서만 켤 수 있습니다.

### 삭제된 기능: TUI

2026-10-19부터 TUI 코드·의존성·전용 시험을 삭제했습니다. 화면은 웹 UI를 씁니다. host에서 `aaa web status`로 주소를 확인합니다. 옛 `aaa attach`와 TUI front 접속도 삭제 안내로 거절합니다.

삭제 직전 main은 annotated tag `tui-last`로 보존합니다. 이유·삭제 커밋·복구 방법·삭제 목록은 [삭제 기록](docs/removed-tui.md)에 있습니다.

### 자주 쓰는 CLI

| 명령 | 하는 일 |
|---|---|
| `aaa machine list` | 머신 연결·디스크·이상 |
| `aaa service restart\|stop\|start --role host\|agent` | 감독자(launchd)로 다시 띄우기(업그레이드 뒤) · 멈추기 · 띄우기. 사람이 터미널에서(`yes`). restart는 아래 coordinator 배포 허용도 가능 |
| `aaa update --from <파일> [--dry-run]` · `aaa update rollback [이름]` · `aaa update list` | 지금 실행 파일을 주어진 빌드로 바꾸기(self-check·`[update] version` 고정 검사 뒤 rename, 옛것은 옆 `.aaa-old/`에 `[update] keep`개) · 보관한 것으로 되돌리기 · 보관 목록. 바꾸기·되돌리기는 사람이 터미널에서(`yes`), 도는 host·agent는 `aaa service restart`로 다시 띄웁니다. 사람이 `aaa safety set <프로젝트> coordinator_deploy on`을 켠 뒤 사용자 채팅 지시를 받은 coordinator는 `--machine`으로 update·rollback·restart를 직접 실행하고 `aaa update status <ID> --wait`로 확인합니다([절차](docs/install-runbook.md#coordinator가-요청받은-배포를-실행하기10-19)) |
| `aaa pane list` · `aaa pane read <pane>` | pane 목록 · 최근 출력 글(`--source visible`이면 지금 화면) |
| `aaa tab create --machine mac-b -- <명령>` | 새 탭에 새 pane |
| `aaa pane run <pane> "…"` | 명령 실행(글 + Enter, 사람의 attach에 막히지 않음) |
| `aaa pane send-text <pane> "…"` · `aaa pane send-keys <pane> enter` | 글만 · 키만 보내기 |
| `aaa pane wait-output <pane> --match "…" --timeout 30000` | 출력에 글이 나올 때까지 기다리기 |
| `aaa question list` · `aaa question answer <id> yes` | 확인창 질문 보기·답하기 |
| `aaa project create <p> --goal "…" --repo <경로>[@머신] --thread-profile codex` · `aaa context <p>` | 프로젝트 만들기 · coordinator 요약(할 일·열린 thread·inbox) |
| `aaa project set <p> <key> <값>` | project.toml 값 하나 바꾸기(goal·thread_profile·coordinator_profile·max_parallel_threads·auto_resolve_days·repos.add·repos.remove) |
| `aaa project open <p> [--profile 이름] [--new]` | coordinator 열기: 돌고 있으면 그 pane을 알리고, 없으면 host 머신에서 프로젝트 폴더로 띄워 workspace `<p>`의 탭 coordinator에 둠(profile 기본: project.toml → `aaa profile default coordinator` → claude) |
| `aaa project pause\|resume\|archive\|unarchive <p>` | 멈추기(ticker·thread start 멈춤) · 보관(목록에서 숨기고 pane을 닫음) |
| `aaa project rename <p> <새 이름>` · `aaa project delete <p>` · `aaa project sweep <p>` | 이름 바꾸기 · 휴지통으로(worktree·branch는 그대로) · 끝난 thread가 남긴 worktree·병합된 branch 치우기 |
| `aaa doctor files [--fix]` | 모든 프로젝트의 coordinator 안내(AGENTS.md)·uploads/·pr-followup routine 보기 · 낡은 안내를 지금 aaa 것으로 다시 쓰기(사람이 쓴 파일은 `.before-aaa`로 옮겨 둠) |
| `aaa thread start <p> --title "…" --machine mac-b --profile codex --kind dir --task-file task.md` | thread 시작 |
| `aaa thread adopt <p> --pane <ID> --title "…"` | 이미 띄운 Claude·Codex pane(셸에서 친 것은 감지된 뒤)을 thread로 데려오기(pane은 그대로, 다시 시작·resolve가 pane을 끝내지 않음) |
| `aaa thread list <p>` · `aaa thread show <p> <id>` | thread 목록 · 보고·Next 보기 |
| `aaa thread ack <p> <id>` · `aaa thread resolve <p> <id>` | 보고 확인 · 끝내기(worktree 정리) |
| `aaa inbox list <p>` · `aaa inbox done <p> <id>` | 보고·질문·끊김 같은 놓치면 안 되는 항목 |
| `aaa routine list <p>` · `aaa routine toggle <p> <이름> --off` | 일정 작업과 PR 사건 routine(`on = "pr"`: thread PR의 check 실패·리뷰에 thread로 글을 보낸다) |

모든 명령은 `--json`으로 기계가 읽는 출력을 줍니다. `pane`·`tab`·`workspace`는 늘 JSON 한 줄이고, 실패하면 stderr에 `{"error":…}`를 쓰고 종료 코드 1입니다(herdr와 같다). 셸 완성은 `aaa completion zsh`(bash·fish·elvish·powershell)입니다. 자세한 것은 `aaa <명령> --help`입니다.

## 개발

CI(`.github/workflows/ci.yml`)와 같은 단계를 로컬에서 한 번에 돈다. 단계의 명령은 `scripts/ci-local.sh`에만 있고 CI도 이 스크립트를 단계별로 부른다(로컬과 CI의 명령이 어긋나지 않는다).

```sh
bash scripts/ci-local.sh                     # 전부: web fmt build clippy test e2e public
bash scripts/ci-local.sh --only test,e2e     # 고른 단계만
bash scripts/ci-local.sh --skip e2e          # 빼고 전부
```

- 끝에 단계마다 시작·끝 시각, 걸린 시간, 결과를 요약한다. 실패한 단계는 로그 위치를 보인다(기본 `target/ci-local/<시각>/<단계>.log`). 하나라도 실패하면 1로 끝난다. 실패를 다시 돌려 통과시키지 않는다.
- web·fmt·build를 차례로 돈 뒤 서로 기대지 않는 `test`·`e2e`·`clippy`는 함께 돈다(test가 가장 긴 길이다). `--tee`이거나 그중 하나만 고르면 차례로 돈다.
- `e2e`는 웹 E2E(Playwright Chromium·WebKit, 한글 IME 기록 재생 포함)다. Unix만이고 playwright를 전역 npm 모듈로 찾는다: `npm install -g playwright@1.59.1 && npx playwright@1.59.1 install chromium webkit`.
- `public`은 공개 저장소 검사(`scripts/check-public.sh`)다. CI와 push 전 로컬에서 검사한다.

## 라이선스

aaa 코드는 [MIT](LICENSE)입니다.

웹 화면에 함께 넣은 npm 패키지(긴 소스 보기의 `@pierre/diffs`·`@pierre/theme`(Apache-2.0)과 Shiki(MIT)·`diff`(BSD-3-Clause), React·xterm 등)의 라이선스 고지는 웹 빌드가 모아 `web/dist/licenses/THIRD_PARTY_NOTICES.txt`에 둡니다(`web/scripts/license-notices.ts`). host는 그것을 웹 주소 `/licenses/THIRD_PARTY_NOTICES.txt`로 보냅니다.

웹 터미널에는 [Meslotendard](https://github.com/cLazyZombie/Meslotendard) 0.3.1 글꼴(Regular·Bold, WOFF2)을 함께 넣었습니다. 이 글꼴은 SIL Open Font License 1.1이고, 구성 원본 Meslo LG(Apache 2.0)·Nerd Fonts·Pretendard(OFL 1.1)의 고지는 [web/public/fonts/meslotendard-0.3.1/](web/public/fonts/meslotendard-0.3.1/)에 있습니다. host는 같은 고지를 웹 주소 `/fonts/meslotendard-0.3.1/`로도 보냅니다.
