# 사용법

웹 화면과 CLI를 쓰는 법이다. 설치는 [README](../README.md#mac에-설치)와 [운영 설치 점검표](install-runbook.md)에 있다.

## 웹

- **목록**: 왼쪽 활동 막대(VS Code처럼)의 단추가 사이드바에 패널 하나를 보입니다. 같은 단추를 다시 누르면 사이드바가 접히고(`⌘⇧B`, Windows·Linux `Ctrl+Shift+B`), 고른 패널과 접힘은 기기마다 기억합니다. 휴대폰에서는 화면 머리의 「목록 열기」로 여는 drawer입니다.
  - **작업**: 맨 위 coordinator, 그 아래 작업 공간(thread가 일하는 저장소 이름) 폴더마다 thread(모든 머신의 thread를 한 폴더에, 둘째 줄에 상태 말과 머신 이름, 첫 줄 끝에 배경 작업 수·계획 진행), 저장소 없는 thread의 「작업 폴더」, thread에 딸리지 않은 pane의 「기타」 묶음. 프로젝트가 둘 이상이면 그 위에 프로젝트 머리가 섭니다. thread 줄도 ⋯ 메뉴로 맨 위에 고정합니다. 배지는 확인 필요·검토 대기 thread와 답을 기다리는 agent 수입니다. 설정 → 모양 「폴더 안 정렬」에서 활동순(질문 먼저, 그다음 최근 바뀐 것)으로 바꿀 수 있습니다.
  - **머신**: 머신마다 연결·끊김과 그 머신의 pane, 새 pane `+`.
  - **특수**: 임무와 사후 평가자(임무 thread는 여기에만 보입니다).
  - 그 아래 **보고서** 단추는 본문을 보고서 목록으로 바꾸고(다시 누르면 앞 화면), 맨 아래 톱니는 설정입니다.
  - 상태는 작업 중·입력 대기·질문·검토 대기·확인 필요·쉼이고, 질문은 터미널 아래 카드로 답합니다.
- **새 pane**: 머신 패널의 머신 이름 옆 `+`나 `⌘⇧O`(Windows·Linux `Ctrl+Shift+O`)로 창을 엽니다. 셸이나 그 머신에서 쓸 수 있는 profile, 폴더(절대 경로나 `~/…`, 최근 폴더), 이름을 고르고 「시작」합니다.
- **터미널**: pane을 누르면 터미널이 열립니다. 직접 쳐도 되고, 아래 **입력 막대(composer)** 를 써도 됩니다.
  - Enter는 보내기, Shift+Enter는 줄바꿈입니다. 휴대폰에서는 Enter가 줄바꿈이고 보내기는 단추입니다.
  - Ctrl+Enter는 줄바꿈 없이 harness steering입니다. 입력 막대나 대화 입력창에 글이 있으면 그 글을 보냅니다. agent가 작업 중이면 지금 턴에 넣고(Claude는 글 뒤 Ctrl+X Ctrl+S, Codex는 글 뒤 Enter), 쉬는 중이면 보내기와 같습니다. 보내기처럼 반영될 때까지 입력창이 잠깁니다. 글이 없으면 작업 중일 때만 대기 메시지를 지금 보내고(Claude Ctrl+X Ctrl+S, Codex 대기 steer는 Esc), 쉬는 중이거나 입력 상자를 알아보지 못하면 아무것도 보내지 않습니다.
  - 키 줄은 Esc·Tab·Ctrl·Alt·방향키 등입니다. 그림은 붙여 넣거나 끌어 놓습니다.
- **크기와 입력(가져오기 없음)**: 보기 권한이 아니면 언제나 입력됩니다. PTY 크기는 「지금 쓰는 화면」이 정합니다. 입력하거나 터미널을 누르거나 창 focus를 얻은 화면이 자기 크기로 다시 겁니다. 탭이 1초 넘게 가려지면 놓습니다. 「화면에 맞추기」로 직접 맞출 수도 있습니다.
- **대화**: Claude·Codex pane은 「대화」로 기록을 읽기 전용으로 봅니다. 휴대폰은 agent pane을 처음부터 대화로 엽니다(설정 → 채팅 「패널 기본 화면」). 「생각 표시」를 켜면 agent의 생각도 접힌 줄로 보입니다. 대화 아래 입력창에서 바로 보내고, 「대기 메시지 자동 전송」을 끄면 작업 중에 쓴 글은 「지금 보내기」까지 붙잡아 둡니다. 빠른 답장 칩도 켤 수 있습니다.
- **알림 켜기**: 설정 → 알림에서 켜고 「테스트 알림」으로 확인합니다. 기기 알림(Web Push)은 coordinator가 보낸 「확인 필요」·「전체 완료」 post만 받습니다. thread 질문·보고는 앱 안 알림으로 확인합니다. iPhone은 홈 화면 앱(Safari 공유 → 홈 화면에 추가)에서만 켤 수 있습니다.

보고서 발행(`library publish --to-user`)의 「새 보고서」 알림은 post 한도를 쓰지 않습니다. host가 확인한 프로젝트 coordinator의 「확인 필요」(`user-action`)·「전체 완료」(`task-complete`)는 합쳐서 분당 30·시간당 60·하루 300건이며 자동 음소거되지 않습니다(24시간 같은 글 중복은 거부). 일반 post 음소거는 다음 coordinator CLI 출력·`aaa context` 맨 위와 웹 앱 안 알림에 표시됩니다. 원인을 확인한 뒤 조작 권한으로 `aaa post unmute <프로젝트> <출처>`를 실행합니다.

## 삭제된 기능: TUI

2026-10-19부터 TUI 코드·의존성·전용 시험을 삭제했습니다. 화면은 웹 UI를 씁니다. host에서 `aaa web status`로 주소를 확인합니다. 옛 `aaa attach`와 TUI front 접속도 삭제 안내로 거절합니다.

삭제 직전 main은 annotated tag `tui-last`로 보존합니다. 이유·삭제 커밋·복구 방법·삭제 목록은 [삭제 기록](removed-tui.md)에 있습니다.

## 자주 쓰는 CLI

host는 프로젝트 하나만 맡으므로 명령의 프로젝트 인자는 생략한다(§8.285). 옛 꼴(`aaa thread show <p> <id>`처럼 앞에 프로젝트)도 한 판 동안 받는다: 이 host의 프로젝트와 같은 이름이면 표준 오류에 경고 한 줄을 남기고 그대로 하고, 다른 이름이면 거부한다. `post`·`safety`·`profile --project`·`project pause|sweep`은 아직 프로젝트를 받는다(S2).

| 명령 | 하는 일 |
|---|---|
| `aaa machine list` | 머신 연결·디스크·이상 |
| `aaa service restart\|stop\|start --role host\|agent` | 감독자(launchd)로 다시 띄우기(업그레이드 뒤) · 멈추기 · 띄우기. 사람이 터미널에서(`yes`). restart는 아래 coordinator 배포 허용도 가능 |
| `aaa deploy start --file <머신>:<후보폴더>/aaa --sha256 <SHA> [--commit <40자리>] [--machines all 또는 쉼표 목록]` · `aaa deploy status <ID> --wait` | coordinator safety 아래 빌드 후보를 Agent link로 전송하고 여러 Mac을 host부터 배포합니다. source/모든 대상에 deploy_stage.v1이 필요합니다. 전체 전송·SHA/self-check 뒤 첫 교체를 하며 같은 ID로 재개합니다. 기존 미리 복사 경로는 --from입니다. 자동 rollback은 L3입니다. |
| `aaa update --from <파일> [--dry-run]` · `aaa update rollback [이름]` · `aaa update list` | 지금 실행 파일을 주어진 빌드로 바꾸기(self-check·`[update] version` 고정 검사 뒤 rename, 옛것은 옆 `.aaa-old/`에 `[update] keep`개) · 보관한 것으로 되돌리기 · 보관 목록. 바꾸기·되돌리기는 사람이 터미널에서(`yes`), 도는 host·agent는 `aaa service restart`로 다시 띄웁니다. 사람이 `aaa safety set <프로젝트> coordinator_deploy on`을 켠 뒤 사용자 채팅 지시를 받은 coordinator는 `--machine`으로 update·rollback·restart를 직접 실행하고 `aaa update status <ID> --wait`로 확인합니다([절차](install-runbook.md#coordinator가-요청받은-배포를-실행하기10-19)) |
| `aaa pane list` · `aaa pane read <pane>` | pane 목록 · 최근 출력 글(`--source visible`이면 지금 화면) |
| `aaa tab create --machine mac-b -- <명령>` | 새 탭에 새 pane |
| `aaa pane run <pane> "…"` | 명령 실행(글 + Enter, 사람의 attach에 막히지 않음) |
| `aaa pane send-text <pane> "…"` · `aaa pane send-keys <pane> enter` | 글만 · 키만 보내기 |
| `aaa pane wait-output <pane> --match "…" --timeout 30000` | 출력에 글이 나올 때까지 기다리기 |
| `aaa question list` · `aaa question answer <id> yes` | 확인창 질문 보기·답하기 |
| `aaa project create <p> --goal "…" --repo <경로>[@머신] --thread-profile codex` · `aaa context` | 프로젝트 만들기 · coordinator 요약(할 일·열린 thread·inbox). host는 보관하지 않은 프로젝트 하나만 맡는다: 둘째 만들기는 거부하고(§8.285), 둘 이상이면 다시 띄울 때 시작하지 않고 줄일 목록을 말한다(§8.283) |
| `aaa set <key> <값>` | project.toml 값 하나 바꾸기(goal·thread_profile·coordinator_profile·max_parallel_threads·auto_resolve_days·repos.add·repos.remove) |
| `aaa coordinator open [--profile 이름] [--new]` | coordinator 열기: 돌고 있으면 그 pane을 알리고, 없으면 host 머신에서 프로젝트 폴더로 띄워 workspace `<p>`의 탭 coordinator에 둠(profile 기본: project.toml → `aaa profile default coordinator` → claude) |
| `aaa project pause\|resume\|archive\|unarchive <p>` | 멈추기(ticker·thread start 멈춤) · 보관(목록에서 숨기고 pane을 닫음) |
| `aaa project rename <p> <새 이름>` · `aaa project delete <p>` · `aaa project sweep <p>` | 이름 바꾸기 · 휴지통으로(worktree·branch는 그대로) · 끝난 thread가 남긴 worktree·병합된 branch 치우기 |
| `aaa doctor files [--fix]` | 모든 프로젝트의 coordinator 안내(AGENTS.md)·uploads/·pr-followup routine 보기 · 낡은 안내를 지금 aaa 것으로 다시 쓰기(사람이 쓴 파일은 `.before-aaa`로 옮겨 둠) |
| `aaa thread start --title "…" --machine mac-b --profile codex --kind dir --task-file task.md` | thread 시작 |
| `aaa thread adopt --pane <ID> --title "…"` | 이미 띄운 Claude·Codex pane(셸에서 친 것은 감지된 뒤)을 thread로 데려오기(pane은 그대로, 다시 시작·resolve가 pane을 끝내지 않음) |
| `aaa thread list` · `aaa thread show <id>` | thread 목록 · 보고·Next 보기 |
| `aaa thread ack <id>` · `aaa thread resolve <id>` | 보고 확인 · 끝내기(worktree 정리) |
| `aaa inbox list` · `aaa inbox done <id>` | 보고·질문·끊김 같은 놓치면 안 되는 항목 |
| `aaa routine list` · `aaa routine runs <이름>` · `aaa routine toggle <이름> --off` | 일정 작업과 PR 사건 routine(`on = "pr"`: thread PR의 check 실패·리뷰에 thread로 글을 보낸다) |

모든 명령은 `--json`으로 기계가 읽는 출력을 줍니다. `pane`·`tab`·`workspace`는 늘 JSON 한 줄이고, 실패하면 stderr에 `{"error":…}`를 쓰고 종료 코드 1입니다(herdr와 같다). 셸 완성은 `aaa completion zsh`(bash·fish·elvish·powershell)입니다. 자세한 것은 `aaa <명령> --help`입니다.
## 머신별 작업 디스크

각 머신의 `agent.toml` 또는 `agent.local.toml`에서 `[agent] work_root`를 정한다. 비우면 기존 `~/aaa-work`를
사용한다. 절대 경로와 `~/…`만 받으며 상대 경로·상위 탈출·디스크 최상위는 거부한다.

```toml
[agent]
work_root = "/Volumes/<disk>/aaa-work"
work_mount = "/Volumes/<disk>"
```

`work_mount`는 실제 마운트 지점이어야 한다. macOS `/Volumes/<볼륨>/…`는 이 값을 생략해도 검사한다.
동명의 일반 폴더만 남아 있거나 볼륨이 없으면 agent 시작을 거부하고 내장 경로로 대체하지 않는다. 실행 중에도
새 작업·기본 routine 폴더 생성·재실행·정리 전에 대상 볼륨을 확인한다. 폴더가 없다는 이유로 다른 디스크의
여유 용량을 정상 보고하지 않는다. 설정 변경은 agent 재시작 때 적용된다.

준비 기록은 `<AAA_STATE_DIR>/agent/thread-preparations/`에, root 이력은 같은 agent 상태 폴더의
`work-roots.json`에 보관한다. 새 agent가 기존 root의 `.preparations/`를 원본 보존 상태로 한 번 가져온다.
기존 thread의 cwd와 파일은 옮기지 않으며, 응답을 잃은 준비의 restart·resolve와 sweep도 생성 당시 root를 따른다.
이전 볼륨을 읽을 수 없으면 없는 자리로 확정하거나 정리를 완료로 기록하지 않는다.

외장 worktree는 Git 잠금으로 자동 prune을 막는다. 정리는 aaa 소유 잠금만 풀고 대상 worktree 하나만 제거한다.
변경 파일·복사 누락·사람의 잠금이 있으면 남기며, 정리 중단 뒤에는 먼저 자기 잠금을 복구한다.
다른 agent 상태 폴더가 같은 root를 쓰려 하면 소유 표지로 거부한다. 기본 파일 허용 폴더에는 이전 root도 포함한다
(명시한 `file_roots`는 그대로 적용한다). 전용 전환 명령은 후속 단계이므로 운영 경로를 아직 바꾸지 않는다. 디스크를 강제로 분리하면 실행 중인 프로그램의 열린 파일까지 복구해 주지는 않는다.


웹 활동 막대 「자동화」의 조건·동작·실행 기록·토글은 [자동화 운영 확인](automations.md)을 본다.
