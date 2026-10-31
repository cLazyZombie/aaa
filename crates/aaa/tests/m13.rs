//! M1-3 E2E: 프로젝트·thread 시작(그 머신 agent가 worktree·작업 폴더를 만든다)·brief 안내·보고 회수·ack·resolve·흐름 사건,
//! harness 상태(화면 감지)·확인창 질문 원격 응답·메시지(queue·interrupt).
//! agent harness는 시험용 가짜 agent(셸 스크립트)다. brief 안내 한 줄을 받으면 brief를 읽고 report.md·library를 쓴다.
//! 가짜 Claude(`claude`)는 Claude Code 화면(입력 상자·spinner·확인창)을 그리고 SessionStart hook으로 세션 ID를 알린다.
//! 실행: `cargo test -p aaa --test m13`

#![cfg(unix)]

#[path = "m13/accounts.rs"]
mod accounts;
#[path = "m13/auto_merge.rs"]
mod auto_merge;
#[path = "m13/automation_completion.rs"]
mod automation_completion;
#[path = "m13/automation_start.rs"]
mod automation_start;
#[path = "m13/instance.rs"]
mod instance;
mod lab;
#[path = "m13/no_pr.rs"]
mod no_pr;
#[path = "m13/preparation.rs"]
mod preparation;
#[path = "m13/report_events.rs"]
mod report_events;
#[path = "m13/secret_k0.rs"]
mod secret_k0;
#[path = "m13/ssh_install.rs"]
mod ssh_install;
#[path = "m13/task_dependencies.rs"]
mod task_dependencies;
#[path = "m13/task_main_merge.rs"]
mod task_main_merge;
#[path = "m13/thread_park.rs"]
mod thread_park;
#[path = "m13/thread_read.rs"]
mod thread_read;
#[path = "m13/thread_stages.rs"]
mod thread_stages;
#[path = "m13/web_publication.rs"]
mod web_publication;
#[path = "m13/work_root.rs"]
mod work_root;

use aaa_common::child_process::StdCommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use aaa_proto::{AgentActivity, FlowEvent, FrontEvent, QuestionState, QuestionView, ThreadView};
use lab::watch::HostWatch;
use lab::{Events, Lab, Node, path_str, read_line_containing, wait_for, wait_for_diag};
use serde_json::Value;

const T: Duration = Duration::from_secs(30);
/// brief를 치기 전 화면이 그대로여야 하는 시간(제품 기본 3초, brief.rs `SETTLE`). 가짜 agent 화면은 뜨자마자 멈추므로
/// 시험에서는 웹 E2E와 같은 값으로 줄인다(`AAA_TEST_BRIEF_SETTLE_MS`, docs/testing.md)
const BRIEF_SETTLE_MS: &str = "300";

/// m13의 host는 모두 여기서 띄운다. `settle_ms`가 `None`이면 제품 기본값(3초)이다
fn spawn_host(lab: &mut Lab, tag: &str, node: &Node, settle_ms: Option<&str>) {
    spawn_host_with_env(lab, tag, node, settle_ms, &[]);
}

fn spawn_host_with_env(lab: &mut Lab, tag: &str, node: &Node, settle_ms: Option<&str>, extra: &[(&str, &str)]) {
    lab.spawn_command(tag, host_command(node, Path::new(lab::BIN), settle_ms, extra));
}

fn host_command(node: &Node, exe: &Path, settle_ms: Option<&str>, extra: &[(&str, &str)]) -> Command {
    let mut command = node.cmd_bin(exe, &["host"]);
    if let Some(ms) = settle_ms {
        command.env("AAA_TEST_BRIEF_SETTLE_MS", ms);
    }
    command.envs(extra.iter().copied());
    command
}

/// brief 안내를 받으면 할 일대로 보고를 쓰는 가짜 agent
const FAKE_AGENT: &str = r#"#!/bin/sh
printf 'fake> '
while IFS= read -r line; do
  case "$line" in
    exit-one) exit 1 ;;
    post-probe\ *) sh "${line#post-probe }"; echo post-probe-done ;;
    *brief.md*)
      path=$(printf '%s' "$line" | sed 's/를 읽고.*//')
      dir=$(dirname "$path")
      echo "brief 받음: $path"
      task=$(awk '/^# 할 일/{f=1;next} /^# 경로/{f=0} f' "$path" | sed '/^$/d')
      # 보고 없이 쉬는 thread
      case "$task" in
        *noreport*) echo "done"; printf 'fake> '; continue ;;
      esac
      echo "결과물" > "$dir/library/out.txt"
      mkdir -p "$dir/library/sub" && echo deep > "$dir/library/sub/d.txt"
      # 복사하지 않는 symlink(사본이 partial이 된다)
      case "$task" in
        *link*) ln -s /etc/hosts "$dir/library/link" ;;
      esac
      # 결과물·commit을 다 남긴 뒤에 보고를 쓴다(실제 agent처럼 보고가 끝 신호다): 보고가 바뀌면 host가 그때의
      # library를 받는다(t-0327 넘김 (나), 0바이트 사본). 보고 뒤에 쓰면 시험이 먼저 resolve해 부하에 따라 실패했다
      case "$task" in
        *big*)
          head -c 2097152 /dev/urandom > "$dir/library/big.bin"
          printf '<!doctype html><p>보고</p><img src="sub/d.txt">' > "$dir/library/index.html" ;;
      esac
      case "$task" in
        *commit*) echo change > change.txt && git add change.txt && git -c user.name=t -c user.email=t@example.com commit -qm change ;;
      esac
      case "$task" in
        *dirty*) echo dirty > dirty.txt ;;
      esac
      printf '[완료] 맡긴 일 끝\n\n## Report\n%s\n\n## Next\n- 결과 확인\n- 다음 단계\n' "$task" > "$dir/report.md.tmp"
      mv "$dir/report.md.tmp" "$dir/report.md"
      echo "done"
      ;;
    *inbox/*.json*)
      # 사후 평가자 역할: 사건 묶음을 읽고 평가 기록을 쓴 뒤 report.md를 다시 쓴다
      f=$(printf '%s' "$line" | tr ' ' '\n' | grep 'inbox/.*[.]json$' | head -n 1)
      dir=$(dirname "$(dirname "$f")")
      name=$(basename "$f" .json)
      mkdir -p "$dir/library/evaluations"
      printf '{"targets":["t-0002"],"batches":["%s"],"summary":"묶음 %s를 봤다","stages":[{"thread":"t-0002","stage":"전체","minutes":1.4}],"proposals":[{"id":"p1","text":"brief 확인을 자동으로"},{"id":"p2","text":"보고 형식 검사"}]}' "$name" "$name" > "$dir/library/evaluations/e-$name.json"
      [ -f "$dir/library/evaluations/bad.json" ] || printf '{"proposals":[]}' > "$dir/library/evaluations/bad.json"
      echo "$name" >> "$dir/processed.txt"
      printf '## Report\n평가 %s\n\n## Next\n- 다음 묶음\n' "$name" > "$dir/report.md.tmp"
      mv "$dir/report.md.tmp" "$dir/report.md"
      echo "evaluated $name"
      ;;
    *"[aaa 보류 준비]"*)
      # 보류 정리 부탁(park.rs): 커밋·push는 시험이 한다. 맡긴 대로 Handoff 보고만 다시 쓴다
      path=$(printf '%s' "$line" | grep -o '[.]aaa/[^)]*/report[.]md' | head -n 1)
      printf '[보류 준비됨] 정리 끝\n\n## Handoff\n- 다음: 검수 뒤 이어 간다 HANDOFF-7\n\n## Next\n- 이어 하기\n' > "$path.tmp"
      mv "$path.tmp" "$path"
      echo "park-ready"
      ;;
    *) echo "got: $line" ;;
  esac
  printf 'fake> '
done
"#;

/// 가짜 Claude Code. aaa는 이 화면으로 상태를 안다(herdr 0.9.5 claude.toml: 입력 상자 `live_prompt_box`→idle,
/// spinner 줄 `live_turn_working`→작업 중, 확인창 `live_blocked_form`·`bash_permission_prompt`→질문. 같은 모양의 화면을
/// herdr `agent explain --file`로 확인했다). 폴더 신뢰 창 → SessionStart hook(세션 ID) → 입력 상자에서 기다린다.
/// 입력을 받으면 UserPromptSubmit hook을 내고 spinner를 그린 채 일한다. 턴은 시험이 끝낸다: 일하는 동안 키를 기다려
/// BEL(^G)이면 마치고, Esc면 Claude처럼 「Interrupted」를 보이고 멈춘다. 멈추는 동안(0.5초 조용할 때까지) 들어온 입력은
/// 버린다: 멈춤이 화면에 보이기 전에 입력하면 사라진다. 일하는 동안 친 글은 Claude처럼 입력 상자에 보이고 줄 세웠다가
/// 턴이 끝나면 다음 턴으로 받는다. 키는 echo 없이 읽는다(키 사이에 echo를 켜면 부하 아래 친 글이 tty echo로 찍혀 상대
/// 커서로 지우는 spinner가 남았다).
/// 선택 창은 Claude처럼 그린다(❯ 커서, 화살표로 옮기고 Enter로 고름, Esc는 취소). 창 하나는 한 번의 write다. 폴더 신뢰
/// 창의 기본 선택은 실제 Claude Code 2.1처럼 「No, exit」다. 권한을 거절하면 턴이 「Interrupted」로 끝난다.
const CLAUDE_AGENT: &str = r#"#!/bin/sh
ESC=$(printf '\033'); BEL=$(printf '\007'); LATE=$(printf '\016'); NL='
'; typed=; queued=
RULE='────────────────────────────────────────'
WORKING=$(printf '\342\234\273 Working\342\200\246 (1s \302\267 esc to interrupt)')
# 실제 Claude처럼 hook 출력(진행 안내 JSON)은 화면에 내지 않고 받는다. `HOOK_LOG`가 있으면 거기 한 줄씩 적는다
hook() { out=$("$AA" hook --agent claude "$1"); [ -z "$out" ] || [ -z "$HOOK_LOG" ] || printf '%s\n' "$out" >> "$HOOK_LOG"; }
key() { stty -icanon -echo min 1 time 0; k=$(dd bs=1 count=1 2>/dev/null); }
keyt() { stty -icanon -echo min 0 time "$1"; k=$(dd bs=1 count=1 2>/dev/null); }
box() { printf '%s\n\342\235\257 \n%s\n  ? for shortcuts' "$RULE" "$RULE"; }
# 입력 대기: 커서를 입력 상자 「❯ 」 뒤에 둔다. Enter 뒤(커서는 상자 아래 선)에는 상자를 지운다.
prompt() { stty icanon echo; box; printf '\033[2A\r\033[2C'; }
taken() { printf '\033[2A\r\033[J'; }
# 진행 보고가 실패해 진단 줄이 추가되어도 spinner를 그리기 전 자리부터 지운다.
spin() { printf '\0337%s\n' "$WORKING"; box; }
unspin() { printf '\0338\033[J'; }
# 일하는 동안 친 글은 Claude처럼 입력 상자에 보인다(커서는 상자 아래 안내 줄 첫 칸)
shown() { printf '\033[2A\r\342\235\257 %s\033[K\033[2B\r' "$typed"; }
interrupted() { printf '  \342\216\277  Interrupted \302\267 What should Claude do instead?\n'; }
# 옛 agent가 띄운 세션의 Claude는 확인창에 답한 뒤에도 Notification(permission_prompt) hook을 늦게 낸다(t-0333).
# 끝은 진행 보고로 알린다(hook은 차례로 host에 간다)
late() {
  hook '{"hook_event_name":"Notification","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","notification_type":"permission_prompt","message":"Claude needs your permission to use Bash"}'
  "$AA" report --percent 60 --activity '늦은 알림 뒤'
}
turn() {
  while key; do
    case "$k" in
      "$BEL") unspin; return ;;
      "$LATE") late ;;
      "$ESC") while keyt 5; [ -n "$k" ]; do :; done; typed=; unspin; interrupted; return ;;
      '') [ -z "$typed" ] || { queued="$queued$typed$NL"; typed=; shown; } ;;
      *) typed="$typed$k"; shown ;;
    esac
  done
}
# 한 번의 write로 쓴다: dd가 here-doc(끝에 줄바꿈 하나)을 한 블록으로 읽어 쓴다. printf는 줄마다 나눠 써서 host가
# 반쯤 그린 창(아래 안내 줄 전)을 볼 수 있다
out() { dd bs=8192 count=1 2>/dev/null <<EOF
$1
EOF
}
# 경합 시험의 준비 표지. 입력 모드와 화면이 모두 준비된 뒤 첫 글자를 받는다.
race_ready() {
  stty -icanon -echo min 1 time 0
  out "$(printf 'race armed\n%s\n\342\235\257 \n%s\n  ? for shortcuts' "$RULE" "$RULE")"
  printf '\033[3A\r\033[2C'
}
# $1 본문 $2 기본 선택(0·1) $3·$4 선택지 $5 아래 안내 $6 처음 그리기 앞에 함께 쓸 것. 창 하나를 한 번에 그리고(커서는
# 안내 줄 아래), 그린 자리에서 다시 그리고, 고른 뒤 지운다.
# 한 프로세스가 함께 온 화살표·Enter를 먼저 처리한다. 셸의 dd·stty·재그림을
# 사이에 끼우면 부하 아래 800ms가 지나 답한 창을 재질문으로 감지할 수 있다.
menu() { pick=$(python3 "$CLAUDE_MENU" "$@"); } 3>&1

perm() {
  unspin
  menu " Bash command

   $1
   Run the tests

 Do you want to proceed?" 0 '1. Yes' '2. No, and tell Claude what to do differently (esc)' 'Esc to cancel · Tab to amend · ctrl+e to explain'
  if [ "$pick" = '1. Yes' ]; then echo "allowed"; spin; else echo "denied"; fi
}
printf '\033[2J\033[H'
menu ' Quick safety check: Do you trust the files in this folder?' 0 'No, exit' 'Yes, I trust this folder' 'Enter to confirm · Esc to cancel'
[ "$pick" = 'Yes, I trust this folder' ] || { echo "trust: $pick"; exit 1; }
hook '{"hook_event_name":"SessionStart","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","source":"startup"}'
echo "marker: ${CLAUDE_CODE_CHILD_SESSION:-none}"
prompt
while :; do
  if [ -n "$queued" ]; then line=${queued%%"$NL"*}; queued=${queued#*"$NL"}
  else IFS= read -r line || break; taken; fi
  hook '{"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}'
  pick=
  case "$line" in
    *brief.md*)
      dir=$(dirname "$(printf '%s' "$line" | sed 's/를 읽고.*//')")
      spin; AAA_HOOK_SOCK="${REPORT_HOOK_SOCK:-$AAA_HOOK_SOCK}" "$AA" report --percent 40 --activity '권한 확인 중'
      printf '%s\n' "$?" > "$dir/progress-exit"; perm 'make test'
      [ "$pick" = cancel ] || printf '[진행 중] 권한 확인 뒤 계속\n\n## Report\n%s\n\n## Next\n- 확인\n' "$pick" > "$dir/report.md" ;;
    *late*)
      # 같은 확인창의 Notification이 답보다 늦게 온다(ci-local에서 경쟁으로 난 차례, plan §8.99). 진행 보고가 같은 agent
      # 연결로 그 뒤를 잇는다
      spin
      hook '{"hook_event_name":"PermissionRequest","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","tool_name":"Bash","tool_input":{"command":"make late"}}'
      perm 'make late'
      hook '{"hook_event_name":"Notification","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","notification_type":"permission_prompt","message":"Claude needs your permission to use Bash"}'
      "$AA" report --percent 40 --activity '늦은 짝 뒤' ;;
    *race*)
      # 치는 동안 뜨는 권한 창(t-0340 보안 감사): 받은 턴(작업 중)을 BEL로 마치면 입력 상자를 보인 채(쉼) 다음 입력의 첫
      # 글자를 받고, Enter 전에 권한 창을 그린다(상자 지우기와 함께 한 번에: host는 친 글에 화면이 반응한 뒤 Enter 전
      # 검사를 한다). 준비 전에 echo를 끄고 상자도 한 번에 그려 TTY echo·늦은 상자 출력을 글 반응으로 오인하지 않게 한다.
      # 그 Enter는 사람이 보지 않은 창의 기본 선택(「1. Yes」)을 고른다
      spin; turn; race_ready; key
      menu " Bash command

   rm -rf build
   Clean the build

 Do you want to proceed?" 0 '1. Yes' '2. No, and tell Claude what to do differently (esc)' 'Esc to cancel · Tab to amend · ctrl+e to explain' "$(printf '\033[1A\r\033[J')"
      echo "race $pick" ;;
    *ask*) spin; perm 'make test' ;;
    # 일하는 동안(키 하나를 받은 뒤: 시험이 작업 중을 본 뒤 보낸다) 끝낸 보고를 쓰고 `Done`을 알린 턴(턴은 시험이 끝낸다).
    # 그 턴이 끝나면 검토 대기의 새 보고다
    *finale*)
      spin; key; printf '[완료] 전체 끝\n\n## Report\nfinale\n\n## Next\n- 병합\n' > "$dir/report.md"; "$AA" report --percent 100 --activity Done ;;
    *) printf 'got: %s\n' "$line"; spin ;;
  esac
  if [ "$pick" = cancel ]; then interrupted; else turn; fi
  [ -n "$queued" ] || prompt
done
"#;

/// 안내를 받아도 화면이 바뀌지 않는 agent(echo 없음). 받은 줄은 파일에 남긴다.
const MUTE_AGENT: &str = "#!/bin/sh\nstty -echo\nexec cat >> \"$MUTE_LOG\"\n";

/// 진행을 보고하는 agent(hook 없음, `aaa report`만): brief를 받으면 배경 CI를 기다린다며 35%와 중간 보고를 쓰고 턴을 끝낸다.
/// `finish`를 받으면 끝낸 보고를 쓰고 `Done`을 알린다(보고를 먼저 쓰고 진행은 마지막, THREAD.md·진행 보고 안내 차례).
const PROGRESS_AGENT: &str = r#"#!/bin/sh
printf 'prog> '
while IFS= read -r line; do
  case "$line" in
    *brief.md*)
      dir=$(dirname "$(printf '%s' "$line" | sed 's/를 읽고.*//')")
      "$AA" report --percent 35 --activity 'CI 기다림'
      printf '[진행 중] 배경 CI를 기다린다\n\n## Report\n중간\n\n## Next\n- CI 확인\n' > "$dir/report.md.tmp"
      mv "$dir/report.md.tmp" "$dir/report.md"
      echo "interim" ;;
    *finish*)
      printf '[완료] 끝냈다\n\n## Report\n끝\n\n## Next\n- 병합\n' > "$dir/report.md.tmp"
      mv "$dir/report.md.tmp" "$dir/report.md"
      "$AA" report --percent 100 --activity Done
      echo "finished"
      # 끝낸 뒤 받은 입력을 보이지 않는다: host는 받음 확인(화면 바뀜)을 accept_wait_ms까지 기다린다
      case "$line" in *quiet*) stty -echo; exec cat > /dev/null ;; esac ;;
    *) echo "got: $line" ;;
  esac
  printf 'prog> '
done
"#;

/// Claude Code처럼 입력창(위 테두리·`❯ `·아래 테두리)을 그리는 harness(hook 없음, profile harness는 claude). 첫 Enter는
/// 무시해 brief 줄이 입력창에 남는다. 다음 Enter부터 받아 대화에 `❯ <줄>`·`got: <줄>`을 남기고 입력창을 비운 뒤, Claude처럼
/// 입력창 위에 일하는 줄(spinner)을 그린다: 화면 감지(plan §8.110)가 작업 중으로 보아 받았다고 안다.
/// 화면은 바뀔 때마다 통째로 다시 그린다(받은 조각을 한 번에 그려 느린 머신에서도 그리는 중에 Enter가 오지 않는다).
const STICKY_AGENT: &str = r#"#!/bin/sh
stty -icanon -echo min 1 time 0
R=$(printf '%.0s\342\224\200' 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30)
log=''; work=''; buf=''; n=0
draw() { printf '\033[2J\033[H%s%s%s\n\342\235\257 %s\n%s\n' "$log" "$work" "$R" "$buf" "$R"; }
draw
while :; do
  c=$(dd bs=4096 count=1 2>/dev/null)
  if [ -z "$c" ]; then
    n=$((n+1))
    [ "$n" -lt 2 ] && continue
    log="$log$(printf '\342\235\257 %s\ngot: %s' "$buf" "$buf")
"
    work="$(printf '\342\234\273 Working\342\200\246 (1s \302\267 esc to interrupt)')
"
    buf=''
  else
    buf="$buf$c"
  fi
  draw
done
"#;

/// 받은 바이트를 16진수로 `$PASTE_LOG`에 잇는 harness. Enter(0d)를 받을 때마다 `ok N`을 쓰고, 두 번째 Enter부터는
/// bracketed paste를 켠다(brief와 첫 메시지는 꺼진 채, 둘째 메시지는 켠 채로 받는다).
const PASTE_AGENT: &str = r#"#!/bin/sh
stty raw -echo
printf 'paste> '
n=0
while c=$(dd bs=4096 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n') && [ -n "$c" ]; do
  printf '%s' "$c" >> "$PASTE_LOG"
  case $c in *0d)
    n=$((n+1))
    [ "$n" -ge 2 ] && printf '\033[?2004h'
    printf '\r\nok %s\r\npaste> ' "$n" ;;
  esac
done
"#;

/// 시작하자마자 폴더 신뢰 확인창을 그리고 기다리는 harness(profile harness custom: hook도 화면 감지도 없다). brief 보내기가
/// 이 창을 보고 질문을 연다. 작업 폴더의 `ask.fifo`에 한 줄이 오면 끝난다(host가 없을 때 세션을 끝내는 시험용).
const ASKER_AGENT: &str = r#"#!/bin/sh
mkfifo ask.fifo
printf ' Quick safety check: Do you trust the files in this folder?\n\n \342\235\257 No, exit\n   Yes, I trust this folder\n\n Enter to confirm \302\267 Esc to cancel'
read x < ask.fifo
"#;

/// 이어 띄울 수 있는 가짜 Claude(harness claude, hook 없음): 대화 ID 인자를 보이고, STICKY_AGENT처럼 입력창 상자를 그려
/// Enter마다 받은 줄을 대화에 `got: <줄>`로 남긴다. 처음에는 `--session-id <id>`로, 이어 띄우면 `--resume <id>`로
/// 뜬다(agent threads.rs `resolve`). Enter 뒤에는 실제 감지기가 작업 중을 볼 때까지 그 화면을 유지한다. hook 없이
/// 입력 받음을 알리는 keeper이며, 시작 유예에 기대어 상태가 아직 없는 동안 화면 변화만으로 받던 경합을 피한다.
/// `EVIL_SID`가 있으면 줄을 받을 때마다 그 값을 세션 ID로 hook(`UserPromptSubmit`)을
/// 낸다(보안 시험: pane 프로그램이 세션 ID를 지어낸다). 한 번에 읽은 글 안의 줄 끝도 Enter다(부하 아래 친 글과 Enter가
/// 한 번에 읽혔다).
const RESUMABLE_AGENT: &str = r#"#!/bin/sh
stty -icanon -echo min 1 time 0
R=$(printf '%.0s\342\224\200' 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30)
# 대화 ID 인자만 한 줄로 보인다(긴 `--settings` 경로에 줄이 접히지 않게)
log=''
while [ $# -gt 0 ]; do
  case "$1" in --session-id|--resume) log="$1 $2
" ;; esac
  shift
done
buf=''; work=''; NL='
'
draw() { printf '\033[2J\033[H%s%s\n\342\235\257 %s\n%s\n%s' "$log" "$R" "$buf" "$R" "$work"; }
working() {
  work="$(printf '\342\234\273 Working\342\200\246 (1s \302\267 esc to interrupt)')
"
  draw
  # hook을 보내지 않는다. 실제 host가 화면 감지의 working 사건을 받은 뒤 쉼으로 돌아간다.
  # labB는 같은 실험실의 논리상 원격이다. 관측용 CLI는 host의 Front socket을 쓴다.
  python3 - "$AA" "$AAA_SESSION_ID" "$AA_TEST_HOST_STATE" <<'PY'
import json, subprocess, sys
while True:
    result = subprocess.run([sys.argv[1], '--state-dir', sys.argv[3], '--json', 'pane', 'get', sys.argv[2]], capture_output=True, text=True)
    if result.returncode:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
        result.check_returncode()
    if (json.loads(result.stdout).get('agent') or {}).get('activity') == 'working':
        break
PY
  [ "$?" -eq 0 ] || exit 1
  work=''
}
draw
while :; do
  c=$(dd bs=4096 count=1 2>/dev/null; printf x); c=${c%x}
  [ -n "$c" ] || break
  while :; do
    case "$c" in
      *"$NL"*)
        buf="$buf${c%%"$NL"*}"; c=${c#*"$NL"}
        log="$log$(printf 'got: %s' "$buf")
"
        buf=''
        [ -n "$EVIL_SID" ] && "$AA" hook --agent claude "{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"$EVIL_SID\"}" >/dev/null
        working ;;
      *) buf="$buf$c"; break ;;
    esac
  done
  draw
done
"#;

/// 프로젝트 폴더를 cwd로 쓰는 coordinator(hook 없음). 입력창은 Claude처럼 「❯ 」다.
const COORD_AGENT: &str = "#!/bin/sh\nprintf '\\342\\235\\257 '\nwhile IFS= read -r line; do\n  case \"$line\" in post-probe\\ *) sh \"${line#post-probe }\"; echo post-probe-done ;; esac\n  echo \"got: $line\"\n  printf '\\342\\235\\257 '\ndone\n";

/// `extra`는 `[projects]`에 덧붙인다. `web`이면 웹을 loopback 임의 port로 연다.
fn host_toml(projects: &Path, extra: &str, web: bool) -> String {
    let listen = if web { "127.0.0.1:0" } else { "" };
    format!(
        "schema = 1\n[host]\nagent_listen = \"127.0.0.1:0\"\n[web]\nlisten = \"{listen}\"\n[local_agent]\nmode = \"none\"\n\
         [projects]\nroot = \"{}\"\nallow_profiles = [\"fake\", \"mute\", \"claude\", \"sticky\", \"prog\", \"asker\", \"paste\", \"resumable\", \"evil\"]\nlibrary_copy_mb = 1\n{extra}",
        path_str(projects)
    )
}

fn fixture_profiles(scripts: &Path, host_state: &Path) -> String {
    format!(
        "[profiles.fake]\nagent = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\n\
         [profiles.mute]\nagent = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ MUTE_LOG = \"{}\" }}\n\
         [profiles.claude]\nagent = \"claude\"\ncommand = [\"{}\"]\nenv = {{ AA = \"{}\", HOOK_LOG = \"{}\", CLAUDE_MENU = \"{}\" }}\n\
         [profiles.sticky]\nagent = \"claude\"\ncommand = [\"{}\"]\n\
         [profiles.prog]\nagent = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ AA = \"{}\" }}\n\
         [profiles.paste]\nagent = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ PASTE_LOG = \"{}\" }}\n\
         [profiles.asker]\nagent = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\n\
         [profiles.resumable]\nagent = \"claude\"\ncommand = [\"{}\"]\nenv = {{ AA = \"{}\", AA_TEST_HOST_STATE = {host_state:?} }}\n\
         [profiles.evil]\nagent = \"claude\"\ncommand = [\"{}\"]\nenv = {{ AA = \"{}\", AA_TEST_HOST_STATE = {host_state:?}, EVIL_SID = \"--dangerously-skip-permissions\" }}\n\
         [profiles.other]\nagent = \"custom\"\ncommand = [\"/bin/sh\"]\n",
        path_str(&scripts.join("fake.sh")),
        path_str(&scripts.join("mute.sh")),
        path_str(&scripts.join("mute.log")),
        path_str(&scripts.join("claude.sh")),
        lab::BIN,
        path_str(&scripts.join("hook.log")),
        path_str(&scripts.join("claude-menu.py")),
        path_str(&scripts.join("sticky.sh")),
        path_str(&scripts.join("prog.sh")),
        lab::BIN,
        path_str(&scripts.join("paste.sh")),
        path_str(&scripts.join("paste.log")),
        path_str(&scripts.join("asker.sh")),
        path_str(&scripts.join("resumable.sh")),
        lab::BIN,
        path_str(&scripts.join("resumable.sh")),
        lab::BIN,
        host_state = path_str(host_state),
    )
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .capture_output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// origin(bare)과 그 clone(origin/HEAD 있음)
fn make_repo(root: &Path) -> PathBuf {
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "-q", "-b", "main"]);
    std::fs::write(seed.join("README.md"), "hi\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-qm", "init"]);
    let origin = root.join("origin.git");
    git(root, &["clone", "-q", "--bare", path_str(&seed), path_str(&origin)]);
    let repo = root.join("repo");
    git(root, &["clone", "-q", path_str(&origin), path_str(&repo)]);
    // PR 시험은 origin을 github 주소로 바꾼다: thread 시작의 fetch가 github.com에 나가거나 credential helper(키체인)를
    // 부르지 않게 https 전송을 막는다(로컬 ref로 진행한다). worktree도 이 설정을 쓴다.
    git(&repo, &["config", "protocol.https.allow", "never"]);
    repo
}

struct World {
    lab: Lab,
    h: Node,
    projects: PathBuf,
    repo: PathBuf,
    scripts: PathBuf,
    addr: String,
    /// host를 띄울 때 주는 brief 화면 멈춤(`spawn_host`). 다시 띄운 host도 같은 값이다
    settle_ms: Option<&'static str>,
    host_env: Vec<(String, String)>,
}

// 바이트 보존 keeper는 식별 이주가 끝난 고정 fixture를 쓴다. 식별 이주는 host task_ids owner가 검사한다.
fn identified_tasks(text: &str) -> String {
    let mut n = 0;
    text.split_inclusive('\n')
        .map(|line| {
            if !line.trim_start().starts_with("- [") {
                return line.to_string();
            }
            n += 1;
            if line.contains(" <!-- aaa-task:") {
                return line.to_string();
            }
            let body = line.trim_end_matches(['\r', '\n']);
            format!("{body} <!-- aaa-task:{n:026} -->{}", &line[body.len()..])
        })
        .collect()
}

fn world() -> World {
    world_with("")
}

fn world_with(extra: &str) -> World {
    world_opts(extra, false)
}

fn world_opts(extra: &str, web: bool) -> World {
    world_setup(extra, web, |_| Vec::new())
}

/// `agent_env`는 실험실 root를 받아 agent labA에 더 줄 환경 변수를 돌려준다(예: 가짜 `gh`를 PATH 앞에)
fn world_setup(extra: &str, web: bool, agent_env: impl FnOnce(&Path) -> Vec<(String, String)>) -> World {
    world_build(extra, web, Some(BRIEF_SETTLE_MS), agent_env)
}

fn world_files(
    extra: &str,
    web: bool,
    settle_ms: Option<&'static str>,
    with_repo: bool,
    agent_env: impl FnOnce(&Path) -> Vec<(String, String)>,
) -> (World, Vec<(String, String)>) {
    let lab = Lab::new();
    let env = agent_env(&lab.root);
    let projects = lab.root.join("projects");
    let scripts = lab.root.join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("fake.sh"), FAKE_AGENT).unwrap();
    std::fs::write(scripts.join("mute.sh"), MUTE_AGENT).unwrap();
    std::fs::write(scripts.join("claude.sh"), CLAUDE_AGENT).unwrap();
    std::fs::write(scripts.join("claude-menu.py"), include_str!("fixtures/claude-menu.py")).unwrap();
    std::fs::write(scripts.join("sticky.sh"), STICKY_AGENT).unwrap();
    std::fs::write(scripts.join("prog.sh"), PROGRESS_AGENT).unwrap();
    std::fs::write(scripts.join("asker.sh"), ASKER_AGENT).unwrap();
    // harness claude는 실행 파일 뒤에 `--settings …`·`--session-id …`를 붙이므로 sh가 아니라 스크립트를 바로 띄운다
    for f in ["claude.sh", "sticky.sh"] {
        std::fs::set_permissions(scripts.join(f), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    std::fs::write(scripts.join("coord.sh"), COORD_AGENT).unwrap();
    std::fs::write(scripts.join("paste.sh"), PASTE_AGENT).unwrap();
    // harness claude는 실행 파일 뒤에 인자를 붙이므로 스크립트를 바로 띄운다
    std::fs::write(scripts.join("resumable.sh"), RESUMABLE_AGENT).unwrap();
    std::fs::set_permissions(scripts.join("resumable.sh"), std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .unwrap();
    let repo = if with_repo {
        make_repo(&lab.root)
    } else {
        let repo = lab.root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        repo
    };
    let h = lab.node("h", None);
    std::fs::write(
        h.cfg.join("host.toml"),
        format!("{}\n{}", host_toml(&projects, extra, web), fixture_profiles(&scripts, &h.state)),
    )
    .unwrap();
    std::fs::write(
        h.cfg.join("agent.toml"),
        format!("schema = 1\n[agent]\nwork_root = {:?}\n[reconnect]\nmax_backoff_s = 1\n", lab.root.join("workA")),
    )
    .unwrap();
    // 가짜 외부 조회 주기만 host에 전달한다. PATH·인증 등 agent 환경을 host에 섞지 않는다.
    let host_env = env.iter().filter(|(key, _)| key == "AAA_TEST_WATCH_POLL_MS").cloned().collect();
    let w = World { lab, h, projects, repo, scripts, addr: String::new(), settle_ms, host_env };
    (w, env)
}

fn world_build(
    extra: &str,
    web: bool,
    settle_ms: Option<&'static str>,
    agent_env: impl FnOnce(&Path) -> Vec<(String, String)>,
) -> World {
    let (w, env) = world_files(extra, web, settle_ms, true, agent_env);
    world_start(w, env)
}

/// FAKE_AGENT의 줄 단위 보고 셸만 쓰는 owner. TUI 입력창·paste가 없으므로
/// 적용되지 않는 화면 안정/paste 간격을 빼고 실제 brief 수신·보고 사건을 기다린다.
/// CLAUDE_AGENT·PASTE_AGENT와 제품 기본 간격은 기존 world가 그대로 검사한다.
fn report_shell_world() -> World {
    let (mut w, env) = world_files("", false, Some("0"), true, |_| Vec::new());
    w.host_env.push(("AAA_TEST_SUBMIT_GAP_MS".into(), "0".into()));
    world_start(w, env)
}

fn world_start(mut w: World, env: Vec<(String, String)>) -> World {
    w.spawn_host("host");
    let st = wait_for("host 준비", T, || w.h.json(&["status"]).ok());
    w.addr = st["agent_listen"].as_str().unwrap().to_string();
    // agent를 Claude Code 세션 안에서 띄운 경우: 그 세션 표지는 aaa 세션에 넘기지 않는다
    let mut envs = vec![("CLAUDE_CODE_CHILD_SESSION", "1"), ("CLAUDECODE", "1")];
    envs.extend(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    // 연결 전부터 같은 Front 연결로 상태 사건을 받는다. CLI 프로세스의 종료와 다음 조회에
    // 준비 판정을 맡기면, agent가 붙었어도 이전 빈 목록/조회 오류만 보고 시한을 넘길 수 있다.
    let mut watch = HostWatch::new(&w.h, T);
    w.lab.spawn_env("agentA", &w.h, &["agent", "--host", "local", "--name", "labA"], &envs);
    watch.wait_diag(
        "labA 연결",
        |s| s.connected("labA").then_some(()),
        || format!("host.log:\n{}\nagentA.log:\n{}", w.lab.log("host"), w.lab.log("agentA")),
    );
    w.h.ok(&["project", "create", "demo"]);
    // 기존 오케스트레이션 keeper는 작업 가능한 실험실을 명시한다. 새 기본 꺼짐은 workspace_access와 E2E99가 소유한다.
    w.h.ok(&["machine", "workspaces", "labA", "--allow", "작업 폴더"]);
    w
}

impl World {
    /// 공통 fake 보고의 두 Next는 해당 시험에서 확인을 마친 뒤 명시적으로 기록한다.
    fn finish_fixture_followups(&self, id: &str) {
        let reason = self.task("fixture-followup-reason.txt", "시험 결과 확인과 다음 단계 검토 완료");
        for entry in self.show(id)["followups"].as_array().unwrap() {
            if entry["handled_ms"].is_null() {
                self.h.ok(&[
                    "thread",
                    "followup",
                    "demo",
                    id,
                    "--id",
                    entry["id"].as_str().unwrap(),
                    "--reason-file",
                    &reason,
                ]);
            }
        }
    }
    fn spawn_host(&mut self, tag: &str) {
        let command = self.host_command(Path::new(lab::BIN));
        self.lab.spawn_command(tag, command);
    }

    /// 배포 사본도 이 World의 화면 안정·외부 조회 설정을 유지한다.
    fn host_command(&self, exe: &Path) -> Command {
        let env: Vec<_> = self.host_env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        host_command(&self.h, exe, self.settle_ms, &env)
    }

    fn task(&self, name: &str, text: &str) -> String {
        let p = self.lab.root.join(name);
        std::fs::write(&p, text).unwrap();
        path_str(&p).to_string()
    }

    fn start(&self, args: &[&str]) -> Value {
        let mut a = vec!["thread", "start", "demo"];
        a.extend_from_slice(args);
        self.h.ok(&a)
    }

    fn show(&self, id: &str) -> Value {
        self.h.ok(&["thread", "show", "demo", id])
    }

    fn wait_show(&self, what: &str, id: &str, f: impl Fn(&Value) -> bool) -> Value {
        self.wait_show_with_timeout(what, id, T, f)
    }

    fn wait_show_with_timeout(&self, what: &str, id: &str, timeout: Duration, f: impl Fn(&Value) -> bool) -> Value {
        let began = Instant::now();
        // 먼저 구독하고 CLI 사본을 읽어 조회와 사건 사이의 틈을 없앤다.
        // 실제 show 계약은 한 번 읽으며, 이후 관측마다 CLI를 다시 띄우지 않는다.
        let mut events = Events::connect(&self.h);
        let initial = self.show(id);
        if f(&initial) {
            return initial;
        }
        events.wait(what, timeout.saturating_sub(began.elapsed()), |event| {
            let FrontEvent::Thread(thread) = event else { return None };
            if thread.project != "demo" || thread.id != id {
                return None;
            }
            let view = serde_json::to_value(thread.as_ref()).unwrap();
            f(&view).then_some(view)
        })
    }

    fn questions(&self, all: bool) -> Vec<Value> {
        let a: &[&str] = if all { &["question", "list", "--all"] } else { &["question", "list"] };
        self.h.ok(a)["questions"].as_array().unwrap().clone()
    }

    fn message(&self, id: &str, text: &str, interrupt: bool) -> Value {
        let mut a = vec!["thread", "message", "demo", id, "--text", text];
        if interrupt {
            a.push("--interrupt");
        }
        self.h.ok(&a)
    }

    /// interrupt 메시지를 보내고 그 줄을 harness가 받았다고 화면에 보이기를 기다린다. CLI는 host가 멈춤을 확인하고
    /// 보낼 때까지 돌아오지 않으므로 따로 돈다: 멈춤을 알지 못하면 stop_wait 동안 막혀 화면 기다림(T)이 먼저 실패한다.
    fn interrupt(&self, id: &str, pane: &str, text: &str) -> (Value, String) {
        std::thread::scope(|s| {
            let sent = s.spawn(|| self.message(id, text, true));
            let screen = self.wait_screen(pane, &format!("got: {text}"));
            (sent.join().unwrap(), screen)
        })
    }

    fn wait_screen(&self, pane: &str, needle: &str) -> String {
        // visible 사본에서 같은 substring을 검사하는 실제 CLI다. host가 출력
        // 사건으로 완료하므로 100ms마다 read 프로세스를 띄우지 않는다.
        let matched = self
            .h
            .json(&[
                "pane",
                "wait-output",
                pane,
                &format!("--match={needle}"),
                "--source",
                "visible",
                "--timeout",
                &T.as_millis().to_string(),
            ])
            .unwrap_or_else(|e| panic!("화면 {needle:?}: {e}\n{}", self.h.screen(pane)));
        matched["read"]["text"].as_str().expect("맞은 visible 화면").to_string()
    }

    fn kinds(&self, after: u64) -> Vec<String> {
        let v = self.h.ok(&["events", "demo", "--after", &after.to_string()]);
        v["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect()
    }

    /// 원격(TLS) agent labB를 등록하고 띄운다
    fn add_remote(&mut self) {
        let b = self.enroll_remote();
        let mut watch = HostWatch::new(&self.h, T);
        self.lab.spawn("agentB", &b, &["agent", "--host", &self.addr, "--name", "labB"]);
        watch.wait_diag(
            "labB 연결",
            |s| s.connected("labB").then_some(()),
            || format!("host.log:\n{}\nagentB.log:\n{}", self.lab.log("host"), self.lab.log("agentB")),
        );
    }

    /// 사본 agent를 직접 띄우는 keeper도 같은 실제 TLS 등록을 먼저 마친다.
    fn enroll_remote(&mut self) -> Node {
        let b = self.lab.node("b", None);
        std::fs::write(
            b.cfg.join("agent.toml"),
            format!("schema = 1\n[agent]\nwork_root = {:?}\n", self.lab.root.join("workB")),
        )
        .unwrap();
        self.h.ok(&["machine", "enroll", "--open", "5m"]);
        let (_, mut out) = self.lab.spawn_piped("enroll", &b, &["agent", "enroll", &self.addr, "--name", "labB"]);
        let line = read_line_containing(&mut out, "--code");
        let code = line.split("--code").nth(1).unwrap().trim().to_string();
        self.h.ok(&["machine", "approve", "labB", "--code", &code]);
        assert!(self.lab.wait_exit("enroll", T).success(), "{}", self.lab.log("enroll"));
        b
    }
}

/// lab의 CLI 실행은 다른 프로세스가 출력 쓰기 끝을 들고 있어도 자식이 끝나면 돌아온다(t-1097). 병렬 시험의 다른 thread가 띄운
/// host·agent가 macOS pipe의 쓰기 끝을 물려받은 경우와 같은 모양을, 출력을 든 채 남는 손자로 결정적으로 만든다.
#[cfg(unix)]
#[test]
fn lab_capture_returns_while_another_process_holds_the_output() {
    let o = lab::captured(Command::new("/bin/sh").args(["-c", "sleep 600 & echo $!"])).expect("sh 실행");
    assert!(o.status.success(), "{o:?}");
    let holder = String::from_utf8_lossy(&o.stdout).trim().to_string();
    // 돌아온 때 손자가 아직 출력을 들고 있었다(pipe였다면 이 손자가 끝날 때까지 돌아오지 않는다)
    let alive = Command::new("kill").args(["-0", &holder]).status_guarded().expect("kill -0").success();
    let _ = Command::new("kill").arg(&holder).status_guarded();
    assert!(alive, "출력을 든 손자 {holder}가 돌아오기 전에 끝났다");
}

/// 준비 대기는 구독 전의 현재 목록과 구독 뒤의 연결 변화를 모두 받아야 한다.
#[test]
fn agent_readiness_uses_initial_state_and_reconnection_events() {
    let mut w = world();
    let mut watch = HostWatch::new(&w.h, T);
    watch.wait("구독 전 연결의 현재 상태", |s| s.connected("labA").then_some(()));
    w.lab.kill("agentA");
    watch.wait("연결 종료 사건", |s| (!s.connected("labA")).then_some(()));
    w.lab.spawn("agentA-reconnected", &w.h, &["agent", "--host", "local", "--name", "labA"]);
    watch.wait("구독 뒤 재연결 사건", |s| s.connected("labA").then_some(()));
}

/// 생성 폴더의 파일 계약: 안내 별칭 등 예정하지 않은 파일도 잡는다.
fn assert_entries(dir: &Path, expected: &[&str]) {
    let mut names: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    names.sort();
    let mut expected: Vec<_> = expected.iter().map(std::ffi::OsString::from).collect();
    expected.sort();
    assert_eq!(names, expected, "{}의 생성 파일 목록", dir.display());
}

/// worktree thread 한 바퀴: 시작 → brief 안내 → 보고·library 사본 → ack → resolve(main 조상 branch를 정리한다)
#[test]
fn worktree_thread_round_trip() {
    let w = report_shell_world();
    let task = w.task("task1.txt", "보고에 X-42를 적는다 link");
    let v = w.start(&[
        "--title",
        "Fix the bug",
        "--machine",
        "labA",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &task,
    ]);
    assert_eq!((v["id"].as_str(), v["status"].as_str()), (Some("t-0001"), Some("open")), "{v}");
    assert_eq!(v["branch"], "aaa/demo/t-0001-fix-the-bug");
    // 웹 「작업」 트리가 저장소 폴더로 묶는 칸(plan §8.260·§8.280): 시작할 때 고른 repo 경로 그대로
    assert_eq!(v["repo"].as_str(), Some(path_str(&w.repo)), "{v}");
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    assert_eq!(cwd, w.lab.root.join("workA").join("repo").join("demo-t-0001"));
    assert!(cwd.join(".aaa/demo-t-0001/brief.md").is_file());
    let brief = std::fs::read_to_string(cwd.join(".aaa/demo-t-0001/brief.md")).unwrap();
    assert!(brief.contains("X-42") && brief.contains("# thread brief") && brief.contains("# 지침"), "{brief}");
    // 작업 폴더·aaa 실행 파일 자리는 agent가 그 머신 값으로 채웠다(plan §8.78)
    let work = cwd.join(".aaa/demo-t-0001");
    assert!(brief.contains(&format!("- 보고: `{}/report.md`", work.display())), "{brief}");
    assert!(brief.contains(&format!("`{} report --percent 25", lab::BIN)), "{brief}");
    assert!(!brief.contains("{{aaa:") && brief.contains("[진행 중]") && brief.contains("[완료]"), "{brief}");
    assert!(w.projects.join("demo/threads/t-0001.task.md").is_file());

    // 보고 사본과 brief 받음 확인은 따로 온다(agent의 보고 감시와 host의 Enter 뒤 화면 보기): 둘 다 기다린다
    let v = w.wait_show("보고 사본·brief 받음", "t-0001", |v| {
        v["report_hash"].is_string() && v["brief"] == "delivered"
    });
    assert_entries(&cwd, &[".git", ".aaa", "README.md"]);
    assert_entries(&work, &["brief.md", "report.md", "library"]);
    assert_eq!(v["next"], serde_json::json!(["결과 확인", "다음 단계"]));
    assert_eq!(v["group"], "ready_for_review");
    // symlink는 복사하지 않아 사본은 partial이다
    assert_eq!(v["copy"], "partial");
    let copied = std::fs::read_to_string(w.projects.join("demo/threads/t-0001.md")).unwrap();
    assert!(copied.contains("X-42"), "{copied}");
    let lib = w.projects.join("demo/library/t-0001");
    assert_eq!(std::fs::read_to_string(lib.join("out.txt")).unwrap(), "결과물\n");
    assert_eq!(std::fs::read_to_string(lib.join("sub/d.txt")).unwrap(), "deep\n");
    assert!(std::fs::symlink_metadata(lib.join("link")).is_err(), "symlink가 복사됐다");
    // 작업 폴더는 git 상태에 보이지 않는다(info/exclude)
    assert_eq!(git(&cwd, &["status", "--porcelain"]), "");

    // 확인한 뒤 harness가 일하지 않으면 「쉼」이다(herdr-projects `group()` 7번 줄)
    let v = w.h.ok(&["thread", "ack", "demo", "t-0001"]);
    assert_eq!(v["group"], "idle");

    // 끝내기는 마지막 사본을 다시 받는다: 복사하지 못한 symlink를 치우면 사본이 다 와(complete) worktree를 지운다
    // (다 오지 않았으면 남긴다, `resolve_keeps_dirty_worktree_and_unmerged_branch`)
    std::fs::remove_file(work.join("library/link")).unwrap();
    let v = w.h.ok(&["thread", "resolve", "demo", "t-0001"]);
    assert_eq!((v["status"].as_str(), v["copy"].as_str()), (Some("resolved"), Some("complete")), "{v}");
    // 기본 목록은 끝난 thread도 보존하며 --open은 끝난 항목을 거른다. 프로젝트는 위치 인자다.
    let listed = w.h.ok(&["thread", "list", "demo"]);
    assert_eq!(listed["threads"][0]["group"], "resolved", "{listed}");
    assert!(w.h.ok(&["thread", "list", "--open"])["threads"].as_array().unwrap().is_empty());
    assert_eq!(w.h.ok(&["thread", "list", "--status", "resolved"])["threads"][0]["id"], "t-0001");
    let text = String::from_utf8_lossy(&w.h.run(&["thread", "list", "demo"]).stdout).to_string();
    assert!(text.starts_with("demo/t-0001\t끝남\t"), "{text}");
    assert!(!cwd.exists(), "고치지 않은 worktree는 지워져야 한다");
    // 현재 branch 끝이 원격 main에 포함되므로 branch도 지운다
    assert_eq!(git(&w.repo, &["branch", "--list", "aaa/demo/*", "--format=%(refname:short)"]), "");
    let resolved = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().last().unwrap().clone();
    assert!(
        resolved["detail"].as_str().unwrap().contains("branch aaa/demo/t-0001-fix-the-bug를 지웠다(main에 포함됐다)"),
        "{resolved}"
    );
    // 자리(worktree)를 치웠으니 pane도 닫는다(herdr-projects `clean`은 그 workspace를 닫는다): 목록에 끝난 줄이 남지 않는다
    let pane = v["pane"].as_str().unwrap();
    let panes = w.h.ok(&["pane", "list"]);
    assert!(
        !panes["panes"].as_array().unwrap().iter().any(|p| p["id"] == pane),
        "끝낸 thread의 pane이 남았다: {panes}"
    );
    assert!(resolved["detail"].as_str().unwrap().contains("pane을 닫았다"), "{resolved}");
    // 마지막 사본은 받은 파일이 그대로라 따로 알리지 않는다(사본 상태만 complete로 바뀐다)
    // 보고 회수와 brief 받음 확인은 독립 경로다. 둘은 시작 뒤·ack 전에 정확히 한 번씩 와야 한다.
    let mut kinds = w.kinds(0);
    assert_eq!(kinds.len(), 5, "{kinds:?}");
    kinds[1..3].sort();
    assert_eq!(kinds, ["thread_started", "brief_delivered", "report_ready", "review_acked", "thread_resolved"]);
    // cursor 뒤만 읽는다
    assert_eq!(w.kinds(3), ["review_acked", "thread_resolved"]);
}

/// 고친 worktree와 병합되지 않은 branch는 남긴다(force 없음). 허용되지 않은 profile은 거부한다.
/// pane은 worktree 보존과 별개로 닫는다. 작업 폴더 thread와 이미 지운 worktree(`worktree_gone`)도 같다.
/// 집에 다 오지 않은 worktree도 남긴다(`clean` `copy_complete`): `--discard-uncopied`면 지우고, `--skip-copy`는 사본 없이
/// 끝내며 남긴다.
#[test]
fn resolve_keeps_dirty_worktree_and_unmerged_branch() {
    let w = report_shell_world();
    let e =
        w.h.json(&[
            "thread",
            "start",
            "demo",
            "--title",
            "x",
            "--profile",
            "other",
            "--repo",
            path_str(&w.repo),
            "--task-file",
            &w.task("t0.txt", "x"),
        ])
        .expect_err("허용되지 않은 profile");
    assert!(e.contains("forbidden"), "{e}");

    let task = w.task("task2.txt", "commit 하고 dirty 파일을 남긴다");
    let v = w.start(&["--title", "keep", "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
    let id = v["id"].as_str().unwrap().to_string();
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    let pane = v["pane"].as_str().unwrap().to_string();
    // fake agent는 안내 줄을 받자마자 보고하므로, host가 전달을 확인해 brief_delivered를 남기는 것이 보고보다 늦을 수 있다.
    // 둘 다 본 뒤 resolve해야 마지막 사건이 thread_resolved다(부하 아래 brief_delivered가 resolve 뒤에 남아 실패했다)
    w.wait_show("보고", &id, |v| v["report_hash"].is_string() && v["brief"] == "delivered");
    // 끝내기 전 확인(관리 GUI W4): 커밋하지 않은 파일·병합되지 않은 변경을 읽고 남기기를 권한다. 기록·사건은 그대로다
    let before = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().len();
    let p = w.h.ok(&["thread", "resolve", "demo", &id, "--dry-run"]);
    assert_eq!((p["kind"].as_str(), p["thread_kind"].as_str()), (Some("resolve_preview"), Some("worktree")), "{p}");
    assert!(p["uncommitted"].as_u64().is_some_and(|n| n >= 1), "{p}");
    assert!(p["changed"].as_u64() > p["uncommitted"].as_u64(), "commit한 파일도 base 대비 바뀐 것이다: {p}");
    assert_eq!((p["merged"].as_bool(), p["keep_default"].as_bool()), (Some(false), Some(true)), "{p}");
    let risks = p["risks"].to_string();
    assert!(risks.contains("커밋하지 않은 파일") && risks.contains("병합되지 않은 변경"), "{p}");
    assert_eq!(w.h.ok(&["thread", "show", "demo", &id])["status"], "open", "미리보기는 끝내지 않는다");
    assert_eq!(w.h.ok(&["events", "demo"])["events"].as_array().unwrap().len(), before);
    let v = w.h.ok(&["thread", "resolve", "demo", &id]);
    assert_eq!(v["status"], "resolved");
    assert!(cwd.join("dirty.txt").exists(), "고친 worktree는 남아야 한다");
    assert_eq!(git(&w.repo, &["branch", "--list", "aaa/demo/*", "--format=%(refname:short)"]), "aaa/demo/t-0001-keep");
    let ev = w.h.ok(&["events", "demo"]);
    let last = ev["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["kind"], "thread_resolved");
    assert!(last["detail"].as_str().unwrap().contains("worktree 유지"), "{last}");
    assert!(last["detail"].as_str().unwrap().contains("pane을 닫았다"), "{last}");
    assert!(!w.h.ok(&["pane", "list"])["panes"].as_array().unwrap().iter().any(|p| p["id"] == pane));

    let listed = |p: &str| w.h.ok(&["pane", "list"])["panes"].as_array().unwrap().iter().any(|x| x["id"] == p);
    // 끝낸 항목은 그 thread의 thread_resolved 사건에서 읽는다(늦게 남는 brief_delivered가 마지막 사건일 수 있다, 위)
    let resolved_with = |id: &str, flags: &[&str]| {
        let mut a = vec!["thread", "resolve", "demo", id];
        a.extend_from_slice(flags);
        w.h.ok(&a);
        let ev = w.h.ok(&["events", "demo"]);
        let found =
            ev["events"].as_array().unwrap().iter().rev().find(|e| e["kind"] == "thread_resolved" && e["thread"] == id);
        found.unwrap_or_else(|| panic!("{id} thread_resolved가 없다: {ev}"))["detail"].as_str().unwrap().to_string()
    };
    let ready = |id: &str| w.wait_show("보고", id, |v| v["report_hash"].is_string() && v["brief"] == "delivered");
    let resolved = |id: &str| resolved_with(id, &[]);
    // 작업 폴더 thread는 늘 닫는다
    // `--summary-file`: 시작한 뒤 작업 요약을 쓴다(plan §8.259). JSON 출력은 thread 한 줄 그대로다
    let summary = w.task("d-summary.md", "# 목표\n작업 폴더 닫기\n");
    let d = w.start(&[
        "--title",
        "dir",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("d.txt", "x"),
        "--summary-file",
        &summary,
    ]);
    assert!(w.show(d["id"].as_str().unwrap())["summary"]["at_ms"].is_u64(), "요약을 쓰지 않았다");
    let (did, dpane) = (d["id"].as_str().unwrap().to_string(), d["pane"].as_str().unwrap().to_string());
    ready(&did);
    let detail = resolved(&did);
    assert!(detail.contains("pane을 닫았다"), "{detail}");
    assert!(!listed(&dpane), "작업 폴더 thread의 pane이 남았다");
    // 사람이 이미 지운 worktree는 잃을 것이 없다: 지운 것으로 치고(git 기록도 치운다) pane을 닫는다
    let g = w.start(&[
        "--title",
        "gone",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("g.txt", "x"),
    ]);
    let (gid, gpane) = (g["id"].as_str().unwrap().to_string(), g["pane"].as_str().unwrap().to_string());
    let gcwd = PathBuf::from(g["cwd"].as_str().unwrap());
    ready(&gid);
    std::fs::remove_dir_all(&gcwd).unwrap();
    let detail = resolved(&gid);
    assert!(detail.contains("worktree 삭제") && detail.contains("worktree는 이미 없다"), "{detail}");
    assert!(detail.contains("pane을 닫았다"), "{detail}");
    assert!(!listed(&gpane), "지운 worktree thread의 pane이 남았다");
    assert!(!git(&w.repo, &["worktree", "list"]).contains(path_str(&gcwd)), "git worktree 기록이 남았다");

    // 집에 다 오지 않은 worktree(복사하지 않는 symlink)는 남긴다: 작업 폴더(`.aaa/`)는 git이 무시하는 파일이라 force 없는
    // `worktree remove`도 지운다(시험 repo는 고친 파일이 없다)
    let partial = |title: &str| {
        let v = w.start(&[
            "--title",
            title,
            "--profile",
            "fake",
            "--repo",
            path_str(&w.repo),
            "--task-file",
            &w.task(&format!("{title}.txt"), "link"),
        ]);
        let id = v["id"].as_str().unwrap().to_string();
        assert_eq!(ready(&id)["copy"], "partial");
        (id, v["pane"].as_str().unwrap().to_string(), PathBuf::from(v["work_dir"].as_str().unwrap()))
    };
    let (pid, ppane, pwork) = partial("part");
    let detail = resolved(&pid);
    assert!(detail.contains("worktree 유지") && detail.contains("마지막 사본에 집에 오지 않은 것이 있다"), "{detail}");
    assert!(detail.contains("집에 오지 않은 것: link: symlink"), "{detail}");
    assert!(pwork.join("library/out.txt").is_file(), "집에 다 오지 않은 worktree를 지웠다");
    assert!(!listed(&ppane), "남긴 worktree의 pane도 닫는다");
    // 사람이 고르면 지운다
    let (xid, xpane, xwork) = partial("discard");
    let detail = resolved_with(&xid, &["--discard-uncopied"]);
    assert!(detail.contains("worktree 삭제") && detail.contains("pane을 닫았다"), "{detail}");
    assert!(!xwork.exists() && !listed(&xpane), "{detail}");
    // 사본 없이 끝내면 집에 온 것을 모르니 남긴다
    let s = w.start(&[
        "--title",
        "skip",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("skip.txt", "x"),
    ]);
    let sid = s["id"].as_str().unwrap().to_string();
    let swork = PathBuf::from(s["work_dir"].as_str().unwrap());
    ready(&sid);
    let e = w.h.json(&["thread", "resolve", "demo", &sid, "--reopen", "--skip-copy"]).expect_err("reopen과 skip-copy");
    assert!(e.contains("cannot be used with"), "{e}");
    let detail = resolved_with(&sid, &["--skip-copy"]);
    assert!(detail.contains("마지막 사본 없이 끝냈다") && detail.contains("worktree 유지"), "{detail}");
    assert!(swork.join("report.md").is_file(), "사본 없이 끝낸 worktree를 지웠다");
}

/// 원격(TLS) 머신의 작업 폴더 thread, host 재시작 뒤 보고 감시 재개, 평가자 역할 필터
#[test]
fn remote_dir_thread_survives_host_restart() {
    let mut w = report_shell_world();
    w.add_remote();
    let v = w.start(&[
        "--title",
        "원격 일",
        "--machine",
        "labB",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("t3.txt", "원격 R-7"),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    assert_eq!(PathBuf::from(v["cwd"].as_str().unwrap()), w.lab.root.join("workB").join("demo").join(&id));
    assert_eq!(v["branch"], Value::Null);
    // 작업 폴더 thread는 저장소가 없어 칸을 싣지 않는다(웹은 「기타」)
    assert!(v.get("repo").is_none(), "{v}");
    // brief 받음 확인까지 기다린다: host를 내리기 전에 끝나지 않으면 새 host가 화면으로 확인해 사건이 하나 더 생긴다
    let v =
        w.wait_show("원격 보고·brief 받음", &id, |v| v["report_hash"].is_string() && v["brief"] == "delivered");
    assert_eq!(v["machine"], "labB");
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    assert_entries(&cwd, &[".aaa"]);
    assert_entries(&cwd.join(".aaa").join(format!("demo-{id}")), &["brief.md", "report.md", "library"]);
    assert!(std::fs::read_to_string(w.projects.join("demo/threads").join(format!("{id}.md"))).unwrap().contains("R-7"));
    let pane = v["pane"].as_str().unwrap().to_string();
    // thread 세션은 worker 역할이다
    let workers = w.h.ok(&["pane", "list", "--role", "worker"]);
    assert!(workers.to_string().contains(&pane));
    assert_eq!(w.h.ok(&["pane", "list", "--role", "evaluator"])["panes"], serde_json::json!([]));
    let seen = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().len() as u64;

    // host 재시작 중에 보고가 바뀌어도, 다시 붙은 agent가 지금 hash를 알려 사본을 다시 받는다
    // 원격 agent가 같은 주소로 다시 붙게 처음 받은 포트를 고정한다
    // 알림의 시간 경계·재시작 영속성은 inbox의 가상 시각 owner가 맡는다.
    // 실제 TLS 재연결 속도를 알림 기준 5초와 비교하지 않는다.
    let toml = std::fs::read_to_string(w.h.cfg.join("host.toml")).unwrap();
    std::fs::write(w.h.cfg.join("host.toml"), toml.replace("127.0.0.1:0", &w.addr)).unwrap();
    w.lab.kill("host");
    // host가 없는 동안 agent 머신에서 보고가 바뀐다
    let work_dir = PathBuf::from(v["work_dir"].as_str().unwrap());
    std::fs::write(work_dir.join("report.md"), "## Report\n갱신\n\n## Next\n- 사용자 확인\n").unwrap();
    w.spawn_host("host2");
    wait_for("labB 재연결", Duration::from_secs(60), || w.h.connected("labB").then_some(()));
    // next 저장은 report_ready 발행보다 먼저다. 상태 조회 대신 수집 완료 사건을 끝 신호로 쓴다.
    let mut flow = w.lab.follow("host2-flow", &w.h, "demo");
    flow.until("재시작 뒤 보고 수집 완료", T, |e| {
        e["kind"] == "report_ready" && e["thread"] == id && e["seq"].as_u64().is_some_and(|s| s > seen)
    });
    let v = w.show(&id);
    assert_eq!(v["next"], serde_json::json!(["사용자 확인"]), "{v}");
    // 보고 첫 줄 태그로 확인 필요를 정하지 않는다(D9): 새 보고는 검토 대기
    assert_eq!(v["group"], "ready_for_review");
    let kinds = w.kinds(seen);
    assert_eq!(kinds, ["report_ready"], "host 재시작 뒤 사건: {kinds:?}");
}
/// task 첨부(plan §8.327): `--from-task`로 다른 머신(TLS, 상태 폴더가 다른 labB)에 thread를 띄우면 메모가 가리키는 host 파일
/// (올린 그림·보고서 안 문서)을 그 머신 작업 폴더 `attachments/`에 복사하고, 할 일 글·brief는 그 머신에서 열 수 있는 경로만 쓴다
#[test]
fn task_attachments_reach_a_remote_agent() {
    let mut w = report_shell_world();
    w.add_remote();
    let demo = w.projects.join("demo");
    std::fs::create_dir_all(demo.join("uploads")).unwrap();
    use sha2::Digest as _;
    let img = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR task attach".to_vec();
    // 올린 파일 이름은 내용 hash다(host가 정한다, 넘길 때 이름과 내용을 대조한다)
    let png = format!("{}.png", &hex::encode(sha2::Sha256::digest(&img))[..16]);
    std::fs::write(demo.join("uploads").join(&png), &img).unwrap();
    let report = w.lab.root.join("plan-report");
    std::fs::create_dir_all(&report).unwrap();
    std::fs::write(report.join("index.html"), "<!doctype html><title>계획</title>").unwrap();
    std::fs::write(report.join("plan.md"), "# 계획서\n\n- 첫 단계\n").unwrap();
    w.h.ok(&["library", "publish", path_str(&report), "--project", "demo", "--name", "plan-report"]);
    std::fs::write(
        demo.join("TASKS.md"),
        format!(
            "# Tasks\n\n## Backlog\n- [ ] 첨부 일\n  고칠 화면\n  ![그림](uploads/{png})\n  \
             [계획서](library/plan-report/plan.md)\n  [없는 그림](uploads/fedcba9876543210.png)\n"
        ),
    )
    .unwrap();
    let v = w.start(&["--from-task", "첨부 일", "--machine", "labB", "--profile", "fake", "--kind", "dir"]);
    let id = v["id"].as_str().unwrap().to_string();
    let warnings = v["warnings"].to_string();
    assert!(warnings.contains("uploads/fedcba9876543210.png"), "host에 없는 첨부는 경고로 알린다: {v}");
    let v = w.wait_show("원격 brief 받음", &id, |v| v["brief"] == "delivered");
    let work = PathBuf::from(v["work_dir"].as_str().unwrap());
    assert!(work.starts_with(w.lab.root.join("workB")), "labB 작업 폴더가 아니다: {}", work.display());
    let names: Vec<String> = std::fs::read_dir(work.join("attachments"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "그림·보고서 문서 둘만 넘긴다: {names:?}");
    assert_eq!(std::fs::read(work.join("attachments").join(&png)).unwrap(), img);
    let doc = names.iter().find(|n| n.ends_with(".md")).expect("보고서 문서 사본");
    assert_eq!(std::fs::read_to_string(work.join("attachments").join(doc)).unwrap(), "# 계획서\n\n- 첫 단계\n");
    let brief = std::fs::read_to_string(work.join("brief.md")).unwrap();
    let task = std::fs::read_to_string(demo.join("threads").join(format!("{id}.task.md"))).unwrap();
    // brief 머리의 uploads 줄(「host 머신에만 있다」)은 원래 안내다. 첨부 자체는 host 경로·host 상대 경로로 가리키지 않는다.
    // 안내 「시작할 때 묻기」의 `![A안](library/a.png)`은 thread 자기 보고서 옆 library라 첨부가 아니다(plan §8.345)
    assert!(!task.contains(path_str(&w.projects)), "할 일에 host 경로가 있다:\n{task}");
    for host in [format!("](uploads/{png})"), "](library/plan-report/".into()] {
        assert!(!brief.contains(&host), "brief가 첨부를 host 쪽으로 가리킨다({host}):\n{brief}");
    }
    assert!(
        task.contains(&format!("![그림](attachments/{png})")) && task.contains(&format!("[계획서](attachments/{doc})")),
        "{task}"
    );
    assert!(task.contains("[없는 그림](uploads/fedcba9876543210.png)"), "넘기지 못한 줄은 그대로 둔다: {task}");
    assert!(
        brief.contains(&format!("- task 첨부 2개(할 일의 `attachments/…`): `{}`", work.join("attachments").display())),
        "{brief}"
    );

    // 원본을 지운 뒤(정리한 uploads·지운 보고서 파일)에도 다시 시작한다: 첫 시작 때 쓴 사본이 작업 폴더에 남는다
    w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    w.h.ok(&["thread", "ack", "demo", &id]);
    let pane = v["pane"].as_str().unwrap().to_string();
    w.h.ok(&["pane", "kill", &pane]);
    wait_for("세션 끝남", T, || (w.h.pane(&pane)["state"] != "running").then_some(()));
    std::fs::remove_file(demo.join("uploads").join(&png)).unwrap();
    std::fs::remove_file(demo.join("library/plan-report/plan.md")).unwrap();
    let again = w.h.ok(&["thread", "restart", "demo", &id]);
    assert_ne!(again["pane"].as_str(), Some(pane.as_str()), "{again}");
    assert_eq!(std::fs::read(work.join("attachments").join(&png)).unwrap(), img, "첫 사본은 그대로다");
}

/// 옛 대화 첨부의 무작위 이름과 실제로 바뀐 원본 모두 시작을 막고, 원본은 그대로 보존한다.
#[test]
fn task_attachments_reject_unverified_uploads_without_changing_them() {
    use sha2::Digest as _;
    let w = world();
    let demo = w.projects.join("demo");
    let uploads = demo.join("uploads");
    std::fs::create_dir_all(&uploads).unwrap();
    let original = b"\x89PNG\r\n\x1a\noriginal";
    let changed = b"\x89PNG\r\n\x1a\nchanged";
    let hash_name = format!("{}.png", &hex::encode(sha2::Sha256::digest(original))[..16]);
    let legacy = "0123456789abcdef.png";
    let cases = [("옛 대화 그림", legacy, original.as_slice()), ("바뀐 그림", hash_name.as_str(), changed.as_slice())];
    for (title, name, bytes) in cases {
        std::fs::write(uploads.join(name), bytes).unwrap();
        std::fs::write(
            demo.join("TASKS.md"),
            format!("# Tasks\n\n## Backlog\n- [ ] {title}\n  ![그림](uploads/{name})\n"),
        )
        .unwrap();
        let e =
            w.h.json(&[
                "thread",
                "start",
                "demo",
                "--from-task",
                title,
                "--machine",
                "labA",
                "--profile",
                "fake",
                "--kind",
                "dir",
            ])
            .expect_err("검증하지 못한 첨부로 시작했다");
        assert!(
            e.contains("이름과 내용 해시가 다르다") && e.contains("예전 대화 첨부") && e.contains("할 일에 다시 첨부"),
            "{e}"
        );
        assert_eq!(std::fs::read(uploads.join(name)).unwrap(), bytes, "거부하며 원본을 고쳤다");
    }
    assert_eq!(std::fs::read_dir(&uploads).unwrap().count(), 2, "원본을 지우거나 이름을 바꿨다");
}

/// 안내를 보내도 화면이 바뀌지 않으면 unknown으로 남기고 자동으로 다시 보내지 않는다(§8.5 a)
/// 입력창을 읽을 수 없는 harness(`mute`: 화면이 바뀌지 않는다): ticker는 한 번 보내고, 다시 보내기 전에 화면으로 앞
/// 시도를 확인할 수 없어 멈춘다(herdr-projects brief.rs `give_up`). 사람이 `thread brief`로 보내면 그때만 한 번 더 간다.
#[test]
fn unknown_brief_is_not_resent() {
    // 무반응 PTY의 unknown·수동 재전송·선행 메시지 거부는 실제 연결로 본다.
    // 기본 10초 받음 시한은 harness의 Tokio 시계 owner가 그대로 검사한다.
    let mut w = world_with("accept_wait_ms = 100\n");
    let mut ev = w.lab.follow("events", &w.h, "demo");
    let v = w.start(&["--title", "mute", "--profile", "mute", "--kind", "dir", "--task-file", &w.task("t4.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let e = ev.kind("brief_unknown", &id, T);
    assert!(e["detail"].as_str().unwrap().contains("입력창을 읽을 수 없어"), "{e}");
    // 배경 task는 멈췄다(brief_unknown 뒤에는 보내는 쪽이 없다): 한 번뿐이다
    let log = w.scripts.join("mute.log");
    let got = std::fs::read_to_string(&log).unwrap();
    assert_eq!(got.matches("brief.md").count(), 1, "{got}");
    assert_eq!(ev.kinds(), ["thread_started", "brief_unknown"]);
    let v = w.show(&id);
    assert_eq!((v["brief"].as_str(), v["group"].as_str()), (Some("unknown"), Some("waiting_on_you")));
    // 사람이 다시 보내면 한 번 더 간다(받았다는 표지가 없어 여전히 모른다)
    let e = w.h.json(&["thread", "brief", "demo", &id]).expect_err("받았는지 모른다");
    assert!(e.contains("아직 보내지 못했다"), "{e}");
    let got = std::fs::read_to_string(&log).unwrap();
    assert_eq!(got.matches("brief.md").count(), 2, "{got}");
    // brief를 받기 전의 메시지는 입력하기 전에 거부한다(brief보다 먼저 가지 않게, herdr-projects threads.rs `prompt`)
    let e = w.h.json(&["thread", "message", "demo", &id, "--text", "MSG-EARLY-1"]).expect_err("brief 전 메시지");
    assert!(e.contains("아직 brief를 받지 않았다"), "{e}");
    assert!(!std::fs::read_to_string(&log).unwrap().contains("MSG-EARLY-1"));
    assert!(!w.kinds(0).iter().any(|k| k.starts_with("message_")), "{:?}", w.kinds(0));
}

/// brief 줄이 입력창에 남으면 ticker는 다시 치지 않고 Enter만 보낸다(herdr-projects brief.rs: 입력창에 남은 줄은
/// Enter, 대화에 보이고 입력창이 비면 받은 것). 받은 thread에는 `thread brief`를 다시 보내지 않는다.
#[test]
fn brief_left_in_box_is_submitted_not_retyped() {
    let mut w = world();
    let mut ev = w.lab.follow("events", &w.h, "demo");
    let v =
        w.start(&["--title", "sticky", "--profile", "sticky", "--kind", "dir", "--task-file", &w.task("t9.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    let e = ev.kind("brief_delivered", &id, T);
    assert!(e["detail"].as_str().unwrap().starts_with("입력창에 남은 줄을 Enter로 보냈다(시도 2번)"), "{e}");
    let screen = w.h.screen(&pane);
    let line = format!(".aaa/demo-{id}/brief.md를 읽고 그대로 하라.");
    // 대화의 `❯ <줄>`과 `got: <줄>`에 한 번씩: 다시 쳤으면 입력창에 두 번 이어 붙어 `got:` 줄이 두 배가 된다
    assert!(screen.contains(&format!("got: {line}\n")), "{screen}");
    assert_eq!(screen.matches("brief.md를 읽고").count(), 2, "{screen}");
    let e = w.h.json(&["thread", "brief", "demo", &id]).expect_err("받은 뒤 brief");
    assert!(e.contains("이미 brief를 받았다"), "{e}");
}

/// thread 메시지(routine 발화도 같은 길)는 프로그램이 bracketed paste를 켰을 때만 감싼다(herdr
/// `encode_api_submission_parts`, src/app/api_helpers.rs). 켜지 않은 프로그램에 감싸 보내면 `^[[200~`가 글로 남는다.
#[test]
fn thread_message_paste_follows_program_mode() {
    let mut w = world();
    let mut ev = w.lab.follow("events", &w.h, "demo");
    let v =
        w.start(&["--title", "paste", "--profile", "paste", "--kind", "dir", "--task-file", &w.task("tp.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    ev.kind("brief_delivered", &id, T);
    w.h.wait_output(&pane, "ok 1", T);
    let log = w.scripts.join("paste.log");
    let hex = |s: &str| s.bytes().map(|b| format!("{b:02x}")).collect::<String>();
    let mut seen = 0;
    let mut next = || {
        let all = std::fs::read_to_string(&log).unwrap();
        let got = all[seen..].to_string();
        seen = all.len();
        got
    };
    // brief 안내도 꺼진 채로는 그대로 + CR
    assert!(next().ends_with(&format!("{}0d", hex("를 읽고 그대로 하라."))));
    // 1) 꺼진 채: 여러 줄 글도 감싸지 않는다
    w.message(&id, "첫 줄\n둘째 줄", false);
    ev.kind("message_accepted", &id, T);
    assert_eq!(next(), format!("{}0d", hex("첫 줄\n둘째 줄")));
    // 2) 프로그램이 켠 뒤(`ok 2`와 같은 출력에서 켰다): 감싼다
    w.h.wait_output(&pane, "ok 2", T);
    w.message(&id, "셋째\n넷째", false);
    ev.kind("message_accepted", &id, T);
    assert_eq!(next(), format!("{}0d", hex("\x1b[200~셋째\n넷째\x1b[201~")));
}

/// 보안 감사 t-0340(agent hooks.rs `normalize` → host harness.rs `on_harness` → orch.rs `thread_restart`): hook이 알린
/// harness 세션 ID는 UUID 꼴만 받는다. 가짜 Claude가 `--dangerously-skip-permissions`를 세션 ID로 알려도 `thread restart
/// --resume`은 aaa가 준 대화 ID로 잇는다(전에는 그 값이 `--resume` 뒤 낱말이 되어 Claude가 선택지로 읽었다). 결함 심기:
/// 검사를 늘 참으로 하면 세션 ID가 남아 첫 단언에서 실패하고, 그 단언을 빼면 새 pane이 aaa의 대화 ID로 뜨지 않는다.
#[test]
fn forged_hook_session_id_does_not_reach_resume() {
    let mut w = world();
    let mut ev = w.lab.follow("events", &w.h, "demo");
    let v = w.start(&["--title", "evil", "--profile", "evil", "--kind", "dir", "--task-file", &w.task("te.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    // brief는 지어낸 세션 ID를 실은 UserPromptSubmit으로 받았다: 사건은 받고 세션 ID는 버렸다
    ev.kind("brief_delivered", &id, T);
    let key = w.h.wait_output(&pane, "--session-id", T)["matched_line"]
        .as_str()
        .unwrap()
        .rsplit(' ')
        .next()
        .unwrap()
        .to_string();
    let ag = w.h.pane(&pane)["agent"].clone();
    assert_eq!((ag["harness"].as_str(), ag["harness_session"].as_str()), (Some("claude"), None), "{ag}");
    let mut hw = HostWatch::new(&w.h, T);
    w.h.ok(&["pane", "kill", &pane]);
    hw.wait("세션 끝남", |x| x.pane(&pane).is_some_and(|p| !p.state.is_running()).then_some(()));
    w.h.ok(&["thread", "restart", "demo", &id, "--resume"]);
    let new = w.show(&id)["pane"].as_str().unwrap().to_string();
    w.h.wait_output(&new, &format!("--resume {key}"), T);
    assert!(!w.h.screen(&new).contains("dangerously"), "{}", w.h.screen(&new));
}

/// 4장 A 재부팅 뒤 thread(herdr 복원 + herdr-projects는 그 pane을 그대로 쓴다): 저장한 대화를 이어 띄운다(`thread
/// restart --resume`과 같은 길, brief 안내 없음). 대화를 이어 띄울 방법이 없는 harness(custom)는 중단으로 두고
/// `restore_failed` 사건과 inbox `session`·`restore failed` 항목을 남긴다. 복원 중에는 「세션이 모두 끝났다」 항목을
/// 내지 않는다. 기다림은 흐름 사건과 출력이다.
#[test]
fn reboot_resumes_threads() {
    let mut w = world();
    let mut ev = w.lab.follow("events", &w.h, "demo");
    let v = w.start(&[
        "--title",
        "resume",
        "--profile",
        "resumable",
        "--kind",
        "dir",
        "--task-file",
        &w.task("tr.txt", "x"),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    ev.kind("brief_delivered", &id, T);
    let args = w.h.wait_output(&pane, "--session-id", T);
    let line = args["matched_line"].as_str().unwrap().to_string();
    let key = line.rsplit(' ').next().unwrap().to_string();
    let f = w.start(&["--title", "plain", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("tf.txt", "y")]);
    let fid = f["id"].as_str().unwrap().to_string();
    let fpane = f["pane"].as_str().unwrap().to_string();
    // 보고와 brief 받음은 따로 온다. 받기 전에 끊으면 복원은 「brief 전이라 새로 띄웠다」라 restore_failed가 없다
    w.wait_show("보고·brief 받음", &fid, |v| v["report_hash"].is_string() && v["brief"] == "delivered");
    // 사람이 붙인 pane 이름은 이어 띄운 thread pane도 잇는다
    w.h.ok(&["pane", "rename", &pane, "리뷰어"]);

    w.lab.kill("agentA");
    assert_eq!(w.lab.kill_session_hosts(), 2);
    w.lab.spawn_env(
        "agentA2",
        &w.h,
        &["agent", "--host", "local", "--name", "labA"],
        &[("CLAUDE_CODE_CHILD_SESSION", "1"), ("CLAUDECODE", "1"), ("AAA_TEST_BOOT_ID", "boot-after")],
    );
    // 이어 띄운 thread: 새 pane이 같은 대화(`--resume <key>`)로 뜨고 thread 기록이 그 pane을 가리킨다
    let e = ev.kind("session_restored", &id, T);
    assert!(e["detail"].as_str().unwrap().contains("대화를 이어 띄웠다"), "{e}");
    let new = w.show(&id)["pane"].as_str().unwrap().to_string();
    assert_ne!(new, pane);
    assert_eq!(w.h.pane(&new)["label"], "리뷰어");
    w.h.wait_output(&new, &format!("--resume {key}"), T);
    // 이어 띄운 대화에 메시지가 간다(brief는 다시 보내지 않는다)
    w.message(&id, "after reboot", false);
    w.h.wait_output(&new, "got: after reboot", T);
    assert!(!w.h.screen(&new).contains("brief.md"), "{}", w.h.screen(&new));
    // custom harness는 이어 띄울 대화가 없다: 중단으로 두고 알린다
    let e = ev.kind("restore_failed", &fid, T);
    assert!(e["detail"].as_str().unwrap().contains("이어 띄우지 못한다"), "{e}");
    assert_eq!(w.show(&fid)["pane"], fpane.as_str());
    assert!(w.h.pane(&fpane)["restore_error"].as_str().is_some_and(|s| s.contains("이어 띄우지 못한다")));
    let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
    assert!(
        items.iter().any(|i| i["item_kind"] == "session"
            && i["event"] == "restore failed"
            && i["summary"].as_str().is_some_and(|s| s.contains(&fid))),
        "{items:?}"
    );
    // 복원 뒤 ticker가 돌아도 「세션이 모두 끝났다」·이은 thread의 「pane closed」는 없다(사라졌다가 돌아온 것으로 보지
    // 않는다). 실패한 thread에는 까닭이 든 항목 하나만 있다
    w.h.ticks(2);
    let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
    assert!(!items.iter().any(|i| i["event"] == "sessions ended"), "{items:?}");
    assert!(!items.iter().any(|i| i["event"] == "pane closed"), "{items:?}");
    assert_eq!(items.iter().filter(|i| i["event"] == "restore failed").count(), 1, "{items:?}");
    // 옛 pane 기록은 지웠다
    assert!(w.h.json(&["pane", "get", &pane]).is_err());
}

/// 이 thread에 열린 질문(종류 `kind`, `not`에 없는 ID)이 생긴 사건
fn open_question(ev: &mut Events, id: &str, kind: &str, not: &[&str]) -> QuestionView {
    open_question_with_timeout(ev, id, kind, not, T)
}

fn open_question_with_timeout(ev: &mut Events, id: &str, kind: &str, not: &[&str], timeout: Duration) -> QuestionView {
    ev.wait(&format!("{kind} 질문"), timeout, |e| match e {
        FrontEvent::Question(q)
            if q.thread.as_deref() == Some(id)
                && q.state == QuestionState::Open
                && q.question_kind == kind
                && !not.contains(&q.id.as_str()) =>
        {
            Some(q.clone())
        }
        _ => None,
    })
}

/// 이 thread의 harness 상태가 `at`(ms) 뒤에 `activity`로 바뀐 사건(thread 기록)
fn agent_became(ev: &mut Events, id: &str, activity: AgentActivity, at: u64) -> ThreadView {
    ev.wait(&format!("{activity:?} 상태"), T, |e| match e {
        FrontEvent::Thread(t)
            if t.id == id && t.agent.as_ref().is_some_and(|a| a.activity == activity && a.since_ms >= at) =>
        {
            Some(t.as_ref().clone())
        }
        _ => None,
    })
}

/// 흐름 사건 `kind`(`detail`에 `about`이 든 것)
fn flow_event(ev: &mut Events, kind: &str, about: &str) -> FlowEvent {
    ev.wait(&format!("{kind} {about}"), T, |e| match e {
        FrontEvent::Flow(f) if f.kind == kind && f.detail.contains(about) => Some(f.clone()),
        _ => None,
    })
}

/// 질문이 닫혔다(Gone)는 사건
fn question_gone(ev: &mut Events, id: &str) {
    ev.wait(&format!("{id} 닫힘"), T, |e| match e {
        FrontEvent::Question(q) if q.id == id && q.state == QuestionState::Gone => Some(()),
        _ => None,
    });
}

/// 끝나거나 닫힌 pane의 질문은 닫힌다(QA t-0337 Q4: 빈 화면이 「답을 기다리는 질문」을 세고, 답하면 「pane이 없다」).
/// 질문은 brief 보내기가 확인창을 보고 열었다(custom harness라 harness 사건으로는 닫히지 않는다). 세션이 끝나면 host가
/// 닫는다: host가 떠 있으면 그때, host·agent가 내려가 있었으면 agent가 다시 붙어 세션을 알릴 때(살아 있는 pane의 질문은
/// 그대로). 참고 herdr-web-ui는
/// 살아 있는 pane 화면에서만 질문을 읽어 닫힌 pane에는 질문이 없다(`server/prompt.ts`·`collector.ts`).
#[test]
fn questions_of_ended_panes_are_closed() {
    let mut w = world();
    let mut ev = Events::connect(&w.h);
    let start = |w: &World, title: &str| {
        let v = w.start(&["--title", title, "--profile", "asker", "--kind", "dir", "--task-file", &w.task(title, "x")]);
        (v["id"].as_str().unwrap().to_string(), v["pane"].as_str().unwrap().to_string())
    };
    // 1) host가 떠 있을 때 pane을 닫는다
    let (a, pa) = start(&w, "ask-a");
    let qa = open_question(&mut ev, &a, "trust", &[]);
    w.h.ok(&["pane", "kill", &pa]);
    question_gone(&mut ev, &qa.id);
    let e = w.h.json(&["question", "answer", &qa.id, "yes"]).expect_err("닫힌 질문");
    assert!(e.contains("이미 닫힌"), "{e}");

    // 2) host·agent가 내려가 있는 동안 세션이 끝난다(운영 업그레이드 중처럼). 그동안 살아 있던 pane(c)의 질문은 남는다.
    //    c의 질문을 먼저 만든다: 다시 붙을 때 질문을 만든 차례로 보므로, 모두 닫는 결함이면 qb보다 먼저 닫힌다
    let (c, _) = start(&w, "ask-c");
    let qc = open_question(&mut ev, &c, "trust", &[]);
    let (b, _) = start(&w, "ask-b");
    let qb = open_question(&mut ev, &b, "trust", &[]);
    let cwd = PathBuf::from(w.show(&b)["cwd"].as_str().unwrap());
    w.lab.kill("agentA");
    w.lab.kill("host");
    // 확인창을 띄운 harness가 끝난다(읽는 쪽이 열 때까지 쓰기가 기다린다)
    std::fs::write(cwd.join("ask.fifo"), "end\n").unwrap();
    w.spawn_host("host2");
    wait_for("host2 준비", T, || w.h.json(&["status"]).ok());
    let mut ev = Events::connect(&w.h);
    w.lab.spawn("agentA2", &w.h, &["agent", "--host", "local", "--name", "labA"]);
    question_gone(&mut ev, &qb.id);
    let open: Vec<String> = w.questions(false).iter().map(|q| q["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(open, std::slice::from_ref(&qc.id), "살아 있는 pane의 질문만 남아야 한다");
}

/// 가짜 Claude의 지금 턴을 마친다(BEL)
fn finish_turn(w: &World, pane: &str) {
    w.h.ok(&["pane", "send-text", pane, "\u{7}"]);
}

/// race 준비 뒤 key()를 부르기 전의 TTY 모드를 잡는다. key()까지 실행한 뒤 확인하면
/// 늦게 꺼진 echo를 놓친다. 실제 PTY에서 준비된 입력창과 한 글자 받음도 함께 확인한다.
#[test]
fn claude_race_ready_prepares_input_before_publishing_prompt() {
    let (functions, _) = CLAUDE_AGENT.split_once("printf '\\033[2J\\033[H'").unwrap();
    let script = format!(
        "{functions}\nrace_ready\nprintf '\\nTTY-MODE-BEGIN\\n'\nstty -a\nprintf '\\nTTY-MODE-END\\n'\nkey\nprintf '\\nreceived:<%s>\\n' \"$k\"\n"
    );
    let mut command = lab::terminal_command(Path::new("/bin/sh"));
    command.args(["-c", &script]);
    let output = lab::Terminal::spawn(command).read(&["fixture race ready"], &[("TTY-MODE-END", "x")], T);
    let mode = output.split("TTY-MODE-BEGIN").nth(1).unwrap().split("TTY-MODE-END").next().unwrap();
    let flags: Vec<_> = mode.split(|c: char| c.is_whitespace() || c == ';').collect();
    assert!(flags.contains(&"-echo") && flags.contains(&"-icanon"), "준비 표지가 입력 모드보다 앞섰다: {mode}");
    assert!(output.contains("race armed") && output.contains("? for shortcuts"), "{output}");
    assert!(output.contains("received:<x>"), "{output}");
}

/// Esc suffix를 읽는 동안 답한 권한 창을 다시 묻거나 다음 입력을 받을 수 있는 idle로 오인하지 않는다.
#[test]
fn claude_menu_dismisses_escape_before_waiting_for_its_suffix() {
    let (functions, _) = CLAUDE_AGENT.split_once("printf '\\033[2J\\033[H'").unwrap();
    let script = format!(
        "{functions}\nmenu 'Do you want to proceed?' 0 '1. Yes' '2. No (esc)' 'Esc to cancel'\nprintf '\\npick:<%s>\\n' \"$pick\"\n"
    );
    let mut command = lab::terminal_command(Path::new("/bin/sh"));
    command.args(["-c", &script]);
    command.env("CLAUDE_MENU", Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-menu.py"));
    // suffix 창이 끝나면 fixture가 끝나 PTY가 닫힌다. Working을 본 뒤 키를 더 쓰면 부하 아래 EIO와 경주한다.
    let output = lab::Terminal::spawn(command).read_raw(&["fixture escape decode"], &[("Esc to cancel", "\u{1b}")], T);
    let (answered, _) = output
        .split_once("Working…")
        .unwrap_or_else(|| panic!("Esc를 읽은 뒤 suffix를 기다리는 동안 Working 표지가 없다: {output:?}"));
    let before = format!("{answered}Working… (1s · esc to interrupt)\n");
    let mut vt = aaa_vt::Vt::new(36, 120);
    vt.feed(before.as_bytes());
    let screen = vt.text("fixture");
    assert!(!screen.lines.iter().any(|line| line.contains("Do you want to proceed?")), "{before:?}");
    let detected = aaa_agent::detect::detect(
        aaa_agent::detect::Harness::Claude,
        aaa_agent::detect::DetectionInput { screen: &vt.detection_text(), osc_title: "", osc_progress: "" },
    );
    assert_eq!(
        detected.state,
        aaa_agent::detect::State::Working,
        "suffix 읽기가 끝나기 전에 다음 메시지를 허용했다: {detected:?}\n{before:?}"
    );
    assert!(detected.visible_working, "{detected:?}\n{before:?}");
    assert!(output.contains("pick:<cancel>"), "{output}");
}

/// 실제 PTY에 함께 온 화살표는 질문을 지우거나 Working으로 바꾸지 않고 선택만 옮긴다.
#[test]
fn claude_menu_arrow_preserves_the_question_until_enter() {
    let (functions, _) = CLAUDE_AGENT.split_once("printf '\\033[2J\\033[H'").unwrap();
    let script = format!(
        "{functions}\nmenu 'Quick safety check: Do you trust the files in this folder?' 0 'No, exit' 'Yes, I trust this folder' 'Enter to confirm · Esc to cancel'\nprintf '\\npick:<%s>\\n' \"$pick\"\n"
    );
    let mut command = lab::terminal_command(Path::new("/bin/sh"));
    command.args(["-c", &script]);
    command.env("CLAUDE_MENU", Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-menu.py"));
    let output = lab::Terminal::spawn(command).read_raw(
        &["fixture arrow redraw"],
        &[("Esc to cancel", "\u{1b}[B"), (" ❯ Yes, I trust this folder", "\r")],
        T,
    );
    let before = output.rsplit_once("\u{1b}[6A\r\u{1b}[J").unwrap().0;
    assert!(output.contains("pick:<Yes, I trust this folder>"), "{output}");
    assert!(!before.contains("Working"), "화살표를 취소로 그렸다: {before:?}");
    let mut vt = aaa_vt::Vt::new(36, 120);
    vt.feed(before.as_bytes());
    let detected = aaa_agent::detect::detect(
        aaa_agent::detect::Harness::Claude,
        aaa_agent::detect::DetectionInput { screen: &vt.detection_text(), osc_title: "", osc_progress: "" },
    );
    assert_eq!(detected.state, aaa_agent::detect::State::Blocked, "{before:?}");
}

/// 함께 온 답은 선택을 옮긴 중간 화면보다 먼저 처리한다.
#[test]
fn claude_menu_consumes_a_batched_answer_before_redrawing() {
    let (functions, _) = CLAUDE_AGENT.split_once("printf '\\033[2J\\033[H'").unwrap();
    let title = "Quick safety check: Do you trust the files in this folder?";
    let script = format!(
        "{functions}\nmenu '{title}' 0 'No, exit' 'Yes, I trust this folder' 'Enter to confirm · Esc to cancel'\nprintf '\\npick:<%s>\\n' \"$pick\"\n"
    );
    let mut command = lab::terminal_command(Path::new("/bin/sh"));
    command.args(["-c", &script]);
    command.env("CLAUDE_MENU", Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-menu.py"));
    let output =
        lab::Terminal::spawn(command).read_raw(&["fixture batched answer"], &[("Esc to cancel", "\u{1b}[B\r")], T);
    assert!(output.contains("pick:<Yes, I trust this folder>"), "{output}");
    assert_eq!(output.matches(title).count(), 1, "Enter가 대기 중인데 질문을 다시 그렸다: {output}");
}

/// thread `id`의 진행 보고가 `text`가 되기를 기다린다
fn progress_seen(ev: &mut Events, id: &str, text: &str) {
    ev.wait(&format!("진행 보고 {text}"), T, |e| match e {
        FrontEvent::Thread(v) if v.id == id && v.agent.as_ref().and_then(|a| a.progress.as_deref()) == Some(text) => {
            Some(())
        }
        _ => None,
    });
}

/// 화면으로 아는 harness 상태(herdr 0.9.5 감지, plan §8.110), 확인창 질문의 원격 응답, 메시지 queue·interrupt(§8.3~8.5).
/// 기다림은 모두 host가 밀어 주는 사건(thread 기록·질문·흐름)으로 끝난다.
#[test]
fn claude_thread_states_questions_and_messages() {
    // interrupt의 멈춤 상한(stop_wait)을 시험 기다림(T)보다 훨씬 길게 둔다: 멈춤을 화면으로 알지 못하면
    // 보내지 않은 채 상한까지 기다려 아래 「got: urgent」 기다림이 실패한다(시간 단언 없이 구별한다, t-0327)
    let w = world_with("stop_wait_ms = 120000\n");
    let mut ev = Events::connect(&w.h);
    let v =
        w.start(&["--title", "claude", "--profile", "claude", "--kind", "dir", "--task-file", &w.task("t5.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();

    // 처음 실행 확인창(폴더 신뢰)은 입력하지 않고 질문으로 올린다
    let q = open_question(&mut ev, &id, "trust", &[]);
    assert_eq!(q.text, "Quick safety check: Do you trust the files in this folder?");
    assert_eq!(w.show(&id)["brief"], "pending");
    // 고를 수 없는 답은 거부한다
    let e = w.h.json(&["question", "answer", &q.id, "maybe"]).expect_err("없는 답");
    assert!(e.contains("yes, no"), "{e}");
    let a = w.h.ok(&["question", "answer", &q.id, "yes"]);
    assert_eq!((a["state"].as_str(), a["answer"].as_str()), (Some("answered"), Some("yes")));

    // 답한 뒤 brief 안내가 가고(작업 중이나 확인창이 보이면 받았다), 권한 확인창이 질문이 된다
    flow_event(&mut ev, "brief_delivered", "안내 줄을 보냈다");
    let p1 = open_question(&mut ev, &id, "permission", &[]);
    assert_eq!(p1.text, "Do you want to proceed?");
    // Claude hook은 진행 보고 안내를 받는다(herdr-projects progress.rs `hook`): SessionStart에 지침, UserPromptSubmit에 안내
    let guided: Vec<Value> = std::fs::read_to_string(w.scripts.join("hook.log"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let said = |i: usize| {
        let o = &guided[i]["hookSpecificOutput"];
        (o["hookEventName"].as_str().unwrap().to_string(), o["additionalContext"].as_str().unwrap().to_string())
    };
    assert_eq!(guided.len(), 2, "{guided:?}");
    let (start, prompt) = (said(0), said(1));
    assert!(start.0 == "SessionStart" && start.1.contains(&format!("`{} report --percent 25", lab::BIN)), "{start:?}");
    assert!(prompt.0 == "UserPromptSubmit" && prompt.1.contains("Waiting for you"), "{prompt:?}");
    let v = w.show(&id);
    let ag = &v["agent"];
    assert_eq!((ag["activity"].as_str(), ag["harness"].as_str()), (Some("question"), Some("claude")), "{v}");
    assert_eq!((ag["percent"].as_u64(), ag["progress"].as_str()), (Some(40), Some("권한 확인 중")), "{v}");
    // 세션 ID는 SessionStart hook으로 잇는다
    assert_eq!(ag["harness_session"], "0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b");
    // 기본 선택(No, exit)이 아니라 Yes까지 옮겨 골랐다
    assert!(w.h.screen(&pane).contains("marker: none"));
    // 확인창이 그대로 떠 있어도(herdr는 800ms마다 다시 알린다) 질문은 하나다
    assert_eq!(w.questions(false).len(), 1);
    assert_eq!(w.h.pane(&pane)["agent"]["activity"], "question");
    let t = lab::now_ms();
    w.h.ok(&["question", "answer", &p1.id, "yes"]);
    let e = w.h.json(&["question", "answer", &p1.id, "yes"]).expect_err("두 번 답하기");
    assert!(e.contains("닫힌"), "{e}");
    // 허락하면 도구가 돈다(spinner). 턴이 끝나 입력 상자가 보이면 idle이다.
    agent_became(&mut ev, &id, AgentActivity::Working, t);
    // 답한 뒤 늦게 온 Notification(permission_prompt) hook(옛 agent가 띄운 세션, t-0333 m13 사건 16)은 답한 질문을 다시
    // 열지 않는다: 참고 herdr 0.9.5처럼 상태 hook은 버린다(`herdr-agent-state.sh`는 `session` 말고는 exit 0). 그 hook 뒤에 낸
    // 진행 보고가 오면 본다
    w.h.ok(&["pane", "send-text", &pane, "\u{e}"]);
    progress_seen(&mut ev, &id, "늦은 알림 뒤");
    assert!(w.questions(false).is_empty(), "답한 질문이 다시 열렸다: {:?}", w.questions(false));
    assert_eq!(w.h.pane(&pane)["agent"]["activity"], "working");
    let t = lab::now_ms();
    finish_turn(&w, &pane);
    agent_became(&mut ev, &id, AgentActivity::Idle, t);
    // 보고 사본: agent가 report.md를 1초마다 보고 host가 받아 간다(그 사건을 기다린다)
    ev.wait("report_ready", T, |e| match e {
        FrontEvent::Flow(f) if f.kind == "report_ready" && f.thread.as_deref() == Some(id.as_str()) => Some(()),
        _ => None,
    });
    assert!(w.show(&id)["report_hash"].is_string());
    assert!(w.h.screen(&pane).contains("allowed"));

    // 작업 중이어도 바로 입력한다: harness가 다음 턴으로 줄 세운다(herdr-projects threads.rs `prompt`). 일하는 중에 보낸
    // 글은 보낸 뒤에도 일하는 중이면 받은 것이다(herdr `agent prompt --wait`, plan §8.100). 「slow one」 턴은 시험이 끝낼
    // 때까지 끝나지 않으므로, 턴 끝을 기다리는 받음 판정이면 m2는 받음 시한에 걸린다.
    let m1 = w.message(&id, "slow one", false);
    assert_eq!(m1["state"], "accepted", "{m1}");
    let m2 = w.message(&id, "second", false);
    assert_eq!(m2["state"], "accepted", "{m2}");
    assert!(!w.h.screen(&pane).contains("got: second"));
    finish_turn(&w, &pane);
    let screen = w.wait_screen(&pane, "got: second");
    assert!(screen.find("got: slow one").unwrap() < screen.find("got: second").unwrap(), "{screen}");
    let t = lab::now_ms();
    finish_turn(&w, &pane);
    agent_became(&mut ev, &id, AgentActivity::Idle, t);
    // 입력한 메시지는 task.md Follow-ups에 남는다(다시 시작하면 brief와 함께 읽는다, herdr-projects append_follow_up)
    let task = std::fs::read_to_string(w.projects.join("demo/threads").join(format!("{id}.task.md"))).unwrap();
    let follow = task.split("\n## Follow-ups\n").nth(1).unwrap_or_else(|| panic!("Follow-ups가 없다: {task}"));
    assert!(follow.contains("\nslow one\n") && follow.find("slow one") < follow.find("second"), "{follow}");

    // 작업 요약(plan §8.259): 요약 없이 지시가 쌓이면 표지가 그 수를 센다. 요약을 쓰면 0이 되고, 그 뒤 지시만 다시 센다
    assert_eq!(w.show(&id)["summary"], serde_json::json!({ "at_ms": null, "pending": 2 }));
    let bad = w.task("bad-summary.md", "## 지금\n- 목표 없음\n");
    let e = w.h.json(&["thread", "summary", "demo", &id, "--file", &bad]).expect_err("목표 없는 요약");
    assert!(e.contains("목표"), "{e}");
    let good = w.task(
        "summary.md",
        "# 목표\n흐름 시험\n## 진행\n- [x] 메시지 두 개 보내기\n- [~] 확인창 답 보내기\n- [ ] 다음 지시 확인\n",
    );
    let v = w.h.ok(&["thread", "summary", "demo", &id, "--file", &good]);
    assert_eq!(
        (v["goal"].as_str(), v["pending"].as_u64(), v["by"].as_str()),
        (Some("흐름 시험"), Some(0), Some("사람")),
        "{v}"
    );
    let mark = w.show(&id)["summary"].clone();
    assert!(mark["at_ms"].is_u64() && mark["pending"] == 0, "{mark}");
    let progress = serde_json::json!([
        {"text": "메시지 두 개 보내기", "state": "done"},
        {"text": "확인창 답 보내기", "state": "working"},
        {"text": "다음 지시 확인", "state": "pending"}
    ]);
    assert_eq!(v["progress"], progress);

    // 답보다 늦게 온 같은 확인창의 짝 hook(PermissionRequest → 답 → Notification, ci-local에서 난 차례, plan §8.99)도 새
    // 질문을 열지 않는다. 진행 보고가 같은 연결로 그 뒤에 오므로, 그것이 보이면 늦은 Notification을 처리한 뒤다
    assert_eq!(w.message(&id, "late ask", false)["state"], "accepted");
    // 요약 뒤에 보낸 지시는 「반영 안 됨」으로 세고 그 첫 줄을 보인다
    let v = w.h.ok(&["thread", "summary", "demo", &id]);
    assert_eq!(v["pending"], 1, "{v}");
    assert_eq!(v["pending_lines"][0]["text"], "late ask", "{v}");
    assert_eq!(v["progress"], progress, "새 지시가 완료 개수나 진행 순서를 바꾸지 않는다");
    assert_eq!(w.show(&id)["summary"]["pending"], 1);
    let pl = open_question(&mut ev, &id, "permission", &[&p1.id]);
    let t = lab::now_ms();
    w.h.ok(&["question", "answer", &pl.id, "no"]);
    progress_seen(&mut ev, &id, "늦은 짝 뒤");
    assert!(w.questions(false).is_empty(), "늦은 짝 hook이 질문을 다시 열었다: {:?}", w.questions(false));
    agent_became(&mut ev, &id, AgentActivity::Idle, t);

    // 치는 동안 뜬 확인창(t-0340 보안 감사): 글을 친 뒤 Enter 전에 권한 창이 뜨면 Enter를 보내지 않는다. 보내면 Enter가
    // 사람이 보지 않은 권한 창의 기본 선택(1. Yes)을 고른다. Enter 전 검사가 없으면 화면에 「race 1. Yes」가 있다. 검사는
    // 프로그램이 친 글에 반응한 뒤다: 고정 300ms에 보면 부하 아래 아직 그리지 않은 창이 Enter를 받았다(ci-local)
    assert_eq!(w.message(&id, "race", false)["state"], "accepted");
    let t = lab::now_ms();
    finish_turn(&w, &pane);
    w.wait_screen(&pane, "race armed");
    agent_became(&mut ev, &id, AgentActivity::Idle, t);
    let m = w.message(&id, "x", false);
    assert_eq!(m["state"], "unknown", "{m}");
    assert!(!w.h.screen(&pane).contains("race 1. Yes"), "Enter가 권한 창을 골랐다: {}", w.h.screen(&pane));
    flow_event(&mut ev, "message_unknown", "Enter를 보내지 않았다");
    // 받음은 Enter가 일으킨 사건으로만 센다: 치는 동안 뜬 확인창(질문)도 받음 사건이라 친 글 앞부터 기다리면 그 글을
    // 받았다고도 적었다(origin 화면 감지와 합친 뒤 난 차례)
    let mid = m["id"].as_str().unwrap();
    let evs = w.h.ok(&["events", "demo"]);
    let said = evs["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "message_accepted" && e["detail"].as_str().is_some_and(|d| d.contains(mid)));
    assert!(!said, "Enter를 보내지 않은 글을 받았다고 적었다: {evs}");
    let pr = open_question(&mut ev, &id, "permission", &[&p1.id, &pl.id]);
    let t = lab::now_ms();
    // 「no」는 Esc다(권한 창의 「No, … (esc)」)
    w.h.ok(&["question", "answer", &pr.id, "no"]);
    w.wait_screen(&pane, "race cancel");
    agent_became(&mut ev, &id, AgentActivity::Idle, t);

    // 거절(Esc)
    let m3 = w.message(&id, "ask again", false);
    assert_eq!(m3["state"], "accepted", "{m3}");
    let p2 = open_question(&mut ev, &id, "permission", &[&p1.id, &pl.id, &pr.id]);
    // 확인창이 열려 있으면 interrupt·메시지는 거부하고 기록도 남기지 않는다(친 글·Enter·Esc를 확인창이 받는다)
    let e =
        w.h.json(&["thread", "message", "demo", &id, "--interrupt", "--text", "NEVER-SENT"])
            .expect_err("질문 중 interrupt");
    assert!(e.contains("확인창"), "{e}");
    let e = w.h.json(&["thread", "message", "demo", &id, "--text", "NEVER-SENT"]).expect_err("질문 중 메시지");
    assert!(e.contains("agent_blocked") && e.contains(&format!("aaa thread read demo {id}")), "{e}");
    let t = lab::now_ms();
    w.h.ok(&["question", "answer", &p2.id, "no"]);
    // 거절하면 Claude는 턴을 멈추고 입력 상자를 보인다: hook 없이 화면으로 idle이다
    agent_became(&mut ev, &id, AgentActivity::Idle, t);
    assert!(w.h.screen(&pane).contains("denied"));

    // interrupt: 일하는 중이면 Esc로 멈춘 뒤, 멈춤(입력 상자)이 보이면 보낸다. 멈추기 전에 보낸 글은 사라진다.
    assert_eq!(w.message(&id, "long run", false)["state"], "accepted");
    // accepted는 hook이 먼저 알릴 수 있다. 실제 앞선 턴의 수신을 확인하고 중단한다.
    w.wait_screen(&pane, "got: long run");
    let (m4, screen) = w.interrupt(&id, &pane, "urgent");
    assert_eq!(m4["state"], "accepted", "{m4}\n{screen}");
    // 앞선 출력이 visible 화면에서 밀려나도 된다. 중단 뒤 긴급 입력의 순서는 유지한다.
    assert!(screen.rfind("Interrupted").is_some_and(|i| screen.find("got: urgent").is_some_and(|u| i < u)), "{screen}");
    // 두 번째 interrupt
    let t = lab::now_ms();
    finish_turn(&w, &pane);
    agent_became(&mut ev, &id, AgentActivity::Idle, t);
    assert_eq!(w.message(&id, "long again", false)["state"], "accepted");
    w.wait_screen(&pane, "got: long again");
    let (m5, screen) = w.interrupt(&id, &pane, "urgent2");
    assert_eq!(m5["state"], "accepted", "{m5}\n{screen}");
    // 거절 셋(늦은 짝·치는 동안 뜬 권한 창·Esc 거절)과 interrupt 둘
    assert_eq!(screen.matches("Interrupted").count(), 5, "{screen}");
    // thread stop: Esc 한 번(세션은 그대로)
    let t = lab::now_ms();
    finish_turn(&w, &pane);
    agent_became(&mut ev, &id, AgentActivity::Idle, t);
    assert_eq!(w.message(&id, "long three", false)["state"], "accepted");
    w.wait_screen(&pane, "got: long three");
    let t = lab::now_ms();
    w.h.ok(&["thread", "stop", "demo", &id]);
    agent_became(&mut ev, &id, AgentActivity::Idle, t);
    let screen = w.h.screen(&pane);
    assert_eq!(screen.matches("Interrupted").count(), 6, "{screen}");
    assert_eq!(w.h.pane(&pane)["state"], "running");
    assert!(!screen.contains("NEVER-SENT"), "{screen}");

    let all = w.questions(true);
    let got: Vec<_> =
        all.iter().map(|q| (q["question_kind"].as_str().unwrap(), q["answer"].as_str().unwrap_or("-"))).collect();
    // 신뢰, brief 권한, 늦은 짝, 치는 동안 뜬 권한 창, 거절
    assert_eq!(
        got,
        [("trust", "yes"), ("permission", "yes"), ("permission", "no"), ("permission", "no"), ("permission", "no")]
    );
    // 질문마다 항목을 만들지 않는다: 30초 안에 답한 확인창은 확인 필요가 아니다(herdr-projects `BLOCKED_DEBOUNCE_SECS`,
    // plan §8.85). 진행을 40%로 알린 채 쓴 보고는 중간 보고다(검토 대기 아님)
    let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
    assert!(items.iter().all(|i| i["item_kind"] != "question" && i["event"] != "blocked on a prompt"), "{items:?}");
    assert!(!items.iter().any(|i| i["event"] == "new report"), "{items:?}");
    let kinds = w.kinds(0);
    let mut msg: Vec<&str> =
        kinds.iter().map(String::as_str).filter(|k| k.starts_with("message_") || k.starts_with("question_")).collect();
    // 치는 동안 뜬 권한 창의 「모름」은 질문(화면 감지)과 따로 와 그 둘의 차례는 정해지지 않는다: 한 번인 것만 보고 뺀다
    assert_eq!(msg.iter().filter(|k| **k == "message_unknown").count(), 1, "{msg:?}");
    msg.retain(|k| *k != "message_unknown");
    assert_eq!(
        msg,
        [
            "question_shown",
            "question_answered",
            "question_shown",
            "question_answered",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "question_shown",
            "question_answered",
            "message_queued",
            "message_accepted",
            "message_queued",
            "question_shown",
            "question_answered",
            "message_queued",
            "message_accepted",
            "question_shown",
            "question_answered",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
        ]
    );
}

/// profile·safety를 사람이 바꾸고 thread가 쓴다(herdr-projects `profile`·`safety`, plan §8.163): 세션 안·터미널 아님은 거부,
/// 사람이 y로 켠 yolo가 다음 thread 인자·context에 닿고(host를 다시 띄우지 않음), host.toml에 더한 profile을
/// agent를 다시 띄우지 않고 허용·사용한다.
#[cfg(unix)]
#[test]
fn safety_and_profiles_by_a_person() {
    let w = world();
    // 대상을 생략하면 이 host의 프로젝트(plan §8.288). 모든 프로젝트 기본값은 --global
    let show =
        |t: &str| -> Value { if t.is_empty() { w.h.ok(&["safety", "show"]) } else { w.h.ok(&["safety", "show", t]) } };
    let row = |v: &Value, key: &str| -> (String, String) {
        let r = v["rows"].as_array().unwrap().iter().find(|r| r["key"] == key).unwrap().clone();
        (r["value"].as_str().unwrap().to_string(), r["source"].as_str().unwrap().to_string())
    };
    let s = show("");
    assert_eq!(s["target"], "demo");
    assert_eq!(row(&s, "yolo"), ("off".into(), "기본값".into()));
    assert_eq!(row(&s, "thread_profiles").1, "host.toml [projects]", "옛 allow_profiles가 아래 층: {s}");
    // 바꾸기는 사람만: 세션 안, 터미널 아님
    let o = w.h.cmd(&["safety", "yolo", "on"]).env("AAA_SESSION_ID", "x").capture_output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("aaa 세션 안에서는"), "{o:?}");
    let o = w.h.run(&["safety", "yolo", "on"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("터미널"), "{o:?}");
    let o = w.h.run(&["profile", "add", "sh1", "--agent", "shell"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("터미널"), "{o:?}");
    // 틀린 값은 사람인지 묻기 전에 알린다
    let o = w.h.run(&["safety", "set", "start_threads", "now"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("propose·auto"), "{o:?}");
    let o = w.h.run(&["profile", "allow", "coordinator", "nope"]);
    // 없는 이름은 사람인지 묻기 전에(정의는 host.toml 한 곳, §8.170)
    assert!(String::from_utf8_lossy(&o.stderr).contains("host.toml에 profile `nope`가 없다"), "{o:?}");
    // 사람이 y가 아니면 바꾸지 않는다
    let out = w.h.in_terminal(&["safety", "yolo", "on"], &[("[y/N]", "n")], T);
    assert!(out.contains("권한 건너뛰기") && out.contains("바꾸지 않았다"), "{out}");
    assert_eq!(row(&show(""), "yolo").0, "off");
    let out = w.h.in_terminal(&["safety", "yolo", "on"], &[("[y/N]", "y")], T);
    assert!(out.contains("demo: yolo = on"), "{out}");
    let s = show("");
    assert_eq!(row(&s, "yolo"), ("on".into(), "프로젝트".into()));
    assert_eq!(row(&s, "start_threads").0, "auto");
    assert_eq!(row(&s, "trust_screens").0, "coordinator");
    assert_eq!(row(&show("--global"), "yolo").0, "off", "다른 프로젝트는 그대로");
    assert!(std::fs::read_to_string(w.h.cfg.join("host.toml")).unwrap().contains("[safety.demo]\nyolo = true\n"));
    assert_eq!(w.h.ok(&["context", "demo", "--peek"])["start_threads"], "auto", "host를 다시 띄우지 않고 닿는다");
    // yolo가 thread 인자에 닿는다: claude harness는 권한 건너뛰기 인자로, 사람이 정의한 profile이라 thread_agent_args는 없다.
    // 옛 꼴(프로젝트 이름)도 받는다
    let out = w.h.in_terminal(&["safety", "set", "demo", "thread_agent_args", "--verbose"], &[("[y/N]", "y")], T);
    assert!(out.contains("thread_agent_args = --verbose"), "{out}");
    let v = w.start(&["--title", "y", "--profile", "claude", "--kind", "dir", "--task-file", &w.task("y.txt", "x")]);
    let argv = w.h.pane(v["pane"].as_str().unwrap())["argv"].to_string();
    assert!(argv.contains("--dangerously-skip-permissions") && !argv.contains("--verbose"), "{argv}");
    // custom harness는 yolo 인자가 없다
    let v = w.start(&["--title", "f", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("f.txt", "x")]);
    assert!(!w.h.pane(v["pane"].as_str().unwrap())["argv"].to_string().contains("dangerously"));
    // `[safety]` 표 하나(plan §8.288)는 프로젝트 표보다 앞선다. 프로젝트 표를 고치면 덮인다고 알린다
    let host_toml = w.h.cfg.join("host.toml");
    let before = std::fs::read_to_string(&host_toml).unwrap();
    std::fs::write(&host_toml, format!("{before}\n[safety]\nyolo = false\n")).unwrap();
    assert_eq!(row(&show(""), "yolo"), ("off".into(), "프로젝트".into()));
    let out = w.h.in_terminal(&["safety", "yolo", "on"], &[("[y/N]", "y")], T);
    assert!(out.contains("host.toml의 [safety] yolo가 이 값을 덮는다"), "{out}");
    std::fs::write(&host_toml, &before).unwrap();
    assert_eq!(row(&show(""), "yolo").0, "on");

    // host.toml에 profile을 더하고, 허용하고, agent를 다시 띄우지 않고 쓴다
    let o = w.h.in_terminal(&["profile", "allow", "threads", "fake", "sh1", "--project", "demo"], &[], T);
    assert!(o.contains("host.toml에 profile `sh1`가 없다"), "{o}");
    let o = w.h.in_terminal(
        &["profile", "add", "sh1", "--agent", "shell", "--description", "셸 하나", "--arg", "-i"],
        &[],
        T,
    );
    assert!(o.contains("profile `sh1`를 더했다: shell · 기본 model · args: -i — 셸 하나"), "{o}");
    let names = w.h.run(&["profile", "list", "--machine", "labA", "--names"]);
    assert!(String::from_utf8_lossy(&names.stdout).lines().any(|l| l == "sh1"), "{names:?}");
    let o = w.h.in_terminal(&["profile", "allow", "threads", "fake", "sh1", "--project", "demo"], &[], T);
    assert!(o.contains("thread는 `demo`에서 fake, sh1를 쓸 수 있다"), "{o}");
    let e = w.h.json(&[
        "thread",
        "start",
        "demo",
        "--title",
        "c",
        "--profile",
        "claude",
        "--task-file",
        &w.task("c.txt", "x"),
    ]);
    assert!(e.expect_err("허용 밖").contains("허용: fake, sh1"));
    let v = w.start(&["--title", "s", "--profile", "sh1", "--kind", "dir", "--task-file", &w.task("s.txt", "x")]);
    assert_eq!(w.h.pane(v["pane"].as_str().unwrap())["argv"], serde_json::json!(["/bin/sh", "-i"]));
    let l = w.h.ok(&["profile", "list", "--machine", "labA", "--project", "demo"]);
    assert_eq!(l["project"]["thread_profiles"], serde_json::json!(["fake", "sh1"]), "{l}");
    assert_eq!(l["project"]["thread_profile"], "claude", "기본값 사슬의 끝: {l}");
    // resolve: 이 머신 profile의 harness·인자(기본 profile도)
    let r = w.h.ok(&["profile", "resolve", "sh1"]);
    assert_eq!((r["harness"].as_str(), r["args"].clone()), (Some("shell"), serde_json::json!(["-i"])));
    assert_eq!(w.h.ok(&["profile", "resolve", "codex"])["builtin"], true);
    let o = w.h.in_terminal(&["profile", "remove", "sh1"], &[], T);
    assert!(o.contains("profile `sh1`를 지웠다"), "{o}");
    let e = w.h.json(&[
        "thread",
        "start",
        "demo",
        "--title",
        "g",
        "--profile",
        "sh1",
        "--task-file",
        &w.task("g.txt", "x"),
    ]);
    assert!(e.expect_err("지운 profile").contains("profile `sh1`가 없다"));

    // host.toml 없이 기본값으로 시작한 host도 첫 profile을 만들 수 있다. agent 파일은 바꾸지 않는다.
    let host_file = w.h.cfg.join("host.toml");
    let saved = w.h.cfg.join("host.saved");
    let agent_file = std::fs::read(w.h.cfg.join("agent.toml")).unwrap();
    std::fs::rename(&host_file, &saved).unwrap();
    let out = w.h.in_terminal(&["profile", "add", "first", "--agent", "shell"], &[], T);
    assert!(out.contains("profile `first`를 더했다"), "{out}");
    assert_eq!(w.h.ok(&["profile", "resolve", "first"])["harness"], "shell");
    assert_eq!(std::fs::read(w.h.cfg.join("agent.toml")).unwrap(), agent_file);
    std::fs::remove_file(&host_file).unwrap();
    std::fs::rename(saved, host_file).unwrap();
}

/// 끝난 thread pane은 새 pane이 자리를 이은 뒤 host·agent에서 지운다. 다른 pane은 건드리지 않는다.
#[test]
fn restart_replaces_exited_panes_locally_and_remotely() {
    let mut w = world();
    w.add_remote();
    let other = w.h.new_pane(None, "labA", Some("다른 작업"), &["/bin/sh"], None);
    for machine in ["labA", "labB"] {
        let v = w.start(&[
            "--title",
            machine,
            "--profile",
            "fake",
            "--machine",
            machine,
            "--kind",
            "dir",
            "--task-file",
            &w.task(&format!("{machine}.txt"), "noreport"),
        ]);
        let id = v["id"].as_str().unwrap();
        let mut old = v["pane"].as_str().unwrap().to_string();
        w.wait_show("첫 brief 전달", id, |v| v["brief"] == "delivered");
        w.h.ok(&["pane", "rename", &old, "이을 이름"]);
        w.h.ok(&["pane", "move", &old, "--new-tab", "-w", machine]);
        let tab = w.h.ok(&["workspace", "get", machine])["tabs"][0]["id"].clone();
        for mode in ["profile", "fresh", "resume", "pane"] {
            // profile 교체는 kill 뒤, 나머지는 자발적 exit 뒤에 시작한다.
            if mode == "profile" {
                w.h.ok(&["pane", "kill", &old]);
            } else {
                w.h.ok(&["pane", "run", &old, "exit-one"]);
            }
            wait_for("옛 pane 종료", T, || (w.h.pane(&old)["state"] == "exited").then_some(()));
            let next = match mode {
                "profile" => w.h.ok(&["thread", "restart", "demo", id, "--profile", "fake"])["pane"].clone(),
                "fresh" => w.h.ok(&["thread", "restart", "demo", id])["pane"].clone(),
                "resume" => w.h.ok(&["thread", "restart", "demo", id, "--resume"])["pane"].clone(),
                _ => w.h.ok(&["pane", "resume", &old])["id"].clone(),
            };
            let new = next.as_str().unwrap();
            assert_ne!(new, old);
            assert_eq!(w.show(id)["pane"], new, "{machine}/{mode}: thread 결속");
            let panes = w.h.ok(&["pane", "list"]);
            assert!(
                !panes["panes"].as_array().unwrap().iter().any(|p| p["id"] == old),
                "{machine}/{mode}: 옛 pane이 기타로 남았다: {panes}"
            );
            let pane = w.h.pane(new);
            assert_eq!(pane["label"], "이을 이름");
            assert!(pane["replaced"].as_array().unwrap().iter().any(|p| p == &old));
            let layout = w.h.ok(&["workspace", "get", machine]);
            assert_eq!(layout["tabs"][0]["id"], tab);
            assert!(layout.to_string().contains(new) && !layout.to_string().contains(&old), "{layout}");
            assert_eq!(w.h.pane(&other)["state"], "running", "다른 pane은 유지");
            if matches!(mode, "profile" | "fresh") {
                w.wait_show("새 brief 전달", id, |v| v["brief"] == "delivered");
            } else {
                w.wait_screen(new, "fake>");
            }
            old = new.to_string();
        }
    }
    // agent가 기억한 옛 pane도 지웠는지 Hello 재생으로 확인한다.
    let before = w.h.ok(&["pane", "list"])["panes"].as_array().unwrap().len();
    // 원격 agent가 접속한 주소를 유지한다. :0을 다시 쓰면 새 포트에 떠 재연결 시험이 되지 않는다.
    let config = w.h.cfg.join("host.toml");
    let original = std::fs::read_to_string(&config).unwrap();
    let pinned = original.replace("agent_listen = \"127.0.0.1:0\"", &format!("agent_listen = {:?}", w.addr));
    assert_ne!(pinned, original);
    std::fs::write(config, pinned).unwrap();
    w.lab.kill("host");
    spawn_host(&mut w.lab, "host-again", &w.h, Some(BRIEF_SETTLE_MS));
    wait_for("두 agent 재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    assert_eq!(w.h.ok(&["pane", "list"])["panes"].as_array().unwrap().len(), before);
}

#[test]
fn restart_spawn_failure_keeps_the_old_pane_until_a_successful_replacement() {
    let w = report_shell_world();
    let v = w.start(&[
        "--title",
        "실패 보존",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("failure.txt", "noreport"),
    ]);
    let (id, old) = (v["id"].as_str().unwrap(), v["pane"].as_str().unwrap());
    w.wait_show("첫 brief 전달", id, |v| v["brief"] == "delivered");
    w.h.ok(&["pane", "move", old, "--new-tab", "-w", "kept"]);
    w.h.ok(&["pane", "kill", old]);
    wait_for("종료", T, || (w.h.pane(old)["state"] == "exited").then_some(()));
    let config = w.h.cfg.join("host.toml");
    let before = std::fs::read_to_string(&config).unwrap();
    let broken = before.replace(
        &format!("command = [\"/bin/sh\", \"{}\"]", path_str(&w.scripts.join("fake.sh"))),
        "command = [\"/nonexistent-aaa-fixture\"]",
    );
    assert_ne!(broken, before);
    std::fs::write(&config, broken).unwrap();
    w.h.json(&["thread", "restart", "demo", id]).expect_err("없는 실행 파일");
    assert_eq!(w.show(id)["pane"], old);
    assert_eq!(w.h.pane(old)["state"], "exited");
    assert!(w.h.ok(&["workspace", "get", "kept"]).to_string().contains(old));
    // 고장난 설정을 고친 뒤 성공해야 비로소 옛 pane을 지운다.
    std::fs::write(&config, before).unwrap();
    let next = w.h.ok(&["thread", "restart", "demo", id]);
    assert_ne!(next["pane"], old);
    assert!(w.h.json(&["pane", "get", old]).is_err());
}

/// 시작이 실패한 thread도 `thread restart`로 다시 띄운다(herdr-projects threads.rs `restart_plan`, `aaa guide threads`,
/// 차이 목록 E8). repo가 아직 git이 아니라 자리를 만들지 못했다(a): 시작 오류는 thread ID와 다시 하는 법을 말하고
/// thread는 failed다. repo를 고친 뒤 restart는 자리부터 다시 만들고 앞선 시도 안내를 넣은 brief를 준다. 실패 글은 지운다.
/// 기록된 worktree 없이 branch만 있으면 반쯤 만든 것이라 거부한다(b). 끝낸 thread는 `--reopen`이 먼저다.
#[test]
fn failed_threads_restart_from_the_place() {
    let w = report_shell_world();
    let raw = w.lab.root.join("raw");
    std::fs::create_dir_all(&raw).unwrap();
    std::fs::write(raw.join("README.md"), "hi\n").unwrap();
    let task = w.task("e8.txt", "repo가 준비되면 한다");
    for title in ["later", "half"] {
        let e =
            w.h.json(&[
                "thread",
                "start",
                "demo",
                "--title",
                title,
                "--profile",
                "fake",
                "--repo",
                path_str(&raw),
                "--task-file",
                &task,
            ])
            .expect_err("git이 아닌 repo");
        let id = if title == "later" { "t-0001" } else { "t-0002" };
        assert!(e.contains(&format!("`aaa thread restart demo {id}`로 다시 한다")), "{e}");
    }
    let v = w.show("t-0001");
    assert_eq!((v["status"].as_str(), v["group"].as_str()), (Some("failed"), Some("waiting_on_you")), "{v}");
    assert!(v["error"].as_str().is_some_and(|m| m.contains("rev-parse")), "{v}");
    let e = w.h.json(&["thread", "restart", "demo", "t-0001", "--resume"]).expect_err("이을 대화 없음");
    assert!(e.contains("이을 대화가 없다"), "{e}");

    git(&raw, &["init", "-q", "-b", "main"]);
    git(&raw, &["add", "."]);
    git(&raw, &["commit", "-qm", "init"]);
    // (b) 반쯤 만든 worktree: 기록에는 자리가 없는데 branch가 있다
    git(&raw, &["branch", "aaa/demo/t-0002-half"]);
    let e = w.h.json(&["thread", "restart", "demo", "t-0002"]).expect_err("반쯤 만든 worktree");
    assert!(e.contains("aaa/demo/t-0002-half") && e.contains("aaa thread resolve"), "{e}");
    assert_eq!(w.show("t-0002")["status"], "failed");

    // (a) 자리부터 다시
    let v = w.h.ok(&["thread", "restart", "demo", "t-0001"]);
    assert_eq!((v["status"].as_str(), v["error"].as_str()), (Some("open"), None), "{v}");
    assert_eq!(v["branch"], "aaa/demo/t-0001-later", "{v}");
    let brief = std::fs::read_to_string(PathBuf::from(v["work_dir"].as_str().unwrap()).join("brief.md")).unwrap();
    assert!(brief.contains("**이 일을 앞서 시도한 것이") && brief.contains("repo가 준비되면 한다"), "{brief}");
    w.wait_show("brief 받음", "t-0001", |v| v["brief"] == "delivered");

    w.h.ok(&["thread", "resolve", "demo", "t-0002"]);
    let e = w.h.json(&["thread", "restart", "demo", "t-0002"]).expect_err("끝낸 thread");
    assert!(e.contains("--reopen"), "{e}");
}

/// 끝낸 inbox가 많은 프로젝트(0.2에서 옮긴 기록 수천 건, t-0334 pilot 결함 2, plan §8.98). 응답이 frame 상한(1 MiB)을
/// 넘어 연결이 말없이 닫히던 것: `inbox list --all`은 크기 한도 안에서 끝내지 않은 항목 모두와 최근 끝낸 항목을 싣고,
/// 못 실은 수를 말한다. 기록은 실제 경로(`aaa migrate inventory`·`import`)로 넣는다.
#[test]
fn big_inbox_lists_within_frame() {
    const DONE: u32 = 4000;
    let mut lab = Lab::new();
    let projects = lab.root.join("projects");
    let h = lab.node("h", Some(&host_toml(&projects, "", false)));
    // 가짜 0.2 프로젝트(herdr-projects inbox.rs `write` 형식): 끝낸 항목 4000개(합쳐 약 2 MB)와 끝내지 않은 항목 둘
    let src = lab.root.join("hp").join("demo");
    std::fs::create_dir_all(src.join("inbox").join("done")).unwrap();
    std::fs::write(src.join("PROJECT.md"), "+++\nname = \"Demo\"\n+++\n\n# 지침\n").unwrap();
    let summary = "끝난 일의 요약이다. ".repeat(12);
    let item = |n: u32| {
        let (hh, mm, ss) = (n / 3600, n / 60 % 60, n % 60);
        let id = format!("20261001T{hh:02}{mm:02}{ss:02}Z-thread-state-t-{n:04}-{n}");
        let text = format!(
            "+++\nid = \"{id}\"\nkind = \"thread-state\"\nsubject = \"t-{n:04}\"\ncreated = \"2026-10-01T{hh:02}:{mm:02}:{ss:02}Z\"\n\
             summary = \"{summary}\"\nevent = \"idle\"\n+++\n"
        );
        (id, text)
    };
    for n in 1..=DONE + 2 {
        let (id, text) = item(n);
        let dir = if n <= DONE { src.join("inbox").join("done") } else { src.join("inbox") };
        std::fs::write(dir.join(format!("{id}.md")), text).unwrap();
    }
    let inv = lab.root.join("inv");
    let o = h.run(&[
        "migrate",
        "inventory",
        "--from",
        path_str(&lab.root.join("hp")),
        "--project",
        "demo",
        "--out",
        path_str(&inv),
        "--home-machine",
        "mac-a",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = h.run(&["migrate", "import", "--inventory", path_str(&inv.join("inventory.json")), "--to-project", "demo"]);
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(
        String::from_utf8_lossy(&o.stdout).contains(&format!("inbox 2+{DONE}")),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    spawn_host(&mut lab, "host", &h, Some(BRIEF_SETTLE_MS));
    wait_for("host 준비", T, || h.json(&["status"]).ok());

    let ids = |v: &Value| -> Vec<String> {
        v["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
    };
    let v = h.ok(&["inbox", "list", "demo", "--all", "--peek"]);
    let shown = ids(&v);
    let omitted = v["omitted_done"].as_u64().unwrap() as usize;
    assert_eq!(v["omitted_open"], 0, "끝내지 않은 항목은 모두 싣는다");
    assert!(omitted > 0 && shown.len() + omitted == DONE as usize + 2, "{} + {omitted}", shown.len());
    // 끝내지 않은 둘과 가장 최근 끝낸 것은 있고, 가장 옛 것은 없다. 차례는 만든 때다
    for n in [DONE + 1, DONE + 2, DONE] {
        assert!(shown.contains(&item(n).0), "{n}");
    }
    assert!(!shown.contains(&item(1).0));
    assert!(shown.windows(2).all(|w| w[0] < w[1]));
    let text = String::from_utf8_lossy(&h.run(&["inbox", "list", "demo", "--all", "--peek"]).stdout).to_string();
    assert!(
        text.contains(&format!("끝낸 항목 {omitted}개는 싣지 못했다(최근 것부터 실었다)")),
        "{:?}",
        text.lines().rev().take(3).collect::<Vec<_>>()
    );
    // 끝내지 않은 항목만 보면 한도 안이다
    let v = h.ok(&["inbox", "list", "demo", "--peek"]);
    assert_eq!((ids(&v).len(), v["omitted_open"].as_u64(), v["omitted_done"].as_u64()), (2, Some(0), Some(0)));
    assert_eq!(h.ok(&["context", "demo", "--peek"])["inbox_open"], 2);
    // 끝내면 가장 최근 끝낸 항목이 된다
    let out = h.run(&["inbox", "done", "demo", "--all"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("2개 항목을 끝냈다"), "{out:?}");
    let v = h.ok(&["inbox", "list", "demo", "--all", "--peek"]);
    assert!(ids(&v).ends_with(&[item(DONE + 1).0, item(DONE + 2).0]));
    assert_eq!(v["omitted_done"].as_u64().unwrap() as usize + ids(&v).len(), DONE as usize + 2);
}

/// harness 종료 사건과 늦은 연결을 위한 session host의 수명은 별개다.
#[test]
fn harness_exit_is_published_while_the_session_host_still_answers() {
    let w = world_setup("", false, |_| vec![("AAA_TEST_SESSION_LINGER_MS".into(), "8000".into())]);
    std::fs::write(
        w.scripts.join("resumable.sh"),
        "#!/bin/sh\nprintf 'owned-exit-ready\\n'\nwhile IFS= read -r line; do\n  [ \"$line\" != owned-exit-now ] || exit 0\ndone\n",
    )
    .unwrap();
    let started = w.start(&[
        "--title",
        "종료 사건",
        "--profile",
        "resumable",
        "--kind",
        "dir",
        "--task-file",
        &w.task("owned-exit.md", "종료 상태 관측"),
    ]);
    let id = started["id"].as_str().unwrap();
    let pane = started["pane"].as_str().unwrap();
    w.h.wait_output(pane, "owned-exit-ready", T);
    let pid = w.h.session_host_pid(pane).expect("이 실험실의 session host PID");
    let mut events = Events::connect(&w.h);
    w.h.ok(&["pane", "run", pane, "owned-exit-now"]);
    events.wait("살아 있는 session host의 harness 종료 사건", T, |event| {
        let FrontEvent::Thread(thread) = event else { return None };
        (thread.id == id && thread.agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Exited)).then_some(())
    });
    assert!(!w.h.session_host_gone(pid, pane), "exited를 알리려고 session host의 유예 종료를 기다렸다");
    assert!(w.h.screen(pane).contains("owned-exit-ready"), "유예 중에도 마지막 화면은 읽는다");
}

/// 끝난 세션을 지우는 resolve·`pane close`는 session host가 종료 뒤 더 응답하는 창(늦게 붙는 agent용, 2초)을 기다리지
/// 않는다(t-0338 넘김: resolve 한 번 2.05초, plan §8.160). 창을 agent가 pump를 기다리는 한도(5초)보다 길게 두면, 창을 기다리는 agent는
/// 「session host가 아직 닫히지 않았다」로 pane을 지우지 못한다
#[test]
fn closing_ended_sessions_does_not_wait_for_the_session_host_linger() {
    let w = world_setup("", false, |_| vec![("AAA_TEST_SESSION_LINGER_MS".into(), "8000".into())]);
    let v = w.start(&[
        "--title",
        "linger",
        "--machine",
        "labA",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("linger.txt", "noreport"),
    ]);
    let (id, pane) = (v["id"].as_str().unwrap().to_string(), v["pane"].as_str().unwrap().to_string());
    w.wait_screen(&pane, "done");
    assert_eq!(w.h.ok(&["thread", "resolve", "demo", &id])["status"], "resolved");
    let ev = w.h.ok(&["events", "demo"]);
    let resolved = ev["events"].as_array().unwrap().iter().find(|e| e["kind"] == "thread_resolved").unwrap();
    assert!(resolved["detail"].as_str().unwrap().contains("pane을 닫았다"), "{resolved}");
    let shell = w.h.new_pane(None, "labA", Some("lg"), &["/bin/cat"], None);
    w.h.ok(&["pane", "close", &shell]);
    let panes = w.h.ok(&["pane", "list"]);
    let ids: Vec<&str> = panes["panes"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert!(!ids.contains(&pane.as_str()) && !ids.contains(&shell.as_str()), "끝낸 pane이 남았다: {panes}");
}

/// resolve는 이미 exit 1로 끝난 pane도 목록과 layout에서 지운다. worktree 보존·adopt 여부와 독립적이다.
#[test]
fn resolve_removes_exited_panes_even_when_the_worktree_is_kept_or_adopted() {
    let w = world();
    let claude = w.scripts.join("claude");
    std::fs::write(&claude, ADOPT_AGENT.replace("@BIN@", lab::BIN)).unwrap();
    std::fs::set_permissions(&claude, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    // 서로 다른 세 thread를 원래 차례로 만든 뒤 각각의 실제 보고를 기다린다.
    // 앞 thread의 brief·보고 대기에 다음 thread의 독립된 준비를 묶지 않는다.
    let mut threads = Vec::new();
    for kind in ["dir", "worktree", "adopted"] {
        let v = if kind == "adopted" {
            let pane = w.h.new_pane(None, "labA", Some("adopt"), &[path_str(&claude)], Some(path_str(&w.repo)));
            wait_for("adopt harness 준비", T, || {
                w.h.pane(&pane)["agent"]["harness_session"].is_string().then_some(())
            });
            w.h.ok(&["thread", "adopt", "demo", "--pane", &pane, "--title", kind])
        } else {
            let task = w.task(&format!("{kind}.txt"), "dirty");
            let mut args = vec!["--title", kind, "--profile", "fake", "--task-file", &task];
            if kind == "worktree" {
                args.extend_from_slice(&["--repo", path_str(&w.repo)]);
            } else {
                args.extend_from_slice(&["--kind", "dir"]);
            }
            w.start(&args)
        };
        threads.push((kind, v));
    }
    for (kind, v) in threads {
        let (id, pane) = (v["id"].as_str().unwrap(), v["pane"].as_str().unwrap());
        w.wait_show("보고와 brief 전달", id, |v| v["report_hash"].is_string() && v["brief"] == "delivered");
        // 신호 없이 자기 fixture를 정상 입력으로 종료한다. 종료 상태를 확인한 뒤에만 resolve한다.
        w.h.ok(&["pane", "run", pane, "exit-one"]);
        wait_for("exit 1", T, || {
            let p = w.h.pane(pane);
            (p["state"] == "exited" && p["code"] == 1).then_some(())
        });
        // pane 목록과 그 pane을 담은 layout을 함께 확인한다.
        w.h.ok(&["pane", "move", pane, "--new-tab", "-w", "exited"]);
        assert_eq!(w.h.ok(&["thread", "resolve", "demo", id])["status"], "resolved");
        let panes = w.h.ok(&["pane", "list"]);
        assert!(!panes["panes"].as_array().unwrap().iter().any(|p| p["id"] == pane), "{kind}: {panes}");
        let layout = w.h.ok(&["workspace", "get", "exited"]);
        assert!(!layout.to_string().contains(pane), "{kind}: {layout}");
        let cwd = Path::new(v["cwd"].as_str().unwrap());
        if kind == "worktree" {
            assert_eq!(std::fs::read_to_string(cwd.join("dirty.txt")).unwrap(), "dirty\n");
        } else if kind == "adopted" {
            assert!(cwd.join(".git").exists(), "데려온 저장소는 남겨야 한다");
        }
    }
}

/// thread 범위(§8.25): 일반 thread 세션 안의 CLI는 보기만 되고, thread가 아닌 pane(사람의 셸) 안의 CLI는 그대로 조작한다
#[test]
fn worker_session_cli_is_read_only() {
    let w = report_shell_world();
    let t = w.start(&["--title", "w", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("w.txt", "x")]);
    let (id, pane) = (t["id"].as_str().unwrap(), t["pane"].as_str().unwrap());
    let shell = w.h.new_pane(None, "labA", Some("sh"), &["/bin/sh"], None);
    let as_session = |s: &str, args: &[&str]| w.h.cmd(args).env("AAA_SESSION_ID", s).capture_output().unwrap();
    for args in [
        &["thread", "next", "demo", id, "--add", "thread가 더한 줄"][..],
        &["thread", "message", "demo", id, "--text", "x"],
        &["pane", "send-text", &shell, "echo hi"],
        &["thread", "ack", "demo", id],
        &["post", "handled", "demo", "p-00000001"],
    ] {
        let o = as_session(pane, args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", String::from_utf8_lossy(&o.stdout));
        assert!(String::from_utf8_lossy(&o.stderr).contains("보기 권한"), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let o = as_session(pane, &["--json", "thread", "show", "demo", id]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // 사람의 셸(thread가 아닌 pane) 안에서는 조작이 된다
    let o = as_session(&shell, &["--json", "thread", "next", "demo", id, "--add", "사람이 더한 줄"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let next = w.h.ok(&["thread", "next", "demo", id])["lines"].to_string();
    assert!(next.contains("사람이 더한 줄") && !next.contains("thread가 더한 줄"), "{next}");
}

/// pairing은 실제 coordinator·worker pane의 프로세스 출처를 확인해 거부한다(환경 변수를 지워도 같다).
#[test]
fn web_pair_requires_a_person_outside_aaa_panes() {
    let w = world();
    let mut ev = Events::connect(&w.h);
    let t =
        w.start(&["--title", "pair", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("pair.txt", "x")]);
    let worker = t["pane"].as_str().unwrap();
    // brief도 글과 Enter를 따로 보낸다. 그 사이 probe를 치면 brief 줄에 붙어 실행되지 않는다.
    ev.wait("worker의 brief 수신·처리", T, |e| match e {
        FrontEvent::Thread(v)
            if v.project == "demo"
                && v.id == t["id"].as_str().unwrap()
                && v.brief == aaa_proto::BriefState::Delivered
                && v.report_hash.is_some() =>
        {
            Some(())
        }
        _ => None,
    });
    let coord = w.h.new_pane(None, "labA", Some("coord"), &["/bin/sh"], Some(path_str(&w.projects.join("demo"))));
    for (name, pane) in [("coordinator", coord.as_str()), ("worker", worker)] {
        let out = w.lab.root.join(format!("pair-{name}.txt"));
        let line = format!(
            "env -u AAA_SESSION_ID -u AAA_HOOK_SOCK {} web pair > {} 2>&1; echo rc=$? >> {}",
            lab::shell_word(lab::BIN),
            lab::shell_word(path_str(&out)),
            lab::shell_word(path_str(&out))
        );
        let line = if name == "worker" {
            let script = w.lab.root.join("pair-probe.sh");
            std::fs::write(&script, format!("#!/bin/sh\n{line}\n")).unwrap();
            format!("post-probe {}", path_str(&script))
        } else {
            line
        };
        w.h.ok(&["pane", "run", pane, &line]);
        let got = wait_for_diag(
            &format!("{name} pane 안 pairing 끝"),
            T,
            || std::fs::read_to_string(&out).ok().filter(|s| s.contains("rc=")),
            || {
                format!(
                    "probe: {:?}\npane: {}\n화면:\n{}",
                    std::fs::read_to_string(&out),
                    w.h.pane(pane),
                    w.h.screen(pane)
                )
            },
        );
        assert!(got.contains("사람이 aaa 밖 터미널이나 웹에서") && got.ends_with("rc=1\n"), "{name}: {got}");
    }
    let outside = w.h.in_terminal(&["web", "pair"], &[], T);
    assert!(!outside.contains("사람이 aaa 밖") && outside.contains("코드"), "{outside}");
}

/// 시험용(plan §8.164): 여러 길로 떼어 낸 뒤 나머지 인자를 실행하고, 그 프로세스와 자손이 끝날 때까지 기다렸다 끝난다.
/// 기다림은 pipe다: 실행한 쪽이 쓰기 끝을 물려받아 모두 끝나면 읽기 쪽이 EOF를 받는다(정한 시간을 자지 않는다).
const DETACH_PL: &str = r#"use strict;
use warnings;
use POSIX ();

my $mode = shift @ARGV;
$^F = 1023;    # pipe를 exec 뒤에도 물려준다
pipe(my $r, my $w) or die "pipe: $!";

# 부모 $p가 끝나 다른 부모에 붙을 때까지, $gone이 있으면 그 프로세스가 거둬질 때까지 기다린다(그 사건이 조건이다)
sub detached_from {
    my ($p, $gone) = @_;
    select(undef, undef, undef, 0.005) while getppid() == $p || ($gone && kill(0, $gone));
}
sub run { close $r; exec @ARGV or die "exec: $!" }
sub wait_all { close $w; my $buf; 1 while sysread($r, $buf, 1); }
# 고아를 만든다: 중간 프로세스가 곧바로 끝나 init에 붙은 뒤 $then을 하고 실행한다
sub orphan {
    my ($then) = @_;
    my $k = fork // die;
    if (!$k) { my $kp = $$; my $x = fork // die; exit 0 if $x; detached_from($kp); $then->() if $then; run() }
    waitpid($k, 0);
}

if ($mode eq 'orphan') {
    # nohup … &: 부모가 끝나 init에 붙는다(세션·제어 터미널은 pane 그대로)
    orphan(); wait_all();
} elsif ($mode eq 'setsid') {
    # 새 세션(제어 터미널 없음), 부모는 살아 있다
    my $x = fork // die;
    if (!$x) { POSIX::setsid() or die "setsid: $!"; run() }
    wait_all(); waitpid($x, 0);
} elsif ($mode eq 'leader') {
    # 새 세션의 leader는 살아 있고(pane 안), 그 세션 안 프로세스만 init에 붙는다(Claude Code Bash 도구 셸 안의 nohup … &)
    my $b = fork // die;
    if (!$b) { POSIX::setsid() or die "setsid: $!"; orphan(); wait_all(); exit 0 }
    wait_all(); waitpid($b, 0);
} elsif ($mode eq 'double') {
    # double fork: 새 세션을 연 중간 프로세스가 끝나 세션 leader가 없다
    my $b = fork // die;
    if (!$b) {
        POSIX::setsid() or die "setsid: $!";
        # 중간 프로세스 pid는 fork 전에 잡는다(자식이 getppid로 읽으면 이미 끝나 1일 수 있다)
        my $bp = $$;
        my $x = fork // die;
        exit 0 if $x;
        detached_from($bp, $bp);
        run();
    }
    waitpid($b, 0); wait_all();
} elsif ($mode eq 'wrapper') {
    # 떼어 낸 뒤 새 세션을 열어 셸로 감싼다: leader(셸)는 살아 있고 init의 자식이다
    orphan(sub { POSIX::setsid() or die "setsid: $!" }); wait_all();
} else {
    die "모르는 방식 $mode";
}
"#;

/// 시험용: `cli.sh <결과 파일> <-|=|세션> <이름>` — env를 지우거나(`-`) 그대로(`=`) 또는 다른 세션을 대고 `thread next --add <이름>`
const BYPASS_CLI: &str = r#"out=$1 claim=$2 name=$3
echo "ppid=$PPID" > "$out"
case "$claim" in
  -) env -u AAA_SESSION_ID -u AAA_HOOK_SOCK "$BIN" thread next demo "$ID" --add "$name" >> "$out" 2>&1 ;;
  =) "$BIN" thread next demo "$ID" --add "$name" >> "$out" 2>&1 ;;
  *) AAA_SESSION_ID=$claim "$BIN" thread next demo "$ID" --add "$name" >> "$out" 2>&1 ;;
esac
echo "rc=$?" >> "$out"
"#;

/// 시험용: thread pane 안에서 차례로 우회해 본다. 끝나면 `done`
const BYPASS: &str = r#"export BIN=$1 ID=$2
out=$3 shell_pane=$4 here=$(dirname "$0")
cli="$here/cli.sh" pl="$here/detach.pl"
sh "$cli" "$out/envu.txt" - envu
sh -c 'unset AAA_SESSION_ID AAA_HOOK_SOCK; exec sh "$0" "$1" = shell' "$cli" "$out/shell.txt"
nohup perl "$pl" orphan sh "$cli" "$out/nohup.txt" - nohup > /dev/null 2>&1
perl "$pl" setsid sh "$cli" "$out/setsid.txt" - setsid
perl "$pl" leader sh "$cli" "$out/leader.txt" - leader
perl "$pl" double sh "$cli" "$out/double.txt" - double
perl "$pl" wrapper sh "$cli" "$out/wrapper.txt" - wrapper
sh "$cli" "$out/forged.txt" "$shell_pane" forged
echo done > "$out/done"
"#;

/// thread 범위를 host가 판정한다(plan §8.164, t-0340 감사): thread pane 안에서 `AAA_SESSION_ID`를 지우거나 다른 셸·nohup·
/// setsid·double fork로 떼어 내거나 사람 셸 pane의 세션을 대도 보기만 된다. 사람 셸 pane에서는 변수를 지워도 조작된다.
#[test]
fn thread_scope_survives_env_and_detach_bypass() {
    let w = world_with("[safety.demo]\nthread_profiles = [\"other\"]\n");
    let t = w.start(&["--title", "셸", "--profile", "other", "--kind", "dir", "--task-file", &w.task("s.txt", "x")]);
    let (id, pane) = (t["id"].as_str().unwrap().to_string(), t["pane"].as_str().unwrap().to_string());
    w.wait_show("brief", &id, |v| v["brief"] == "delivered");
    let shell = w.h.new_pane(None, "labA", Some("sh"), &["/bin/sh"], None);
    let dir = w.lab.root.join("bypass");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("detach.pl"), DETACH_PL).unwrap();
    std::fs::write(dir.join("cli.sh"), BYPASS_CLI).unwrap();
    std::fs::write(dir.join("bypass.sh"), BYPASS).unwrap();
    let out = w.lab.root.join("bypass-out");
    std::fs::create_dir_all(&out).unwrap();
    let line = format!(
        "sh {} {} {id} {} {shell}",
        lab::shell_word(path_str(&dir.join("bypass.sh"))),
        lab::shell_word(lab::BIN),
        lab::shell_word(path_str(&out))
    );
    w.h.ok(&["pane", "run", &pane, &line]);
    wait_for_diag(
        "우회 시도 끝",
        T,
        || out.join("done").exists().then_some(()),
        || format!("화면: {}", w.h.ok(&["pane", "read", &pane])),
    );
    let thread = ": thread 세션 안의 CLI는 보기만 한다";
    let detached = ": 터미널도 aaa pane도 없는 떼어 낸 프로세스라 보기만 한다";
    let mut tried = Vec::new();
    for (name, why) in [
        ("envu", thread),
        ("shell", thread),
        ("nohup", thread),
        ("setsid", thread),
        ("leader", thread),
        ("double", detached),
        ("wrapper", detached),
        ("forged", thread),
    ] {
        let got = std::fs::read_to_string(out.join(format!("{name}.txt"))).unwrap();
        tried.push(name);
        // Linux subreaper(systemd --user) 밑이면 init 대신 그 프로세스에 붙어 「leader가 init의 자식」을 못 본다(남는 한계)
        if name == "wrapper" && !got.starts_with("ppid=1\n") {
            eprintln!("wrapper: init이 아닌 프로세스에 붙었다(subreaper), 판정을 건너뛴다: {got}");
            continue;
        }
        assert!(
            got.contains(&format!("보기 권한으로는 조작할 수 없다{why}")) && got.ends_with("rc=1\n"),
            "{name}: {got}"
        );
    }
    let next = w.h.ok(&["thread", "next", "demo", &id])["lines"].to_string();
    assert!(tried.iter().all(|n| !next.contains(&format!("\"{n}\""))), "{next}");
    // 사람의 셸 pane(thread가 아니다)에서는 변수를 지워도 조작된다
    let ctl = out.join("control.txt");
    let line = format!(
        "BIN={} ID={id} sh {} {} - control",
        lab::shell_word(lab::BIN),
        lab::shell_word(path_str(&dir.join("cli.sh"))),
        lab::shell_word(path_str(&ctl))
    );
    w.h.ok(&["pane", "run", &shell, &line]);
    let got = wait_for("사람 셸 pane의 조작", T, || {
        std::fs::read_to_string(&ctl).ok().filter(|s| s.ends_with("rc=0\n") || s.ends_with("rc=1\n"))
    });
    assert!(got.ends_with("rc=0\n"), "{got}");
    assert!(w.h.ok(&["thread", "next", "demo", &id])["lines"].to_string().contains("\"control\""));
}

/// 동시 thread 한도는 host가 기동 전 집행한다. 거부된 요청은 thread와 pane을 만들지 않는다.
#[test]
fn parallel_threads_enforce_capacity_before_launch() {
    let w = report_shell_world();
    std::fs::write(w.projects.join("demo/project.toml"), "schema = 1\nmax_parallel_threads = 2\n").unwrap();
    let start = |n: &str| {
        w.h.json(&[
            "thread",
            "start",
            "demo",
            "--title",
            n,
            "--profile",
            "fake",
            "--kind",
            "dir",
            "--task-file",
            &w.task(&format!("{n}.txt"), "x"),
        ])
    };
    start("m1").unwrap();
    let v2 = start("m2").unwrap();
    assert!(v2.get("warnings").is_none(), "{v2}");
    let panes = w.h.ok(&["pane", "list"])["panes"].as_array().unwrap().len();
    // 한도에 닿으면 실제 기동을 보내지 않는다.
    let o = w.h.run(&[
        "thread",
        "start",
        "demo",
        "--title",
        "m3",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("m3.txt", "x"),
    ]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(!o.status.success(), "{out}");
    assert!(format!("{out}{}", String::from_utf8_lossy(&o.stderr)).contains("aaa task reserve"));
    assert_eq!(w.h.ok(&["thread", "list", "demo"])["threads"].as_array().unwrap().len(), 2);
    assert_eq!(w.h.ok(&["pane", "list"])["panes"].as_array().unwrap().len(), panes);
}

/// coordinator safety(herdr-projects `check_allowed`·`Safety::launch_args`의 coordinator 몫): 사람이 터미널에서
/// `profile allow coordinator`로 정한 목록에 없는 profile은 기본값이어도 거부하고, yolo면 coordinator를 harness의 권한 건너뛰기
/// 인자로 띄운다(host가 profile을 풀며 붙인다, `resolved_coordinator_profile`). safety가 없는 프로젝트는 그대로다.
#[test]
fn project_open_follows_coordinator_safety() {
    let w = world();
    // safety가 없으면 인자가 없다(host는 프로젝트 하나라 같은 프로젝트에서 safety를 쓰기 전에 본다, plan §8.285)
    let v = w.h.ok(&["coordinator", "open"]);
    assert!(!v["pane"]["argv"].to_string().contains("dangerously"), "{v}");
    let plain = v["pane"]["id"].as_str().unwrap().to_string();
    w.h.ok(&["pane", "kill", &plain]);
    wait_for("safety 전 coordinator 끝", T, || (w.h.pane(&plain)["state"] != "running").then_some(()));
    let host_toml = w.h.cfg.join("host.toml");
    let mut text = std::fs::read_to_string(&host_toml).unwrap();
    text.push_str("\n[safety.demo]\nyolo = true\n");
    std::fs::write(&host_toml, text).unwrap();
    let o = w.h.cmd(&["profile", "allow", "coordinator", "resumable", "--project", "demo"]).capture_output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("터미널"), "{o:?}");
    let out = w.h.in_terminal(&["profile", "allow", "coordinator", "resumable", "--project", "demo"], &[], T);
    assert!(out.contains("coordinator는 `demo`에서 resumable를 쓸 수 있다"), "{out}");
    let show = w.h.ok(&["safety", "show", "demo"]);
    let row = show["rows"].as_array().unwrap().iter().find(|r| r["key"] == "coordinator_profiles").unwrap().clone();
    assert_eq!((row["value"].as_str(), row["source"].as_str()), (Some("resumable"), Some("프로젝트")), "{show}");
    // 목록에 없으면 기본값(claude)이어도 아무것도 띄우지 않는다
    let e = w.h.json(&["project", "open", "demo"]).expect_err("허용되지 않은 profile");
    assert!(e.contains("coordinator에 허용되지 않았다") && e.contains("허용: resumable"), "{e}");
    let v = w.h.ok(&["project", "open", "demo", "--profile", "resumable"]);
    let argv = v["pane"]["argv"].to_string();
    assert!(argv.contains("--dangerously-skip-permissions"), "yolo는 coordinator에도 닿는다: {argv}");
}

/// 사람이 띄운 가짜 Claude(실행 파일 이름이 `claude`라 aaa가 화면으로 상태를 본다, hook settings 없음). 띄운 인자를 보이고
/// SessionStart hook으로 대화 ID를 알린 뒤, Claude처럼 입력 상자를 그려 Enter마다 받은 줄을 남긴다. brief 안내를 받으면
/// 그 pane 안에서 자기 thread에 Next 줄을 더해 보고(thread 범위 시험) 결과를 `probe.txt`에, 보고를 report.md에 쓴다.
/// 그 턴은 실제 Claude처럼 입력 상자 위에 「Working… esc to interrupt」를 다음 입력까지 그린다: hook이 없는 pane은 화면
/// 감지의 작업 중이 받았다는 사건이다(없으면 host가 `accept_wait_ms`를 다 기다린다). `@BIN@`은 aaa 실행 파일이다.
const ADOPT_AGENT: &str = r#"#!/bin/sh
stty icanon echo
R=$(printf '%.0s\342\224\200' 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30)
log="args: $*
"
"@BIN@" hook --agent claude '{"hook_event_name":"SessionStart","session_id":"5d1c2b3a-4e5f-4a6b-8c7d-9e0f1a2b3c4d","source":"startup"}'
buf=''; st=''
# 입력은 Enter 전부터 상자 안에 보여야 한다. 공통 Claude fixture의 prompt처럼
# 커서를 상자 안으로 돌려 tty echo가 실제 친 글을 그리게 한다.
draw() { printf '\033[2J\033[H%s%s%s\n\342\235\257 %s\n%s\n\033[2A\r\033[2C' "$log" "$st" "$R" "$buf" "$R"; }
draw
while IFS= read -r buf; do
    st=''
    case "$buf" in
      exit-one) exit 1 ;;
      *brief.md*)
        st=$(printf '\342\234\273 Working\342\200\246 (1s \302\267 esc to interrupt)\n_'); st=${st%_}
        draw
        dir=$(dirname "$(printf '%s' "$buf" | sed 's/를 읽고.*//')")
        id=$(basename "$dir"); id=${id#demo-}
        "@BIN@" thread next demo "$id" --add probe > "$dir/probe.tmp" 2>&1; echo "rc=$?" >> "$dir/probe.tmp"
        mv "$dir/probe.tmp" "$dir/probe.txt"
        printf '## Report\n데려온 pane이 보고했다\n\n## Next\n- 확인\n' > "$dir/report.md.tmp"
        mv "$dir/report.md.tmp" "$dir/report.md" ;;
    esac
    log="$log$(printf 'got: %s' "$buf")
"
    buf=''
  draw
done
"#;

/// 일반 기동은 할 일을 먼저 남기고 pane 준비 전에 연결한다. 실패한 준비도 재기동할 할 일로 남으며,
/// 끝난 일은 기존 완료 보관으로 간다. 본문의 머리·체크 목록은 별도 task로 해석하지 않는다.
#[test]
fn manual_threads_always_have_tasks_and_archive_the_result() {
    let w = report_shell_world();
    let body = "# 작업 범위\n- [ ] 중첩 검사\n원문을 보존한다\n";
    let task = w.task("manual.md", body);
    let start = |title: &str, extra: &[&str]| {
        let mut args = vec!["--title", title, "--profile", "fake", "--task-file", &task];
        args.extend_from_slice(extra);
        w.start(&args)
    };
    let first = start("즉시 맡긴 일", &["--kind", "dir", "--background"]);
    let id = first["id"].as_str().unwrap();
    let board = w.h.ok(&["task", "list"]);
    assert_eq!(board["tasks"].as_array().unwrap().len(), 1, "{board}");
    let row = &board["tasks"][0];
    assert_eq!(row["thread"], id, "pane 준비 응답 전 자동 연결");
    assert_eq!(row["profile"], "fake");
    assert_eq!(row["machine"], "labA");
    assert_eq!(row["repo"], "작업 폴더");
    let short_id = row["short_id"].as_str().unwrap().to_string();
    let stored = std::fs::read_to_string(w.projects.join(format!("demo/threads/{id}.task.md"))).unwrap();
    assert_eq!(stored, body);
    assert!(row["notes"].as_array().unwrap().iter().any(|n| n == "> # 작업 범위"), "{row}");
    assert!(row["notes"].as_array().unwrap().iter().any(|n| n == "> - [ ] 중첩 검사"), "{row}");
    w.wait_show("완료 보고", id, |v| v["report_hash"].is_string() && v["brief"] == "delivered");
    w.finish_fixture_followups(id);
    w.h.ok(&["thread", "resolve", "demo", id]);
    // resolve는 thread 종료까지다. 연결된 task의 완료 보관은 다음 ticker 판정 뒤에 읽는다.
    w.h.ticks(1);
    let archive = w.h.ok(&["task", "archive"]);
    let entry = archive["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["task"]["short_id"] == short_id)
        .unwrap_or_else(|| panic!("완료한 task가 보관되지 않았다: {archive}"));
    assert_eq!(entry["task_id"], row["id"]);
    assert_eq!(entry["task"]["thread"], id);
    assert_eq!(entry["reason"], "보고 [완료]");
    assert_eq!(entry["report"], format!("threads/{id}.md"));
    assert!(!w.h.ok(&["task", "list"])["tasks"].as_array().unwrap().iter().any(|t| t["short_id"] == short_id));
    // 뒤 tick에도 같은 완료 결과를 중복 보관하지 않는다.
    w.h.ticks(1);
    assert_eq!(w.h.ok(&["task", "archive"]), archive);
    // 기동의 입력 검사를 통과한 뒤 agent 준비가 실패하면 할 일을 잃지 않는다.
    let failed = w.h.json(&[
        "thread",
        "start",
        "demo",
        "--title",
        "준비 실패",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--base",
        "missing-ref",
        "--task-file",
        &task,
    ]);
    assert!(failed.is_err(), "없는 기준 ref로 성공했다");
    let board = w.h.ok(&["task", "list"]);
    let row = board["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "준비 실패").unwrap();
    let failed_id = row["thread"].as_str().unwrap();
    assert_eq!(w.show(failed_id)["status"], "failed");
    // 이미 등록된 할 일도 --link-task 없이 같은 ID에 연결한다.
    w.h.ok(&["task", "add", "기존 할 일", "--profile", "fake", "--machine", "labA", "--repo", "작업 폴더"]);
    let before = w.h.ok(&["task", "list"]);
    let original = before["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "기존 할 일").unwrap();
    let started = w.start(&["--from-task", original["short_id"].as_str().unwrap()]);
    let after = w.h.ok(&["task", "list"]);
    let now = after["tasks"].as_array().unwrap().iter().find(|t| t["id"] == original["id"]).unwrap();
    assert_eq!(now["thread"], started["id"]);
    assert_eq!(after["tasks"].as_array().unwrap().len(), before["tasks"].as_array().unwrap().len());
}

/// 할 일 보기(T1, plan §8.306): `tasks`(`aaa task list`)가 목록·형식 어긋남·task 없는 열린 thread를 주고, 보기 권한
/// 세션도 읽는다. 파일을 직접 고치면 다음 tick이 `tasks` 사건(새 sha)을 내고, 고치지 않은 tick은 내지 않는다. `--from-task`는 owner 칸이
/// 상태 메모인 task를 사람 owner로 오인하지 않고 정확한 까닭으로 거부한다.
#[test]
fn task_board_reads_tasks_and_follows_file_edits() {
    use sha2::Digest as _;
    let w = report_shell_world();
    let pdir = w.projects.join("demo");
    // ticker는 완성된 파일 판만 관측한다. 쓰는 도중 tick도 끝내고 rename으로 한 번에 공개한다.
    let write = |text: &str| {
        let pending = pdir.join("TASKS.md.pending");
        std::fs::write(&pending, text).unwrap();
        w.h.ticks(1);
        std::fs::rename(pending, pdir.join("TASKS.md")).unwrap();
    };
    let task = w.task("tb.txt", "x");
    let linked = w.start(&["--title", "이은 일", "--profile", "fake", "--kind", "dir", "--task-file", &task]);
    let other = w.start(&["--title", "따로 도는 일", "--profile", "fake", "--kind", "dir", "--task-file", &task]);
    let (lid, oid) = (linked["id"].as_str().unwrap(), other["id"].as_str().unwrap());
    let md = format!(
        "# Tasks\n\n## Backlog\n나중 일은 later.md에 모은다\n- [ ] 이은 일 (fake@labA) · {lid}\n  - 메모\n  - [ ] 하위 (me)\n\
         - [ ] 정리 (t-0101 S4 뒤, 사용자 지시 시)\n- [ ] 실패 조사 (미할당)\n## 빈 목록\n"
    );
    let md = identified_tasks(&md);
    // 먼저 원본을 기억시킨다. 연결 뒤 md의 첫 관측 사건이 오는 순서를 부하 없이도 검증한다.
    w.h.ok(&["task", "list"]);
    let mut ev = Events::connect(&w.h);
    write(&md);
    let sha = |t: &str| hex::encode(sha2::Sha256::digest(t.as_bytes()));
    w.h.ok(&["ticker", "tick"]);
    let (baseline, ()) = ev.wait_from(0, "초기 할 일 관측", T, |e| match e {
        FrontEvent::Tasks { project, sha256 } if project == "demo" && *sha256 == sha(&md) => Some(()),
        _ => None,
    });

    let b = w.h.ok(&["task", "list"]);
    assert_eq!(b["sha256"].as_str(), Some(sha(&md).as_str()), "{b}");
    assert_eq!(
        b["lists"],
        serde_json::json!([{"name": "Backlog", "notes": ["나중 일은 later.md에 모은다"]}, {"name": "빈 목록", "notes": []}])
    );
    let tasks = b["tasks"].as_array().unwrap();
    let row =
        |title: &str| tasks.iter().find(|t| t["title"] == title).unwrap_or_else(|| panic!("{title}: {b}")).clone();
    assert_eq!((row("이은 일")["line"].as_u64(), row("이은 일")["thread"].as_str()), (Some(5), Some(lid)));
    assert_eq!(row("하위")["parent"].as_u64(), Some(5));
    let tidy = row("정리 (t-0101 S4 뒤, 사용자 지시 시)");
    assert!(
        tidy["owner"].is_null() && tidy["problems"][0].as_str().unwrap().starts_with("owner 칸이 owner 꼴이 아님"),
        "{tidy}"
    );
    assert_eq!(row("실패 조사")["owner"], Value::Null);
    assert_eq!(b["unlinked_threads"].as_array().unwrap().iter().map(|t| t["id"].clone()).collect::<Vec<_>>(), [oid]);
    // context도 같은 파서다: 형식 어긋남을 보이고 사람 owner로 쓰지 않는다
    let text = String::from_utf8_lossy(&w.h.run(&["context", "demo", "--peek"]).stdout).to_string();
    assert!(
        text.contains("형식 어긋남: owner 칸이 owner 꼴이 아님") && !text.contains("(t-0101 S4 뒤, 사용자 지시 시) ·"),
        "{text}"
    );
    // --from-task: 상태 메모 owner는 정확한 까닭으로 거부한다(예전에는 「사람(…)이 맡은 task」)
    let e =
        w.h.json(&["thread", "start", "demo", "--from-task", "정리 (t-0101 S4 뒤, 사용자 지시 시)", "--kind", "dir"])
            .expect_err("형식 어긋남");
    assert!(e.contains("owner 칸 형식 어긋남, 고친 뒤 시작"), "{e}");

    // 보기 권한(worker 세션 안 CLI)도 읽는다
    let pane = other["pane"].as_str().unwrap();
    let o = w.h.cmd(&["--json", "task", "list"]).env("AAA_SESSION_ID", pane).capture_output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o =
        w.h.cmd(&["thread", "next", "demo", oid, "--add", "x"]).env("AAA_SESSION_ID", pane).capture_output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("보기 권한"), "그 세션은 보기 권한이다");

    // 고치지 않은 tick은 사건을 내지 않는다: 다음에 고친 뒤 오는 첫 `tasks` 사건이 고친 sha다
    w.h.ok(&["ticker", "tick"]);
    let md2 = identified_tasks(&md.replace("## 빈 목록\n", "## 빈 목록\n- [ ] 새로 더한 일\n"));
    write(&md2);
    w.h.ok(&["ticker", "tick"]);
    let (_, got) = ev.wait_from(baseline + 1, "tasks 사건", T, |e| match e {
        aaa_proto::FrontEvent::Tasks { project, sha256 } if project == "demo" => Some(sha256.clone()),
        _ => None,
    });
    assert_eq!(got, sha(&md2));
    assert_eq!(w.h.ok(&["task", "list"])["sha256"].as_str(), Some(sha(&md2).as_str()));
}

/// 할 일 편집(T2, plan §8.311): `aaa task add`는 그 블록만 붙이고 나머지 바이트를 그대로 두며 기록·`tasks` 사건을 남긴다. 읽은 뒤
/// 파일을 직접 고치면 같은 블록 저장은 `revision_conflict`(쓰지 않음), 다른 블록 저장은 성공이고, 지운 task는 고쳐도 되살아나지
/// 않는다. 쓰기에 묻힐 뻔한 직접 편집과 tick이 본 직접 편집은 「파일 직접」으로 적힌다. 되살리기는 한 번만. thread(보기 권한)·
/// 확인되지 않은 세션 pane은 거부하고 그 프로젝트 coordinator pane은 허용한다.
#[test]
fn task_edit_keeps_bytes_conflicts_and_logs() {
    use aaa_proto::FrontEvent;
    use sha2::Digest as _;
    let w = report_shell_world();
    let pdir = w.projects.join("demo");
    let file = pdir.join("TASKS.md");
    // 직접 편집 fixture도 완성된 판을 rename으로 넘긴다. std::fs::write는 truncate와 write 사이에
    // ticker가 빈 파일을 읽어 모든 task를 삭제/추가로 기록할 수 있다. 임시 파일을 쓰는 동안 tick을
    // 끝내 그 순서를 매번 검사한다. host의 task 편집 API는 거치지 않는다.
    let write = |text: &str| {
        let pending = pdir.join("TASKS.md.pending");
        std::fs::write(&pending, text).unwrap();
        w.h.ticks(1);
        std::fs::rename(pending, &file).unwrap();
    };
    let md = "# Tasks\n\n## Backlog\n나중 일은 later.md에 모은다\n- [ ] 고칠 일 (me)\n  메모\n- [ ] 다른 일\n<!-- 모르는 줄 -->\n";
    let md = identified_tasks(md);
    write(&md);
    let sha = |t: &str| hex::encode(sha2::Sha256::digest(t.as_bytes()));
    let read = || std::fs::read_to_string(&file).unwrap();
    let mut ev = Events::connect(&w.h);
    // 실험실 프로젝트의 빈 TASKS.md를 이미 본 ticker는 직접 쓴 두 task를 「파일 직접」 더하기로 적는다
    w.h.ok(&["ticker", "tick"]);
    let count = || w.h.ok(&["task", "log"])["entries"].as_array().unwrap().len();
    let seeded = count();

    // 더하기: 그 블록만 붙는다
    let v = w.h.ok(&["task", "add", "새 일", "--note", "새 메모"]);
    let new_id =
        v["board"]["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "새 일").unwrap()["id"].as_str().unwrap();
    let new_line = format!("- [ ] 새 일 <!-- aaa-task:{new_id} -->");
    let added = format!("{md}{new_line}\n  새 메모\n");
    assert_eq!(read(), added);
    assert_eq!((v["kind"].as_str(), v["log_ids"].as_array().map(Vec::len)), (Some("task_edited"), Some(1)), "{v}");
    let (at, _) = ev.wait_from(0, "더한 뒤 tasks 사건", T, |e| match e {
        FrontEvent::Tasks { sha256, .. } if *sha256 == sha(&added) => Some(()),
        _ => None,
    });
    // 자기 쓰기는 다음 tick이 「파일 직접」으로 적지 않는다
    w.h.ok(&["ticker", "tick"]);
    let log = w.h.ok(&["task", "log"]);
    let entries = log["entries"].as_array().unwrap();
    assert_eq!(entries.len(), seeded + 1, "{log}");
    assert_eq!(
        (entries[0]["action"].as_str(), entries[0]["by"].as_str(), entries[0]["title"].as_str()),
        (Some("add"), Some("터미널"), Some("새 일"))
    );
    assert_eq!(entries[0]["after"].as_str(), Some(format!("{new_line}\n  새 메모").as_str()));

    // 읽기 → 파일 직접 편집 → 같은 블록 저장은 충돌, 다른 블록 저장은 성공
    let board = w.h.ok(&["task", "list"]);
    let block = |title: &str| -> String {
        let t = board["tasks"].as_array().unwrap().iter().find(|t| t["title"] == title).unwrap();
        t["block_sha256"].as_str().unwrap().to_string()
    };
    let (fix, other) = (block("고칠 일"), block("다른 일"));
    let direct = added.replace("  메모\n", "  메모 고침(직접)\n");
    write(&direct);
    // 충돌: 종료 코드 1, 까닭과 그 task의 지금 원문, 쓰지 않음
    let conflict = |args: &[&str]| -> Value {
        let o = w.h.run(&[&["--json", "task"], args].concat());
        assert_eq!(o.status.code(), Some(1), "{o:?}");
        let v: Value = serde_json::from_slice(&o.stderr).unwrap_or_else(|_| panic!("{o:?}"));
        v["conflict"].clone()
    };
    let c = conflict(&["edit", "고칠 일", "--title", "고친 제목", "--block", &fix]);
    assert_eq!(c["latest"]["raw"].as_str(), Some(identified_tasks("- [ ] 고칠 일 (me)").as_str()), "{c}");
    assert_eq!(c["latest"]["notes"], serde_json::json!(["메모 고침(직접)"]), "{c}");
    assert_eq!(read(), direct, "충돌이면 쓰지 않는다");
    let v = w.h.ok(&["task", "rm", "다른 일", "--block", &other]);
    assert!(v["conflict"].is_null() && v["log_ids"].as_array().unwrap().len() == 1, "{v}");
    let deleted = direct.replace("- [ ] 다른 일 <!-- aaa-task:00000000000000000000000002 -->\n", "");
    assert_eq!(read(), deleted);
    // 지운 task를 고치려 하면 충돌이다(되살아나지 않는다)
    let c = conflict(&["edit", "다른 일", "--title", "다른 제목", "--block", &other]);
    assert!(c["latest"].is_null(), "{c}");
    assert_eq!(read(), deleted);
    // 쓰기 전에 있던 직접 편집은 쓰기에 묻히지 않고 「파일 직접」으로 먼저 적혔다
    let log = w.h.ok(&["task", "log"]);
    let rows: Vec<(String, String, String)> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["action"].as_str().unwrap().into(),
                e["by"].as_str().unwrap().into(),
                e["title"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        rows[..3],
        [
            ("delete".into(), "터미널".into(), "다른 일".into()),
            ("update".into(), "파일 직접".into(), "고칠 일".into()),
            ("add".into(), "터미널".into(), "새 일".into())
        ],
        "{log}"
    );
    let del_id = log["entries"][0]["id"].as_i64().unwrap().to_string();

    // 되살리기는 원래 목록 끝에, 한 번만
    w.h.ok(&["task", "restore", &del_id]);
    assert_eq!(read(), format!("{deleted}- [ ] 다른 일 <!-- aaa-task:00000000000000000000000002 -->\n"));
    let e = w.h.json(&["task", "restore", &del_id]).expect_err("두 번");
    assert!(e.contains("이미 되살렸다"), "{e}");

    // 저장 검사: `)` 끝 제목, 중복 제목, 사람 owner는 「사람으로 저장」
    let e = w.h.json(&["task", "add", "상태 (S4 뒤)"]).expect_err("`)` 끝");
    assert!(e.contains("`)`로 끝나면"), "{e}");
    let e = w.h.json(&["task", "add", "다른 일"]).expect_err("중복");
    assert!(e.contains("같은 제목"), "{e}");
    let e = w.h.json(&["task", "edit", "새 일", "--owner", "Priya"]).expect_err("사람 owner");
    assert!(e.contains("사람으로 저장"), "{e}");
    w.h.ok(&["task", "edit", "새 일", "--owner", "Priya", "--person"]);
    assert!(read().contains(&format!("- [ ] 새 일 (Priya) <!-- aaa-task:{new_id} -->\n  새 메모\n")), "{}", read());

    // 파일 직접 편집 → 다음 tick이 사건과 「파일 직접」 기록
    let logged = count();
    let before_tick = read().replace("- [ ] 새 일 (Priya)", "- [ ] 새 일 (me)");
    write(&before_tick);
    w.h.ok(&["ticker", "tick"]);
    ev.wait_from(at + 1, "직접 편집 뒤 tasks 사건", T, |e| match e {
        FrontEvent::Tasks { sha256, .. } if *sha256 == sha(&before_tick) => Some(()),
        _ => None,
    });
    let first = w.h.ok(&["task", "log"])["entries"][0].clone();
    assert_eq!((first["by"].as_str(), first["action"].as_str()), (Some("파일 직접"), Some("update")), "{first}");
    assert_eq!(first["title"].as_str(), Some("새 일"), "{first}");
    assert_eq!(count(), logged + 1, "직접 편집한 task 하나만 기록한다");
    assert_eq!(
        first["before"].as_str(),
        Some(format!("- [ ] 새 일 (Priya) <!-- aaa-task:{new_id} -->\n  새 메모").as_str())
    );

    // 권한: thread 세션(보기 권한)·확인되지 않은 세션 pane은 거부, 그 프로젝트 coordinator pane은 허용
    let task = w.task("te.txt", "x");
    let worker = w.start(&["--title", "편집 시험 thread", "--profile", "fake", "--kind", "dir", "--task-file", &task]);
    let o =
        w.h.cmd(&["task", "add", "thread가 더함"])
            .env("AAA_SESSION_ID", worker["pane"].as_str().unwrap())
            .capture_output()
            .unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("보기 권한"), "{o:?}");
    let shell = w.h.new_pane(None, "labA", None, &["/bin/sh"], None);
    let o = w.h.cmd(&["task", "add", "셸이 더함"]).env("AAA_SESSION_ID", &shell).capture_output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("그 프로젝트 coordinator만"), "{o:?}");
    assert!(!read().contains("thread가 더함") && !read().contains("셸이 더함"));
    // pane 안에서 `aaa task add`를 돌린다(그 pane의 확인된 세션으로): 끝 코드와 stderr
    let probe = |pane: &str, run: &str, name: &str, title: &str| {
        let prefix = w.lab.root.join(name);
        let script = w.task(
            &format!("{name}.sh"),
            &format!(
                "'{}' --json task add '{title}' > '{p}.out' 2> '{p}.err'\nprintf '%s' \"$?\" > '{p}.rc'\nprintf '{name}-finished\\n'\n",
                lab::BIN,
                p = prefix.display()
            ),
        );
        w.h.ok(&["pane", "run", pane, &format!("{run} {script}")]);
        w.h.wait_output(pane, &format!("{name}-finished"), T);
        let read = |ext: &str| std::fs::read_to_string(prefix.with_extension(ext)).unwrap();
        (read("rc"), read("err"))
    };
    // 확인된 세션 pane이라도 coordinator가 아니면(사람 셸) 거부한다
    let (rc, err) = probe(&shell, "sh", "shell-probe", "확인된 셸이 더함");
    assert!(rc != "0" && err.contains("그 프로젝트 coordinator만"), "{rc} {err}");
    assert!(!read().contains("확인된 셸이 더함"));
    let coordinator = w.h.ok(&["project", "open", "demo", "--profile", "fake"]);
    let pane = coordinator["pane"]["id"].as_str().unwrap().to_string();
    w.wait_screen(&pane, "fake>");
    let (rc, err) = probe(&pane, "post-probe", "task-probe", "coordinator가 더함");
    assert_eq!(rc, "0", "{err}");
    let board = w.h.ok(&["task", "list"]);
    let added_id =
        board["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "coordinator가 더함").unwrap()["id"]
            .as_str()
            .unwrap();
    assert!(read().contains(&format!("- [ ] coordinator가 더함 <!-- aaa-task:{added_id} -->\n")), "{}", read());
    let first = w.h.ok(&["task", "log"])["entries"][0].clone();
    assert_eq!(
        (first["by"].as_str(), first["title"].as_str()),
        (Some("coordinator"), Some("coordinator가 더함")),
        "{first}"
    );
}

/// 할 일 thread 연결과 자동 정리(T3, plan §8.316, t-0469 §4 P2): `thread start --from-task --link-task`는 그 task 줄 끝에
/// thread 번호를 붙인다. 이어진 thread가 끝나면 다음 tick이 한 번 판정한다: 보고 `[완료]`면 줄을 지우고(기록 「host 자동
/// 정리」·inbox `task auto-cleaned`), 보고가 없으면 줄을 남겨 확인 필요로 보인다(`task list`·`context`·inbox `task needs
/// check`). 하위가 모두 끝난 상위는 지우지 않고 그 thread를 이어 확인 필요로 올린다. 판정은 한 번이다: 되살린 task를 다음
/// tick이 다시 지우지 않는다.
#[test]
fn task_thread_link_and_auto_clean() {
    let w = report_shell_world();
    let file = w.projects.join("demo/TASKS.md");
    let read = || std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        identified_tasks("# Tasks\n\n## Backlog\n- [ ] 끝낼 일\n- [ ] noreport 일\n- [ ] 결정 일\n- [ ] 상위 일\n  - [ ] 하위 일\n- [ ] 남는 일\n"),
    )
    .unwrap();
    let start = |title: &str| -> String {
        let v = w.start(&["--from-task", title, "--link-task", "--profile", "fake", "--kind", "dir"]);
        assert!(v["warnings"].as_array().is_none_or(Vec::is_empty), "{v}");
        v["id"].as_str().unwrap().to_string()
    };
    let e = w.h.json(&["thread", "start", "demo", "--title", "x", "--task-file", &w.task("x.md", "x"), "--link-task"]);
    assert!(e.expect_err("--from-task 없이").contains("--link-task는 --from-task와 함께"));
    let (done, quiet, decide, child) = (start("끝낼 일"), start("noreport 일"), start("결정 일"), start("하위 일"));
    assert_eq!(
        read(),
        identified_tasks(&format!(
            "# Tasks\n\n## Backlog\n- [ ] 끝낼 일 · {done}\n- [ ] noreport 일 · {quiet}\n- [ ] 결정 일 · {decide}\n- [ ] 상위 일\n\
             \x20 - [ ] 하위 일 · {child}\n- [ ] 남는 일\n"
        ))
    );
    // 열린 thread와 이어진 task는 아직 판정하지 않는다
    w.h.ok(&["ticker", "tick"]);
    assert!(read().contains(&format!("끝낼 일 · {done}")));
    for id in [&done, &decide, &child] {
        w.wait_show("보고", id, |v| v["report_hash"].is_string() && v["brief"] == "delivered");
    }
    w.wait_show("brief 전달", &quiet, |v| v["brief"] == "delivered");
    // 공통 fake 보고의 결과 확인·다음 단계는 이 owner에서 확인했다. 후속 처리 없이 자동 보관하지 않는다.
    for id in [&done, &decide, &child] {
        w.finish_fixture_followups(id);
    }
    // 결정 일은 마지막 보고를 `[결정 필요]`로 바꾼다: host가 새 보고를 받은 뒤 끝낸다
    let first = w.show(&decide);
    let work = PathBuf::from(first["work_dir"].as_str().unwrap());
    std::fs::write(work.join("report.md"), "[결정 필요] 둘 중 고른다\n\n## Report\n고를 것\n\n## Next\n- 고르기\n")
        .unwrap();
    w.wait_show("결정 보고", &decide, |v| v["report_hash"] != first["report_hash"]);
    for id in [&done, &quiet, &decide, &child] {
        w.h.ok(&["thread", "resolve", "demo", id]);
    }
    w.h.ok(&["ticker", "tick"]);
    let cleaned = format!(
        "# Tasks\n\n## Backlog\n- [ ] noreport 일 · {quiet}\n- [ ] 결정 일 · {decide}\n- [ ] 상위 일 · {child}\n- [ ] 남는 일\n"
    );
    let without_ids = |text: String| {
        text.lines().map(|l| l.split(" <!-- aaa-task:").next().unwrap()).collect::<Vec<_>>().join("\n") + "\n"
    };
    assert_eq!(without_ids(read()), cleaned);

    // 기록: 지운 둘은 「host 자동 정리」
    let log = w.h.ok(&["task", "log"]);
    let autos: Vec<&Value> = log["entries"].as_array().unwrap().iter().filter(|e| e["action"] == "completed").collect();
    let mut titles: Vec<&str> = autos.iter().map(|e| e["title"].as_str().unwrap()).collect();
    titles.sort_unstable();
    assert_eq!(titles, ["끝낼 일", "하위 일"], "{log}");
    assert!(autos.iter().all(|e| e["by"] == "host 자동 정리"), "{log}");
    // 확인 필요: 까닭이 보인다
    let board = w.h.ok(&["task", "list"]);
    let attention = |b: &Value, title: &str| -> String {
        let t = b["tasks"].as_array().unwrap().iter().find(|t| t["title"] == title).unwrap_or_else(|| panic!("{b}"));
        t["attention"].as_str().unwrap_or_default().to_string()
    };
    assert_eq!(attention(&board, "noreport 일"), format!("{quiet} 끝남: 보고가 없다"));
    assert_eq!(attention(&board, "결정 일"), format!("{decide} 끝남: 보고 표지 [결정 필요]"));
    assert_eq!(attention(&board, "상위 일"), format!("하위 task가 모두 끝났다(마지막 {child})"));
    assert_eq!(attention(&board, "남는 일"), "");
    let text = String::from_utf8_lossy(&w.h.run(&["task", "list"]).stdout).to_string();
    assert!(text.contains(&format!("확인 필요: {quiet} 끝남: 보고가 없다")), "{text}");
    let text = String::from_utf8_lossy(&w.h.run(&["context", "demo", "--peek"]).stdout).to_string();
    assert!(text.contains(&format!("확인 필요: {quiet} 끝남: 보고가 없다")), "{text}");
    // coordinator inbox
    let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
    let events = |thread: &str| -> Vec<String> {
        let mut v: Vec<String> = items
            .iter()
            .filter(|i| i["item_kind"] == "task" && i["subject"] == thread)
            .map(|i| i["event"].as_str().unwrap().to_string())
            .collect();
        v.sort();
        v
    };
    assert_eq!(events(&done), ["task auto-cleaned"], "{items:?}");
    assert_eq!(events(&quiet), ["task needs check"], "{items:?}");
    assert_eq!(events(&decide), ["task needs check"], "{items:?}");
    assert_eq!(events(&child), ["task auto-cleaned", "task needs check"], "{items:?}");
    let restore =
        items.iter().find(|i| i["subject"] == done.as_str() && i["event"] == "task auto-cleaned").unwrap()["summary"]
            .as_str()
            .unwrap()
            .to_string();
    let archive = w.h.ok(&["task", "archive"]);
    let stored = archive["entries"].as_array().unwrap();
    assert_eq!(stored.len(), 2, "{archive}");
    let completed = stored.iter().find(|a| a["task"]["title"] == "끝낼 일").unwrap();
    let id = completed["task_id"].as_str().unwrap();
    assert_eq!(completed["task"]["thread"], done);
    assert_eq!(completed["reason"], "보고 [완료]");
    assert_eq!(completed["report"], format!("threads/{done}.md"));
    assert!(completed["body"].as_str().unwrap().contains("끝낼 일"));
    assert!(restore.contains(&format!("`aaa task archive --restore {id}`")), "{restore}");
    assert!(w.h.json(&["task", "restore", &autos[0]["id"].to_string()]).is_err(), "완료 보관과 삭제 복원은 다르다");
    w.h.ok(&["task", "archive", "--restore", id]);
    w.h.ok(&["ticker", "tick"]);
    assert_eq!(without_ids(read()), format!("{cleaned}- [ ] 끝낼 일\n"));
    let board = w.h.ok(&["task", "list"]);
    let revived = board["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "끝낼 일").unwrap();
    assert_eq!(revived["id"], id);
    assert!(revived["thread"].is_null());
    assert_eq!(attention(&board, "끝낼 일"), "");
    assert!(w.h.ok(&["task", "queue"])["entries"].as_array().unwrap().is_empty());
    let n = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().len();
    assert_eq!(n, items.len(), "다시 판정하지 않는다");
}

#[test]
fn pane_run_with_profile_starts_harness_with_hooks() {
    use aaa_common::front::FrontClient;
    use aaa_proto::{FrontRequest, FrontResponse};
    let w = world();
    let run = |profile: Option<&str>, cwd: Option<String>| FrontRequest::PaneRun {
        machine: Some("labA".into()),
        name: None,
        argv: vec![],
        cwd,
        cols: None,
        rows: None,
        profile: profile.map(String::from),
    };
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    rt.block_on(async {
        let mut c = FrontClient::connect_as(&w.h.state, "web-new").await.unwrap();
        let p = match c.call(run(Some("sticky"), Some(path_str(&w.repo).to_string()))).await.unwrap() {
            Ok(FrontResponse::Pane(p)) => p,
            other => panic!("profile로 띄우기: {other:?}"),
        };
        assert_eq!(p.name, "sticky", "이름을 비우면 profile 이름");
        assert_eq!(p.argv[0], path_str(&w.scripts.join("sticky.sh")));
        let settings =
            p.argv.iter().position(|a| a == "--settings").map(|i| p.argv[i + 1].clone()).expect("claude hook 설정");
        assert!(settings.ends_with(&format!("/sessions/{}/claude-settings.json", p.id)), "{settings}");
        assert!(Path::new(&settings).is_file());
        assert!(p.argv.iter().any(|a| a == "--session-id"), "{:?}", p.argv);
        // thread처럼 profile harness의 화면으로 상태를 본다(sticky.sh는 이름으로 harness를 알 수 없다)
        let spec: Value = serde_json::from_str(
            &std::fs::read_to_string(Path::new(&settings).with_file_name("spec.json")).expect("세션 spec.json"),
        )
        .unwrap();
        assert_eq!(spec["spec"]["harness"], "claude", "{spec}");
        let e = c.call(run(Some("nope"), None)).await.unwrap().expect_err("없는 profile");
        assert!(e.message.contains("profile `nope`"), "{}", e.message);
        let e = c.call(run(None, Some("~/aaa-no-such-dir".into()))).await.unwrap().expect_err("없는 폴더");
        let home = std::env::var("HOME").unwrap();
        assert!(e.message.contains(&format!("{home}/aaa-no-such-dir")), "~를 집으로 푼다: {}", e.message);
    });
}

/// N-1 agent는 계속 연결된다. 중앙 정의를 받을 기능이 없으면 시작만 이유와 함께 거부한다.
#[tokio::test(flavor = "multi_thread")]
async fn legacy_agent_stays_connected_but_cannot_drop_host_profile() {
    use aaa_common::conn::{FrameRx, spawn_writer};
    use aaa_proto::{AgentHello, LinkMsg, MAX_FRAME, Version};
    let legacy_link = Version { major: aaa_proto::AGENT_LINK.major, minor: aaa_proto::AGENT_LINK.minor - 1 };
    // 동기 실험실은 Front 상태 감시용 runtime도 돌린다. async 시험의 runtime과 중첩하지 않는다.
    let w = tokio::task::spawn_blocking(world).await.unwrap();
    let stream = aaa_common::ipc::connect(&w.h.state.join("host/agent.sock")).await.unwrap();
    let (r, writer) = tokio::io::split(stream);
    let tx = spawn_writer(writer, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    tx.json(&LinkMsg::Hello(AgentHello {
        proto: legacy_link,
        machine_id: "legacy-agent".into(),
        machine_name: "legacy".into(),
        boot_id: "legacy-boot".into(),
        os: "macos".into(),
        arch: "aarch64".into(),
        software: "legacy-test".into(),
        features: vec!["terminal.v1".into()],
        dev_mode: false,
        sha256: None,
        selfcheck: None,
        sessions: vec![],
        identity: None,
        harness: vec![],
    }))
    .await
    .unwrap();
    let welcome: LinkMsg = tokio::time::timeout(T, rx.control()).await.unwrap().unwrap();
    assert!(matches!(welcome, LinkMsg::Welcome(ref w) if w.proto == legacy_link), "{welcome:?}");
    let e =
        w.h.json(&[
            "thread",
            "start",
            "demo",
            "--machine",
            "legacy",
            "--title",
            "legacy",
            "--profile",
            "fake",
            "--kind",
            "dir",
            "--task-file",
            &w.task("legacy.txt", "x"),
        ])
        .expect_err("기능 없는 agent");
    assert!(e.contains("host_profiles.v1 없음") && e.contains("agent를 갱신"), "{e}");
    // 거부가 연결을 끊지 않았다는 관찰 가능한 응답이다.
    tx.json(&LinkMsg::Ping { seq: 42 }).await.unwrap();
    let pong: LinkMsg = tokio::time::timeout(T, rx.control()).await.unwrap().unwrap();
    assert!(matches!(pong, LinkMsg::Pong { seq: 42 }), "{pong:?}");
    // 지금 FRONT_API의 N-1 front도 계속 붙는다(1.31일 때 1.30으로 적었던 것을 판이 올라도 N-1로 본다). profile 때문에 minor를
    // 더 올리지 않는다.
    let legacy = Version { major: aaa_proto::FRONT_API.major, minor: aaa_proto::FRONT_API.minor - 1 };
    use aaa_proto::{FrontMsg, FrontRequest, FrontResponse, Outcome};
    let stream = aaa_common::ipc::connect(&w.h.state.join("host/front.sock")).await.unwrap();
    let (r, writer) = tokio::io::split(stream);
    let tx = spawn_writer(writer, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    tx.json(&FrontMsg::Hello { proto: legacy, client: "legacy-front".into(), session: None }).await.unwrap();
    let welcome: FrontMsg = tokio::time::timeout(T, rx.control()).await.unwrap().unwrap();
    assert!(matches!(welcome, FrontMsg::Welcome { proto, .. } if proto == legacy), "{welcome:?}");
    tx.json(&FrontMsg::Req { id: "status".into(), body: FrontRequest::Status }).await.unwrap();
    tokio::time::timeout(T, async {
        loop {
            match rx.control::<FrontMsg>().await.unwrap() {
                FrontMsg::Res { id, result: Outcome::Ok(FrontResponse::Status(_)) } if id == "status" => break,
                FrontMsg::Event(_) => {}
                other => panic!("옛 front 상태 응답: {other:?}"),
            }
        }
    })
    .await
    .unwrap();
}

/// 흐름 사건은 seq 차례로 나간다(10-18 ci-local `merged_pr_resolves_the_thread`: seq 21 `thread_resolved`가 22 뒤에 나가
/// `events --follow`가 21을 버렸다). host가 한 사건을 저장한 뒤 알리기 전에 붙잡혀 있는 동안(`AAA_TEST_FLOW_HOLD`) 같은
/// 프로젝트에 다른 사건이 들어온다. 그 사건은 먼저 저장된 사건보다 먼저 나가지 않는다.
#[test]
fn flow_events_go_out_in_seq_order() {
    let mut lab = Lab::new();
    let projects = lab.root.join("projects");
    let h = lab.node("h", Some(&host_toml(&projects, "", false)));
    let control = lab.root.join("flow.sock");
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let listener = rt.block_on(async { tokio::net::UnixListener::bind(&control).unwrap() });
    lab.spawn_env("host", &h, &["host"], &[("AAA_TEST_FLOW_HOLD", path_str(&control))]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    h.ok(&["project", "create", "demo"]);
    let routines = projects.join("demo/routines");
    std::fs::create_dir_all(&routines).unwrap();
    for name in ["hold", "free"] {
        std::fs::write(
            routines.join(format!("{name}.md")),
            "+++\n# 차례 시험\nschedule = \"every 1h\"\nenabled = false\n+++\n본문\n",
        )
        .unwrap();
    }
    rt.block_on(async {
        use aaa_common::front::FrontClient;
        use aaa_proto::{FrontMsg, FrontRequest, FrontResponse};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::time::timeout;

        async fn signal(listener: &tokio::net::UnixListener) -> (tokio::net::UnixStream, String) {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.unwrap() > 0, "flow 제어 연결이 알림 없이 닫혔다");
            (reader.into_inner(), line)
        }
        async fn response(client: &mut FrontClient, id: &str) -> FrontResponse {
            loop {
                match client.next().await.expect("flow 요청 연결").expect("flow 응답 전 연결 종료") {
                    FrontMsg::Res { id: got, result } if got == id => return result.into_result().unwrap(),
                    _ => {}
                }
            }
        }
        let toggle =
            |name: &str| FrontRequest::RoutineToggle { project: "demo".into(), name: name.into(), on: Some(true) };
        // 요청·조회·구독 연결을 먼저 열어 둔다. 기다리는 동안 CLI를 반복 spawn하지 않는다.
        let mut events = FrontClient::connect(&h.state).await.unwrap();
        let mut query = FrontClient::connect(&h.state).await.unwrap();
        let mut held = FrontClient::connect(&h.state).await.unwrap();
        let mut free = FrontClient::connect(&h.state).await.unwrap();
        let held_id = held.send(toggle("hold")).await.unwrap();
        let (mut gate, line) = tokio::select! {
            signal = timeout(T, signal(&listener)) => signal.expect("저장 후 hold 알림이 오지 않았다"),
            result = response(&mut held, &held_id) => panic!("hold 없이 요청이 먼저 끝났다: {result:?}"),
        };
        assert_eq!(line, "Hhold: 켬\n", "저장 후 실제로 붙잡힌 사건");
        let stored = timeout(
            T,
            query.call(FrontRequest::Events { project: Some("demo".into()), after: 0, limit: None, tail: false }),
        )
        .await
        .expect("붙잡힌 사건 조회가 끝나지 않았다")
        .unwrap()
        .unwrap();
        let FrontResponse::Events { events: stored } = stored else { panic!("사건 조회: {stored:?}") };
        assert!(stored.iter().any(|e| e.detail == "hold: 켬"), "hold 알림 전에 사건이 저장돼야 한다: {stored:?}");

        let free_id = free.send(toggle("free")).await.unwrap();
        // 정상 경로는 잠금 대기를 알린다. 잠금 결함이면 free가 먼저 끝나므로 실제 발행 순서로 잡는다.
        let free_finished = tokio::select! {
            signal = timeout(T, signal(&listener)) => {
                let (_, line) = signal.expect("두 번째 사건의 잠금 대기 알림이 오지 않았다");
                assert_eq!(line, "Wfree: 켬\n", "기다리는 사건의 출처");
                false
            }
            _ = response(&mut free, &free_id) => true,
        };
        gate.write_all(b"R").await.unwrap();
        timeout(T, response(&mut held, &held_id)).await.expect("hold 해제 뒤 응답이 오지 않았다");
        if !free_finished {
            timeout(T, response(&mut free, &free_id)).await.expect("다음 사건 응답이 오지 않았다");
        }
        let mut got = Vec::new();
        timeout(T, async {
            while got.len() < 2 {
                if let FrontMsg::Event(FrontEvent::Flow(f)) = events.next().await.unwrap().unwrap()
                    && f.kind == "routine_toggled"
                    && !got.iter().any(|(seq, _)| *seq == f.seq)
                {
                    got.push((f.seq, f.detail));
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("routine_toggled 둘이 오지 않았다: {got:?}"));
        let held = got.iter().find(|(_, d)| d == "hold: 켬").unwrap().0;
        let free = got.iter().find(|(_, d)| d == "free: 켬").unwrap().0;
        assert_eq!(free, held + 1, "{got:?}");
        assert_eq!(
            got.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
            [held, free],
            "나간 차례가 seq 차례가 아니다: {got:?}"
        );
    });
}

#[test]
fn post_sender_is_judged_not_claimed() {
    let w = report_shell_world();
    let t = w.start(&[
        "--title",
        "출처 시험",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("post-source.txt", "noreport"),
    ]);
    let id = t["id"].as_str().unwrap();
    let pane = t["pane"].as_str().unwrap();
    w.wait_show("brief 받음", id, |t| t["brief"] == "delivered");
    let other = w.h.new_pane(None, "labA", None, &["/bin/sh"], None);
    let spec: Value = serde_json::from_slice(
        &std::fs::read(w.h.state.join("agent/sessions").join(&other).join("spec.json")).unwrap(),
    )
    .unwrap();
    let borrowed = aaa_agent::hooks::hook_token(spec["token"].as_str().unwrap());
    let out = w.lab.root.join("post-source-out");
    std::fs::create_dir_all(&out).unwrap();
    let detach = w.task("post-detach.pl", DETACH_PL);
    let script=w.task("post-source-probe.sh",&format!(r#"BIN='{bin}'
OUT='{out}'
try_post() {{ name=$1; shift; "$@" > "$OUT/$name" 2>&1; printf '\nrc=%s\n' "$?" >> "$OUT/$name"; }}
try_post token env AAA_HOOK_TOKEN=forged "$BIN" post coordinator --notice --topic test --title 자료
try_post borrowed env AAA_SESSION_ID='{other}' AAA_HOOK_TOKEN='{borrowed}' "$BIN" post coordinator --notice --topic test --title 자료
try_post envu env -u AAA_SESSION_ID -u AAA_HOOK_SOCK "$BIN" post coordinator --notice --topic test --title 자료
try_post double perl '{detach}' double sh -c '"$@"; code=$?; printf "\ninner_rc=%s\n" "$code"; exit "$code"' _ "$BIN" post coordinator --notice --topic test --title 자료
try_post barrier "$BIN" post coordinator --data --topic test --title 자료
"#,bin=lab::BIN,out=out.display()));
    w.h.ok(&["pane", "run", pane, &format!("post-probe {script}")]);
    w.h.wait_output(pane, "post-probe-done", T);
    for (name, reason) in [("token", "token"), ("borrowed", "peer pid"), ("envu", "보기 권한"), ("double", "peer pid")]
    {
        let text = std::fs::read_to_string(out.join(name)).unwrap();
        let status = if name == "double" { "inner_rc=1" } else { "rc=1" };
        assert!(text.contains(reason) && text.contains(status), "{name}: {text}");
    }
    let text = std::fs::read_to_string(out.join("barrier")).unwrap();
    assert!(text.contains("rc=0"), "{text}");
    assert_eq!(w.h.ok(&["post", "list", "demo"])["posts"].as_array().unwrap().len(), 1);
    // thread가 아닌 셸은 조작 권한으로 Front에 붙어 coordinator 몫으로 보내지만 coordinator에게는 보낼 수 없다(plan §8.288).
    // Front에 붙지 못해 hook 길로 가도(coordinator의 hook 길도 같다) peer+token이 맞아도 host에서 거부한다.
    let outside = out.join("outside");
    let hooked = out.join("outside-hook");
    w.h.ok(&[
        "pane",
        "run",
        &other,
        &format!(
            "'{bin}' post coordinator --notice --topic test --title 자료 > '{out}' 2>&1; AAA_STATE_DIR=/nonexistent '{bin}' post coordinator --notice --topic test --title 자료 > '{hook}' 2>&1; printf 'outside-post-%s\\n' done",
            bin = lab::BIN,
            out = outside.display(),
            hook = hooked.display()
        ),
    ]);
    w.h.wait_output(&other, "outside-post-done", T);
    assert!(std::fs::read_to_string(outside).unwrap().contains("이 목적지로 post할 수 없"));
    assert!(std::fs::read_to_string(hooked).unwrap().contains("열린 thread 세션에서만"));
    assert_eq!(w.h.ok(&["post", "list", "demo"])["posts"].as_array().unwrap().len(), 1);
}

/// 실제 coordinator→host→TLS agent에서 사본 binary를 바꾼다. 공용 시험 binary·운영 서비스는 건드리지 않는다.
#[test]
fn coordinator_deployment_is_gated_and_survives_backend_restarts() {
    let mut phases =
        aaa_test_support::phases::Phases::new("coordinator_deployment_is_gated_and_survives_backend_restarts");
    // 줄 단위 coordinator/worker 셸만 쓴다. 저장소·TUI 준비는 필요 없다.
    let (mut w, env) = world_files("", false, Some("0"), false, |_| Vec::new());
    w.host_env.push(("AAA_TEST_SUBMIT_GAP_MS".into(), "0".into()));
    let mut w = world_start(w, env);
    w.add_remote();
    let b = w.lab.node("b", None);
    let bin = w.lab.root.join("deploy-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("aaa");
    lab::deployment_binary(Path::new(lab::BIN), &exe);
    let original = std::fs::read(&exe).unwrap();
    let candidate = w.lab.root.join("deploy-candidate");
    lab::variant_binary(&exe, &candidate, "aaa-deployment-test");
    let changed = std::fs::read(&candidate).unwrap();
    // launchd adapter도 같은 typed 경로로 본다. HOME·PATH는 이 실험실뿐이며 실제 서비스에는 닿지 않는다.
    let service_home = w.lab.root.join("service-home");
    let service_bin = w.lab.root.join("service-bin");
    let service_log = w.lab.root.join("service-calls");
    let service_fail = w.lab.root.join("service-fail");
    let plist_dir = service_home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&plist_dir).unwrap();
    std::fs::create_dir_all(&service_bin).unwrap();
    for role in ["host", "agent"] {
        std::fs::write(plist_dir.join(format!("dev.aaa.{role}.plist")), "<plist/>").unwrap();
    }
    let launchctl = service_bin.join("launchctl");
    std::fs::write(
        &launchctl,
        r#"#!/bin/sh
case "$1" in
print) printf 'state = running\npid = 100\n' ;;
kickstart) printf '%s\n' "$*" >> "$DEPLOY_SERVICE_LOG"
           [ ! -f "$DEPLOY_SERVICE_FAIL" ] || { echo 'injected restart failure' >&2; exit 1; } ;;
*) exit 1 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&launchctl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let service_path = format!("{}:{}", service_bin.display(), std::env::var("PATH").unwrap());
    let service_env = [
        ("HOME", service_home.to_str().unwrap()),
        ("PATH", service_path.as_str()),
        ("DEPLOY_SERVICE_LOG", service_log.to_str().unwrap()),
        ("DEPLOY_SERVICE_FAIL", service_fail.to_str().unwrap()),
    ];
    w.lab.kill("agentA");
    w.lab.spawn_env("agentA", &w.h, &["agent", "--host", "local", "--name", "labA"], &service_env);
    w.lab.kill("agentB");
    w.lab.spawn_command("agentB-copy", {
        let mut cmd = b.cmd_bin(&exe, &["agent", "--host", &w.addr, "--name", "labB"]);
        cmd.envs(service_env);
        cmd
    });
    // 새 프로세스의 연결 로그와 host 상태를 함께 본다. 원격만 기다리면 로컬 agent가 아직 시작도 하지 않아
    // 곧바로 여는 coordinator가 AgentDisconnected로 실패할 수 있다.
    wait_for("로컬·원격 사본 agent 연결", T, || {
        if !w.lab.log("agentA").contains("host에 연결했다") || !w.lab.log("agentB-copy").contains("host에 연결했다")
        {
            return None;
        }
        let v = w.h.json(&["machine", "list"]).ok()?;
        let machines = v["machines"].as_array()?;
        ["labA", "labB"]
            .iter()
            .all(|name| machines.iter().any(|m| m["name"] == *name && m["connection"] == "connected"))
            .then_some(())
    });
    let coordinator = w.h.ok(&["project", "open", "demo", "--profile", "fake"]);
    let pane = coordinator["pane"]["id"].as_str().unwrap().to_string();
    w.wait_screen(&pane, "fake>");
    let pid = w.h.pane(&pane)["pid"].clone();
    let mut probe_number = 0;
    let probe = |w: &World, pane: &str, n: &mut usize, args: &str| -> (String, String, String) {
        *n += 1;
        let prefix = w.lab.root.join(format!("deploy-probe-{n}"));
        let script = w.task(
            &format!("deploy-probe-{n}.sh"),
            &format!(
                "'{}' --json {args} > '{}.out' 2> '{}.err'\nprintf '%s' \"$?\" > '{}.rc'\nprintf 'deploy-finished-{n}\\n'\n",
                lab::BIN,
                prefix.display(),
                prefix.display(),
                prefix.display()
            ),
        );

        w.h.ok(&["pane", "run", pane, &format!("post-probe {script}")]);
        w.h.wait_output(pane, &format!("deploy-finished-{n}"), T);
        let read = |suffix: &str| std::fs::read_to_string(prefix.with_extension(suffix)).unwrap();
        (read("rc"), read("out"), read("err"))
    };
    phases.mark("contract", "authorization-update-and-rollback");
    let update = format!("update --machine labB --from '{}'", candidate.display());
    let (rc, _, err) = probe(&w, &pane, &mut probe_number, &update);
    assert_eq!(rc, "1", "{err}");
    assert!(err.contains("coordinator_deploy on"), "{err}");
    assert_eq!(std::fs::read(&exe).unwrap(), original);
    // coordinator는 safety를 직접 켤 수 없다.
    let (rc, _, err) = probe(&w, &pane, &mut probe_number, "safety set coordinator_deploy on");
    assert_eq!(rc, "1");
    assert!(err.contains("aaa 세션 안"), "{err}");
    let enabled = w.h.in_terminal(&["safety", "set", "coordinator_deploy", "on"], &[("바꿀까?", "y")], T);
    assert!(enabled.contains("coordinator_deploy = on"), "{enabled}");

    // 자기 신고한 coordinator ID로는 배포 권한을 얻지 못한다(peer PID가 밖이다).
    let spoof =
        w.h.cmd(&["update", "--machine", "labB", "--from", candidate.to_str().unwrap()])
            .env("AAA_SESSION_ID", &pane)
            .capture_output()
            .unwrap();
    assert!(!spoof.status.success(), "자기 신고로 배포가 허용됐다: {spoof:?}");
    assert!(String::from_utf8_lossy(&spoof.stderr).contains("coordinator만"), "{spoof:?}");

    let (rc, out, err) = probe(&w, &pane, &mut probe_number, &update);
    assert_eq!(rc, "0", "{out}\n{err}");
    let job: Value = serde_json::from_str(&out).unwrap();
    let id = job["job"]["id"].as_str().unwrap();
    let (rc, out, err) = probe(&w, &pane, &mut probe_number, &format!("update status {id} --machine labB --wait"));
    assert_eq!(rc, "0", "{out}\n{err}");
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["job"]["state"], "completed");
    assert_eq!(std::fs::read(&exe).unwrap(), changed);
    let updated: Value = serde_json::from_str(&out).unwrap();
    let installed: Value = serde_json::from_str(updated["job"]["result"].as_str().unwrap()).unwrap();
    use sha2::Digest as _;
    assert_eq!(installed["sha256"], hex::encode(sha2::Sha256::digest(&changed)));
    assert_eq!(installed["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        updated["running_sha256"],
        hex::encode(sha2::Sha256::digest(&original)),
        "교체만으로 실행 이미지가 바뀌지 않는다"
    );

    // 같은 오래 사는 agent에서 rollback도 처음 정한 고정 경로를 써야 한다(Linux current_exe는 옛 inode를 가리킨다).
    let (rc, out, err) = probe(&w, &pane, &mut probe_number, "update rollback --machine labB");
    assert_eq!(rc, "0", "{out}\n{err}");
    let rollback: Value = serde_json::from_str(&out).unwrap();
    let rollback_id = rollback["job"]["id"].as_str().unwrap();
    let (rc, out, err) =
        probe(&w, &pane, &mut probe_number, &format!("update status {rollback_id} --machine labB --wait"));
    assert_eq!(rc, "0", "{out}\n{err}");
    assert_eq!(std::fs::read(&exe).unwrap(), original);

    // host가 peer PID로 worker를 판정한다. env를 지워도 배포하지 못한다.
    let worker = w.start(&[
        "--title",
        "배포 차단",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("deploy-worker.txt", "noreport"),
    ]);
    let worker_pane = worker["pane"].as_str().unwrap();
    w.wait_show("worker brief", worker["id"].as_str().unwrap(), |t| t["brief"] == "delivered");
    let envless = w.task(
        "deploy-envless.sh",
        &format!(
            "env -u AAA_SESSION_ID -u AAA_HOOK_SOCK '{}' --json {update} > '{}' 2>&1\n",
            lab::BIN,
            w.lab.root.join("deploy-worker-denied").display()
        ),
    );
    w.h.ok(&["pane", "run", worker_pane, &format!("post-probe {envless}")]);
    w.h.wait_output(worker_pane, "post-probe-done", T);
    assert!(std::fs::read_to_string(w.lab.root.join("deploy-worker-denied")).unwrap().contains("thread는 계속 막는다"));
    assert_eq!(std::fs::read(&exe).unwrap(), original);

    phases.mark("contract", "service-restart");
    if cfg!(target_os = "macos") {
        // 재시작은 재시작만 하는 rollout이다(W6c, 답 1A): 독립 감시가 새 agent를 확인해야 끝난다. 가짜 launchctl은 기록만
        // 하므로 감독자 대신 같은 실행 파일로 다시 띄운다. host 재시작은 m13 `rollout`이 웹 응답까지 본다
        let restart = |w: &World, n: &mut usize| {
            let (rc, out, err) = probe(w, &pane, n, "service restart --role agent --machine labB");
            assert_eq!(rc, "0", "{out}\n{err}");
            let receipt: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(receipt["rollout"]["spec"]["restart"], "agent", "{out}");
            receipt["rollout"]["id"].as_str().unwrap().to_string()
        };
        let id = restart(&w, &mut probe_number);
        wait_for("원격 agent 감독자 재시작 기록", T, || {
            std::fs::read_to_string(&service_log).ok().filter(|c| c.contains("/dev.aaa.agent"))
        });
        w.lab.kill("agentB-copy");
        w.lab.spawn_command("agentB-restarted", {
            let mut cmd = b.cmd_bin(&exe, &["agent", "--host", &w.addr, "--name", "labB"]);
            cmd.envs(service_env);
            cmd
        });
        let (rc, out, err) = probe(&w, &pane, &mut probe_number, &format!("deploy status {id} --wait"));
        assert_eq!(rc, "0", "{out}\n{err}");
        let done: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(done["rollout"]["state"], "done", "{out}");
        let calls = std::fs::read_to_string(&service_log).unwrap();
        assert_eq!(calls.lines().count(), 1, "{calls}");
        assert!(calls.lines().all(|l| l.starts_with("kickstart -k gui/") && l.ends_with("/dev.aaa.agent")), "{calls}");
        // 감독자 재시작이 실패하면 작업을 다시 보내지 않는다. 감시가 한 번 더 다시 띄워 보고, 그래도 확인하지 못하면 복구 확인 필요다
        std::fs::write(&service_fail, "fail").unwrap();
        let id = restart(&w, &mut probe_number);
        let (rc, out, err) = probe(&w, &pane, &mut probe_number, &format!("deploy status {id} --wait"));
        assert_eq!(rc, "1", "{out}\n{err}");
        let failed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(failed["rollout"]["state"], "recovery_failed", "{out}");
        assert!(failed["rollout"]["error"].as_str().unwrap().contains("injected restart failure"), "{out}");
        assert_eq!(
            std::fs::read_to_string(&service_log).unwrap().lines().count(),
            3,
            "작업 한 번과 감시의 복구 한 번만"
        );
        std::fs::remove_file(&service_fail).unwrap();
        // 개별 update가 설치 파일을 바꾼 뒤의 재시작은 같은 판 확인이 아니라 그 파일 띄우기다: 감시를 세우지 않고 재시작 rollout은
        // 중단으로 남기며, 같은 ID의 감시 밖 개별 재시작 작업으로 간다(답 1A의 「개별 update·rollback은 guard 밖」)
        let (rc, out, err) = probe(&w, &pane, &mut probe_number, &update);
        assert_eq!(rc, "0", "{out}\n{err}");
        let id = serde_json::from_str::<Value>(&out).unwrap()["job"]["id"].as_str().unwrap().to_string();
        let (rc, out, err) = probe(&w, &pane, &mut probe_number, &format!("update status {id} --machine labB --wait"));
        assert_eq!(rc, "0", "{out}\n{err}");
        assert_eq!(std::fs::read(&exe).unwrap(), changed);
        let (rc, out, err) = probe(&w, &pane, &mut probe_number, "service restart --role agent --machine labB");
        assert_eq!(rc, "0", "{out}\n{err}");
        assert!(err.contains("감시 밖 개별 재시작"), "{err}");
        let job: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            (&job["job"]["action"]["action"], &job["job"]["action"]["service"]),
            (&"restart".into(), &"agent".into()),
            "{out}"
        );
        let id = job["job"]["id"].as_str().unwrap();
        let (rc, out, err) = probe(&w, &pane, &mut probe_number, &format!("update status {id} --machine labB --wait"));
        assert_eq!(rc, "0", "{out}\n{err}");
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["job"]["state"], "completed", "{out}");
        assert_eq!(std::fs::read_to_string(&service_log).unwrap().lines().count(), 4, "개별 재시작 한 번");
        let (_, out, err) = probe(&w, &pane, &mut probe_number, &format!("deploy status {id}"));
        let stopped: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}\n{err}"));
        assert_eq!(stopped["rollout"]["state"], "stopped", "{out}");
        assert!(stopped["rollout"]["error"].as_str().unwrap().contains(&format!("aaa update status {id}")), "{out}");
    }

    // 사람이 설정을 철회하면 다음 요청부터 즉시 막는다. 이전 작업 조회는 계속 된다.
    let off = w.h.in_terminal(&["safety", "set", "demo", "coordinator_deploy", "off"], &[("바꿀까?", "y")], T);
    assert!(off.contains("coordinator_deploy = off"), "{off}");
    let (rc, _, err) = probe(&w, &pane, &mut probe_number, &update);
    assert_eq!(rc, "1");
    assert!(err.contains("coordinator_deploy on"), "{err}");

    phases.mark("contract", "backend-restart");
    let config = w.h.cfg.join("host.toml");
    std::fs::write(&config, std::fs::read_to_string(&config).unwrap().replace("127.0.0.1:0", &w.addr)).unwrap();
    w.lab.kill("host");
    w.spawn_host("host-restored");
    wait_for("host와 원격 agent 재연결", T, || {
        w.h.json(&["status"]).ok().filter(|_| w.h.connected("labA") && w.h.connected("labB"))
    });
    w.lab.kill("agentA");
    w.lab.spawn("agentA-restored", &w.h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("coordinator agent 재연결", T, || {
        (w.lab.log("agentA-restored").contains("host에 연결했다") && w.h.connected("labA")).then_some(())
    });
    assert_eq!(w.h.pane(&pane)["pid"], pid, "coordinator 프로세스가 바뀌었다");
    let (rc, out, err) =
        probe(&w, &pane, &mut probe_number, &format!("update status {rollback_id} --machine labB --wait"));
    assert_eq!(rc, "0", "재시작 후 같은 pane의 CLI: {out}\n{err}");
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["job"]["state"], "completed");
    eprintln!("재시작 뒤 같은 coordinator pane의 배포 조회: {out}");
    phases.mark("cleanup", "owned-resources");
}

/// macOS: 표준 FD가 먼저 닫혀도 controlling TTY의 미수집 출력을 보존해야 한다.
#[test]
#[cfg(target_os = "macos")]
fn terminal_capture_keeps_output_after_stdio_close_before_reading() {
    let lab = Lab::new();
    let source = lab.root.join("stdio-close.c");
    let bin = lab.root.join("stdio-close");
    let closed = lab.root.join("closed");
    std::fs::write(
        &source,
        r#"
#include <unistd.h>
#include <fcntl.h>
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    const char out[] = "pty-output-before-close";
    const char err[] = "\033[31msecret-prefix-tail\033[0m";
    if (write(1, out, sizeof(out)-1) != sizeof(out)-1) return 3;
    if (write(2, err, sizeof(err)-1) != sizeof(err)-1) return 4;
    close(0); close(1); close(2);
    int done = open(argv[1], O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (done < 0) return 5;
    if (write(done, "closed", 6) != 6) return 6;
    close(done);
    return 0;
}
"#,
    )
    .unwrap();
    let compiled = Command::new("cc").arg(&source).arg("-o").arg(&bin).capture_output().unwrap();
    assert!(compiled.status.success(), "{}", String::from_utf8_lossy(&compiled.stderr));
    let mut command = lab::terminal_command(&bin);
    command.arg(&closed);
    let terminal = lab::Terminal::spawn(command);
    // 자식이 표준 FD를 모두 닫았다는 사건이 reader보다 먼저다. 시간·스케줄러에 기대지 않는다.
    wait_for("표준 FD 닫힘 영수증", lab::LOAD_TIMEOUT, || {
        std::fs::read(&closed).ok().filter(|b| b == b"closed")
    });
    let output = terminal.read(&["stdio-close"], &[], T);
    assert_eq!(output, "pty-output-before-close\x1b[31msecret-prefix-tail\x1b[0m");
}

/// coordinator·브라우저 없이 이동한 차례로 기동하고, 막힌 앞 예약은 보존하며 가능한 뒤 예약을 기동한다.
#[test]
fn task_queue_runs_possible_fifo_after_resolve_without_coordinator() {
    let mut w = report_shell_world();
    w.h.ok(&["project", "set", "demo", "max_parallel_threads", "6"]);
    w.h.ok(&["project", "set", "demo", "thread_profile", "fake"]);
    let mut active = Vec::new();
    for n in 0..6 {
        let task = w.task(&format!("slot-{n}.md"), if n == 0 { "완료 보고" } else { "noreport" });
        let v = w.start(&["--title", &format!("자리 {n}"), "--profile", "fake", "--kind", "dir", "--task-file", &task]);
        active.push(v["id"].as_str().unwrap().to_string());
    }
    let first = active[0].clone();
    w.wait_show("완료 보고에도 자리 보유", &first, |v| {
        v["report_hash"].is_string() && v["brief"] == "delivered"
    });
    std::fs::write(w.projects.join("demo/TASKS.md"),"## Backlog\n- [ ] 앞 예약 (fake@mac-offline) [작업 폴더]\n  독립 설명 · 완료 범위\n- [ ] 가능한 예약 (fake@labA) [작업 폴더]\n  독립 설명 · 완료 범위\n- [ ] 맨 앞으로 옮길 예약 (fake@labA) [작업 폴더]\n  독립 설명 · 완료 범위\n").unwrap();
    w.h.ok(&["task", "reserve", "앞 예약"]);
    w.h.ok(&["task", "reserve", "가능한 예약"]);
    w.h.ok(&["task", "reserve", "맨 앞으로 옮길 예약"]);
    w.h.ok(&["ticker", "tick"]);
    assert_eq!(w.h.ok(&["thread", "list", "demo"])["threads"].as_array().unwrap().len(), 6);
    let queue = w.h.ok(&["task", "queue"]);
    let rows = queue["entries"].as_array().unwrap();
    assert!(rows.iter().all(|r| r["state"] == "queued"));
    let first_id = rows[0]["task_id"].as_str().unwrap().to_string();
    let second_id = rows[1]["task_id"].as_str().unwrap().to_string();
    let moved_id = rows[2]["task_id"].as_str().unwrap().to_string();
    let moved = w.h.ok(&["task", "queue-move", &moved_id, "1"]);
    assert_eq!(moved["entries"][0]["task_id"], moved_id);
    assert_eq!(moved["entries"][0]["position"], 1);
    assert_eq!(moved["entries"][0]["created_ms"], rows[2]["created_ms"]);
    // host 재시작에도 6개의 기존 자리와 승인판·차례를 이어 읽는다.
    w.lab.kill("host");
    w.spawn_host("host-restored");
    wait_for("재연결", T, || w.h.connected("labA").then_some(()));
    let q = w.h.ok(&["task", "queue"]);
    assert_eq!(q["entries"][0]["task_id"], moved_id);
    assert_eq!(q["entries"][1]["task_id"], first_id);
    assert_eq!(q["entries"][2]["task_id"], second_id);
    w.h.ok(&["ticker", "tick"]);
    assert_eq!(w.h.ok(&["thread", "list", "demo"])["threads"].as_array().unwrap().len(), 6);
    w.h.ok(&["thread", "resolve", "demo", &first]);
    let promoted = wait_for("맨 앞으로 옮긴 예약의 실제 기동", T, || {
        let q = w.h.json(&["task", "queue"]).ok()?;
        let row = q["entries"].as_array()?.iter().find(|r| r["task_id"] == moved_id)?;
        row["thread"].as_str().map(str::to_string)
    });
    w.wait_show("맨 앞 예약 pane 기동", &promoted, |v| v["status"] == "open" && v["pane"].is_string());
    let q = w.h.ok(&["task", "queue"]);
    assert!(
        q["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["task_id"] != moved_id)
            .all(|r| r["state"] == "queued" && r["thread"].is_null())
    );
    assert_eq!(w.h.ok(&["thread", "list", "demo"])["threads"].as_array().unwrap().len(), 7);
    w.h.ok(&["thread", "resolve", "demo", &promoted]);
    // ticker 명령이나 coordinator의 다음 턴 없이 resolve 사건이 dispatch를 깨워야 한다.
    let started = wait_for("가능한 예약의 실제 기동", T, || {
        let q = w.h.json(&["task", "queue"]).ok()?;
        let row = q["entries"].as_array()?.iter().find(|r| r["task_id"] == second_id)?;
        row["thread"].as_str().map(str::to_string)
    });
    let v = w.wait_show("예약 pane 기동", &started, |v| v["status"] == "open" && v["pane"].is_string());
    assert_eq!(v["machine"], "labA");
    assert_eq!(v["profile"], "fake");
    let q = w.h.ok(&["task", "queue"]);
    let waiting = q["entries"].as_array().unwrap().iter().find(|r| r["task_id"] == first_id).unwrap();
    assert_eq!(waiting["state"], "queued");
    assert_eq!(w.h.ok(&["thread", "list", "demo"])["threads"].as_array().unwrap().len(), 8);
    let board = w.h.ok(&["task", "list"]);
    assert_eq!(board["tasks"].as_array().unwrap().iter().find(|t| t["id"] == second_id).unwrap()["thread"], started);
    w.h.ok(&["task", "unreserve", &second_id]);
    let unchanged = w.show(&started);
    assert_eq!(unchanged["pane"], v["pane"]);
    assert_eq!(unchanged["status"], "open", "기동 확정 뒤 예약 취소는 thread를 끝내지 않는다");
    w.h.ok(&["task", "unreserve", &first_id]);
    let q = w.h.ok(&["task", "queue"]);
    assert_eq!(
        q["entries"].as_array().unwrap().iter().find(|r| r["task_id"] == first_id).unwrap()["state"],
        "cancelled"
    );
}

/// 실제 coordinator CLI의 명시 지시는 예약을 유지하고 새 값으로 시작한다.
#[test]
fn coordinator_user_request_changes_choices_without_losing_reservations() {
    let mut w = world();
    w.h.ok(&["project", "set", "demo", "max_parallel_threads", "1"]);
    let task = w.task("busy.md", "noreport");
    let busy = w.start(&["--title", "실행 자리", "--profile", "fake", "--kind", "dir", "--task-file", &task]);
    let busy_id = busy["id"].as_str().unwrap();
    w.wait_show("자리 확보", busy_id, |v| v["status"] == "open" && v["brief"] == "delivered");
    for title in ["지시 변경", "취소 뒤 변경"] {
        w.h.ok(&[
            "task",
            "add",
            title,
            "--profile",
            "mute",
            "--machine",
            "labA",
            "--repo",
            "작업 폴더",
            "--note",
            "독립 설명과 완료 범위",
        ]);
        w.h.ok(&["task", "reserve", title]);
    }
    w.h.ok(&["task", "add", "미예약 변경", "--profile", "fake", "--machine", "labA"]);
    let initial = w.h.ok(&["task", "queue"])["entries"].as_array().unwrap().clone();
    let task_id = initial[0]["task_id"].as_str().unwrap();
    let cancelled_id = initial[1]["task_id"].as_str().unwrap();
    w.h.ok(&["task", "unreserve", cancelled_id]);
    let coord = w.h.ok(&["project", "open", "demo", "--profile", "fake"]);
    let pane = coord["pane"]["id"].as_str().unwrap();
    w.wait_screen(pane, "fake>");
    let mut number = 0;
    let mut probe = |command: &str| {
        number += 1;
        let prefix = w.lab.root.join(format!("request-{number}"));
        let script = w.task(&format!("request-{number}.sh"), &format!(
            "'{}' --json {command} > '{p}.out' 2> '{p}.err'\nprintf '%s' \"$?\" > '{p}.rc'\nprintf 'request-{number}-finished\\n'\n",
            lab::BIN, p=prefix.display()
        ));
        w.h.ok(&["pane", "run", pane, &format!("post-probe {script}")]);
        w.h.wait_output(pane, &format!("request-{number}-finished"), T);
        (
            std::fs::read_to_string(prefix.with_extension("rc")).unwrap(),
            std::fs::read_to_string(prefix.with_extension("err")).unwrap(),
        )
    };
    let file = w.projects.join("demo/TASKS.md");
    let original = std::fs::read_to_string(&file).unwrap();
    for (command, error) in [
        ("task edit '지시 변경' --profile fake", "사용자가 웹이나 aaa 밖 CLI".to_string()),
        ("task edit '지시 변경' --profile fake --user-request ' '", "근거".to_string()),
        (
            "task edit '지시 변경' --profile fake --note '범위 교체' --user-request '사용자가 변경 지시'",
            "변경에만".to_string(),
        ),
        ("task edit '지시 변경' --profile unknown --user-request '사용자가 변경 지시'", "profile".to_string()),
        ("thread start demo --from-task '지시 변경' --profile fake", "task owner와 --profile이 다르다".to_string()),
    ] {
        let (rc, err) = probe(command);
        assert_ne!(rc, "0", "{command}");
        assert!(err.contains(&error), "{command}: {err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    }
    let (rc, err) = probe(&format!("thread restart demo {busy_id} --profile mute"));
    assert_ne!(rc, "0");
    assert!(err.contains("정해진 모델 변경"), "{err}");
    let (rc, err) = probe("task edit '미예약 변경' --profile mute --user-request '사용자가 모델만 변경 지시'");
    assert_eq!(rc, "0", "{err}");
    let board = w.h.ok(&["task", "list"]);
    let unreserved = board["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "미예약 변경").unwrap();
    assert_eq!(unreserved["profile"], "mute");
    assert_eq!(unreserved["machine"], "labA");
    let (rc, err) = probe("task edit '미예약 변경' --machine '' --user-request '사용자가 머신만 자동으로 변경 지시'");
    assert_eq!(rc, "0", "{err}");
    let board = w.h.ok(&["task", "list"]);
    let unreserved = board["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "미예약 변경").unwrap();
    assert_eq!(unreserved["profile"], "mute");
    assert!(unreserved["machine"].is_null());
    let reason = "사용자가 채팅에서 fake와 자동 머신으로 변경 지시";
    let (rc, err) = probe(&format!("task edit '{task_id}' --profile fake --machine '' --user-request '{reason}'"));
    assert_eq!(rc, "0", "{err}");
    let (rc, err) = probe("task edit '취소 뒤 변경' --profile fake --user-request '사용자가 취소한 할 일도 변경 지시'");
    assert_eq!(rc, "0", "{err}");
    let queue = w.h.ok(&["task", "queue"]);
    let updated = &queue["entries"][0];
    assert_eq!(updated["task_id"], task_id);
    assert_eq!(updated["created_ms"], initial[0]["created_ms"]);
    assert_eq!(updated["position"], initial[0]["position"]);
    assert_eq!(updated["state"], "queued");
    assert_eq!(updated["profile"], "fake");
    assert!(updated["machine"].is_null());
    assert_eq!(updated["selection"]["profile"], "fake");
    assert!(updated["selection"]["actor"].as_str().unwrap().contains(reason));
    assert_eq!(queue["entries"][1]["state"], "cancelled");
    assert_eq!(queue["entries"][1]["selection"]["profile"], "fake");
    let history = w.h.ok(&["task", "log", task_id]);
    let entry = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["by"].as_str().is_some_and(|s| s.contains(reason)))
        .unwrap();
    assert!(entry["by"].as_str().unwrap().contains("coordinator가 사용자 지시로 변경"));
    assert!(entry["before"].as_str().unwrap().contains("mute@labA"));
    assert!(entry["after"].as_str().unwrap().contains("(fake)"));
    // 호스트 재시작 뒤에도 승인판·순서·변경 출처를 유지한다.
    w.lab.kill("host");
    w.spawn_host("host-request-restored");
    wait_for("재연결", T, || w.h.connected("labA").then_some(()));
    let restored = w.h.ok(&["task", "queue"]);
    assert_eq!(restored["entries"][0]["approved_block"], updated["approved_block"]);
    assert_eq!(restored["entries"][0]["selection"], updated["selection"]);
    w.h.ok(&["thread", "resolve", "demo", busy_id]);
    let started = wait_for("새 선택으로 예약 기동", T, || {
        let q = w.h.json(&["task", "queue"]).ok()?;
        q["entries"][0]["thread"].as_str().map(str::to_string)
    });
    let launched = w.wait_show("새 모델 pane", &started, |v| v["status"] == "open" && v["pane"].is_string());
    assert_eq!(launched["profile"], "fake");
    assert_eq!(launched["machine"], "labA");
    assert_eq!(w.h.ok(&["task", "queue"])["entries"][1]["state"], "cancelled");
    assert_eq!(w.h.ok(&["thread", "list", "demo"])["threads"].as_array().unwrap().len(), 2);
}
