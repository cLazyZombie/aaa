# 사용법

웹 화면과 CLI를 쓰는 법이다. 설치는 [README](../README.md#mac에-설치)와 [운영 설치 점검표](install-runbook.md)에 있다.

## 웹

- **목록**: 왼쪽 활동 막대(VS Code처럼)의 단추가 사이드바에 패널 하나를 보입니다. 같은 단추를 다시 누르면 사이드바가 접히고(`⌘⇧B`, Windows·Linux `Ctrl+Shift+B`), 고른 패널과 접힘은 기기마다 기억합니다. 휴대폰에서는 화면 머리의 「목록 열기」로 여는 drawer입니다.
  - **작업**: 맨 위 coordinator, 그 아래 작업 공간(thread가 일하는 저장소 이름) 폴더마다 thread(모든 머신의 thread를 한 폴더에, 둘째 줄에 상태 말과 머신 이름, 첫 줄 끝에 배경 작업 수·계획 진행), 저장소 없는 thread의 「작업 폴더」, thread에 딸리지 않은 pane의 「기타」 묶음. thread 줄도 ⋯ 메뉴로 맨 위에 고정합니다. 배지는 확인 필요·검토 대기 thread와 답을 기다리는 agent 수입니다. 설정 → 모양 「폴더 안 정렬」에서 활동순(질문 먼저, 그다음 최근 바뀐 것)으로 바꿀 수 있습니다.
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
- **대화 그림을 할 일로 넘기기**: 프로젝트 대화창과 할 일 첨부는 저장한 파일 내용으로 이름을 정합니다. 메모의 `![그림](uploads/…)`는 thread 시작 때 작업 폴더로 복사됩니다. 예전 대화창이 무작위 이름으로 저장한 그림은 「이름과 내용 해시가 다르다」로 거부될 수 있습니다. 원본을 확인한 뒤 할 일에 다시 첨부하고 메모의 옛 링크를 새 링크로 바꾸세요. 원본 파일은 지우거나 이름을 바꾸지 않습니다.
- **머신 등록**: 설정 → 머신에서 「등록 창 열기」 뒤 새 머신에서 `aaa agent enroll <host 주소> --name <이름>`을 실행합니다. 대기 목록의 「승인…」에 새 머신 화면의 6자리 코드를 넣고 이름을 맞춰 봅니다(세 번 틀리면 그 요청을 버립니다). 등록된 머신은 같은 곳에서 폐기합니다. host 머신은 폐기하지 않습니다.
- **웹 공개**: 설정 → 폰 및 기기의 「tailnet에 공개」·「공개 끄기」(tailscale serve). 공개 주소로 연 화면에서 끄면 그 화면이 끊길 수 있어, 끈 뒤 들어올 다른 경로를 보이고 「다른 경로를 확인했다」를 켜야 끕니다.
- **안전·profile**: 설정 → 안전에서 safety 값과 출처를 보고 yolo·start_threads·trust_screens·coordinator_deploy를 바꿉니다(yolo 켜기는 한 번 더 확인). 설정 → profile에서 claude·codex·shell profile을 더하고 고치고 지우며, 허용 목록·기본 profile을 바꿉니다. 모두 「적용할까요?」 창에서 실제 실행 인자를 본 뒤 씁니다. custom profile(실행 명령)과 `thread_agent_args`·`coordinator_agent_args`·`routine_commands`는 host의 aaa 세션 밖 터미널에서만 바꿉니다. 그사이 host.toml이 바뀌었으면 쓰지 않고 다시 읽습니다.
- **작업 폴더·hook**: 설정 → 머신 아래 「작업 폴더·hook」에서 머신을 고르고, 절대 경로를 「점검…」(검사 파일을 쓰고 읽고 지움) 한 뒤 「전환…」합니다. 다음 agent 시작부터 쓰고 지금 도는 thread는 그대로입니다. Codex 전역 hook은 같은 곳에서 설치·제거합니다(Claude는 실행마다 넣어 설치할 것이 없습니다).
- **알림 켜기**: 설정 → 알림에서 켜고 「테스트 알림」으로 확인합니다. 기기 알림(Web Push)은 coordinator가 보낸 「확인 필요」·「전체 완료」 post만 받습니다. thread 질문·보고는 앱 안 알림으로 확인합니다. iPhone은 홈 화면 앱(Safari 공유 → 홈 화면에 추가)에서만 켤 수 있습니다.

보고서 발행(`library publish --to-user`)의 「새 보고서」 알림은 post 한도를 쓰지 않습니다. host가 확인한 프로젝트 coordinator의 「확인 필요」(`user-action`)·「전체 완료」(`task-complete`)는 합쳐서 분당 30·시간당 60·하루 300건이며 자동 음소거되지 않습니다(24시간 같은 글 중복은 거부). 일반 post 음소거는 다음 coordinator CLI 출력·`aaa context` 맨 위와 웹 앱 안 알림에 표시됩니다. 원인을 확인한 뒤 조작 권한으로 `aaa post unmute <출처>`를 실행합니다.

## 삭제된 기능: TUI

2026-10-19부터 TUI 코드·의존성·전용 시험을 삭제했습니다. 화면은 웹 UI를 씁니다. host에서 `aaa web status`로 주소를 확인합니다. 옛 `aaa attach`와 TUI front 접속도 삭제 안내로 거절합니다.

삭제 직전 main은 annotated tag `tui-last`로 보존합니다. 이유·삭제 커밋·복구 방법·삭제 목록은 [삭제 기록](removed-tui.md)에 있습니다.

## 기기·머신 신뢰와 웹 공개 권한

pairing 코드 발급, 머신 등록 창 열기·승인·폐기, 기기 폐기, 웹 공개 켜기·끄기는 aaa pane 안에서 실행할 수 없다. coordinator도 포함한다. host는 `local:…@…`·worker·evaluator·detached actor를 요청 실행 전에 거부하며, 사람이 aaa 밖 터미널이나 웹에서 하도록 안내한다. aaa 밖 Local은 기존처럼 사용할 수 있다. 웹 Controller는 pairing·기기 폐기와 함께 머신 등록 창·승인·폐기, 웹 공개 켜기·끄기, 보고서 올리기·공유 링크 키 교체를 한다(FRONT_API 1.80). 웹은 마지막 조작 기기와 host 머신을 폐기하지 않는다: 마지막 조작 기기는 다른 조작 기기를 먼저 페어링하거나 host 터미널의 `aaa web revoke`로, host 머신은 폐기 대상이 아니다. Reader는 목록만 본다.

coordinator의 상태 조회·post·일반 thread 관리·허용된 배포 경로는 이 제한의 대상이 아니다. 같은 OS 사용자 권한의 agent가 pane 밖 터미널을 흉내 내거나 host.toml을 직접 고치는 한계는 남는다. 이 제한은 OS 사용자 격리를 제공하지 않는다.

## 자주 쓰는 CLI

host는 프로젝트 하나만 맡으므로 명령의 프로젝트 인자는 생략한다(§8.285). 옛 꼴(`aaa thread show <p> <id>`처럼 앞에 프로젝트)도 한 판 동안 받는다: 이 host의 프로젝트와 같은 이름이면 표준 오류에 경고 한 줄을 남기고 그대로 하고, 다른 이름이면 거부한다. `post`(coordinator·사람의 셸 pane은 Front로, thread pane은 hook으로)·`safety`·`profile list --project`도 생략한다(§8.288). `project pause|resume|sweep|archive|delete`도 이름 없이 쓰고, `project rename <새 이름>`은 새 이름만 준다(S5). 보관한 프로젝트나 여럿이 남은 옛 상태의 이름은 옛 꼴로 지정할 수 있다. 보관을 풀 때는 `project unarchive <이름>`을 쓴다. 프로젝트 설정은 홈의 `aaa.toml`(`[[workspaces]]`)이 있으면 그것을, 없으면 `project.toml`(`[[repos]]`)을 읽고 쓴다. aaa는 aaa.toml을 만들지 않는다.

| 명령 | 하는 일 |
|---|---|
| `aaa --instance <이름> <명령>` · `aaa --instance <이름> init` | 같은 Mac의 둘째 aaa를 쓰기(env `AAA_INSTANCE`도 같다, pane 안에서는 물려받는다) · 만들기(폴더·겹치지 않는 포트·홈, 사람이 터미널에서). 이름이 없으면 지금 aaa다([절차](install-runbook.md#12-같은-mac의-둘째-aaa8294)) |
| `aaa machine list` | 머신 연결·디스크·이상 |
| 웹 → 머신 → 배포 카드 | 최신 배포의 전송·단계·실패·복구를 Reader도 봅니다. 단절이면 마지막 수신 상태이며 재연결 뒤 최신 기록을 받습니다. 검증 정보에서 전체 ID/SHA를 봅니다. 카드에서는 배포를 시작하지 않습니다. |
| `aaa service restart\|stop\|start --role host\|agent` | 감독자(launchd)로 다시 띄우기(업그레이드 뒤) · 멈추기 · 띄우기. 사람이 터미널에서(`yes`). restart는 아래 coordinator 배포 허용도 가능 |
| `aaa deploy start --file <머신>:<후보폴더>/aaa --sha256 <SHA> [--commit <40자리>] [--machines all 또는 쉼표 목록]` · `aaa deploy status <ID> --wait` | coordinator safety 아래 빌드 후보를 Agent link로 전송하고 여러 Mac을 host부터 배포합니다. source/모든 대상에 deploy_stage.v1, 모든 대상에 rollout_recovery.v1이 필요합니다. 전체 전송·SHA/self-check 뒤 첫 교체를 하며 같은 ID로 재개합니다. 기존 미리 복사 경로는 --from입니다. 실패 머신만 배포 전 SHA로 자동 복구하며 recovering 뒤 stopped_rolled_back 또는 recovery_failed를 남깁니다. 앞 머신은 유지합니다. |
| `aaa update --from <파일> [--dry-run]` · `aaa update rollback [이름]` · `aaa update list` | 지금 실행 파일을 주어진 빌드로 바꾸기(self-check·`[update] version` 고정 검사 뒤 rename, 옛것은 옆 `.aaa-old/`에 `[update] keep`개) · 보관한 것으로 되돌리기 · 보관 목록. 바꾸기·되돌리기는 사람이 터미널에서(`yes`), 도는 host·agent는 `aaa service restart`로 다시 띄웁니다. 사람이 `aaa safety set coordinator_deploy on`을 켠 뒤 사용자 채팅 지시를 받은 coordinator는 `--machine`으로 update·rollback·restart를 직접 실행하고 `aaa update status <ID> --wait`로 확인합니다([절차](install-runbook.md#coordinator가-요청받은-배포를-실행하기10-19)) |
| `aaa pane list` · `aaa pane read <pane>` | pane 목록 · 최근 출력 글(`--source visible`이면 지금 화면) |
| `aaa tab create --machine mac-b -- <명령>` | 새 탭에 새 pane |
| `aaa pane run <pane> "…"` | 명령 실행(글 + Enter, 사람의 attach에 막히지 않음) |
| `aaa pane send-text <pane> "…"` · `aaa pane send-keys <pane> enter` | 글만 · 키만 보내기 |
| `aaa pane wait-output <pane> --match "…" --timeout 30000` | 출력에 글이 나올 때까지 기다리기 |
| `aaa question list` · `aaa question answer <id> yes` | 확인창 질문 보기·답하기 |
| `aaa project create <p> --goal "…" --repo <경로>[@머신] --thread-profile codex` · `aaa context` | 프로젝트 만들기 · coordinator 요약(할 일·열린 thread·inbox). host는 보관하지 않은 프로젝트 하나만 맡는다: 둘째 만들기는 거부하고(§8.285), 둘 이상이면 다시 띄울 때 시작하지 않고 줄일 목록을 말한다(§8.283). 인스턴스에서는 이름을 생략하면 인스턴스 이름이다(§8.294) |
| `aaa set <key> <값>` | 설정 값 하나 바꾸기(aaa.toml, 없으면 project.toml: goal·thread_profile·coordinator_profile·max_parallel_threads·auto_resolve_days·repos.add·repos.remove, workspaces.add·remove도 같다, task_notes.add·remove: 할 일 화면 「나중」 탭이 읽는 프로젝트 폴더 안 Markdown) |
| `aaa task list` | 할 일 목록(TASKS.md) 읽기: 목록별 task, 형식 어긋남, task 없는 열린 thread, 「나중」 파일. 웹은 활동 막대 「할 일」(§8.306) |
| `aaa task add "<제목>" [--list 목록] [--profile 모델] [--machine 머신] [--owner me\|이름] [--repo 저장소]` · `aaa task edit "<제목>" [--title …] [--profile …] [--machine …] [--owner …] [--repo …] [--thread t-NNNN]` · `aaa task move\|rm "<제목>"` · `aaa task restore <기록 id>` · `aaa task log` | 할 일 고치기: 바뀐 task 줄만 다시 쓰고 나머지 줄은 그대로 둔다. `--block <sha256>`이면 그 판의 블록으로 고정하고, 그사이 바뀌었으면 쓰지 않는다(충돌). 지운 task는 기록에서 되살린다. `--repo`는 저장소 칸(project.toml repos 경로나 그 마지막 이름, 또는 `작업 폴더`, §8.330)이고 `--from-task`가 그 저장소로 띄운다. `--profile`·`--machine`은 agent에게 맡길 모델·머신 칸이고(§8.335, 줄에는 `(모델@머신)`으로 적는다) 칸마다 따로 고친다(빈 글이면 그 칸만 비운다). 사람의 할 일은 `--owner me`(사람 이름은 `--person`)이다. 목록은 `aaa task list-add\|list-rename\|list-merge\|list-rm\|list-move`. aaa 세션 안에서는 그 프로젝트 coordinator만 고친다. 웹은 할 일 화면의 폼·⋯ 메뉴·「기록」 탭(§8.311) |
| `aaa coordinator open [--profile 이름] [--new]` | coordinator 열기: 돌고 있으면 그 pane을 알리고, 없으면 host 머신에서 프로젝트 폴더로 띄워 workspace `<p>`의 탭 coordinator에 둠(profile 기본: project.toml → `aaa profile default coordinator` → claude) |
| `aaa project pause\|resume\|archive` · `aaa project unarchive <p>` | 멈추기(ticker·thread start 멈춤) · 보관(목록에서 숨기고 pane을 닫음). 보관 해제는 다른 활성·멈춤 프로젝트가 없을 때만 가능 |
| `aaa project rename <새 이름>` · `aaa project delete` · `aaa project sweep` | 이름 바꾸기 · 휴지통으로(worktree·branch는 그대로) · 끝난 thread가 남긴 worktree·병합된 branch 치우기 |
| `aaa doctor files [--fix]` | 모든 프로젝트의 coordinator 안내(AGENTS.md)·uploads/·pr-followup 규칙 보기(TOML 변환본이 있으면 옛 routine을 다시 만들지 않음) · 낡은 안내를 지금 aaa 것으로 다시 쓰기(사람이 쓴 파일은 `.before-aaa`로 옮겨 둠) |
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
(명시한 `file_roots`는 그대로 적용한다). 로컬 전환은 다음 명령으로 준비한다. 디스크를 강제로 분리하면 실행 중인 프로그램의 열린 파일까지 복구해 주지는 않는다.


```sh
aaa work-root show
aaa work-root check --path '/Volumes/<disk>/aaa-work' --mount '/Volumes/<disk>'
aaa work-root set '/Volumes/<disk>/aaa-work' --mount '/Volumes/<disk>'
```

`show`는 다음 시작의 설정값·마지막 agent 시작 기록·이전 root와 재시작 필요 여부를 보여 준다.
마지막 시작 기록은 지금 host와 연결됐다는 증명이 아니므로 연결도 따로 확인한다.
`check`는 설정을 바꾸지 않고 실제 mount와 쓰기·읽기·삭제를 점검하며, 없는 작업 root는 만든다.
`set`은 기존 root의 준비 기록을 먼저 가져오고 이전 root·소유권·외장 Git 잠금을 보존한 뒤
`agent.local.toml`의 두 key만 원자적으로 저장한다. 주석·다른 설정은 유지한다.
같은 명령을 다시 실행할 수 있고, 등록 완료 뒤 저장 전에 중단되어도 기존 설정과 원본 작업을 보존한다.
읽기 전용·symlink·hardlink·잘못된 설정은 덮지 않으며, 준비 도중 다른 편집자가 바꾼 설정도 덮지 않는다.

이 명령들은 이 머신의 `--config-dir`·`--state-dir`(또는 해당 환경 변수)을 사용하고 서비스를 재시작하지 않는다.
새 요청을 잠깐 멈추고 진행 중 준비·정리의 종료를 확인한 뒤 `check` → `set` → `config check`를 실행한다.
그 뒤 운영 담당자가 agent를 한 대씩 재시작하고 host의 연결과 새 thread의 실제 cwd를 확인한다.
기존 pane·cwd·파일은 이동하지 않으며, 새 thread부터 변경한 경로를 사용한다.
분리 중 실패한 resolve는 볼륨을 연결한 뒤 같은 `thread resolve`로 이어 간다. `project sweep`은 끝낸 thread가 남긴 자리용이다.

웹 활동 막대 「자동화」의 조건·동작·실행 기록·토글은 [자동화 운영 확인](automations.md)을 본다.

사람은 aaa 밖 host 터미널에서 `aaa deploy start --file mac-b:~/aaa-candidate/aaa --sha256 <SHA> --commit <40자리커밋>`의 후보/대상을 읽고 정확히 `yes`를 입력한다(FRONT1.55 이상). coordinator_deploy 설정은 바꾸지 않는다. 같은 `aaa deploy status <ID> --wait`로 재시작을 넘는다. TOML `do="deploy"`는 배포 제안과 빌드 안내만 만든다([자동화](automations.md#배포는-제안까지만l5)).
