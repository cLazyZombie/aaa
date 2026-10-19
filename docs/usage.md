# 사용법

웹 화면과 CLI를 쓰는 법이다. 설치는 [README](../README.md#mac에-설치)와 [운영 설치 점검표](install-runbook.md)에 있다.

## 웹

- **목록**: 왼쪽 활동 막대(VS Code처럼)의 단추가 사이드바에 패널 하나를 보입니다. 같은 단추를 다시 누르면 사이드바가 접히고(`⌘⇧B`, Windows·Linux `Ctrl+Shift+B`), 고른 패널과 접힘은 기기마다 기억합니다. 휴대폰에서는 화면 머리의 「목록 열기」로 여는 drawer입니다.
  - **프로젝트**: 프로젝트마다 coordinator와 thread(모든 머신의 thread를 한 묶음에, 둘째 줄에 상태 말과 머신 이름), thread에 딸리지 않은 pane의 「기타」 묶음. 배지는 확인 필요·검토 대기 thread 수입니다.
  - **머신**: 머신마다 연결·끊김과 그 머신의 pane, 새 pane `+`.
  - **에이전트**: 모든 Claude·Codex pane. 배지는 답을 기다리는 agent 수입니다. 설정 → 모양 「에이전트 정렬」에서 활동순(질문 먼저, 그다음 최근 바뀐 것)으로 바꿀 수 있습니다.
  - **특수**: 임무와 사후 평가자.
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

## 삭제된 기능: TUI

2026-10-19부터 TUI 코드·의존성·전용 시험을 삭제했습니다. 화면은 웹 UI를 씁니다. host에서 `aaa web status`로 주소를 확인합니다. 옛 `aaa attach`와 TUI front 접속도 삭제 안내로 거절합니다.

삭제 직전 main은 annotated tag `tui-last`로 보존합니다. 이유·삭제 커밋·복구 방법·삭제 목록은 [삭제 기록](removed-tui.md)에 있습니다.

## 자주 쓰는 CLI

| 명령 | 하는 일 |
|---|---|
| `aaa machine list` | 머신 연결·디스크·이상 |
| `aaa service restart\|stop\|start --role host\|agent` | 감독자(launchd)로 다시 띄우기(업그레이드 뒤) · 멈추기 · 띄우기. 사람이 터미널에서(`yes`). restart는 아래 coordinator 배포 허용도 가능 |
| `aaa update --from <파일> [--dry-run]` · `aaa update rollback [이름]` · `aaa update list` | 지금 실행 파일을 주어진 빌드로 바꾸기(self-check·`[update] version` 고정 검사 뒤 rename, 옛것은 옆 `.aaa-old/`에 `[update] keep`개) · 보관한 것으로 되돌리기 · 보관 목록. 바꾸기·되돌리기는 사람이 터미널에서(`yes`), 도는 host·agent는 `aaa service restart`로 다시 띄웁니다. 사람이 `aaa safety set <프로젝트> coordinator_deploy on`을 켠 뒤 사용자 채팅 지시를 받은 coordinator는 `--machine`으로 update·rollback·restart를 직접 실행하고 `aaa update status <ID> --wait`로 확인합니다([절차](install-runbook.md#coordinator가-요청받은-배포를-실행하기10-19)) |
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
| `aaa routine list <p>` · `aaa routine runs <p> <이름>` · `aaa routine toggle <p> <이름> --off` | 일정 작업과 PR 사건 routine(`on = "pr"`: thread PR의 check 실패·리뷰에 thread로 글을 보낸다) |

모든 명령은 `--json`으로 기계가 읽는 출력을 줍니다. `pane`·`tab`·`workspace`는 늘 JSON 한 줄이고, 실패하면 stderr에 `{"error":…}`를 쓰고 종료 코드 1입니다(herdr와 같다). 셸 완성은 `aaa completion zsh`(bash·fish·elvish·powershell)입니다. 자세한 것은 `aaa <명령> --help`입니다.

웹 활동 막대 「자동화」의 조건·동작·실행 기록·토글은 [자동화 운영 확인](automations.md)을 본다.
