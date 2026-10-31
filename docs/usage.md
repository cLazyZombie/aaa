# 사용법

웹 화면과 CLI를 쓰는 법이다. 설치는 [README](../README.md#mac에-설치)와 [운영 설치 점검표](install-runbook.md)에 있다.

host DB의 정기 암호화 백업·보관 회전·새 파일 복원은 [host DB 백업](host-db-backup.md)을 따른다.

## 웹

- **목록**: 왼쪽 활동 막대(VS Code처럼)의 단추가 사이드바에 패널 하나를 보입니다. 같은 단추를 다시 누르면 사이드바가 접히고(`⌘⇧B`, Windows·Linux `Ctrl+Shift+B`), 고른 패널과 접힘은 기기마다 기억합니다. 휴대폰에서는 화면 머리의 「목록 열기」로 여는 drawer입니다. 첫 조회가 늦게 끝나 초기 화면을 자동 선택해도 사용자가 연 목록은 유지하며, 목록 항목을 직접 고르면 닫습니다.
  - **작업**: 맨 위 coordinator, 그 아래 작업 공간(thread가 일하는 저장소 이름) 폴더마다 thread(모든 머신의 thread를 한 폴더에, 둘째 줄에 상태 말과 머신 이름, 첫 줄 끝에 배경 작업 수·계획 진행), 저장소 없는 thread의 「작업 폴더」, thread에 딸리지 않은 pane의 「기타」 묶음. thread 줄도 ⋯ 메뉴로 맨 위에 고정합니다. 배지는 확인 필요·검토 대기 thread와 답을 기다리는 agent 수입니다. 설정 → 모양 「폴더 안 정렬」에서 활동순(질문 먼저, 그다음 최근 바뀐 것)으로 바꿀 수 있습니다.
  - **머신**: 머신마다 연결·끊김과 그 머신의 pane, 새 pane `+`.
  - **특수**: 임무와 사후 평가자(임무 thread는 여기에만 보입니다).
  - **아이디어**(전구): 다듬는 중·완료 아이디어 목록과 「새 아이디어」. 고르면 본문이 노트와 그 아이디어 전용 thread의 대화입니다([아이디어](#아이디어)).
  - 그 아래 **보고서** 단추는 본문을 보고서 목록으로 바꾸고(다시 누르면 앞 화면), 맨 아래 톱니는 설정입니다.
  - 상태는 작업 중·입력 대기·질문·검토 대기·확인 필요·쉼이고, 질문은 터미널 아래 카드로 답합니다.
- **새 pane**: 머신 패널의 머신 이름 옆 `+`나 `⌘⇧O`(Windows·Linux `Ctrl+Shift+O`)로 창을 엽니다. 셸이나 그 머신에서 쓸 수 있는 profile, 폴더(절대 경로나 `~/…`, 최근 폴더), 이름을 고르고 「시작」합니다.
- **최근 화면**: 설치한 PWA에서 Ctrl+Tab으로 방문한 화면을 최근 순서로 고르고, Ctrl+Shift+Tab으로 거꾸로 고릅니다. 아이디어·자동화·답할 것은 오른쪽에 연 노트·규칙·개별 카드만 기록하며, 왼쪽 목록과 예전에 저장된 목록 방문 기록은 전환 후보에 넣지 않습니다. Ctrl을 놓으면 이동하고 Esc로 취소합니다.
- **터미널**: pane을 누르면 터미널이 열립니다. 직접 쳐도 되고, 아래 **입력 막대(composer)** 를 써도 됩니다.
  - Enter는 보내기, Shift+Enter는 줄바꿈입니다. 휴대폰에서는 Enter가 줄바꿈이고 보내기는 단추입니다.
  - Ctrl+Enter는 줄바꿈 없이 harness steering입니다. 입력 막대나 대화 입력창에 글이 있으면 그 글을 보냅니다. agent가 작업 중이면 지금 턴에 넣고(Claude는 글 뒤 Ctrl+X Ctrl+S, Codex는 글 뒤 Enter), 쉬는 중이면 보내기와 같습니다. 보내기처럼 반영될 때까지 입력창이 잠깁니다. 글이 없으면 작업 중일 때만 대기 메시지를 지금 보내고(Claude Ctrl+X Ctrl+S, Codex 대기 steer는 Esc), 쉬는 중이거나 입력 상자를 알아보지 못하면 아무것도 보내지 않습니다.
  - 키 줄은 Esc·Tab·Ctrl·Alt·방향키 등입니다. 그림은 붙여 넣거나 끌어 놓습니다.
- **크기와 입력(가져오기 없음)**: 보기 권한이 아니면 언제나 입력됩니다. PTY 크기는 「지금 쓰는 화면」이 정합니다. 입력하거나 터미널을 누르거나 창 focus를 얻은 화면이 자기 크기로 다시 겁니다. 탭이 1초 넘게 가려지면 놓습니다. 「화면에 맞추기」로 직접 맞출 수도 있습니다.
- **대화**: Claude·Codex pane은 「대화」로 기록을 읽기 전용으로 봅니다. 휴대폰은 agent pane을 처음부터 대화로 엽니다(설정 → 채팅 「패널 기본 화면」). 「생각 표시」를 켜면 agent의 생각도 접힌 줄로 보입니다. 대화 아래 입력창에서 바로 보내고, 「대기 메시지 자동 전송」을 끄면 작업 중에 쓴 글은 「지금 보내기」까지 붙잡아 둡니다. 빠른 답장 칩도 켤 수 있습니다. 대화의 질문 링크 칩(「q-NNNN 질문 · 답할 것」)에는 질문 글과 선택지 단추가 붙어, 누르면 그 자리에서 답합니다(「답할 것」 카드로 답한 것과 같고, 칩은 「답함 · 1A」로 바뀝니다).
- **대화 그림을 할 일로 넘기기**: 프로젝트 대화창과 할 일 첨부는 저장한 파일 내용으로 이름을 정합니다. 메모의 `![그림](uploads/…)`는 thread 시작 때 작업 폴더로 복사됩니다. 예전 대화창이 무작위 이름으로 저장한 그림은 「이름과 내용 해시가 다르다」로 거부될 수 있습니다. 원본을 확인한 뒤 할 일에 다시 첨부하고 메모의 옛 링크를 새 링크로 바꾸세요. 원본 파일은 지우거나 이름을 바꾸지 않습니다.
- **머신 등록**: 설정 → 머신에서 「등록 창 열기」 뒤 새 머신에서 `aaa agent enroll <host 주소> --name <이름>`을 실행합니다. 대기 목록의 「승인…」에 새 머신 화면의 6자리 코드를 넣고 이름을 맞춰 봅니다(세 번 틀리면 그 요청을 버립니다). 등록된 머신은 같은 곳에서 폐기합니다. host 머신은 폐기하지 않습니다.
- **웹 공개**: 설정 → 폰 및 기기의 「tailnet에 공개」·「공개 끄기」(tailscale serve). 공개 주소로 연 화면에서 끄면 그 화면이 끊길 수 있어, 끈 뒤 들어올 다른 경로를 보이고 「다른 경로를 확인했다」를 켜야 끕니다.
- **안전·profile**: 설정 → 안전에서 safety 값과 출처를 보고 yolo·start_threads·trust_screens·coordinator_deploy를 바꿉니다(yolo 켜기는 한 번 더 확인). 설정 → profile에서 claude·codex·shell profile을 더하고 고치고 지우며, 허용 목록·기본 profile을 바꿉니다. 모두 「적용할까요?」 창에서 실제 실행 인자를 본 뒤 씁니다. custom profile(실행 명령)과 `thread_agent_args`·`coordinator_agent_args`·`routine_commands`는 host의 aaa 세션 밖 터미널에서만 바꿉니다. 그사이 host.toml이 바뀌었으면 쓰지 않고 다시 읽습니다.
- **작업 폴더·hook**: 설정 → 머신 아래 「작업 폴더·hook」에서 머신을 고르고, 절대 경로를 「점검…」(검사 파일을 쓰고 읽고 지움) 한 뒤 「전환…」합니다. 다음 agent 시작부터 쓰고 지금 도는 thread는 그대로입니다. Codex 전역 hook은 같은 곳에서 설치·제거합니다(Claude는 실행마다 넣어 설치할 것이 없습니다).
- **알림 켜기**: 설정 → 알림에서 켜고 「테스트 알림」으로 확인합니다. 기기 알림(Web Push)은 coordinator가 보낸 「확인 필요」·「전체 완료」 post만 받습니다. thread 질문·보고는 앱 안 알림으로 확인합니다. iPhone은 홈 화면 앱(Safari 공유 → 홈 화면에 추가)에서만 켤 수 있습니다.

일반 thread→coordinator post는 출처마다 최근 60초 30건·1시간 200건·24시간 2,000건이 기본입니다. 짧은 측정 핑퐁과 장시간 작업을 허용하고, 한 시간 안 한도 초과 거부 20회에 자동 음소거합니다. `host.toml`의 `[post]`에서 `burst = 30`, `burst_secs = 60`, `hourly = 200`, `daily = 2000`, `mute_after = 20`을 바꿀 수 있습니다([설정 예시](../crates/aaa-config/examples/host.toml)). `pair_hourly = 100`은 목적지 쌍별 한도의 예약값이며 현재 thread→coordinator 경로에는 적용하지 않습니다. 설정에서 생략한 값에 새 기본값을 쓰며, 이미 명시한 값과 기존 음소거는 그대로 유지합니다. 같은 글은 24시간 동안 중복 거부하며, 이 거부는 한도 초과·자동 음소거 횟수에 넣지 않습니다.

보고서 발행(`library publish --to-user`)의 「새 보고서」 알림은 post 한도를 쓰지 않습니다. host가 확인한 프로젝트 coordinator의 「확인 필요」(`user-action`)·「전체 완료」(`task-complete`)는 합쳐서 분당 30·시간당 60·하루 300건이며 자동 음소거되지 않습니다(24시간 같은 글 중복은 거부). 일반 post 음소거는 다음 coordinator CLI 출력·`aaa context` 맨 위와 웹 앱 안 알림에 표시됩니다. 원인을 확인한 뒤 조작 권한으로 `aaa post unmute <출처>`를 실행합니다.

## 삭제된 기능: TUI

2026-10-19부터 TUI 코드·의존성·전용 시험을 삭제했습니다. 화면은 웹 UI를 씁니다. host에서 `aaa web status`로 주소를 확인합니다. 옛 `aaa attach`와 TUI front 접속도 삭제 안내로 거절합니다.

삭제 직전 main은 annotated tag `tui-last`로 보존합니다. 이유·삭제 커밋·복구 방법·삭제 목록은 [삭제 기록](removed-tui.md)에 있습니다.

## 기기·머신 신뢰와 웹 공개 권한

pairing 코드 발급, 머신 등록 창 열기·승인·폐기, 기기 폐기, 웹 공개 켜기·끄기는 aaa pane 안에서 실행할 수 없다. coordinator도 포함한다. host는 `local:…@…`·worker·evaluator·detached actor를 요청 실행 전에 거부하며, 사람이 aaa 밖 터미널이나 웹에서 하도록 안내한다. aaa 밖 Local은 기존처럼 사용할 수 있다. 웹 Controller는 pairing·기기 폐기와 함께 머신 등록 창·승인·폐기, 웹 공개 켜기·끄기, 보고서 올리기·공유 링크 키 교체를 한다(FRONT_API 1.80). 웹은 마지막 조작 기기와 host 머신을 폐기하지 않는다: 마지막 조작 기기는 다른 조작 기기를 먼저 페어링하거나 host 터미널의 `aaa web revoke`로, host 머신은 폐기 대상이 아니다. Reader는 목록만 본다.

설치(프로젝트 만들기와 첫 기기 짝짓기) 뒤의 운영은 웹으로 한다. CLI 몇 가지의 웹 자리: `aaa pane kill`은 pane 줄 메뉴 「세션 끝내기」(자리는 남고 「다시 열기」), `aaa project open --new`는 coordinator 메뉴 「하나 더 띄우기」, `aaa library list`·`revoke`는 설정 「이 aaa」 → 공유 링크의 「낸 링크」, `aaa library link --days`는 보고 링크 줄의 수명 고르기다. host 터미널에 남는 것은 임의 명령·경로를 여는 safety(`routine_commands`·agent 인자)와 custom profile, 보기 전용 짝짓기 코드(`aaa web pair --reader`), coordinator 내부 알림(`inbox`·`events`·`post`), 할 일 목록 머리 편집, `library import`·기한 지정 정리, release 받기·`update rollback`이다(plan §8.417).

공개 주소(HTTPS)로 연 조작 기기는 위 승인 필요 동작마다 그 기기 passkey로 사용자 확인(Face ID·지문·암호)을 한 번 한다(관리 GUI W7, FRONT_API 1.101). passkey는 짝지은 뒤 10분 안에만 등록한다: 짝지은 직후 본문 위 「passkey 등록」이나 설정 → 폰 및 기기 「이 기기 passkey」. passkey가 없는 기기는 승인할 수 없고, 짝지은 뒤 10분이 지나면 등록도 할 수 없다(등록한 passkey는 그 뒤에도 쓴다). host Mac 화면·passkey가 있는 기기·aaa 밖 터미널(`aaa web pair`)에서 짝짓기 코드를 받아 그 브라우저를 다시 짝지은 뒤 10분 안에 등록한다. 공개 주소(이름)가 바뀌어도 다시 짝짓는다. host Mac에서 연 `http://127.0.0.1:<port>` 화면과 aaa 밖 터미널은 passkey 없이 지금처럼 승인한다.

coordinator의 상태 조회·post·일반 thread 관리·허용된 배포 경로는 이 제한의 대상이 아니다. 같은 OS 사용자 권한의 agent가 pane 밖 터미널을 흉내 내거나 host.toml을 직접 고치는 한계는 남는다. 이 제한은 OS 사용자 격리를 제공하지 않는다.

## 원격 thread의 host 자료 읽기

host가 아닌 머신의 aaa 세션에서도 `thread list`·`thread show`·`thread summary`·`thread library`,
`library show`·`library list`(`ls`)·`library cat`, `search`·`record`를 쓸 수 있다.
로컬 host socket이 없거나 연결을 거부하면 그 세션의 agent 연결을 통해 host로 전달한다.
출력과 JSON은 host에서 같은 명령을 실행했을 때와 같다. `thread show`는 상태·보고 ID·Next를,
`library show <r-ID>`의 JSON은 회수한 보고 본문과 파일 목록을 준다.

```sh
aaa thread show t-0001
aaa --json library show r-0001
aaa library cat r-0001 plans/design.md
aaa search '설계 결정'
aaa record <프로젝트> <검색 결과 key>
```

원격 읽기는 `thread_read.v1`을 지원하는 host·agent가 필요하다. 구버전 agent·host나 연결 끊김은
명령 오류로 알린다. 세션 밖 원격 셸에는 이 경로를 열지 않는다. agent가 세션 token과 실제 요청
프로세스를 확인하고 host가 그 머신의 실행 중 세션인지 확인한다. 전달 경로는 읽기 허용 목록과
Reader 권한을 함께 적용한다. 쓰기·승인·병합 명령의 권한은 그대로다.

## 자주 쓰는 CLI

host는 프로젝트 하나만 맡으므로 명령의 프로젝트 인자는 생략한다(§8.285). 옛 꼴(`aaa thread show <p> <id>`처럼 앞에 프로젝트)도 한 판 동안 받는다: 이 host의 프로젝트와 같은 이름이면 표준 오류에 경고 한 줄을 남기고 그대로 하고, 다른 이름이면 거부한다. `post`(coordinator·사람의 셸 pane은 Front로, thread pane은 hook으로)·`safety`·`profile list --project`도 생략한다(§8.288). `project pause|resume|sweep|archive|delete`도 이름 없이 쓰고, `project rename <새 이름>`은 새 이름만 준다(S5). 보관한 프로젝트나 여럿이 남은 옛 상태의 이름은 옛 꼴로 지정할 수 있다. 보관을 풀 때는 `project unarchive <이름>`을 쓴다. 프로젝트 설정은 홈의 `aaa.toml`(`[[workspaces]]`)이 있으면 그것을, 없으면 `project.toml`(`[[repos]]`)을 읽고 쓴다. aaa는 aaa.toml을 만들지 않는다.

| 명령 | 하는 일 |
|---|---|
| `aaa --instance <이름> <명령>` · `aaa --instance <이름> init` | 같은 Mac의 둘째 aaa를 쓰기(env `AAA_INSTANCE`도 같다, pane 안에서는 물려받는다) · 만들기(폴더·겹치지 않는 포트·홈, 사람이 터미널에서). 이름이 없으면 지금 aaa다([절차](install-runbook.md#12-같은-mac의-둘째-aaa8294)) |
| `aaa machine list` | 머신 연결·디스크·이상 |
| `aaa machine rename <옛 이름> <새 이름>` · 설정 → 머신 → 이름 바꾸기 | 등록·열린 대화를 유지하며 이름과 참조를 함께 옮긴다. host 자신은 제외, 세션 밖 CLI·웹 조작 기기만 가능 |
| 웹 → 머신 → 배포 카드 | 최신 배포의 전송·단계·실패·복구를 Reader도 봅니다. 단절이면 마지막 수신 상태이며 재연결 뒤 최신 기록을 받습니다. 검증 정보에서 전체 ID/SHA를 봅니다. 카드에서는 배포를 시작하지 않습니다. |
| `aaa service restart\|stop\|start --role host\|agent` | 감독자(launchd)로 다시 띄우기(업그레이드 뒤) · 멈추기 · 띄우기. 사람이 터미널에서(`yes`). restart는 아래 coordinator 배포 허용도 가능 |
| `aaa deploy start --file <머신>:<후보폴더>/aaa --sha256 <SHA> [--commit <40자리>] [--machines all 또는 쉼표 목록]` · `aaa deploy status <ID> --wait` | coordinator safety 아래 빌드 후보를 Agent link로 전송하고 여러 Mac을 host부터 배포합니다. source/모든 대상에 deploy_stage.v1, 모든 대상에 rollout_recovery.v1이 필요합니다. 전체 전송·SHA/self-check 뒤 첫 교체를 하며 같은 ID로 재개합니다. 기존 미리 복사 경로는 --from입니다. 실패 머신만 배포 전 SHA로 자동 복구하며 recovering 뒤 stopped_rolled_back 또는 recovery_failed를 남깁니다. 앞 머신은 유지합니다. |
| `aaa update --from <파일> [--dry-run]` · `aaa update rollback [이름]` · `aaa update list` | 지금 실행 파일을 주어진 빌드로 바꾸기(self-check·`[update] version` 고정 검사 뒤 rename, 옛것은 옆 `.aaa-old/`에 `[update] keep`개) · 보관한 것으로 되돌리기 · 보관 목록. 바꾸기·되돌리기는 사람이 터미널에서(`yes`), 도는 host·agent는 `aaa service restart`로 다시 띄웁니다. 사람이 `aaa safety set coordinator_deploy on`을 켠 뒤 사용자 채팅 지시를 받은 coordinator는 `--machine`으로 update·rollback·restart를 직접 실행하고 `aaa update status <ID> --wait`로 확인합니다(restart는 재시작만 하는 rollout이라 `aaa deploy status <ID> --wait`, 독립 감시가 새 프로세스를 확인합니다)([절차](install-runbook.md#coordinator가-요청받은-배포를-실행하기10-19)) |
| `aaa pane list` · `aaa pane read <pane>` | pane 목록 · 최근 출력 글(`--source visible`이면 지금 화면) |
| `aaa tab create --machine mac-b -- <명령>` | 새 탭에 새 pane |
| `aaa pane run <pane> "…"` | 명령 실행(글 + Enter, 사람의 attach에 막히지 않음) |
| `aaa pane send-text <pane> "…"` · `aaa pane send-keys <pane> enter` | 글만 · 키만 보내기 |
| `aaa pane wait-output <pane> --match "…" --timeout 30000` | 출력에 글이 나올 때까지 기다리기 |
| `aaa question list` · `aaa question answer <id> yes` | 확인창 질문 보기·답하기 |
| `aaa project create <p> --goal "…" --repo <경로>[@머신] --thread-profile codex` · `aaa context` | 프로젝트 만들기 · coordinator 요약(할 일·열린 thread·inbox). host는 보관하지 않은 프로젝트 하나만 맡는다: 둘째 만들기는 거부하고(§8.285), 둘 이상이면 다시 띄울 때 시작하지 않고 줄일 목록을 말한다(§8.283). 인스턴스에서는 이름을 생략하면 인스턴스 이름이다(§8.294) |
| `aaa set <key> <값>` | 설정 값 하나 바꾸기(aaa.toml, 없으면 project.toml: goal·thread_profile·coordinator_profile·max_parallel_threads·auto_resolve_days·repos.add·repos.remove, workspaces.add·remove도 같다) |
| `aaa task list` | 할 일 목록(TASKS.md) 읽기: 목록별 task ID(`d-0001`)·제목, 형식 어긋남, task 없는 열린 thread. 웹은 활동 막대 「할 일」(§8.306) |
| `aaa idea list` · `aaa idea show <n-ID> [--rev N]` · `aaa idea new "<제목>" [--file F]` | 아이디어 목록 · 노트(첫 줄에 판) · 새 아이디어. 웹은 활동 막대 「아이디어」([아이디어](#아이디어)) |
| `aaa idea save <n-ID> --base <판> --file F [--summary "…"]` · `aaa idea say <n-ID> "…"` · `aaa idea copy <n-ID>` | 판을 주고 노트 저장(그사이 다른 판이 있으면 쓰지 않고 `F.base-<판>.md`·`F.current-<판>.md`를 남기고 종료 코드 3) · 전용 thread에 말하기 · 복사해서 새 아이디어 |
| `aaa task add "<제목>" [--list 목록] [--profile 모델] [--machine 머신] [--owner me\|이름] [--repo 저장소]` · `aaa task edit <task ID\|제목> [--title …] [--profile …] [--machine …] [--user-request 근거] [--owner …] [--repo …] [--thread t-NNNN]` · `aaa task move\|rm <task ID\|제목>` · `aaa task restore <task ID\|기록 id>` · `aaa task log [task ID\|제목]` | 할 일 고치기: 바뀐 task 줄만 다시 쓰고 나머지 줄은 그대로 둔다. `--block <sha256>`이면 그 판의 블록으로 고정하고, 그사이 바뀌었으면 쓰지 않는다(충돌). 지운 task는 기록에서 되살린다. `--repo`는 저장소 칸(project.toml repos 경로나 그 마지막 이름, 또는 `작업 폴더`, §8.330)이고 `--from-task`가 그 저장소로 띄운다. `--profile`·`--machine`은 agent에게 맡길 모델·머신 칸이고(§8.335, 줄에는 `(모델@머신)`으로 적는다) 칸마다 따로 고친다(빈 글이면 그 칸만 비운다). 사람의 할 일은 `--owner me`(사람 이름은 `--person`)이다. 목록은 `aaa task list-add\|list-rename\|list-merge\|list-rm\|list-move`. aaa 세션 안에서는 그 프로젝트 coordinator만 고친다. 웹은 할 일 화면의 폼·⋯ 메뉴·「기록」 탭(§8.311) |
| `aaa coordinator open [--profile 이름] [--new]` | coordinator 열기: 돌고 있으면 그 pane을 알리고, 없으면 host 머신에서 프로젝트 폴더로 띄워 workspace `<p>`의 탭 coordinator에 둠(profile 기본: project.toml → `aaa profile default coordinator` → claude) |
| `aaa project pause\|resume\|archive` · `aaa project unarchive <p>` | 멈추기(ticker·thread start 멈춤) · 보관(목록에서 숨기고 pane을 닫음). 보관 해제는 다른 활성·멈춤 프로젝트가 없을 때만 가능 |
| `aaa project rename <새 이름>` · `aaa project delete` · `aaa project sweep` | 이름 바꾸기 · 휴지통으로(worktree·branch는 그대로) · 끝난 thread가 남긴 worktree·병합된 branch 치우기 |
| `aaa doctor files [--fix]` | 모든 프로젝트의 coordinator 안내(AGENTS.md)·uploads/ 보기·폐지된 자동화 원본 보존 · 낡은 안내를 지금 aaa 것으로 다시 쓰기(사람이 쓴 파일은 `.before-aaa`로 옮겨 둠) |
| `aaa guide [주제]` · `aaa guide --all` | coordinator·thread가 그 일을 할 때 읽는 세부 안내(질문 post 문법·보고서·thread 한도·작업 요약·확인창·TASKS.md 형식·CI·배포 절차·자동화·auto mode·웹 보고서 sandbox). host에 묻지 않고 이 실행 파일의 글을 보인다. 주제 없이 부르면 목록. AGENTS.md는 매 턴 규칙만 두고 이 주제를 가리킨다(§8.491) |
| `aaa task add "…" --profile codex --machine mac-b --repo '작업 폴더'` → `aaa thread start --from-task d-0001 --link-task` | 새 작업은 할 일을 먼저 기록하고 thread로 시작한다. `--from-task`는 연결 옵션을 생략해도 자동 연결한다. 옛 `--title … --task-file …` 직접 시작도 host가 할 일을 먼저 만든다. 자동화 do.start는 예외다 |
| `aaa thread adopt --pane <ID> --title "…"` | 이미 띄운 Claude·Codex pane(셸에서 친 것은 감지된 뒤)을 thread로 데려오기(pane은 그대로, 다시 시작·resolve가 pane을 끝내지 않음) |
| `aaa thread list` · `aaa thread show <id>` | thread 목록 · 보고·Next 보기 |
| `aaa thread ack <id>` · `aaa thread resolve <id>` | 보고 확인 · 끝내기(worktree 정리) |
| `aaa thread park <id> [--now] [--note "…"]` · `aaa thread unpark <id> [--machine M] [--dry-run]` | 보류: 자리·pane을 돌려주고 worktree·대화·보고는 남긴다(기본은 thread가 커밋·push·`## Handoff`를 쓴 `[보류 준비됨]` 뒤 닫는다, `--now`는 바로, `--cancel`은 거두기). 이어 하기: 같은 머신은 같은 대화, 다른 머신은 push된 branch로 새 worktree와 요약 brief의 새 대화(§8.426). 웹은 thread 화면 「관리 → 보류·이어 하기」 |
| `aaa inbox list` · `aaa inbox done <id>` | 보고·질문·끊김 같은 놓치면 안 되는 항목 |
| `aaa routine list` · `aaa routine runs <이름>` · `aaa routine toggle <이름> --off` | 일정·thread·CI·Git 사건 자동화 |

모든 명령은 `--json`으로 기계가 읽는 출력을 줍니다. `pane`·`tab`·`workspace`는 늘 JSON 한 줄이고, 실패하면 stderr에 `{"error":…}`를 쓰고 종료 코드 1입니다(herdr와 같다). 셸 완성은 `aaa completion zsh`(bash·fish·elvish·powershell)입니다. 자세한 것은 `aaa <명령> --help`입니다.
## 머신 배정

머신은 host가 workspace 허용 → 독점 예약 → 착지 중 → 열린 thread÷비중(기본 CPU 수) 차례로 고른다. 머신을 비워 두려면
웹 기기 정보 「자동 배정」이나 `aaa machine reserve <머신> --holder <d-…|t-…>`로 독점 예약을 건다. 자동화 `machines`를
비우면 같은 차례로 자동 선택한다. 자세한 규칙: [머신 배정](machine-assignment.md).

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

사람은 aaa 밖 host 터미널에서 `aaa deploy start --file mac-b:~/aaa-candidate/aaa --sha256 <SHA> --commit <40자리커밋>`의 후보/대상을 읽고 정확히 `yes`를 입력한다(FRONT1.55 이상). coordinator_deploy 설정은 바꾸지 않는다. 같은 `aaa deploy status <ID> --wait`로 재시작을 넘는다. TOML `do="deploy"`는 배포 제안과 빌드 안내만 만든다([자동화](automations.md#배포는-제안까지만l5)). 웹 조작 기기는 설정 「배포」에서 빌드 머신의 후보를 「점검」(provenance·SHA 대조와 dry-run)한 뒤 확인 창 하나로 시작하고, 같은 rollout ID를 「배포 기록」에서 다시 보며, 머신마다 agent·host를 다시 띄운다(FRONT_API 1.82, [설치 안내](install-runbook.md#웹에서-배포재시작관리-gui-w6a-front_api-182)). host 설정이 깨지면 host는 같은 웹 주소에 안전 모드 화면만 열어 오류 줄과 가린 원문을 보이고, 조작 기기에서 「마지막 정상 설정으로 되돌리기」(지금 파일은 `.broken-<시각>`으로 남김)나 「다시 읽기」로 정상 host로 돌아온다([설치 안내](install-runbook.md#설정이-깨졌을-때-안전-모드관리-gui-w6b)).

할 일은 웹 목록·`aaa task list`·`aaa context`에 보이는 `d-0001` 같은 ID로 가리킨다.
제목 변경·이동·삭제·복원에도 같은 번호이며, 지운 번호는 재사용하지 않는다. 제목 입력도 계속 받는다.
이주·보관·복원의 자세한 규칙은 [할 일 ID](task-ids.md)에 있다.

할 일을 자리 나면 시작하려면 설명·완료 범위 메모와 저장소(또는 `작업 폴더`)를 저장한 뒤
`aaa task reserve d-0001`로 그 내용의 실행을 승인한다. 기본 모델도 예약 시점에 고정한다.
`aaa task queue`는 차례·모델/effort·머신·대기 사유·실제 thread를 읽는다.
`aaa task queue --json`도 같은 읽기 전용 조회이며 승인판·차례·대기 이유·변경 시각을 바꾸지 않는다.
목록만 보려고 `reserve`를 다시 부르지 않는다. `reserve`는 지금 내용으로 실행 승인을 갱신하는 변경이다.
`aaa task unreserve <task ID>`는 예약을 취소하며 실행 중인 thread는 그대로 둔다.
`aaa task queue-move <task ID> <순번>`으로 차례를 바꾼다(1부터, 0은 맨 앞).
내용을 고쳤으면 확인 후 같은 `reserve` 명령으로 승인판을 갱신한다. 파일 직접 수정도 이전 승인으로 시작하지 않는다.
앞 할 일이 끝난 뒤에 시작할 일은 `aaa task reserve d-0003 --after d-0001 d-0002`로 예약한다.
`--after d-0001 --after d-0002`처럼 반복해도 된다. 제목도 받지만 예약할 때 고정 ID로 바꾸며 중복 ID는 한 번만 저장한다.
웹은 예약 확인의 「앞 할 일」에서 여러 항목을 체크한다. 여러 선행 예약은 Front API 1.150 이상 host가 필요하다.
모두 완료 보관되면 자리 차례에 들고, 그 전에는 아직 끝나지 않은 ID를 「앞 할 일 d-0001, d-0002 완료 대기」로 보여 준다.
대기 중에는 thread·agent 세션을 미리 만들지 않으며, 뒤의 독립 예약은 시작할 수 있다. 모든 선행이 끝나도 실행 한도가 차 있으면 자리 대기를 유지한다.
완료 체크·보고 `[완료]`·병합 후 실제 완료 보관된 것이 기준이다. 실패·보류·중단·예약 취소만으로는 선행이 끝난 것이 아니다.
시안 승인처럼 사람이 정하는 일은 승인한 뒤 앞 할 일을 완료로 표시하면 된다. 완료 보관에서 복원한 선행은 다시 완료 보관될 때까지 기다린다.
앞 할 일이 완료 없이 지워지면 다른 선행의 진행 여부와 관계없이 「확인 필요」가 된다. 복원하거나 조건을 고친 뒤 예약을 갱신한다.
서로 기다리는 순환과 자기 참조는 예약·갱신 때 거부한다. 재시작과 중복 완료 통지에도 저장된 기동 의도로 같은 예약을 대조한다.
예약을 갱신할 때도 모든 `--after`를 다시 준다. 생략하면 조건을 해제한다. 구판 클라이언트로 다중 조건을 덮어쓰는 갱신은 거부한다.

보고 첫 줄 `[병합 준비됨]`은 「병합 대기」(초록), `[착지]`는 「병합 중」(주황)으로 웹 할 일·thread와 CLI에 보인다.
병합 대기와 착지 한 자리는 실행 한도·예약 자리에서 뺀다. 착지 검증 실패를 수리하는 동안은 `[착지]`를 유지하고,
착지를 그만두거나 다음 단계 구현을 시작하면 `[진행 중]`으로 돌린다. `[완료]`만으로는 자리 제외가 아니다.
표지는 첫 줄에 하나만 쓰며, 옛 표지 없는 보고·겹친 표지는 작업으로 센다. 새로 셀 때 한도를 넘으면 새 시작만 막는다.

### 자동 병합

할 일마다 「자동 병합」을 켜고 끈다(기본 off, 프로젝트 기본값 없음). on이면 thread가 `[병합 준비됨]`으로 보고한 뒤
coordinator가 사용자에게 묻지 않고 착지 차례대로 착지 창을 준다. 착지는 그대로 한 번에 하나다. off면 지금처럼 사용자 승인 뒤 준다.
thread는 이어진 할 일의 값을 보이므로 다시 시작·이어 하기도 같은 값이다. 착지 중에 끄면 이미 시작한 착지는 되돌리지 않는다.

- 웹: 할 일 상세의 「자동 병합」 스위치, 새 할 일 폼·「바로 시작」·예약 확인의 칸, thread 화면 머리 아래의 스위치.
  할 일 줄·「모든 thread」 목록·thread 제목·작업 요약 카드에 「자동 병합」 표지가, 「작업」 목록 thread 줄에는 병합 아이콘이 보인다.
  「작업」 목록 줄의 첫 줄은 「번호 · 제목」만 두고, 표지(병합 대기·병합 중·자동 병합 아이콘 등)는 둘째 줄 상태 말 앞에 둔다(한 줄 모드는 이름 뒤).
- CLI: `aaa task add|edit|reserve … --auto-merge on|off`, `aaa thread start --auto-merge on|off`,
  `aaa thread auto-merge <thread> [on|off]`(생략하면 thread show).
- 바꾼 사람·시각·off→on은 `aaa task log`의 `auto_merge` 기록에 남는다. 사용자가 바꾸면 coordinator inbox에 `auto-merge on/off`가 남는다.
- coordinator는 사용자가 채팅에서 말했을 때만 `--user-request '<사용자 지시>'`와 함께 바꾼다. thread 세션은 바꾸지 못한다.
  값은 host DB에 있어 TASKS.md를 직접 고쳐 켜지 않는다.

host는 사용자·coordinator가 띄운 Worker의 `max_parallel_threads`를 실제 한도로 집행한다.
자동화 thread(`automations/*.toml`의 `do.start`)는 이 수와 예약 자리에서 빼고 규칙별 동시 상한(기본 1개)으로 제한한다.
웹·`aaa context`·`aaa thread list`의 표지는 「자동화 thread」이며 사건·일정 출처는 상세에 보인다. 검토·답 대기·Idle·보고 [완료]에도 자리를 보유하며
resolve 정리를 마쳐야 반환한다. 한도가 차면 새 기동을 보내지 않는다. 앞 예약의 고정 머신이 끊겼으면 가능한 뒤 예약을 먼저 시작한다.
coordinator와 웹이 닫혀도 host가 시작하며, 알려지지 않은 모델·없어진 저장소·첨부 문제·시작 실패는 확인 필요로 남긴다.
한도가 찬 상태에서 사용자가 직접 시작하려면 웹 「thread로 바로 시작」의 「한도 무시하고 시작」을 누르고 다시 확인한다.
CLI는 `aaa thread start --from-task d-0001 --link-task --over-limit`로 시작한다. 이미 예약한 task는 예약을 먼저 취소한다.
강제 시작도 열린 thread 수에 포함하므로 한도 아래로 자리가 날 때까지 예약은 기다린다.
「한도 초과로 시작」 표지와 요청자·시각이 기록에 남으며 host 재시작 뒤에도 보존한다.
coordinator는 사용자가 채팅에서 해당 task를 강제 시작하라고 명시한 경우에만 다음처럼 근거를 남긴다.

```sh
aaa thread start --from-task d-0001 --link-task --over-limit --user-request '사용자가 d-0001을 한도 무시하고 시작하라고 지시'
```

근거는 비지 않은 한 줄·2000바이트 이하이다. 일반 시작 승인이나 예약·자동 시작에는 이 옵션을 쓰지 않는다.

사용자가 채팅으로 모델·머신 변경을 지시하면 coordinator도 다음처럼 바꾼다.

```sh
aaa task edit d-0001 --profile codex --machine mac-a --user-request '사용자가 채팅에서 codex와 mac-a로 변경 지시'
```

바꿀 칸만 준다. 빈 글은 해당 칸을 비운다. `--user-request`는 근거 한 줄을 요구하며 모델·머신 외의 칸이나 여러 task를 함께 고치지 않는다.
할 일 기록에는 「coordinator가 사용자 지시로 변경」과 근거, 변경 전후 값이 남고 감사에도 같은 근거를 남긴다.
미기동 예약은 취소 없이 순서·생성 시각을 유지하고 새 값으로 시작한다. 취소된 예약은 변경해도 취소 상태를 유지한다.
이미 기동한 예약이나 다른 내용이 먼저 바뀐 예약은 거부한다. 상태·내용을 확인해 별도로 처리한다.
사용자 지시 없는 변경·하향, 원문 편집 우회, 시작·재시작 옵션으로 다른 모델·머신을 쓰는 동작은 계속 거부한다.
웹이나 aaa 밖 CLI의 일반 편집은 기존처럼 가능하며, 예약의 선택을 일반 편집으로 바꾸면 내용을 확인해 예약을 갱신한다.

원격 main 포함·보고 [완료]의 좁은 완료 판정을 유지하면서 결과는 `aaa task archive`에 보관한다.
`aaa task archive --restore <task ID>`는 같은 ID·내용·첨부를 활성 목록에 돌리고 실행 연결은 푼다. 자동 재예약·재기동은 하지 않는다.
옛 삭제 기록의 `aaa task restore <기록 ID>`는 별도다. 보관 저장에 실패하면 활성 task를 지우지 않는다.
보관의 결과물과 메모가 참조하는 보고서 사본은 기한·용량 정리와 보고서 삭제에서 보호한다. 같은 ID로 복원해도 완료 당시 결과는 남긴다.
웹 할 일 상세에서 예약 내용을 확인·예약·갱신·취소·위/아래/맨 앞으로를 조작하고, 「완료 보관」에서 결과를 보고 복원한다.
웹 할 일 상세는 제목·예약 상태·속성(저장소·담당·모델·머신·상위)·메모 차례이고, 맡기기·바로 시작·예약·예약 풀기는 상세 아래 고정 줄에 있다.
예약 확인(앞 할 일 고르기)도 그 줄 자리에서 열린다. 속성은 고르면 바로 저장하고, 제목·메모·사람 이름은 「저장」으로 쓴다. 예약 차례 옮기기와 지우기는 그 줄의 ⋯에 있다.
웹 할 일은 한 목록이다(§8.393). 예약은 맨 위에 예약 차례대로 보라 표지로 보이고(확인 필요는 노랑), 그 밑에 일반 할 일이 파일 차례로 있다.
행을 예약 영역에 끌어 놓으면 내용 확인 뒤 그 자리에 예약하고, 예약 행을 아래로 끌면 예약을 풀어 원래 자리로 돌아간다.
상위를 예약하면 하위가 함께 올라가고, 머신 연결 대기 같은 특별한 까닭과 풀면 돌아갈 자리는 상세에만 보인다.
거르기 칩 「예약」과 「저장소」 고르기(이 기기에 기억)는 함께 걸리며, 거른 채 옮겨도 숨은 할 일의 차례는 그대로다.
휴대폰은 끌기 대신 ⋯ 시트와 「순서」의 ▲▼를 쓴다. 파일의 옛 `## 목록` 머리는 그대로 읽고 쓰며 웹에서는 나누어 보이지 않는다.
여러 고정 ID가 모두 완료되기를 기다리는 예약(위 `--after`)을 제공한다. 저장소별 착지 gate·자동 병합 정책은 이 선행 조건에 포함하지 않는다.

### 머신 이름 바꾸기

`aaa machine rename mac-b mac-c` 또는 설정 → 머신의 「이름 바꾸기」를 쓴다. 재등록·승인·서비스 재설치·재시작은 필요 없다. 열린 pane·thread·세션 ID와 장치 키는 그대로다. 다른 머신·등록 대기 이름·host 이름과 겹치거나 이름 규칙(영문·숫자·`._-`, 1~64자)을 벗어나면 거부한다. host 자신의 이름은 로컬 agent 실행 설정과 묶여 있어 이 기능으로 바꾸지 않는다. aaa 세션 안의 agent(coordinator·thread)는 요청할 수 없다.

프로젝트 설정의 repos/workspaces, TASKS.md의 모델@머신, 예약의 승인판·선택 고정, 완료 보관의 복원 블록, 자동화와 예전 routine/mission의 머신 칸, workspace 허용·배포·thread 기록의 표시도 따라 바뀐다. 예약 기동·자동화 실행·workspace 지표 회수 중이면 충돌을 알린다. 끝난 뒤 다시 요청한다. 당시 증거인 감사 로그·명령 출력·설치 계획/영수증·보고서·brief·사용자가 쓴 메모·경로·OS/Tailscale 이름은 바꾸지 않는다. 예전 이름으로 만든 외부 셸 스크립트나 따로 보관한 설정은 사용자가 고친다.

host의 등록 ID가 이름의 정본이다. `machine_label.v1` agent는 변경 이름을 상태 폴더의 `machine-label`에 저장하고 즉시 적용한다. 이 값은 실행 옵션·agent.toml의 초기 이름보다 우선하며 세션과 독립적이다. 끊긴 agent는 다음 연결에서 동기화한다. 구판 agent는 자기 초기 이름을 계속 보고해도 host가 TLS 장치 ID로 현재 이름을 고르므로 옛 이름이 부활하거나 다른 머신과 충돌하지 않는다. 동기화 실패는 경로·하위 오류와 함께 로그에 남긴다.

변경은 파일별 잠금·원문 대조와 SQLite 트랜잭션으로 처리한다. 파일과 DB 사이에서 host가 종료돼도 시작할 때 journal을 복구한다. 복구 중 직접 편집을 발견하면 그 글을 덮지 않고 경로를 알리며 멈춘다.

할 일 메모를 보태려면 `aaa task edit <ID> --add-note "한 줄"`을 여러 번 쓴다. `--note`는 전체 교체, `--clear-notes`는 전체 삭제다. 세 방식은 함께 쓸 수 없다. 기존 메모 줄이 빠지거나 바뀌면 저장 결과에 「메모 N줄→M줄」과 기록 번호가 나온다. `aaa --json task log`의 해당 기록 `before`에서 이전 원문을 찾는다. 웹 상세의 「메모 추가」는 손대지 않은 일에 바로 저장하고, 고치는 중이거나 새 일이면 초안에 더해 「저장」 때 함께 쓴다. 잘못된 제목은 CLI·웹 저장에서 거부하며 thread 번호는 연결 칸, 상태·긴 글은 메모로 옮긴다.

### 아이디어

아이디어는 한 번에 정리되지 않는다. 그래서 정본은 계속 고쳐지는 노트(중간 정리본)이고, 대화할 때마다 그 노트가 새로 고쳐진다. 짧은 ID는 `n-0001`이다([짧은 번호](short-ids.md)).

- **저장**: host 프로젝트 폴더 `ideas/<ULID>/`에 노트 `note.md`, 고칠 때마다 남는 판 `revs/<판>.md`, 메타 `idea.json`, 그림·영상(내용 해시 이름)을 둔다. 노트는 `![설명](<이름>)`으로 같은 폴더의 그림을 가리킨다.
- **바꾸는 길 셋**: 대화(전용 thread가 답하며 노트를 고친다), 직접 고치기(「고치기」 → textarea → 「저장」, 그림·영상은 붙여 넣기·끌어 놓기), 인용 의견(노트 글을 골라 「의견」, 그림·영상은 오른쪽 위 「의견」으로 통째로). 의견은 이 기기에 모였다가 대화 칸의 「입력창에 넣기」로 한 번에 입력창에 붙고, 넣은 의견은 모음에서 지워진다. 보내기는 사람이 누른다.
- **판과 겹침**: 사람·thread 모두 읽은 판을 주고 저장한다. 그사이 다른 판이 생겼으면 쓰지 않는다. thread는 두 판을 받아 합쳐 다시 저장하고, 사람의 글은 버리지 않고 `merge/`에 둔 채 thread에 합치기를 맡긴다(그동안 노트 머리는 「합치는 중」).
- **고친 것 알리기**: 사람이 노트를 직접 고치면 다음에 대화로 말을 보낼 때 그 앞에 `(노트 고침: 판 9 「미정」 1줄 더함)`처럼 고친 판과 절이 붙는다. 저장하는 순간에는 따로 알리지 않는다.
- **전용 thread**: 아이디어마다 하나, 처음 말할 때 host 머신의 작업 폴더 thread로 뜬다. 동시 thread 한도에 세지 않고, 할 일·작업 목록·coordinator 요약에 보이지 않는다. 대화가 `[projects] idea_rest_secs`(기본 1800초) 넘게 쉬면 알림 없이 보류하고, 아이디어 화면을 열거나 다시 말하면 같은 대화로 잇는다. 잇지 못하면 새 thread가 노트를 읽고 이어 간다. 질문 카드는 정말 막혔을 때만 쓴다.
- **할 일로 넘기기**: 「할 일로 넘기기」에서 초안을 여럿 만들 수 있다(제목·저장소·메모). 메모 첫 줄에 `[아이디어 n-0001](ideas/n-0001)`이 붙고, 아이디어 머리에는 넘긴 `d-` ID가 보인다. 그 할 일로 thread를 시작하면 노트와 노트의 그림·영상이 작업 폴더 `attachments/`로 넘어간다(대화 기록은 넘기지 않는다). 넘긴 아이디어는 완료로 보관되고 고칠 수 없다. 다시 다듬으려면 「복사해서 새 아이디어」를 쓴다.
- **휴대폰**: 「목록 | 노트 | 대화」가 한 줄로 놓인 페이지처럼 넘긴다. 노트에서 왼쪽으로 쓸면 대화, 대화에서 오른쪽으로 쓸면 노트, 노트에서 오른쪽으로 쓸면 목록이다. 머리의 「노트 | 대화」 단추로도 바꾸고, 「대화」의 수는 모은 의견 수다. 화면 왼쪽 끝 24px에서 시작한 쓸기(브라우저 뒤로)·글을 고르는 중·입력칸·가로로 밀린 표 위에서는 넘기지 않는다.

예전 할 일 화면의 「나중」 탭과 `task_notes` 설정은 없어졌다. project.toml에 남은 `task_notes` 줄은 읽지 않는다.

### 끝난 thread의 후속

끝난 thread를 resolve하기 전에 보고·요약의 Next를 확인한다. 후속이 남으면 후속 할 일을 만들고 `aaa thread followup <thread> --id <후속 ID> --task <d-ID>`로 잇거나, 일부러 버리는 사유를 `--reason-file <파일>`로 기록한다. 후속 ID는 `aaa thread show <thread>`가 보인다. 정리만 남은 줄은 `정리:`로 구분하고 정리를 마친 사실을 사유로 기록한다. 일반 inbox 종료·완료 체크·연결 해제로는 후속이 닫히지 않는다. 자동화가 시작한 thread의 Next는 그 자동화의 다음 실행이 맡으므로 후속 원장에서 뺀다. 시작 출처로만 판정하며 이름·문구로 빼지 않는다.

### 프로젝트 memory

프로젝트 폴더의 `MEMORY.md`(색인)와 `memory/*.md`는 thread를 시작할 때 brief에 들어간다. 이름순으로 host.toml `[projects] brief_memory_chars`(기본 32,000자)까지 넣고, 넘친 파일은 이름만 적는다. coordinator는 세션을 시작하거나 compaction한 뒤 직접 읽는다. 파일 맨 앞에 머리를 두면 받는 thread를 고른다.

```
---
audience: coordinator
---
```

- `audience: coordinator`: thread brief에 넣지 않는다(coordinator 운영 규칙).
- `repos: rc, pt`: 그 저장소(project.toml repos 경로의 마지막 이름)에서 도는 thread에만 넣는다. 작업 폴더 thread는 받지 않는다.
- 머리가 없거나 닫히지 않은 파일은 모든 thread에 넣는다. 머리는 brief에 싣지 않고, 뺀 파일은 글자 수 상한에 세지 않는다.

## 웹 화면 이동 단축키

| 기본 키 | 이동할 화면 |
|---|---|
| Option(Alt)+O | coordinator 대화 |
| Option(Alt)+Q | 답할 것(질문) |
| Option(Alt)+T | 모든 thread |
| Option(Alt)+D | 할 일 |

설정 → 단축키에서 조합을 기록하거나 끄고 기본값으로 되돌린다. 입력 중에도 이동하며 쓰던 초안은 남는다. 같은 키를 다시 눌러도 그 화면에 머문다. 한글 등 IME 조합 중에는 이동하지 않는다. macOS Option이 특수 문자나 dead key를 만들어도 `KeyboardEvent.code`의 물리 키로 판단한다. 기존에 저장한 조합이 새 기본값과 같으면 저장한 조합이 우선한다. coordinator가 없으면 기존 coordinator 단추처럼 기본 profile로 연다. 보기 전용 기기는 실행 중인 coordinator만 열 수 있다. OS·브라우저가 먼저 가져가는 조합은 웹에서 받을 수 없으므로 설치한 PWA에서 쓰거나 설정에서 바꾼다.

설치한 PWA의 대화 입력창에서는 Ctrl+W로 커서 앞 공백과 그 앞 한 단어를 함께 지운다. 단어는 공백(줄바꿈·탭 포함)으로 구분하므로 `hello 한글/문장!`의 끝에서 누르면 `hello `가 남는다. 선택한 글이 있으면 선택만 지우고, 커서 뒤 글은 보존한다. 브라우저 편집 기록으로 되돌릴 수 있다. 조합 중이나 Ctrl에 다른 보조키를 더 누른 경우는 가로채지 않는다. 일반 브라우저 탭·터미널·다른 입력칸에는 이 동작을 적용하지 않는다.

단어 경계는 [GNU Readline의 unix-word-rubout](https://tiswww.case.edu/php/chet/readline/rluserman.html#Killing-Commands)을 따른다. 문장부호·경로 구분자까지 나누는 [fish의 Ctrl+W](https://fishshell.com/docs/current/interactive.html#shared-bindings)와 비교해 한글·영문을 똑같이 다루는 공백 경계를 택했다. 라이브러리 코드는 가져오지 않았다. 브라우저 예약 키는 [Chrome 공식 단축키](https://support.google.com/chrome/answer/157179)를 참고한다.
