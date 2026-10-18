//! M1-3 E2E: 프로젝트·thread 시작(그 머신 agent가 worktree·작업 폴더를 만든다)·brief 안내·보고 회수·ack·resolve·흐름 사건,
//! harness 상태(화면 감지)·확인창 질문 원격 응답·메시지(queue·interrupt).
//! agent harness는 시험용 가짜 agent(셸 스크립트)다. brief 안내 한 줄을 받으면 brief를 읽고 report.md·library를 쓴다.
//! 가짜 Claude(`claude`)는 Claude Code 화면(입력 상자·spinner·확인창)을 그리고 SessionStart hook으로 세션 ID를 알린다.
//! 실행: `cargo test -p aaa --test m13`

#![cfg(unix)]

mod lab;

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
    let env: Vec<(&str, &str)> = settle_ms.map(|ms| ("AAA_TEST_BRIEF_SETTLE_MS", ms)).into_iter().collect();
    lab.spawn_env(tag, node, &["host"], &env);
}

/// brief 안내를 받으면 할 일대로 보고를 쓰는 가짜 agent
const FAKE_AGENT: &str = r#"#!/bin/sh
printf 'fake> '
while IFS= read -r line; do
  case "$line" in
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
      printf '## Report\n%s\n\n## Next\n- 결과 확인\n- 다음 단계\n' "$task" > "$dir/report.md.tmp"
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
# 실제 Claude처럼 hook 출력(진행 안내 JSON)은 화면에 내지 않고 받는다. `HOOK_LOG`가 있으면 거기 한 줄씩 적는다
hook() { out=$("$AA" hook --agent claude "$1"); [ -z "$out" ] || [ -z "$HOOK_LOG" ] || printf '%s\n' "$out" >> "$HOOK_LOG"; }
key() { stty -icanon -echo min 1 time 0; k=$(dd bs=1 count=1 2>/dev/null); stty icanon; }
keyt() { stty -icanon -echo min 0 time "$1"; k=$(dd bs=1 count=1 2>/dev/null); stty icanon; }
box() { printf '%s\n\342\235\257 \n%s\n  ? for shortcuts' "$RULE" "$RULE"; }
# 입력 대기: 커서를 입력 상자 「❯ 」 뒤에 둔다. Enter 뒤(커서는 상자 아래 선)에는 상자를 지운다.
prompt() { stty echo; box; printf '\033[2A\r\033[2C'; }
taken() { printf '\033[2A\r\033[J'; }
spin() { printf '\342\234\273 Working\342\200\246 (1s \302\267 esc to interrupt)\n'; box; }
unspin() { printf '\033[4A\r\033[J'; }
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
# $1 본문 $2 기본 선택(0·1) $3·$4 선택지 $5 아래 안내 $6 처음 그리기 앞에 함께 쓸 것. 창 하나를 한 번에 그리고(커서는
# 안내 줄 아래), 그린 자리에서 다시 그리고, 고른 뒤 지운다.
menu() {
  sel=$2; h=$(( $(printf '%s\n' "$1" | wc -l) + 5 )); pre=${6:-}
  while :; do
    if [ "$sel" = 0 ]; then o=$(printf ' \342\235\257 %s\n   %s' "$3" "$4"); else o=$(printf '   %s\n \342\235\257 %s' "$3" "$4"); fi
    out "$pre$(printf '%s\n%s\n%s\n\n %s' "$RULE" "$1" "$o" "$5")"
    pre=$(printf '\033[%sA\r\033[J' "$h")
    key
    if [ "$k" = "$ESC" ]; then
      keyt 2
      if [ "$k" = "[" ]; then keyt 2; case "$k" in A) sel=0;; B) sel=1;; esac; continue; fi
      pick=cancel; break
    fi
    if [ -z "$k" ]; then if [ "$sel" = 0 ]; then pick=$3; else pick=$4; fi; break; fi
  done
  printf '\033[%sA\r\033[J' "$h"
}
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
      spin; "$AA" report --percent 40 --activity '권한 확인 중'; perm 'make test'
      [ "$pick" = cancel ] || printf '## Report\n%s\n\n## Next\n- 확인\n' "$pick" > "$dir/report.md" ;;
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
      # 검사를 한다). 그 Enter는 사람이 보지 않은 창의 기본 선택(「1. Yes」)을 고른다
      spin; turn; echo "race armed"; prompt; key
      menu " Bash command

   rm -rf build
   Clean the build

 Do you want to proceed?" 0 '1. Yes' '2. No, and tell Claude what to do differently (esc)' 'Esc to cancel · Tab to amend · ctrl+e to explain' "$(printf '\033[1A\r\033[J')"
      echo "race $pick" ;;
    *ask*) spin; perm 'make test' ;;
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
/// 뜬다(agent threads.rs `resolve`). `EVIL_SID`가 있으면 줄을 받을 때마다 그 값을 세션 ID로 hook(`UserPromptSubmit`)을
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
buf=''; NL='
'
draw() { printf '\033[2J\033[H%s%s\n\342\235\257 %s\n%s\n' "$log" "$R" "$buf" "$R"; }
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
        [ -n "$EVIL_SID" ] && "$AA" hook --agent claude "{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"$EVIL_SID\"}" >/dev/null ;;
      *) buf="$buf$c"; break ;;
    esac
  done
  draw
done
"#;

/// 프로젝트 폴더를 cwd로 쓰는 coordinator(hook 없음). 입력창은 Claude처럼 「❯ 」다.
const COORD_AGENT: &str = "#!/bin/sh\nprintf '\\342\\235\\257 '\nwhile IFS= read -r line; do\n  echo \"got: $line\"\n  printf '\\342\\235\\257 '\ndone\n";

/// `extra`는 `[projects]`에 덧붙인다. `web`이면 웹을 loopback 임의 port로 연다.
fn host_toml(projects: &Path, extra: &str, web: bool) -> String {
    let listen = if web { "127.0.0.1:0" } else { "" };
    format!(
        "schema = 1\n[host]\nagent_listen = \"127.0.0.1:0\"\n[web]\nlisten = \"{listen}\"\n[local_agent]\nmode = \"none\"\n\
         [projects]\nroot = \"{}\"\nallow_profiles = [\"fake\", \"mute\", \"claude\", \"sticky\", \"prog\", \"asker\", \"paste\", \"resumable\", \"evil\"]\nlibrary_copy_mb = 1\n{extra}",
        path_str(projects)
    )
}

fn agent_toml(work_root: &Path, scripts: &Path) -> String {
    format!(
        "schema = 1\n[agent]\nwork_root = \"{}\"\n\
         [profiles.fake]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\n\
         [profiles.mute]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ MUTE_LOG = \"{}\" }}\n\
         [profiles.claude]\nharness = \"claude\"\ncommand = [\"{}\"]\nenv = {{ AA = \"{}\", HOOK_LOG = \"{}\" }}\n\
         [profiles.sticky]\nharness = \"claude\"\ncommand = [\"{}\"]\n\
         [profiles.prog]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ AA = \"{}\" }}\n\
         [profiles.paste]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ PASTE_LOG = \"{}\" }}\n\
         [profiles.asker]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\n\
         [profiles.resumable]\nharness = \"claude\"\ncommand = [\"{}\"]\n\
         [profiles.evil]\nharness = \"claude\"\ncommand = [\"{}\"]\nenv = {{ AA = \"{}\", EVIL_SID = \"--dangerously-skip-permissions\" }}\n\
         [profiles.other]\nharness = \"custom\"\ncommand = [\"/bin/sh\"]\n",
        path_str(work_root),
        path_str(&scripts.join("fake.sh")),
        path_str(&scripts.join("mute.sh")),
        path_str(&scripts.join("mute.log")),
        path_str(&scripts.join("claude.sh")),
        lab::BIN,
        path_str(&scripts.join("hook.log")),
        path_str(&scripts.join("sticky.sh")),
        path_str(&scripts.join("prog.sh")),
        lab::BIN,
        path_str(&scripts.join("paste.sh")),
        path_str(&scripts.join("paste.log")),
        path_str(&scripts.join("asker.sh")),
        path_str(&scripts.join("resumable.sh")),
        path_str(&scripts.join("resumable.sh")),
        lab::BIN,
    )
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
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
}

fn world() -> World {
    world_with("")
}

/// brief 화면 멈춤을 제품 기본값(3초)으로 두는 실험실. 가짜 agent는 brief를 읽자마자 보고를 써서, 멈춤을 줄이면 agent가
/// 1초마다 보는 보고가 host의 받음 확인(Enter 뒤 화면 보기)보다 먼저 올 수 있다. 그러면 받음 확인 때의 보고 hash가 그 보고라
/// 「brief 뒤 새 보고 없음」(orch.rs 6번 줄 `just_delivered`)이 되어, 확인한 뒤에도 창 동안 작업 중이다. 확인 뒤 바로 쉼을
/// 보는 시험은 이 실험실을 쓴다(제품 모서리, t-0327 report)
fn world_product_settle() -> World {
    world_build("", false, None, |_| Vec::new())
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

fn world_build(
    extra: &str,
    web: bool,
    settle_ms: Option<&'static str>,
    agent_env: impl FnOnce(&Path) -> Vec<(String, String)>,
) -> World {
    let lab = Lab::new();
    let env = agent_env(&lab.root);
    let projects = lab.root.join("projects");
    let scripts = lab.root.join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("fake.sh"), FAKE_AGENT).unwrap();
    std::fs::write(scripts.join("mute.sh"), MUTE_AGENT).unwrap();
    std::fs::write(scripts.join("claude.sh"), CLAUDE_AGENT).unwrap();
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
    let repo = make_repo(&lab.root);
    let h = lab.node("h", Some(&host_toml(&projects, extra, web)));
    std::fs::write(h.cfg.join("agent.toml"), agent_toml(&lab.root.join("workA"), &scripts)).unwrap();
    let mut w = World { lab, h, projects, repo, scripts, addr: String::new(), settle_ms };
    w.spawn_host("host");
    let st = wait_for("host 준비", T, || w.h.json(&["status"]).ok());
    w.addr = st["agent_listen"].as_str().unwrap().to_string();
    // agent를 Claude Code 세션 안에서 띄운 경우: 그 세션 표지는 aaa 세션에 넘기지 않는다
    let mut envs = vec![("CLAUDE_CODE_CHILD_SESSION", "1"), ("CLAUDECODE", "1")];
    envs.extend(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    w.lab.spawn_env("agentA", &w.h, &["agent", "--host", "local", "--name", "labA"], &envs);
    wait_for("labA 연결", T, || w.h.connected("labA").then_some(()));
    w.h.ok(&["project", "create", "demo"]);
    w
}

impl World {
    fn spawn_host(&mut self, tag: &str) {
        spawn_host(&mut self.lab, tag, &self.h, self.settle_ms);
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
        wait_for_diag(
            what,
            T,
            || Some(self.show(id)).filter(|v| f(v)),
            || format!("thread: {}\nevents: {}", self.show(id), self.h.ok(&["events", "demo"])),
        )
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
        wait_for_diag(
            &format!("화면 {needle:?}"),
            T,
            || Some(self.h.screen(pane)).filter(|s| s.contains(needle)),
            || self.h.screen(pane),
        )
    }

    /// coordinator nudge의 시간 창이 지난 뒤 tick한다. host는 화면이 `idle` 동안 그대로여야 입력창을 보고, 입력창이
    /// `quiet` 동안 비어 보여야 보낸다(inbox.rs `nudge`). 창마다 host가 지금 화면·입력창을 본 tick이 끝난 뒤부터 잰다:
    /// 창보다 일찍 tick하면 결함이 있어도 보내지 않아 「보내지 않았다」 단언이 늘 통과한다.
    fn nudge_windows(&self, idle: Duration, quiet: Duration) {
        for window in [idle, quiet] {
            self.h.ticks(1);
            // fixed wait: 시간 규칙의 창 자체다(설정 값). 지난 뒤의 tick이 판정한다
            std::thread::sleep(window);
        }
        self.h.ticks(1);
    }

    fn wait_for_count(&self, pane: &str, needle: &str, n: usize) {
        wait_for_diag(
            &format!("화면 {needle:?} {n}번"),
            T,
            || (self.h.screen(pane).matches(needle).count() >= n).then_some(()),
            || self.h.screen(pane),
        )
    }

    fn kinds(&self, after: u64) -> Vec<String> {
        let v = self.h.ok(&["events", "demo", "--after", &after.to_string()]);
        v["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect()
    }

    /// 원격(TLS) agent labB를 등록하고 띄운다
    fn add_remote(&mut self) {
        let b = self.lab.node("b", None);
        std::fs::write(b.cfg.join("agent.toml"), agent_toml(&self.lab.root.join("workB"), &self.scripts)).unwrap();
        self.h.ok(&["machine", "enroll", "--open", "5m"]);
        let (_, mut out) = self.lab.spawn_piped("enroll", &b, &["agent", "enroll", &self.addr, "--name", "labB"]);
        let line = read_line_containing(&mut out, "--code");
        let code = line.split("--code").nth(1).unwrap().trim().to_string();
        self.h.ok(&["machine", "approve", "labB", "--code", &code]);
        assert!(self.lab.wait_exit("enroll", T).success(), "{}", self.lab.log("enroll"));
        self.lab.spawn("agentB", &b, &["agent", "--host", &self.addr, "--name", "labB"]);
        wait_for("labB 연결", T, || self.h.connected("labB").then_some(()));
    }
}

/// worktree thread 한 바퀴: 시작 → brief 안내 → 보고·library 사본 → ack → resolve(PR이 없으니 branch는 남긴다)
#[test]
fn worktree_thread_round_trip() {
    let w = world_product_settle();
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
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    assert_eq!(cwd, w.lab.root.join("workA").join("repo").join("demo-t-0001"));
    assert!(cwd.join(".aaa/demo-t-0001/brief.md").is_file());
    let brief = std::fs::read_to_string(cwd.join(".aaa/demo-t-0001/brief.md")).unwrap();
    assert!(brief.contains("X-42") && brief.contains("# thread brief") && brief.contains("# 프로젝트 지침"), "{brief}");
    // 작업 폴더·aaa 실행 파일 자리는 agent가 그 머신 값으로 채웠다(plan §8.78)
    let work = cwd.join(".aaa/demo-t-0001");
    assert!(brief.contains(&format!("- 보고: `{}/report.md`", work.display())), "{brief}");
    assert!(brief.contains(&format!("`{} report --percent 25", lab::BIN)), "{brief}");
    assert!(!brief.contains("{{aaa:") && !brief.contains("[진행 중]"), "{brief}");
    assert!(w.projects.join("demo/threads/t-0001.task.md").is_file());

    // 보고 사본과 brief 받음 확인은 따로 온다(agent의 보고 감시와 host의 Enter 뒤 화면 보기): 둘 다 기다린다
    let v = w.wait_show("보고 사본·brief 받음", "t-0001", |v| {
        v["report_hash"].is_string() && v["brief"] == "delivered"
    });
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
    // 끝난 thread도 목록에 남는다(herdr-projects `thread list <slug>`, `--all` 없음). 프로젝트는 위치 인자다.
    let listed = w.h.ok(&["thread", "list", "demo"]);
    assert_eq!(listed["threads"][0]["group"], "resolved", "{listed}");
    let text = String::from_utf8_lossy(&w.h.run(&["thread", "list", "demo"]).stdout).to_string();
    assert!(text.starts_with("demo/t-0001\t끝남\t"), "{text}");
    assert!(!cwd.exists(), "고치지 않은 worktree는 지워져야 한다");
    // branch는 PR이 병합됐을 때만 지운다(herdr-projects threads.rs `clean`): 새 commit이 없어도 남긴다
    assert_eq!(
        git(&w.repo, &["branch", "--list", "aaa/demo/*", "--format=%(refname:short)"]),
        "aaa/demo/t-0001-fix-the-bug"
    );
    let resolved = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().last().unwrap().clone();
    assert!(
        resolved["detail"]
            .as_str()
            .unwrap()
            .contains("branch aaa/demo/t-0001-fix-the-bug를 남겼다: PR이 병합되지 않았다"),
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
    assert_eq!(w.kinds(0), ["thread_started", "brief_delivered", "report_ready", "review_acked", "thread_resolved"]);
    // cursor 뒤만 읽는다
    assert_eq!(w.kinds(3), ["review_acked", "thread_resolved"]);
}

/// 고친 worktree와 병합되지 않은 branch는 남긴다(force 없음). 허용되지 않은 profile은 거부한다.
/// pane은 그 thread의 자리를 치웠을 때만 닫는다(herdr-projects threads.rs `clean`: worktree를 지우며 workspace를, tab 종류는
/// tab을 닫는다). 남긴 worktree의 pane 줄은 남고(세션만 끝남), 작업 폴더 thread와 이미 지운 worktree(`worktree_gone`)는 닫는다.
/// 집에 다 오지 않은 worktree도 남긴다(`clean` `copy_complete`): `--discard-uncopied`면 지우고, `--skip-copy`는 사본 없이
/// 끝내며 남긴다.
#[test]
fn resolve_keeps_dirty_worktree_and_unmerged_branch() {
    let w = world();
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
    let v = w.h.ok(&["thread", "resolve", "demo", &id]);
    assert_eq!(v["status"], "resolved");
    assert!(cwd.join("dirty.txt").exists(), "고친 worktree는 남아야 한다");
    assert_eq!(git(&w.repo, &["branch", "--list", "aaa/demo/*", "--format=%(refname:short)"]), "aaa/demo/t-0001-keep");
    let ev = w.h.ok(&["events", "demo"]);
    let last = ev["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["kind"], "thread_resolved");
    assert!(last["detail"].as_str().unwrap().contains("worktree 유지"), "{last}");
    assert!(!last["detail"].as_str().unwrap().contains("pane을 닫았다"), "{last}");
    wait_for("남긴 worktree의 세션 끝남", T, || (w.h.pane(&pane)["state"] != "running").then_some(()));

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
    let d = w.start(&["--title", "dir", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("d.txt", "x")]);
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
    assert!(listed(&ppane), "남긴 worktree의 pane 줄은 남는다");
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
    let mut w = world();
    w.add_remote();
    let connected_at = Instant::now();
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
    // brief 받음 확인까지 기다린다: host를 내리기 전에 끝나지 않으면 새 host가 화면으로 확인해 사건이 하나 더 생긴다
    let v =
        w.wait_show("원격 보고·brief 받음", &id, |v| v["report_hash"].is_string() && v["brief"] == "delivered");
    assert_eq!(v["machine"], "labB");
    assert!(std::fs::read_to_string(w.projects.join("demo/threads").join(format!("{id}.md"))).unwrap().contains("R-7"));
    let pane = v["pane"].as_str().unwrap().to_string();
    // thread 세션은 worker 역할이다
    let workers = w.h.ok(&["pane", "list", "--role", "worker"]);
    assert!(workers.to_string().contains(&pane));
    assert_eq!(w.h.ok(&["pane", "list", "--role", "evaluator"])["panes"], serde_json::json!([]));
    let seen = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().len() as u64;

    // host 재시작 중에 보고가 바뀌어도, 다시 붙은 agent가 지금 hash를 알려 사본을 다시 받는다
    // 원격 agent가 같은 주소로 다시 붙게 처음 받은 포트를 고정한다
    // 끊김은 새 host가 처음 본 때부터 잰다(herdr-projects `Outage`): 연결된 지 기준(5초)보다 오래된 머신도
    // 다시 붙는 동안 「오래 끊김」이 되지 않는다. 매초 검사해 붙기 전 검사가 반드시 있게 한다.
    let toml = std::fs::read_to_string(w.h.cfg.join("host.toml")).unwrap();
    let toml = toml
        .replace("127.0.0.1:0", &w.addr)
        .replace("library_copy_mb = 1\n", "library_copy_mb = 1\noutage_secs = 5\ntick_secs = 1\n");
    assert!(toml.contains("outage_secs = 5"), "{toml}");
    std::fs::write(w.h.cfg.join("host.toml"), toml).unwrap();
    std::thread::sleep(Duration::from_secs(7).saturating_sub(connected_at.elapsed()));
    w.lab.kill("host");
    // host가 없는 동안 agent 머신에서 보고가 바뀐다
    let work_dir = PathBuf::from(v["work_dir"].as_str().unwrap());
    std::fs::write(work_dir.join("report.md"), "## Report\n갱신\n\n## Next\n- 사용자 확인\n").unwrap();
    w.spawn_host("host2");
    wait_for("labB 재연결", Duration::from_secs(60), || w.h.connected("labB").then_some(()));
    let v = w.wait_show("갱신된 보고", &id, |v| v["next"] == serde_json::json!(["사용자 확인"]));
    // 보고 첫 줄 태그로 사람 차례를 정하지 않는다(D9): 새 보고는 검토 대기
    assert_eq!(v["group"], "ready_for_review");
    assert_eq!(w.kinds(seen), ["report_ready"]);
    // host2가 다시 붙기 전에 낸 outage가 있으면 여기와 아래 항목 시각 확인이 잡는다
    let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
    assert!(!items.iter().any(|i| i["item_kind"] == "outage"), "host 재시작에 outage가 생겼다: {items:?}");

    // 머신이 끊긴 채 host를 다시 띄워도 기준(5초) 전에는 알리지 않는다. 옛 코드는 마지막 연결 시각(7초 넘게 전)부터
    // 재서 첫 검사에 바로 알렸다. 기준 뒤 한 번, 돌아오면 한 번 알린다.
    // 「기준 전에는 없다」는 시험 시계로 재지 않고 host가 남긴 항목 시각으로 본다: host3를 띄운 때보다 5초 넘게 뒤여야 한다.
    // host3가 끊김을 처음 본 때는 띄운 때보다 늦으므로 부하로 늦어져도 거짓 실패가 없고, 일찍 알리면 늘 잡힌다.
    w.lab.kill("agentB");
    w.lab.kill("host2");
    let spawned_ms = aaa_common::util::now_ms();
    w.spawn_host("host3");
    let downs = || -> Vec<Value> {
        let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
        items.into_iter().filter(|i| i["item_kind"] == "outage" && i["event"] == "down").collect()
    };
    let outage = |event: &str| {
        let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
        items.iter().filter(|i| i["item_kind"] == "outage" && i["event"] == event).count()
    };
    wait_for("host3 응답", T, || w.h.json(&["inbox", "list", "demo", "--peek"]).ok());
    let down = wait_for("outage down", T, || downs().into_iter().next());
    let at = down["created_at_ms"].as_u64().unwrap();
    assert!(
        at >= spawned_ms + 5000,
        "host가 끊김을 본 지 기준(5초) 전에 outage가 생겼다: 띄운 뒤 {}ms {down}",
        at as i64 - spawned_ms as i64
    );
    // 알린 끊김이 이어지는 동안 host를 다시 띄워도 그 끊김은 이어진다(plan §8.134): 다시 「down」을 쓰지 않고, 돌아오면
    // 「recovered」를 한 번 쓴다. 옛 host는 새로 띄우면 알린 것을 잊어 회복을 영영 쓰지 않았다.
    w.lab.kill("host3");
    // 위 `outage`가 w.h를 빌려 쥐고 있어 칸을 따로 넘긴다
    spawn_host(&mut w.lab, "host4", &w.h, w.settle_ms);
    wait_for("host4 응답", T, || w.h.json(&["inbox", "list", "demo", "--peek"]).ok());
    let b = w.lab.node("b", None);
    w.lab.spawn("agentB2", &b, &["agent", "--host", &w.addr, "--name", "labB"]);
    wait_for("outage recovered", T, || (outage("recovered") == 1).then_some(()));
    assert_eq!(outage("down"), 1);
}

/// 안내를 보내도 화면이 바뀌지 않으면 unknown으로 남기고 자동으로 다시 보내지 않는다(§8.5 a)
/// 입력창을 읽을 수 없는 harness(`mute`: 화면이 바뀌지 않는다): ticker는 한 번 보내고, 다시 보내기 전에 화면으로 앞
/// 시도를 확인할 수 없어 멈춘다(herdr-projects brief.rs `give_up`). 사람이 `thread brief`로 보내면 그때만 한 번 더 간다.
#[test]
fn unknown_brief_is_not_resent() {
    let mut w = world();
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
    ev.kind("report_ready", &fid, T);
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
    ev.wait(&format!("{kind} 질문"), T, |e| match e {
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
            Some(t.clone())
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

    // 답보다 늦게 온 같은 확인창의 짝 hook(PermissionRequest → 답 → Notification, ci-local에서 난 차례, plan §8.99)도 새
    // 질문을 열지 않는다. 진행 보고가 같은 연결로 그 뒤에 오므로, 그것이 보이면 늦은 Notification을 처리한 뒤다
    assert_eq!(w.message(&id, "late ask", false)["state"], "accepted");
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
    let (m4, screen) = w.interrupt(&id, &pane, "urgent");
    assert_eq!(m4["state"], "accepted", "{m4}");
    let after = &screen[screen.find("got: long run").unwrap()..];
    assert!(after.find("Interrupted").is_some_and(|i| after.find("got: urgent").is_some_and(|u| i < u)), "{screen}");
    // 두 번째 interrupt
    let t = lab::now_ms();
    finish_turn(&w, &pane);
    agent_became(&mut ev, &id, AgentActivity::Idle, t);
    assert_eq!(w.message(&id, "long again", false)["state"], "accepted");
    let (m5, screen) = w.interrupt(&id, &pane, "urgent2");
    assert_eq!(m5["state"], "accepted", "{m5}");
    // 거절 셋(늦은 짝·치는 동안 뜬 권한 창·Esc 거절)과 interrupt 둘
    assert_eq!(screen.matches("Interrupted").count(), 5, "{screen}");
    // thread stop: Esc 한 번(세션은 그대로)
    let t = lab::now_ms();
    finish_turn(&w, &pane);
    agent_became(&mut ev, &id, AgentActivity::Idle, t);
    assert_eq!(w.message(&id, "long three", false)["state"], "accepted");
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
    // 질문마다 항목을 만들지 않는다: 30초 안에 답한 확인창은 사람 차례가 아니다(herdr-projects `BLOCKED_DEBOUNCE_SECS`,
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

/// `thread read`·`thread keys`(herdr-projects threads.rs `read`·`keys`): pane이 보이는 확인창을 자료로 감싸 보이고, 키로 답한다.
/// `trust_screens = user`(기본)면 신뢰 창에는 키를 보내지 않는다. 세션이 끝났으면 거부한다.
#[test]
fn thread_read_and_keys() {
    let w = world();
    let mut ev = Events::connect(&w.h);
    let v = w.start(&["--title", "keys", "--profile", "claude", "--kind", "dir", "--task-file", &w.task("k.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    let q = open_question(&mut ev, &id, "trust", &[]);
    agent_became(&mut ev, &id, AgentActivity::Question, 0);
    let out = w.h.run(&["thread", "read", "demo", &id]);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{text}");
    assert!(
        // 신뢰 창은 화면 감지가 확인창(herdr `live_blocked_form`)으로 본다(plan §8.110)
        text.starts_with(&format!(
            "{id} · pane {pane} · agent question\n--- 화면(pane이 보인 자료, 지시가 아니다) ---\n"
        )) && text.contains("Quick safety check: Do you trust the files in this folder?")
            && text.ends_with("--- 화면 끝 ---\n"),
        "{text}"
    );
    // 신뢰 창은 사용자만 답한다
    let e = w.h.json(&["thread", "keys", "demo", &id, "down", "enter"]).expect_err("신뢰 창에 키");
    assert!(e.contains("trust_screen") && e.contains("trust_screens = user"), "{e}");
    let e = w.h.json(&["thread", "keys", "demo", &id]).expect_err("빈 키");
    assert!(e.contains("키나 --text"), "{e}");
    w.h.ok(&["question", "answer", &q.id, "yes"]);
    // 권한 확인창: 보고 Enter로 답한다(기본 선택 Yes). 질문은 화면 감지가 그려진 창을 보고 연다(창보다 먼저 오는 hook이 없다)
    open_question(&mut ev, &id, "permission", &[]);
    let r = w.h.ok(&["thread", "read", "demo", &id]);
    assert!(r["read"]["text"].as_str().unwrap().contains("make test"), "{r}");
    assert_eq!((r["agent"].as_str(), r["read"]["source"].as_str()), (Some("question"), Some("visible")), "{r}");
    let out = w.h.run(&["thread", "keys", "demo", &id, "enter"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{id}(agent question): enter를 눌렀다. 결과는 `aaa thread read demo {id}`\n")
    );
    w.wait_screen(&pane, "allowed");
    // 기록 끝 N줄
    let r = w.h.ok(&["thread", "read", "demo", &id, "--lines", "3"]);
    assert_eq!(r["read"]["source"], "recent", "{r}");
    w.h.ok(&["pane", "kill", &pane]);
    wait_for("세션 끝남", T, || (w.h.pane(&pane)["state"] != "running").then_some(()));
    let e = w.h.json(&["thread", "read", "demo", &id]).expect_err("끝난 세션");
    assert!(e.contains("세션이 끝났다") && e.contains(&format!("aaa thread restart demo {id}")), "{e}");
}

/// memory가 brief에 들어간다: 결정도 memory에 적는다(D10 G9, herdr-projects COORDINATOR.md). 끝난 세션은 다시 시작한다
/// (새 brief·resume). 결정이 적힌 옛 decisions.md는 brief에 넣지 않고, memory로 옮기라고 내용당 한 번 알린다.
#[test]
fn memory_and_restart() {
    let w = world_product_settle();
    let dir = w.projects.join("demo");
    assert!(!dir.join("decisions.md").exists(), "새 프로젝트에는 결정 기록이 없다");
    std::fs::write(dir.join("MEMORY.md"), "# Memory\n\n- [API](memory/api.md): API 판\n").unwrap();
    std::fs::write(dir.join("memory/api.md"), "API는 v2로 한다\n").unwrap();
    // `aaa decision`은 없다: 무엇을 하면 되는지 말한다
    let o = w.h.run(&["decision", "add", "demo", "--text", "x"]);
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success() && e.contains("memory(MEMORY.md 색인과"), "{e}");

    let v = w.start(&[
        "--title",
        "mem",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("t6.txt", "memory 확인"),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    let brief_path = PathBuf::from(v["work_dir"].as_str().unwrap()).join("brief.md");
    let brief = std::fs::read_to_string(&brief_path).unwrap();
    assert!(brief.contains("## memory/api.md\n\nAPI는 v2로 한다") && !brief.contains("결정 기록"), "{brief}");
    // brief 받음 확인까지 기다린다: 확인 전에 pane이 닫히면 brief가 pending으로 남아 작업 중이다(6번 줄)
    w.wait_show("보고·brief 받음", &id, |v| v["report_hash"].is_string() && v["brief"] == "delivered");

    // 실행 중이면 다시 시작하지 않는다
    let e = w.h.json(&["thread", "restart", "demo", &id]).expect_err("실행 중 restart");
    assert!(e.contains("실행 중"), "{e}");
    // 보고를 쓴 뒤 닫힌 세션은 끝난 일이다: 사람 차례가 아니다(herdr-projects `group()` 3번 줄)
    w.h.ok(&["thread", "ack", "demo", &id]);
    w.h.ok(&["pane", "kill", &pane]);
    wait_for("세션 끝남", T, || (w.h.pane(&pane)["state"] != "running").then_some(()));
    assert_eq!(w.show(&id)["group"], "idle");

    // 새로 시작: task·memory를 다시 읽어 brief를 새로 쓰고 안내한다
    std::fs::write(dir.join("memory/api.md"), "API는 v3로 바꾼다\n").unwrap();
    let v = w.h.ok(&["thread", "restart", "demo", &id]);
    let pane2 = v["pane"].as_str().unwrap().to_string();
    assert_ne!(pane2, pane);
    let brief = std::fs::read_to_string(&brief_path).unwrap();
    // 다시 시작 안내는 thread 규칙 다음, 프로젝트 지침 앞이다(herdr-projects `compose_brief`)
    let at = |n: &str| brief.find(n).unwrap_or_else(|| panic!("{n} 없음: {brief}"));
    assert!(
        at("# thread brief") < at("**이 일을 앞서 시도한 것이")
            && at("**이 일을 앞서 시도한 것이") < at("# 프로젝트 지침")
    );
    assert!(brief.contains("API는 v3로 바꾼다") && !brief.contains("v2"), "{brief}");
    let v = w.wait_show("다시 안내", &id, |v| v["brief"] == "delivered");
    // hook이 없는 harness는 일하는지 알 수 없다: 방금 안내를 받은 thread는 작업 중이다(herdr-projects ticker `delivered`,
    // 다음 tick에야 7번 줄로 쉼이 된다). 같은 보고(같은 hash)를 다시 썼으니 새 보고는 없다.
    assert_eq!(v["group"], "working");
    let seen = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().len() as u64;

    // resume: 같은 대화를 잇는다(custom harness는 같은 명령). brief 안내는 하지 않는다: 새 세션이 입력을 기다리고
    // ticker가 두 번 돈 뒤에도 brief를 받지 않았다.
    w.h.ok(&["pane", "kill", &pane2]);
    wait_for("세션 끝남", T, || (w.h.pane(&pane2)["state"] != "running").then_some(()));
    let v = w.h.ok(&["thread", "restart", "demo", &id, "--resume"]);
    let pane3 = v["pane"].as_str().unwrap().to_string();
    w.wait_screen(&pane3, "fake>");
    w.h.ticks(2);
    assert!(!w.h.screen(&pane3).contains("brief 받음"), "{}", w.h.screen(&pane3));
    assert_eq!(w.kinds(seen), ["thread_restarted"]);

    // 프로젝트 폴더는 agent가 고칠 수 있다: symlink·FIFO인 decisions.md는 읽지 않는다(FIFO를 열면 ticker가 멈춘다,
    // t-0340 보안 감사)
    let outside = w.lab.root.join("decisions-outside.md");
    std::fs::write(&outside, "# 결정 기록\n\n## 1 · 2026-10-16 12:00Z\n\n밖의 파일\n").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("decisions.md")).unwrap();
    w.h.ticks(2);
    std::fs::remove_file(dir.join("decisions.md")).unwrap();
    assert!(Command::new("mkfifo").arg(dir.join("decisions.md")).status().unwrap().success());
    w.h.ticks(2);
    std::fs::remove_file(dir.join("decisions.md")).unwrap();
    // 결정이 적힌 옛 decisions.md: brief에 넣지 않고, memory로 옮기라고 내용당 한 번 알린다(host는 memory를 쓰지 않는다)
    std::fs::write(dir.join("decisions.md"), "# 결정 기록\n\n## 1 · 2026-10-16 12:00Z\n\nAPI는 v1이다\n").unwrap();
    w.h.ticks(2);
    let items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    let old: Vec<&Value> = items.iter().filter(|i| i["subject"] == "decisions.md").collect();
    assert_eq!(old.len(), 1, "{items:?}");
    assert_eq!((old[0]["item_kind"].as_str(), old[0]["event"].as_str()), (Some("config-error"), Some("not read")));
    assert!(old[0]["summary"].as_str().unwrap().contains("결정 1건은 thread brief에 더 들어가지 않는다"), "{old:?}");
    assert!(!std::fs::read_dir(dir.join("memory")).unwrap().any(|e| e.unwrap().file_name() != "api.md"));
}

/// thread start 기본값(herdr-projects threads.rs `start`·`placement`·`thread_profile`, 차이 목록 A1·A15): profile을 빼면
/// project.toml `thread_profile`, kind를 빼면 repo가 있으면 worktree·없으면 작업 폴더. 미할당 task도 `thread_profile`로
/// 시작하고, 이미 열린 thread가 맡은 task는 막지 않고 경고한다.
#[test]
fn start_defaults_profile_kind_and_task() {
    let w = world();
    let dir = w.projects.join("demo");
    let task = w.task("d.txt", "x");
    let e = w.h.json(&["thread", "start", "demo", "--title", "d", "--task-file", &task]).expect_err("profile 없음");
    assert!(e.contains("project.toml에 thread_profile"), "{e}");
    std::fs::write(dir.join("project.toml"), "schema = 1\nthread_profile = \"fake\"\n").unwrap();
    let a = w.start(&["--title", "d", "--task-file", &task]);
    assert_eq!((a["profile"].as_str(), a["thread_kind"].as_str()), (Some("fake"), Some("dir")), "{a}");
    let b = w.start(&["--title", "r", "--repo", path_str(&w.repo), "--task-file", &task]);
    assert_eq!((b["profile"].as_str(), b["thread_kind"].as_str()), (Some("fake"), Some("worktree")), "{b}");
    assert!(b["branch"].as_str().is_some_and(|s| s.starts_with("aaa/demo/")), "{b}");
    // 미할당 task는 thread_profile, 이미 열린 thread가 맡은 task는 경고만 한다(CLI는 `경고:` 줄)
    let aid = a["id"].as_str().unwrap();
    std::fs::write(dir.join("TASKS.md"), format!("# Tasks\n\n## Backlog\n- [ ] 새 일\n- [ ] 맡긴 일 · {aid}\n"))
        .unwrap();
    let c = w.start(&["--from-task", "새 일"]);
    assert_eq!((c["profile"].as_str(), c["title"].as_str()), (Some("fake"), Some("새 일")), "{c}");
    assert!(c.get("warnings").is_none(), "{c}");
    let o = w.h.run(&["thread", "start", "demo", "--from-task", "맡긴 일"]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out}{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.starts_with(&format!("경고: 열린 {aid}가 이미 이 task를 맡고 있다. 그래도 시작했다\n")), "{out}");
}

/// thread restart --profile(herdr-projects threads.rs `restart`, 차이 목록 E8): 그 머신 agent.toml에서 다시 해석해 새 대화로
/// 띄운다. 허용 목록에 없는 profile·`--resume`과 함께는 거부한다(agent.toml에 없는 profile은 agent 유닛 시험).
#[test]
fn restart_with_another_profile() {
    let w = world();
    let v = w.start(&["--title", "p", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("p.txt", "x")]);
    let (id, pane) = (v["id"].as_str().unwrap().to_string(), v["pane"].as_str().unwrap().to_string());
    w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    w.h.ok(&["pane", "kill", &pane]);
    wait_for("세션 끝남", T, || (w.h.pane(&pane)["state"] != "running").then_some(()));
    let o = w.h.run(&["thread", "restart", "demo", &id, "--resume", "--profile", "mute"]);
    assert_eq!(o.status.code(), Some(2), "{}", String::from_utf8_lossy(&o.stderr));
    let e = w.h.json(&["thread", "restart", "demo", &id, "--profile", "nope"]).expect_err("허용하지 않은 profile");
    assert!(e.contains("allow_profiles에 없는 profile: nope"), "{e}");
    assert_eq!(w.show(&id)["profile"], "fake", "거부한 restart는 기록을 바꾸지 않는다");
    let v = w.h.ok(&["thread", "restart", "demo", &id, "--profile", "mute"]);
    assert_eq!(v["profile"], "mute", "{v}");
    let pane2 = v["pane"].as_str().unwrap().to_string();
    assert_ne!(pane2, pane);
    // 새 세션은 mute profile의 명령이다(옛 fake 명령이 아니다)
    wait_for("새 세션", T, || (w.h.pane(&pane2)["state"] == "running").then_some(()));
    let argv = w.h.pane(&pane2)["argv"].to_string();
    assert!(argv.contains("mute.sh") && !argv.contains("fake.sh"), "{argv}");
    assert_eq!(w.kinds(0).iter().filter(|k| *k == "thread_restarted").count(), 1);
}

/// 시작이 실패한 thread도 `thread restart`로 다시 띄운다(herdr-projects threads.rs `restart_plan`, COORDINATOR_GUIDE,
/// 차이 목록 E8). repo가 아직 git이 아니라 자리를 만들지 못했다(a): 시작 오류는 thread ID와 다시 하는 법을 말하고
/// thread는 failed다. repo를 고친 뒤 restart는 자리부터 다시 만들고 앞선 시도 안내를 넣은 brief를 준다. 실패 글은 지운다.
/// 기록된 worktree 없이 branch만 있으면 반쯤 만든 것이라 거부한다(b). 끝낸 thread는 `--reopen`이 먼저다.
#[test]
fn failed_threads_restart_from_the_place() {
    let w = world();
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

/// auto-resolve(herdr-projects steps.rs `auto_resolve`, 차이 목록 E11): 쉼으로 `auto_resolve_days`가 지나면 마지막 사본 뒤
/// 끝내고, 항목이 `--reopen`을 안내한다. `--reopen`은 기록만 되돌리고 아무것도 띄우지 않는다. 하루는 시험에서 1초다.
#[test]
fn auto_resolve_and_reopen() {
    let w = world_with("tick_secs = 1\nauto_resolve_day_secs = 1\n");
    let dir = w.projects.join("demo");
    std::fs::write(dir.join("project.toml"), "schema = 1\nauto_resolve_days = 2\n").unwrap();
    let v = w.start(&["--title", "a", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("a.txt", "x")]);
    let (id, pane) = (v["id"].as_str().unwrap().to_string(), v["pane"].as_str().unwrap().to_string());
    w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    w.h.ok(&["thread", "ack", "demo", &id]);
    w.h.ok(&["pane", "kill", &pane]);
    w.wait_show("쉼", &id, |v| v["group"] == "idle");
    let v = w.wait_show("자동 resolve", &id, |v| v["status"] == "resolved");
    assert_eq!(v["group"], "resolved");
    let items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    let done: Vec<&Value> = items.iter().filter(|i| i["subject"] == id.as_str() && i["event"] == "resolved").collect();
    assert_eq!(done.len(), 1, "{items:?}");
    let summary = done[0]["summary"].as_str().unwrap();
    assert!(
        summary.contains("쉼으로 auto_resolve_days(2일)가 지나 저절로 끝냈다")
            && summary.contains(&format!("aaa thread resolve demo {id} --reopen")),
        "{summary}"
    );
    // 다시 연다: 기록만 열리고 아무것도 띄우지 않는다. 다시 끝나지 않게 auto-resolve를 끈다(0)
    std::fs::write(dir.join("project.toml"), "schema = 1\nauto_resolve_days = 0\n").unwrap();
    let o = w.h.run(&["thread", "resolve", "demo", &id, "--reopen"]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out}{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.starts_with(&format!("경고: 아무것도 띄우지 않았다. `aaa thread restart demo {id}`")), "{out}");
    assert_eq!(w.show(&id)["status"], "open");
    let e = w.h.json(&["thread", "resolve", "demo", &id, "--reopen"]).expect_err("열린 thread");
    assert!(e.contains("끝낸 thread가 아니다"), "{e}");
    // 작업 폴더 thread는 폴더가 남아 다시 띄울 수 있다. ticker가 두 번 돌아도 다시 끝나지 않는다(0이면 끈다)
    let v = w.h.ok(&["thread", "restart", "demo", &id]);
    w.wait_show("다시 안내", &id, |v| v["brief"] == "delivered");
    w.h.ticks(2);
    assert_eq!(w.show(&id)["status"], "open", "{v}");
    let kinds = w.kinds(0);
    assert!(kinds.contains(&"thread_reopened".to_string()), "{kinds:?}");
}

/// inbox: 묶음 전이(새 보고·쉼, plan §8.85)·설정 오류가 항목이 되고, coordinator가 입력을 기다릴 때 처음 보는 묶음을 한 번만
/// 알린다(§8.17)
#[test]
fn inbox_and_nudge() {
    let (idle, quiet) = (Duration::from_secs(2), Duration::from_secs(1));
    let w = world_with(&format!(
        "tick_secs = 1\nnudge_idle_secs = {}\nnudge_quiet_secs = {}\n",
        idle.as_secs(),
        quiet.as_secs()
    ));
    let nudges = || w.kinds(0).iter().filter(|k| *k == "nudge_sent").count();
    let dir = w.projects.join("demo");
    let coord =
        w.h.new_pane(None, "labA", None, &["/bin/sh", path_str(&w.scripts.join("coord.sh"))], Some(path_str(&dir)));
    w.wait_screen(&coord, "❯");
    // 사람이 입력창에 쓰던 글이 있으면 알리지 않는다
    w.h.ok(&["pane", "send-text", &coord, "draft-text"]);
    w.wait_screen(&coord, "❯ draft-text");

    let v = w.start(&["--title", "inbox", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("t7.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    // 새 보고는 묶음이 검토 대기일 때 hash당 한 번 항목이 된다
    let items = wait_for("새 보고 항목", T, || {
        Some(w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone())
            .filter(|v| !v.is_empty())
    });
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(
        (items[0]["item_kind"].as_str(), items[0]["event"].as_str()),
        (Some("thread-state"), Some("new report"))
    );
    // 쓰던 글이 남아 있으면 nudge 창이 지나도 보내지 않는다
    w.nudge_windows(idle, quiet);
    assert_eq!(nudges(), 0, "{:?}", w.kinds(0));
    assert!(!w.h.screen(&coord).contains("[aaa inbox]"), "{}", w.h.screen(&coord));

    // 쓰던 글을 보내고 나면 알린다
    w.h.ok(&["pane", "send-keys", &coord, "enter"]);
    w.wait_screen(&coord, "got: draft-text");
    let screen = w.wait_screen(&coord, "got: [aaa inbox]");
    assert!(screen.contains(&format!("got: [aaa inbox] {id} new report (aaa inbox list demo)")), "{screen}");
    // 같은 묶음은 다시 보내지 않는다: 받은 뒤 화면이 그대로 머문 다음 nudge 창이 다시 지나도 한 번이다
    wait_for_diag(
        "알림을 받은 뒤 입력 대기",
        T,
        || w.h.screen(&coord).trim_end().ends_with('❯').then_some(()),
        || w.h.screen(&coord),
    );
    w.nudge_windows(idle, quiet);
    assert_eq!(nudges(), 1, "{:?}", w.kinds(0));
    assert_eq!(w.h.screen(&coord).matches("got: [aaa inbox]").count(), 1, "{}", w.h.screen(&coord));

    // context가 보인 항목은 「봤다」가 되어 다음 알림에 다시 나오지 않는다(herdr-projects `context`).
    // 확인(ack)하면 검토 대기에서 쉼으로 바뀐 전이 항목만 새로 알린다.
    let c = w.h.ok(&["context", "demo", "--peek"]);
    assert_eq!(c["inbox"].as_array().unwrap().len(), 1, "{c}");
    assert_eq!(c["inbox"][0]["seen"], false);
    w.h.ok(&["context", "demo"]);
    assert_eq!(w.h.ok(&["inbox", "list", "demo", "--peek"])["items"][0]["seen"], true);
    w.h.ok(&["thread", "ack", "demo", &id]);
    w.wait_for_count(&coord, "got: [aaa inbox]", 2);
    let screen = w.h.screen(&coord);
    assert!(screen.contains(&format!("got: [aaa inbox] {id} idle (aaa inbox list demo)")), "{screen}");

    // 보면(peek 아님) 「봤다」가 되고, 더 알리지 않는다
    let items = w.h.ok(&["inbox", "list", "demo"])["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 2);
    let items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    assert!(items.iter().all(|i| i["seen"] == true), "{items:?}");
    let inbox = dir.join("inbox");
    let files = |d: &Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        v.sort();
        v
    };
    assert_eq!(files(&inbox).len(), 2);
    let report_id = items.iter().find(|i| i["event"] == "new report").unwrap()["id"].as_str().unwrap().to_string();
    let md = std::fs::read_to_string(inbox.join(format!("{report_id}.md"))).unwrap();
    assert!(
        md.starts_with("+++\n") && md.contains("kind = \"thread-state\"") && md.contains("event = \"new report\""),
        "{md}"
    );

    // 끝내기: 파일은 inbox/done으로
    let e = w.h.json(&["inbox", "done", "demo", "no-such-item"]).expect_err("없는 항목");
    assert!(e.contains("no-such-item"), "{e}");
    let done = w.h.ok(&["inbox", "done", "demo", &report_id]);
    assert_eq!((done["done"].as_u64(), done["items"].as_array().unwrap().len()), (Some(1), 1), "{done}");
    // 끝낸 수를 말한다(herdr-projects 「N item(s) moved to inbox/done」)
    let out = w.h.run(&["inbox", "done", "demo", "--all"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1개 항목을 끝냈다(inbox/done으로 옮김)\n");
    assert!(w.h.ok(&["inbox", "list", "demo"])["items"].as_array().unwrap().is_empty());
    assert_eq!(w.h.ok(&["inbox", "list", "demo", "--all"])["items"].as_array().unwrap().len(), 2);
    assert!(files(&inbox).is_empty());
    assert_eq!(files(&inbox.join("done")).len(), 2);

    // project.toml을 읽지 못하면 내용당 한 번 알린다
    std::fs::write(dir.join("project.toml"), "schema = [\n").unwrap();
    let screen = w.wait_screen(&coord, "got: [aaa inbox] project.toml not usable");
    assert!(screen.contains("got: [aaa inbox] project.toml not usable (aaa inbox list demo)"), "{screen}");
    // ticker가 같은 내용을 두 번 더 봐도 항목은 하나다(tick마다 다시 읽는다, 내용 hash당 한 번)
    w.h.ticks(2);
    assert_eq!(w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().len(), 1);
    let nudges = w.kinds(0).iter().filter(|k| *k == "nudge_sent").count();
    assert_eq!(nudges, 3);
    assert_eq!(w.h.screen(&coord).matches("got: [aaa inbox]").count(), 3);
}

/// coordinator가 없거나 project.toml `nudge = false`면 nudge는 처음 보는 묶음을 본 것으로만 적는다(herdr-projects steps.rs
/// `nudge`, 차이 목록 3장 G6, plan §8.136): 항목은 coordinator의 다음 턴 `context`가 보인다. 나중에 생긴 coordinator를 그
/// 묶음으로 깨우지 않고, 새 항목으로 묶음이 바뀌면 그때 알린다.
#[test]
fn nudge_only_marks_the_set_without_a_coordinator_or_when_off() {
    let (idle, quiet) = (Duration::from_secs(2), Duration::from_secs(1));
    let w = world_with(&format!(
        "tick_secs = 1\nnudge_idle_secs = {}\nnudge_quiet_secs = {}\n",
        idle.as_secs(),
        quiet.as_secs()
    ));
    let nudges = || w.kinds(0).iter().filter(|k| *k == "nudge_sent").count();
    let dir = w.projects.join("demo");
    // 보고를 쓰는 thread를 하나 띄우고 그 「새 보고」 항목이 생기기를 기다린다
    let report = |title: &str| {
        let v = w.start(&[
            "--title",
            title,
            "--profile",
            "fake",
            "--kind",
            "dir",
            "--task-file",
            &w.task(&format!("{title}.txt"), "x"),
        ]);
        let id = v["id"].as_str().unwrap().to_string();
        wait_for("새 보고 항목", T, || {
            let items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
            items.iter().any(|i| i["subject"] == id.as_str() && i["event"] == "new report").then_some(())
        });
        id
    };

    // 1) coordinator가 없을 때 생긴 묶음: tick이 본 것으로 적는다
    let a = report("a");
    w.h.ticks(1);
    let coord =
        w.h.new_pane(None, "labA", None, &["/bin/sh", path_str(&w.scripts.join("coord.sh"))], Some(path_str(&dir)));
    w.wait_screen(&coord, "❯");
    // 2) 나중에 생긴 coordinator는 그 묶음으로 깨우지 않는다(옛 aaa는 생기면 알렸다)
    w.nudge_windows(idle, quiet);
    assert_eq!(nudges(), 0, "{:?}", w.kinds(0));
    assert!(!w.h.screen(&coord).contains("[aaa inbox]"), "{}", w.h.screen(&coord));
    // 3) 새 항목이 생겨 묶음이 바뀌면 알린다: 아직 보지 않은 앞 항목도 함께다
    let b = report("b");
    let screen = w.wait_screen(&coord, "got: [aaa inbox]");
    assert!(screen.contains(&format!("got: [aaa inbox] {a} new report; {b} new report")), "{screen}");
    wait_for_diag(
        "알림을 받은 뒤 입력 대기",
        T,
        || w.h.screen(&coord).trim_end().ends_with('❯').then_some(()),
        || w.h.screen(&coord),
    );

    // 4) `nudge = false`: coordinator가 입력을 기다려도 알리지 않고 묶음만 적는다
    std::fs::write(dir.join("project.toml"), "schema = 1\nnudge = false\n").unwrap();
    let c = report("c");
    w.nudge_windows(idle, quiet);
    assert_eq!(nudges(), 1, "{:?}", w.kinds(0));
    // 5) 다시 켜도 이미 적은 묶음은 알리지 않는다(항목은 context가 보인다)
    std::fs::write(dir.join("project.toml"), "schema = 1\n").unwrap();
    w.nudge_windows(idle, quiet);
    assert_eq!(nudges(), 1, "{:?}", w.kinds(0));
    assert_eq!(w.h.screen(&coord).matches("got: [aaa inbox]").count(), 1, "{}", w.h.screen(&coord));
    let ctx = w.h.ok(&["context", "demo", "--peek"]);
    assert!(ctx["inbox"].as_array().unwrap().iter().any(|i| i["subject"] == c.as_str()), "{ctx}");
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

/// 묶음 전이 항목(herdr-projects steps.rs `write_thread_items`, plan §8.85). 한 머신의 agent 둘(labA·labB):
/// - 진행을 보고하는 thread(labA)가 배경 CI를 기다리며 중간 보고(35%)를 쓰고 턴을 끝내도 검토 대기가 아니다: 항목·알림이
///   없다(t-0334 결함 6). `Done`을 알리면 끝낸 보고 하나로 「new report」 한 건·알림 한 번.
/// - 보고 없이 쉬는 thread(labB)는 「idle」(알림 없음), 그 pane이 닫히면 「pane closed」(알림, 다시 시작 안내).
/// - 확인(ack)하면 「idle」. resolve는 상태를 먼저 써서, 보고 없는 worktree thread의 세션을 끝내고 정리해도 「pane closed」·
///   세션 항목 없이 「resolved」 한 건이다(알림 없음, t-0334 결함 7).
#[test]
fn thread_state_items_follow_groups() {
    let (port, rx) = webhook_server(false);
    // brief 뒤 작업 중으로 치는 창(기본 15초)을 줄인다: 보고 없는 B·D의 쉼이 그만큼 늦다. A(진행 보고)는 첫 보고가 brief를
    // 받았다는 표시라 이 창을 쓰지 않는다. 창 자체는 aaa-host `a_just_delivered_brief_is_working_until_a_report_or_a_tick`
    let mut w = world_with(&format!(
        "tick_secs = 1\ndelivered_working_secs = 5\n[notify]\nwebhook = \"http://127.0.0.1:{port}/hook\"\ndebounce_secs = 1\n"
    ));
    w.add_remote();
    let items = |w: &World| -> Vec<Value> {
        w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone()
    };
    let has = |w: &World, id: &str, event: &str| items(w).iter().any(|i| i["subject"] == id && i["event"] == event);
    let wait_item = |w: &World, id: &str, event: &str| {
        wait_for_diag(
            &format!("{id} {event} 항목"),
            T,
            || has(w, id, event).then_some(()),
            || format!("inbox: {:?}\nthread: {}", items(w), w.show(id)),
        )
    };
    // 받은 알림 항목(대상, 사건)
    let mut pushed: Vec<(String, String)> = Vec::new();
    let wait_push = |id: &str, event: &str, pushed: &mut Vec<(String, String)>| {
        let want = (id.to_string(), event.to_string());
        while !pushed.contains(&want) {
            let (_, body) = rx.recv_timeout(T).unwrap_or_else(|_| panic!("{id} {event} 알림: 받은 것 {pushed:?}"));
            let v: Value = serde_json::from_str(&body).unwrap();
            for i in v["items"].as_array().unwrap() {
                pushed.push((i["subject"].as_str().unwrap().into(), i["event"].as_str().unwrap().into()));
            }
        }
    };

    let start = |machine: &str, profile: &str, task: &str| -> (String, String) {
        let v = w.start(&[
            "--title",
            task,
            "--machine",
            machine,
            "--profile",
            profile,
            "--kind",
            "dir",
            "--task-file",
            &w.task(&format!("{task}.txt"), task),
        ]);
        (v["id"].as_str().unwrap().to_string(), v["pane"].as_str().unwrap().to_string())
    };
    let (a, _) = start("labA", "prog", "progress");
    let (b, b_pane) = start("labB", "fake", "noreport");

    // A: 중간 보고는 작업 중(진행 35%)
    let v = w.wait_show("중간 보고", &a, |v| v["report_hash"].is_string() && v["agent"]["percent"] == 35);
    assert_eq!(v["group"], "working", "{v}");
    let interim = v["report_hash"].as_str().unwrap().to_string();
    // B: 보고 없이 쉰다
    wait_item(&w, &b, "idle");
    // A: Done → 끝낸 보고로 검토 대기, 그 보고의 「new report」 한 건
    assert_eq!(w.message(&a, "finish", false)["state"], "accepted");
    let v = w.wait_show("끝낸 보고", &a, |v| {
        v["group"] == "ready_for_review" && v["report_hash"].as_str().is_some_and(|h| h != interim)
    });
    let done = v["report_hash"].as_str().unwrap().to_string();
    wait_item(&w, &a, "new report");
    wait_push(&a, "new report", &mut pushed);
    // B: pane이 닫히면 사람 차례
    w.h.ok(&["pane", "kill", &b_pane]);
    wait_item(&w, &b, "pane closed");
    wait_push(&b, "pane closed", &mut pushed);
    // A: 확인하면 쉼, 끝내면 resolved
    w.h.ok(&["thread", "ack", "demo", &a]);
    wait_item(&w, &a, "idle");
    let v = w.h.ok(&["thread", "resolve", "demo", &a]);
    assert_eq!(v["status"], "resolved");
    assert!(has(&w, &a, "resolved"), "{:?}", items(&w));
    // B: 다시 시작하면 다시 쉰다. D는 B와 따로 돌아 함께 띄운다(보고 없는 thread의 쉼은 brief 뒤 작업 중 창을 기다린다)
    w.h.ok(&["thread", "restart", "demo", &b]);
    // D: 보고 없이 쉬는 worktree thread를 끝낸다(세션 종료 뒤 worktree 정리가 이어진다, t-0334가 본 0.4초 창).
    // 상태를 먼저 써서 닫힌 pane을 사람 차례로 보지 않는다: resolved뿐이다
    let v = w.start(&[
        "--title",
        "worktree",
        "--machine",
        "labB",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("d.txt", "noreport"),
    ]);
    let (d, d_pane) = (v["id"].as_str().unwrap().to_string(), v["pane"].as_str().unwrap().to_string());
    wait_for_diag(
        "B 두 번째 idle",
        T,
        || (items(&w).iter().filter(|i| i["subject"] == b.as_str() && i["event"] == "idle").count() == 2).then_some(()),
        || format!("{:?}", items(&w)),
    );
    wait_item(&w, &d, "idle");
    let v = w.h.ok(&["thread", "resolve", "demo", &d]);
    assert_eq!(v["status"], "resolved");
    let panes = w.h.ok(&["pane", "list"]);
    assert!(
        !panes["panes"].as_array().unwrap().iter().any(|p| p["id"] == d_pane.as_str()),
        "끝낸 thread의 pane이 남았다: {panes}"
    );
    // 울타리: host가 D의 pane을 지운 뒤 묶음 셈을 두 번 더 돌았다(「pane closed」·「sessions ended」가 나올 차례가 지났다)
    w.h.ticks(2);

    let got: Vec<(String, String, String)> = items(&w)
        .iter()
        .map(|i| {
            let s = |k: &str| i[k].as_str().unwrap().to_string();
            (s("subject"), s("event"), s("summary"))
        })
        .collect();
    let events = |id: &str| -> Vec<&str> { got.iter().filter(|g| g.0 == id).map(|g| g.1.as_str()).collect() };
    assert_eq!(events(&a), ["new report", "idle", "resolved"], "{got:?}");
    assert_eq!(events(&b), ["idle", "pane closed", "idle"], "{got:?}");
    assert_eq!(events(&d), ["idle", "resolved"], "{got:?}");
    assert!(got.iter().all(|g| g.1 != "sessions ended"), "{got:?}");
    let report = got.iter().find(|g| g.0 == a && g.1 == "new report").unwrap();
    assert!(report.2.contains(&format!("(보고 {})", &done[..12])) && !report.2.contains(&interim[..12]), "{report:?}");
    let closed = got.iter().find(|g| g.0 == b && g.1 == "pane closed").unwrap();
    assert!(
        closed.2.starts_with(&format!("{b} \"noreport\": 사람 차례(pane closed)"))
            && closed.2.contains(&format!("aaa thread restart demo {b}")),
        "{closed:?}"
    );
    // 알림: 사람이 할 일만 나간다(쉼·resolved는 inbox에만). 알림은 차례대로 오므로, 중간 보고에 항목이 생겼다면 끝낸 보고
    // 알림보다 먼저(또는 함께) 받았다. 마지막 알림 뒤에 생긴 항목(쉼·resolved)은 알릴 것이 아니다.
    assert_eq!(pushed, [(a.clone(), "new report".to_string()), (b.clone(), "pane closed".to_string())]);
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

/// thread 범위(§8.25): 일반 thread 세션 안의 CLI는 보기만 되고, thread가 아닌 pane(사람의 셸) 안의 CLI는 그대로 조작한다
#[test]
fn worker_session_cli_is_read_only() {
    let w = world();
    let t = w.start(&["--title", "w", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("w.txt", "x")]);
    let (id, pane) = (t["id"].as_str().unwrap(), t["pane"].as_str().unwrap());
    let shell = w.h.new_pane(None, "labA", Some("sh"), &["/bin/sh"], None);
    let as_session = |s: &str, args: &[&str]| w.h.cmd(args).env("AAA_SESSION_ID", s).output().unwrap();
    for args in [
        &["thread", "next", "demo", id, "--add", "thread가 더한 줄"][..],
        &["thread", "message", "demo", id, "--text", "x"],
        &["pane", "send-text", &shell, "echo hi"],
        &["thread", "ack", "demo", id],
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

/// 동시 thread 한도는 프로젝트 설정이고 경고만 한다(herdr-projects threads.rs `start`, D10 A13)
#[test]
fn parallel_threads_only_warn() {
    let w = world();
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
    // 한도에 닿으면 시작은 하고 경고한다(CLI는 `경고:` 줄)
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
    assert!(o.status.success(), "{out}{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.starts_with("경고: 열린 thread가 이미 2개다(project.toml max_parallel_threads 2)"), "{out}");
}

/// routine(§8.9): coordinator가 없으면 건너뛰고, 안내는 처리 전까지 하나만, 명령은 host.toml 허용과 사람의 승인이 있어야
/// 대상 머신 agent가 실행한다. 출력이 바뀔 때만 항목이 되고, 명령을 고치면 다시 승인해야 한다.
#[test]
fn routines() {
    let mut w = world_with("tick_secs = 1\nroutine_commands = [\"demo\"]\n");
    w.add_remote();
    let dir = w.projects.join("demo");
    let put = |name: &str, body: &str| std::fs::write(dir.join("routines").join(format!("{name}.md")), body).unwrap();
    std::fs::write(dir.join("value.txt"), "v1\n").unwrap();
    put("ping", "+++\nschedule = \"every 2s\"\n+++\n상태를 확인한다\n");
    put("cmd", "+++\nschedule = \"every 2s\"\ncommand = \"cat value.txt\"\n+++\n값을 본다\n");
    put("far", "+++\nschedule = \"every 2s\"\ncommand = \"pwd\"\nmachine = \"labB\"\n+++\n");
    // 꺼진 `on = "pr"`은 쓸 수 있는 routine이다(옮기기가 꺼서 넣은 0.2 pr-followup.md, t-0334). 없는 trigger만 오류다.
    put("pr", "+++\non = \"pr\"\nenabled = false\n+++\n");
    put("slack", "+++\non = \"slack\"\n+++\n");
    let items = |kind: &str, subject: &str| -> Vec<Value> {
        w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["item_kind"] == kind && i["subject"] == subject)
            .cloned()
            .collect()
    };
    let routine = |name: &str| -> Value {
        w.h.ok(&["routine", "list", "demo"])["routines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()
            .clone()
    };

    // coordinator가 없으면 돌지 않고 센다(밀린 실행도 쌓지 않는다)
    wait_for("coordinator 없음", T, || (routine("ping")["no_coordinator"].as_u64() >= Some(2)).then_some(()));
    assert!(items("routine", "ping").is_empty() && items("routine-approval", "cmd").is_empty());
    assert!(items("config-error", "pr").is_empty());
    assert_eq!(
        (routine("pr")["schedule"].as_str(), routine("pr")["enabled"].as_bool()),
        (Some("on pr: opened, checks-failed, review, merged"), Some(false))
    );
    assert_eq!(items("config-error", "slack").len(), 1);
    assert!(routine("slack")["error"].as_str().unwrap().contains("없는 trigger"));

    let coord =
        w.h.new_pane(None, "labA", None, &["/bin/sh", path_str(&w.scripts.join("coord.sh"))], Some(path_str(&dir)));
    w.wait_screen(&coord, "❯");
    // 안내 routine: 처리하지 않은 항목이 있으면 새로 만들지 않고 건너뛴다
    let ping = wait_for("ping 항목", T, || items("routine", "ping").into_iter().next());
    assert_eq!((ping["event"].as_str(), ping["body"].as_str()), (Some("due"), Some("상태를 확인한다")));
    wait_for("ping 건너뜀", T, || (routine("ping")["skipped"].as_u64() >= Some(2)).then_some(()));
    assert_eq!(items("routine", "ping").len(), 1);
    // 승인 전에는 실행하지 않고, 같은 명령에는 「승인 필요」를 한 번만 남긴다
    wait_for("승인 필요", T, || items("routine-approval", "cmd").into_iter().next());
    // 항목을 본 뒤 차례가 한 번 더 돈 것은 지난 실행 시각으로 본다(승인이 없어도 차례마다 옮기고, 항목을 먼저 남긴다). routine.rs
    // `routine_pass`는 항목을 남긴 뒤 last_run_ms를 저장하므로, 항목을 본 때 읽은 값은 그 차례가 저장하기 전일 수 있다: 두 번
    // 넘어가기를 기다린다(한 번이면 그 차례 자신의 저장일 수 있어 다음 차례를 보지 못한다)
    let mut last = routine("cmd")["last_run_ms"].as_u64();
    for n in 1..=2 {
        last = wait_for(&format!("승인 필요 뒤 차례 {n}"), T, || {
            Some(routine("cmd")["last_run_ms"].as_u64()).filter(|now| *now > last)
        });
    }
    assert_eq!(items("routine-approval", "cmd").len(), 1);
    assert!(items("routine", "cmd").is_empty());

    // 승인: aaa 세션 안이거나 터미널이 아니면 거부한다
    let mut c = w.h.cmd(&["routine", "approve", "demo", "cmd"]);
    let o = c.env("AAA_SESSION_ID", "x").output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("aaa 세션 안에서는"), "{o:?}");
    let o = w.h.cmd(&["routine", "approve", "demo", "cmd"]).env_remove("AAA_SESSION_ID").output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("터미널"), "{o:?}");
    // 사람이 터미널에서 이름을 치면 승인된다(시험은 script로 PTY를 만든다)
    let approve = |name: &str, typed: &str| -> String {
        let line = format!("{} routine approve demo {name}", lab::BIN);
        let mut c = Command::new("script");
        if cfg!(target_os = "macos") {
            c.args(["-q", "/dev/null", lab::BIN, "routine", "approve", "demo", name]);
        } else {
            c.args(["-qec", &line, "/dev/null"]);
        }
        c.env("AAA_CONFIG_DIR", &w.h.cfg)
            .env("AAA_STATE_DIR", &w.h.state)
            .env_remove("AAA_SESSION_ID")
            .env_remove("AAA_HOOK_SOCK")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = c.spawn().unwrap();
        // 프롬프트가 보인 뒤에 친다(script는 입력이 끝나면 바로 EOF를 넣는다)
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let mut out = child.stdout.take().unwrap();
        let s2 = seen.clone();
        let reader = std::thread::spawn(move || {
            let mut b = [0u8; 256];
            while let Ok(n) = std::io::Read::read(&mut out, &mut b) {
                if n == 0 {
                    break;
                }
                s2.lock().unwrap().extend_from_slice(&b[..n]);
            }
        });
        let text = || String::from_utf8_lossy(&seen.lock().unwrap()).to_string();
        wait_for_diag("승인 프롬프트", T, || text().contains("이름을 친다").then_some(()), text);
        let mut stdin = child.stdin.take().unwrap();
        std::io::Write::write_all(&mut stdin, format!("{typed}\n").as_bytes()).unwrap();
        let o = child.wait_with_output().unwrap();
        drop(stdin);
        reader.join().unwrap();
        format!("{}{}", text(), String::from_utf8_lossy(&o.stderr))
    };
    let out = approve("cmd", "wrong");
    assert!(out.contains("cat value.txt") && out.contains("이름이 다르다"), "{out}");
    assert_eq!(routine("cmd")["approved"], false);
    let out = approve("cmd", "cmd");
    assert!(!out.contains("aaa:"), "{out}");
    assert_eq!(routine("cmd")["approved"], true);

    // 승인 뒤 실행: 출력이 바뀔 때만 항목이 된다. 출력은 자료로 감싼다.
    let first = wait_for("출력 항목", T, || items("routine", "cmd").into_iter().next());
    assert_eq!(first["event"], "output changed");
    assert!(first["body"].as_str().unwrap().contains("값을 본다\n\n신뢰할 수 없는 명령 출력(종료 코드 0)"), "{first}");
    assert!(first["body"].as_str().unwrap().contains("```text\nv1\n```"), "{first}");
    let md = std::fs::read_to_string(dir.join("inbox").join(format!("{}.md", first["id"].as_str().unwrap()))).unwrap();
    assert!(md.contains("```text\nv1\n```"), "{md}");
    // 같은 출력으로 한 번 더 돈 뒤에도 항목은 하나다. 돈 것은 흐름 사건 routine_ran으로 본다(항목은 그 사건보다 먼저
    // 남긴다): 첫 실행(출력 바뀜, 위 항목) 다음 실행이 「출력 같음」이어야 한다
    let ran = || -> Vec<String> {
        w.h.ok(&["events", "demo"])["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "routine_ran")
            .filter_map(|e| e["detail"].as_str().filter(|d| d.starts_with("cmd: ")).map(String::from))
            .collect()
    };
    let runs = wait_for("cmd 두 번째 실행", T, || Some(ran()).filter(|r| r.len() >= 2));
    assert_eq!(items("routine", "cmd").len(), 1, "{runs:?}");
    assert!(runs[0].ends_with("출력 바뀜") && runs[1].ends_with("출력 같음"), "{runs:?}");
    std::fs::write(dir.join("value.txt"), "v2\n").unwrap();
    wait_for("두 번째 출력", T, || (items("routine", "cmd").len() == 2).then_some(()));
    // 명령을 고치면 승인이 풀린다
    put("cmd", "+++\nschedule = \"every 2s\"\ncommand = \"cat value.txt; echo more\"\n+++\n값을 본다\n");
    wait_for("다시 승인 필요", T, || (items("routine-approval", "cmd").len() == 2).then_some(()));
    assert_eq!(routine("cmd")["approved"], false);

    // 다른 머신: 그 머신 agent가 자기 작업 폴더(<work_root>/<project>/routines)에서 실행한다
    approve("far", "far");
    let far = wait_for("원격 출력", T, || items("routine", "far").into_iter().next());
    assert!(far["body"].as_str().unwrap().contains("/workB/demo/routines\n"), "{far}");
    let ran = w.kinds(0).iter().filter(|k| *k == "routine_ran").count();
    assert!(ran >= 3, "{:?}", w.kinds(0));
    assert!(w.kinds(0).contains(&"routine_approved".to_string()));
}

/// HTTP/1.1 GET 한 번(Connection: close). (상태, 머리글 소문자, 본문)
fn http_get(addr: &str, path: &str) -> (u16, String, Vec<u8>) {
    use std::io::{Read, Write};
    let mut c = std::net::TcpStream::connect(addr).unwrap();
    c.set_read_timeout(Some(T)).unwrap();
    write!(c, "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").unwrap();
    let mut raw = Vec::new();
    c.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("머리글 끝");
    let head = String::from_utf8_lossy(&raw[..split]).to_lowercase();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head, raw[split + 4..].to_vec())
}

/// 결과물 보기(§8.7): host 사본은 바로, 복사 상한을 넘어 agent에 남긴 파일은 agent에서 스트리밍한다.
/// 보기 경로는 그 thread 결과물만 읽는 짧은 권한이고, 결과물 HTML은 sandbox로 간다.
#[test]
fn library_view() {
    let w = world_opts("", true);
    let web = w.h.ok(&["status"])["web_listen"].as_str().unwrap().to_string();
    let v = w.start(&[
        "--title",
        "big",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("t9.txt", "big link"),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    let agent_lib = PathBuf::from(v["work_dir"].as_str().unwrap()).join("library");
    let v = w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    assert_eq!(v["copy"], "partial", "{v}");

    let lib = w.h.ok(&["thread", "library", "demo", &id]);
    assert!(lib["report"].as_str().unwrap().contains("## Report"), "{lib}");
    let files: Vec<(String, u64, bool)> = lib["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["path"].as_str().unwrap().into(), f["size"].as_u64().unwrap(), f["on_agent"].as_bool().unwrap()))
        .collect();
    assert!(files.contains(&("big.bin".into(), 2_097_152, true)), "{files:?}");
    assert!(files.contains(&("out.txt".into(), 10, false)), "{files:?}");
    // symlink는 목록에 없다
    assert!(!files.iter().any(|f| f.0 == "link"), "{files:?}");
    let base = lib["base"].as_str().unwrap().to_string();

    // host 사본: sandbox CSP, 같은 host 웹 화면 안 frame만
    let (st, head, body) = http_get(&web, &format!("{base}index.html"));
    assert_eq!(st, 200, "{head}");
    assert!(head.contains("content-security-policy: sandbox allow-scripts"), "{head}");
    assert!(head.contains("x-frame-options: sameorigin") && head.contains("text/html"), "{head}");
    assert!(String::from_utf8_lossy(&body).contains("<p>보고</p>"));
    let (st, _, body) = http_get(&web, &format!("{base}sub/d.txt"));
    assert_eq!((st, body.as_slice()), (200, b"deep\n".as_slice()));
    // agent에 남긴 파일: 조각으로 스트리밍한 바이트가 원본과 같다
    let (st, head, body) = http_get(&web, &format!("{base}big.bin"));
    assert_eq!(st, 200, "{head}");
    assert!(head.contains("content-length: 2097152"), "{head}");
    assert!(body == std::fs::read(agent_lib.join("big.bin")).unwrap(), "본문 {}바이트가 원본과 다르다", body.len());

    // 경로 밖·목록 밖·틀린 권한
    let (st, _, _) = http_get(&web, &format!("{base}..%2F..%2Fbrief.md"));
    assert_eq!(st, 400);
    assert_eq!(http_get(&web, &format!("{base}link")).0, 404);
    let (st, _, body) = http_get(&web, "/v/0000/out.txt");
    assert_eq!(st, 401, "{}", String::from_utf8_lossy(&body));
    // agent 쪽 파일이 사라졌으면 기다리지 않고 바로 알린다: 본문의 까닭이 agent의 답(없는 파일)이다.
    // 기다렸다면 「agent 응답 시간 초과」(15초)나 「연결이 끊겼다」가 온다(시간 단언 없이 구별한다)
    std::fs::remove_file(agent_lib.join("big.bin")).unwrap();
    let (st, _, body) = http_get(&web, &format!("{base}big.bin"));
    let body = String::from_utf8_lossy(&body);
    assert_eq!(st, 502, "{body}");
    assert!(body.contains("agent에 남긴 결과물을 읽지 못한다") && body.contains("(os error 2)"), "{body}");

    // 보고 뒤에 바뀐 library는 보고가 그대로여도 resolve의 마지막 사본에서 다시 받는다
    // (herdr-projects threads.rs final_copy → copy_home_*: hash와 상관없이 library를 다시 복사)
    std::fs::write(agent_lib.join("out.txt"), "나중 결과물\n").unwrap();
    std::fs::write(agent_lib.join("late.txt"), "늦게\n").unwrap();
    w.h.ok(&["thread", "resolve", "demo", &id]);
    let lib = w.h.ok(&["thread", "library", "demo", &id]);
    let base = lib["base"].as_str().unwrap().to_string();
    let (st, _, body) = http_get(&web, &format!("{base}out.txt"));
    assert_eq!((st, String::from_utf8_lossy(&body).into_owned()), (200, "나중 결과물\n".to_string()));
    let (st, _, body) = http_get(&web, &format!("{base}late.txt"));
    assert_eq!((st, body.as_slice()), (200, "늦게\n".as_bytes()));
    assert_eq!(http_get(&web, &format!("{base}big.bin")).0, 404, "지운 파일이 사본에 남았다");
    // 보고는 그대로라 report_ready는 다시 내지 않고, 바뀐 library만 알린다(바뀌지 않았으면 worktree_thread_round_trip처럼 없다)
    let kinds = w.kinds(0);
    assert_eq!(kinds.iter().filter(|k| *k == "report_ready").count(), 1, "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| *k == "library_copied").count(), 1, "{kinds:?}");
}

/// 알림 webhook을 받는 loopback 서버: 요청 줄과 본문을 넘긴다. `ok_first_only`면 두 번째 요청부터 실패(500)를 돌려준다.
fn webhook_server(ok_first_only: bool) -> (u16, std::sync::mpsc::Receiver<(String, String)>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
    std::thread::spawn(move || {
        for (n, conn) in listener.incoming().enumerate() {
            let Ok(mut c) = conn else { continue };
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut len = 0;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h == "\r\n" {
                    break;
                }
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0u8; len];
            r.read_exact(&mut body).unwrap();
            let status = if n == 0 || !ok_first_only { "200 OK" } else { "500 Internal Server Error" };
            write!(c, "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            if tx.send((line.trim().to_string(), String::from_utf8(body).unwrap())).is_err() {
                break;
            }
        }
    });
    (port, rx)
}

/// 알림 webhook(§8.7): 사람이 볼 항목을 debounce 동안 모아 프로젝트마다 한 통 보내고, 실패하면 다시 보내지 않는다.
#[test]
fn notify_webhook() {
    // 두 번째 요청부터는 실패를 돌려준다
    let (port, rx) = webhook_server(true);
    // 옛 종류 이름(report)도 thread 항목(thread-state)으로 읽는다(plan §8.85)
    let w = world_with(&format!(
        "[notify]\nwebhook = \"http://127.0.0.1:{port}/hook\"\ndebounce_secs = 2\nkinds = [\"report\", \"session\"]\n"
    ));
    let ids: Vec<String> = ["n1", "n2"]
        .iter()
        .map(|n| {
            w.start(&[
                "--title",
                n,
                "--profile",
                "fake",
                "--kind",
                "dir",
                "--task-file",
                &w.task(&format!("{n}.txt"), "x"),
            ])["id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    // 두 보고가 debounce 안에 오면 한 통이다. 항목은 만든 차례(id 끝 번호)다. 두 thread가 보고하는 차례는 시작 차례와
    // 다를 수 있다(부하가 크면 뒤에 띄운 thread가 brief를 먼저 받는다)
    let (line, body) = rx.recv_timeout(T).expect("알림");
    assert_eq!(line, "POST /hook HTTP/1.1");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["project"], "demo");
    // 항목은 들어온 순서(notify batch), 곧 inbox 항목을 만든 차례(id 끝 번호)다
    let items = v["items"].as_array().unwrap();
    let mut got: Vec<_> =
        items.iter().map(|i| (i["subject"].as_str().unwrap(), i["event"].as_str().unwrap())).collect();
    let first = got[0].0;
    got.sort();
    assert_eq!(got, [(ids[0].as_str(), "new report"), (ids[1].as_str(), "new report")], "{body}");
    let made: Vec<u64> =
        items.iter().map(|i| i["id"].as_str().unwrap().rsplit('-').next().unwrap().parse().unwrap()).collect();
    assert!(made[0] < made[1], "{body}");
    let text = v["text"].as_str().unwrap();
    assert!(text.starts_with(&format!("[aaa] demo\n- {first} new report")), "{body}");
    assert!(text.contains("\n- t-0001 new report") && text.contains("\n- t-0002 new report"), "{body}");
    wait_for("notify_sent", T, || w.kinds(0).contains(&"notify_sent".to_string()).then_some(()));
    // 보고 없이 쉬던 thread의 세션이 끝나면 사람 차례(pane closed)라 알린다. 쉼(idle)은 알리지 않는다.
    // 받는 쪽이 실패하면 기록만 하고 다시 보내지 않는다.
    let n3 =
        w.start(&["--title", "n3", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("n3.txt", "noreport")]);
    let (n3, pane3) = (n3["id"].as_str().unwrap().to_string(), n3["pane"].as_str().unwrap().to_string());
    w.wait_screen(&pane3, "done");
    w.h.ok(&["pane", "kill", &pane3]);
    let (_, body) = rx.recv_timeout(T).expect("pane closed 알림");
    let v: Value = serde_json::from_str(&body).unwrap();
    let got: Vec<_> = v["items"].as_array().unwrap().iter().map(|i| i["event"].as_str().unwrap()).collect();
    assert_eq!(got, ["pane closed"], "{body}");
    assert!(body.contains(&format!("- {n3} pane closed: {n3} \\\"n3\\\": 사람 차례(pane closed)")), "{body}");
    wait_for("notify_failed", T, || w.kinds(0).contains(&"notify_failed".to_string()).then_some(()));
    // 실패한 알림을 다시 보내지 않는다: 다음 알림(n4 pane closed)을 일으키면 받은 다음 요청에 그 항목만 있다. 다시 보냈다면
    // 실패한 항목이 그보다 먼저(따로 또는 같은 묶음으로) 온다(알림은 한 줄로 차례대로 보낸다, notify.rs `start_notifier`)
    let n4 =
        w.start(&["--title", "n4", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("n4.txt", "noreport")]);
    let (n4, pane4) = (n4["id"].as_str().unwrap().to_string(), n4["pane"].as_str().unwrap().to_string());
    w.wait_screen(&pane4, "done");
    w.h.ok(&["pane", "kill", &pane4]);
    let (_, body) = rx.recv_timeout(T).expect("n4 pane closed 알림");
    let v: Value = serde_json::from_str(&body).unwrap();
    let got: Vec<_> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["subject"].as_str().unwrap(), i["event"].as_str().unwrap()))
        .collect();
    assert_eq!(got, [(n4.as_str(), "pane closed")], "실패한 알림을 다시 보냈다: {body}");
    let failed = w.h.ok(&["events", "demo"])["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "notify_failed")
        .unwrap()
        .clone();
    assert_eq!(failed["detail"], "HTTP 500");
    // 주소(비밀값이 들어갈 수 있다)는 기록에 남지 않는다
    assert!(!w.h.ok(&["events", "demo"]).to_string().contains("/hook"));
}

/// 머신 건강(§8.10): 디스크 부족·toolchain 실패면 새 세션을 거부하고 inbox에 남긴다. 고치고 다시 띄우면 풀린다.
#[test]
fn machine_health() {
    let mut w = world();
    let t = w.start(&["--title", "h", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("h.txt", "x")]);
    let id = t["id"].as_str().unwrap().to_string();
    w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    let base = agent_toml(&w.lab.root.join("workA"), &w.scripts);
    let restart = |w: &mut World, health: &str| {
        w.lab.kill("agentA");
        std::fs::write(w.h.cfg.join("agent.toml"), format!("{base}{health}")).unwrap();
        w.lab.spawn("agentA", &w.h.clone(), &["agent", "--host", "local", "--name", "labA"]);
    };
    // 1EB가 필요하다고 하고, 실패하는 toolchain 명령을 둔다
    restart(
        &mut w,
        "[health]\nevery_s = 1\nmin_free_gb = 1073741824\n[health.toolchain]\n\
         ok = [\"/bin/sh\", \"-c\", \"echo tool 9.9\"]\nbad = [\"/bin/sh\", \"-c\", \"echo broken >&2; exit 2\"]\n",
    );
    let m =
        wait_for("건강 이상", T, || w.h.machine("labA").filter(|m| !m["problems"].as_array().unwrap().is_empty()));
    let problems = m["problems"].to_string();
    assert!(
        problems.contains("디스크 여유") && problems.contains("toolchain bad 실패") && problems.contains("broken"),
        "{m}"
    );
    let tools = &m["health"]["toolchain"];
    assert_eq!((tools[1]["name"].as_str(), tools[1]["version"].as_str()), (Some("ok"), Some("tool 9.9")), "{m}");
    assert!(m["clock_skew_ms"].as_i64().unwrap().abs() < 5_000, "{m}");
    // 새 세션·thread는 거부한다
    let e = w.h.json(&["tab", "create", "--machine", "labA", "--", "/bin/sh"]).expect_err("점검 이상");
    assert!(e.contains("점검 이상") && e.contains("toolchain bad"), "{e}");
    let e =
        w.h.json(&[
            "thread",
            "start",
            "demo",
            "--title",
            "x",
            "--profile",
            "fake",
            "--kind",
            "dir",
            "--task-file",
            &w.task("x.txt", "x"),
        ])
        .expect_err("점검 이상");
    assert!(e.contains("점검 이상"), "{e}");
    let st = String::from_utf8_lossy(&w.h.run(&["status"]).stdout).to_string();
    assert!(st.contains("이상: 디스크 여유"), "{st}");
    // 열린 thread가 있는 프로젝트 inbox에 한 번
    let health = |w: &World| -> Vec<(String, String)> {
        w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["item_kind"] == "health")
            .map(|i| (i["event"].as_str().unwrap().to_string(), i["summary"].as_str().unwrap().to_string()))
            .collect()
    };
    let items = wait_for("inbox 이상 항목", T, || Some(health(&w)).filter(|v| !v.is_empty()));
    assert!(items.len() == 1 && items[0].0 == "problem" && items[0].1.contains("labA"), "{items:?}");
    // 같은 이상의 새 점검(at_ms)이 오고 ticker가 그것을 본 뒤에도 하나다
    let at = |w: &World| w.h.machine("labA").and_then(|m| m["health"]["at_ms"].as_u64());
    let seen = at(&w).expect("health at_ms");
    wait_for("다음 점검", T, || (at(&w) > Some(seen)).then_some(()));
    w.h.ticks(1);
    assert_eq!(health(&w).len(), 1, "같은 이상은 다시 남기지 않는다");

    // 고치고 다시 띄우면 풀린다
    restart(&mut w, "[health]\nevery_s = 1\nmin_free_gb = 0\n");
    wait_for("건강 회복", T, || {
        w.h.machine("labA").filter(|m| m["health"].is_object() && m["problems"].as_array().unwrap().is_empty())
    });
    let items = wait_for("inbox 회복 항목", T, || Some(health(&w)).filter(|v| v.len() == 2));
    assert_eq!(items[1].0, "ok", "{items:?}");
    w.h.ok(&["tab", "create", "--machine", "labA", "--", "/bin/sh"]);
}

/// coordinator 일 흐름(부록 A): TASKS.md owner로 thread 시작, assignable 확인, Next 목록·보내기, context 요약.
#[test]
fn coordinator_flow() {
    let w = world();
    let pdir = w.projects.join("demo");
    std::fs::write(
        pdir.join("TASKS.md"),
        "# Tasks\n\n## Backlog\n- [ ] 로그인 고치기 (fake@labA)\n  - 메모: 테스트 먼저\n- [ ] 배포 (me)\n\
         - [ ] 사람 일 (Priya Rao)\n- [ ] 미할당 일\n",
    )
    .unwrap();
    // 맡길 수 있는 owner: host 허용 목록 ∩ 그 머신 profile
    let a = w.h.ok(&["assignable", "demo"]);
    let la = a["owners"].as_array().unwrap().iter().find(|o| o["machine"] == "labA").unwrap().clone();
    assert_eq!(
        la["profiles"],
        serde_json::json!(["asker", "claude", "evil", "fake", "mute", "paste", "prog", "resumable", "sticky"]),
        "{a}"
    );
    w.h.ok(&["assignable", "demo", "--check", "fake@labA"]);
    for (bad, why) in
        [("other@labA", "쓸 수 있는 profile이 아니다"), ("Priya Rao", "사람"), ("@nowhere", "모르는 머신")]
    {
        let e = w.h.json(&["assignable", "demo", "--check", bad]).expect_err(bad);
        assert!(e.contains(why), "{bad}: {e}");
    }
    // TASKS.md task로 시작: owner가 profile·machine, 메모가 할 일에 붙는다
    for (title, why) in
        [("배포", "me"), ("사람 일", "사람"), ("미할당 일", "--profile"), ("없는 일", "TASKS.md에 없는")]
    {
        let e = w.h.json(&["thread", "start", "demo", "--from-task", title, "--kind", "dir"]).expect_err(title);
        assert!(e.contains(why), "{title}: {e}");
    }
    let t = w.h.ok(&["thread", "start", "demo", "--from-task", "로그인 고치기", "--kind", "dir"]);
    let id = t["id"].as_str().unwrap().to_string();
    assert_eq!(
        (t["title"].as_str(), t["profile"].as_str(), t["machine"].as_str()),
        (Some("로그인 고치기"), Some("fake"), Some("labA"))
    );
    let task = std::fs::read_to_string(pdir.join("threads").join(format!("{id}.task.md"))).unwrap();
    assert!(task.contains("## Notes from the task list") && task.contains("메모: 테스트 먼저"), "{task}");
    // coordinator가 ` · t-…`를 붙인다(같은 task를 다시 맡기면 막지 않고 경고한다: `start_defaults_profile_kind_and_task`)
    let md =
        std::fs::read_to_string(pdir.join("TASKS.md")).unwrap().replace("(fake@labA)", &format!("(fake@labA) · {id}"));
    std::fs::write(pdir.join("TASKS.md"), md).unwrap();

    // Next: 보고서 `## Next` + 더한 줄, N번째 줄을 thread에 보낸다
    // `--line`은 메시지라 brief를 받은 뒤에만 간다: host는 brief를 친 뒤 다음 화면 보기(250ms)에서야 delivered로 적는데,
    // 가짜 agent는 brief 줄을 읽자마자 보고를 쓴다(부하에서 「아직 brief를 받지 않았다」, 다른 시험의 `ready`와 같은 기다림)
    let report_next =
        w.wait_show("보고", &id, |v| v["report_hash"].is_string() && v["brief"] == "delivered")["next"].clone();
    let mut want: Vec<Value> = report_next.as_array().unwrap().clone();
    assert!(!want.is_empty(), "가짜 agent 보고에는 Next가 있다");
    assert_eq!(w.h.ok(&["thread", "next", "demo", &id])["lines"], report_next);
    want.push("두 번째 일".into());
    let v = w.h.ok(&["thread", "next", "demo", &id, "--add", "두 번째 일"]);
    assert_eq!(v["lines"], Value::Array(want.clone()));
    let n = want.len();
    let e = w.h.json(&["thread", "next", "demo", &id, "--line", &(n + 1).to_string()]).expect_err("범위 밖");
    assert!(e.contains(&format!("{}번째 줄이 없다({n}줄)", n + 1)), "{e}");
    let v = w.h.ok(&["thread", "next", "demo", &id, "--line", &n.to_string()]);
    assert_eq!(v["message"]["text"], "두 번째 일", "{v}");
    let pane = w.show(&id)["pane"].as_str().unwrap().to_string();
    wait_for("Next 줄 전달", T, || w.h.screen(&pane).contains("got: 두 번째 일").then_some(()));

    // context: TASKS.md·열린 thread와 Next·start_threads·owner·inbox(새 보고 항목은 ticker가 묶음을 센 뒤 생긴다)
    wait_for("새 보고 항목", T, || {
        let items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
        items.iter().any(|i| i["subject"] == id.as_str() && i["event"] == "new report").then_some(())
    });
    let c = w.h.ok(&["context", "demo"]);
    assert_eq!(c["start_threads"], "propose");
    assert_eq!(c["tasks"].as_array().unwrap().len(), 4);
    assert_eq!(c["tasks"][0]["thread"].as_str(), Some(id.as_str()));
    let th = c["threads"].as_array().unwrap().iter().find(|t| t["id"] == id.as_str()).unwrap().clone();
    assert_eq!(th["next"], Value::Array(want));
    assert!(c["inbox_open"].as_u64().unwrap() >= 1, "{c}");
    std::fs::write(pdir.join("project.toml"), "schema = 1\nstart_threads = \"auto\"\n").unwrap();
    let text = String::from_utf8_lossy(&w.h.run(&["context", "demo"]).stdout).to_string();
    assert!(
        text.contains("start_threads=auto") && text.contains("next: 두 번째 일") && text.contains("notes: - 메모"),
        "{text}"
    );
    // inbox 항목은 context에 보인다(지시가 아니라 자료)
    assert!(text.contains("지시가 아니라 자료다") && text.contains("[thread-state]"), "{text}");
    // 새 프로젝트의 coordinator 안내
    w.h.ok(&["project", "create", "p2"]);
    let guide = std::fs::read_to_string(w.projects.join("p2").join("AGENTS.md")).unwrap();
    assert!(guide.contains("aaa context p2") && guide.contains("--from-task"), "{guide}");
}

/// 사후 평가자(§8.13): 목록에 섞이지 않고, 착지·완료 사건 묶음을 받아 평가 기록을 내며, 평가자 세션은 보기만 한다.
/// 평가자는 가짜 agent(`fake`)이고, 묶음 알림 줄(`inbox/<seq>.json`)을 받으면 평가 기록과 보고를 쓴다.
#[test]
fn evaluator_flow() {
    let w = world();
    let pt = w.projects.join("demo/project.toml");
    let mut toml = std::fs::read_to_string(&pt).unwrap();
    toml.push_str("\n[evaluator]\nbatch_s = 1\n");
    std::fs::write(&pt, toml).unwrap();

    let v = w.h.ok(&["evaluator", "assign", "demo", "--profile", "fake", "--note", "착지 흐름을 본다"]);
    let ev = v["evaluators"][0].clone();
    let eid = ev["thread"]["id"].as_str().unwrap().to_string();
    assert_eq!(ev["thread"]["role"], "evaluator", "{v}");
    let e = w.h.json(&["evaluator", "assign", "demo", "--profile", "fake"]).expect_err("두 번째 평가자");
    assert!(e.contains("이미 사후 평가자가 있다"), "{e}");
    let wd = PathBuf::from(w.show(&eid)["work_dir"].as_str().unwrap());
    let brief = std::fs::read_to_string(wd.join("brief.md")).unwrap();
    assert!(brief.contains("# 사후 평가자 규칙") && !brief.contains("# thread brief"), "{brief}");
    assert!(brief.contains("착지 흐름을 본다"), "{brief}");
    w.wait_show("평가자 brief", &eid, |v| v["brief"] == "delivered");

    // 목록에 섞이지 않는다: thread·pane 목록과 context에는 없고 role로만 보인다
    let ids = |v: &Value, key: &str| -> Vec<String> {
        v[key].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect()
    };
    assert!(!ids(&w.h.ok(&["thread", "list", "demo"]), "threads").contains(&eid));
    assert_eq!(ids(&w.h.ok(&["thread", "list", "--role", "evaluator"]), "threads"), std::slice::from_ref(&eid));
    let pane = w.show(&eid)["pane"].as_str().unwrap().to_string();
    assert!(!ids(&w.h.ok(&["pane", "list"]), "panes").contains(&pane));
    assert_eq!(ids(&w.h.ok(&["pane", "list", "--role", "evaluator"]), "panes"), std::slice::from_ref(&pane));
    let c = w.h.ok(&["context", "demo"]);
    assert!(c["threads"].as_array().unwrap().is_empty(), "{c}");
    assert_eq!(c["evaluator"]["thread"]["id"], eid.as_str());

    // 평가자 세션 안의 CLI는 보기만 한다: 다른 thread에 지시·채택이 거부되고 보기는 된다
    let as_eval = |args: &[&str]| w.h.cmd(args).env("AAA_SESSION_ID", &pane).output().unwrap();
    for args in [
        &["thread", "message", "demo", "t-0001", "--text", "x"][..],
        &["evaluator", "unassign", "demo"],
        &["thread", "start", "demo", "--title", "x", "--profile", "fake", "--kind", "dir", "--task-file", "/dev/null"],
    ] {
        let o = as_eval(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", String::from_utf8_lossy(&o.stdout));
        assert!(String::from_utf8_lossy(&o.stderr).contains("보기 권한"), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let o = as_eval(&["--json", "evaluator", "status", "demo"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    // 일반 thread가 끝나면 묶음이 평가자 작업 폴더 inbox/에 생긴다(PR 병합 `pr_merged`는 바로 보낸다)
    let t =
        w.start(&["--title", "work", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("e1.txt", "X-77")]);
    let tid = t["id"].as_str().unwrap().to_string();
    w.wait_show("보고", &tid, |v| v["report_hash"].is_string());
    w.h.ok(&["thread", "resolve", "demo", &tid]);
    let inbox = wd.join("inbox");
    let batches = |n: usize| {
        wait_for_diag(
            &format!("사건 묶음 {n}개"),
            T,
            || {
                let mut v: Vec<PathBuf> = std::fs::read_dir(&inbox)
                    .ok()?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == "json"))
                    .collect();
                v.sort();
                (v.len() >= n).then_some(v)
            },
            || format!("evaluator: {}\nevents: {}", w.h.ok(&["evaluator", "status", "demo"]), w.kinds(0).join(",")),
        )
    };
    // 완료는 1초 뒤 묶음이다. 완료까지 넘어갈 때까지 기다린다.
    let all_kinds = |files: &[PathBuf]| -> (Vec<String>, Vec<Value>) {
        let mut kinds = Vec::new();
        let mut docs = Vec::new();
        for f in files {
            let b: Value = serde_json::from_slice(&std::fs::read(f).unwrap()).unwrap();
            for e in b["events"].as_array().unwrap() {
                assert_ne!(e["role"], "evaluator", "평가자 자신의 사건이 들어갔다: {e}");
                kinds.push(e["kind"].as_str().unwrap().to_string());
            }
            docs.push(b);
        }
        (kinds, docs)
    };
    let files = wait_for_diag(
        "완료 사건까지 넘김",
        T,
        || {
            let f = batches(1);
            all_kinds(&f).0.contains(&"thread_resolved".to_string()).then_some(f)
        },
        || w.h.ok(&["evaluator", "status", "demo"]).to_string(),
    );
    let (kinds, docs) = all_kinds(&files);
    for k in ["thread_started", "report_ready", "thread_resolved"] {
        assert!(kinds.contains(&k.to_string()), "{k} 없음: {kinds:?}");
    }
    // 묶음은 seq가 겹치지 않고, thread 이력·시간 요약·보고 본문을 담는다
    for pair in docs.windows(2) {
        assert!(pair[0]["to_seq"].as_u64() < pair[1]["from_seq"].as_u64(), "묶음이 겹친다");
    }
    let last = docs.last().unwrap();
    let th = last["threads"].as_array().unwrap().iter().find(|t| t["id"] == tid.as_str()).unwrap();
    assert!(th["summary"]["started_at_ms"].is_u64() && th["summary"]["resolved_ms"].is_u64(), "{th}");
    assert!(th["report"].as_str().unwrap().contains("X-77"), "{th}");

    // 평가 기록을 받는다: 목록·inbox 항목·잘못된 기록은 한 번만 알림
    let first = files[0].file_stem().unwrap().to_str().unwrap().to_string();
    let eval_id = format!("e-{first}");
    let got = wait_for_diag(
        "평가 기록",
        T,
        || {
            let v = w.h.ok(&["evaluation", "list", "demo"]);
            v["evaluations"].as_array().unwrap().iter().find(|e| e["id"] == eval_id.as_str()).cloned()
        },
        || format!("inbox: {}\nevents: {}", w.h.ok(&["inbox", "list", "demo", "--peek"]), w.kinds(0).join(",")),
    );
    assert_eq!(got["evaluator"], eid.as_str());
    assert_eq!(got["doc"]["stages"][0]["minutes"], 1, "소수 분은 반올림");
    assert!(w.projects.join("demo").join(got["file"].as_str().unwrap()).is_file());
    let inbox_items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    let evals: Vec<&Value> = inbox_items.iter().filter(|i| i["item_kind"] == "evaluation").collect();
    assert!(evals.iter().any(|i| i["subject"] == eval_id.as_str() && i["event"] == "evaluation"), "{inbox_items:?}");
    assert_eq!(evals.iter().filter(|i| i["event"] == "invalid").count(), 1, "{inbox_items:?}");
    assert!(
        !inbox_items.iter().any(|i| i["item_kind"] == "thread-state" && i["subject"] == eid.as_str()),
        "평가자 보고가 thread 항목이 됐다"
    );

    // 채택·보류는 평가 기록에 남는다(뒤 thread가 알아야 하면 coordinator가 memory에 적는다, G9). 보류까지 정하면 inbox
    // 항목이 끝난다
    let a = w.h.ok(&["evaluation", "adopt", "demo", &eval_id, "p1", "--note", "다음 주"]);
    let dc = &a["doc"]["proposals"][0]["decision"];
    assert_eq!((dc["adopted"].as_bool(), dc["note"].as_str()), (Some(true), Some("다음 주")), "{a}");
    assert!(dc.get("decision_num").is_none(), "{a}");
    let e = w.h.json(&["evaluation", "adopt", "demo", &eval_id, "p1"]).expect_err("두 번 채택");
    assert!(e.contains("이미 채택"), "{e}");
    let e = w.h.json(&["evaluation", "hold", "demo", &eval_id, "p9"]).expect_err("없는 개선안");
    assert!(e.contains("없는 개선안"), "{e}");
    w.h.ok(&["evaluation", "hold", "demo", &eval_id, "p2"]);
    let left = w.h.ok(&["evaluation", "list", "demo"]);
    assert!(!left["evaluations"].as_array().unwrap().iter().any(|e| e["id"] == eval_id.as_str()), "{left}");
    let open = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    assert!(!open.iter().any(|i| i["subject"] == eval_id.as_str() && i["event"] == "evaluation"), "{open:?}");

    // 평가자를 다시 띄워도 넘긴 사건은 다시 넘기지 않는다: 다음 묶음은 그 뒤 seq부터
    let before = w.h.ok(&["evaluator", "status", "demo"])["evaluators"][0].clone();
    w.h.ok(&["pane", "kill", &pane]);
    let eval_pane = || -> Value {
        w.h.ok(&["pane", "list", "--role", "evaluator"])["panes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == pane.as_str())
            .cloned()
            .unwrap()
    };
    wait_for("평가자 종료", T, || (eval_pane()["state"] != "running").then_some(()));
    let st = w.h.ok(&["evaluator", "status", "demo"])["evaluators"][0].clone();
    assert_eq!(st["running"], false, "{st}");
    w.h.ok(&["thread", "restart", "demo", &eid]);
    w.wait_show("평가자 다시 brief", &eid, |v| v["brief"] == "delivered");
    let t2 =
        w.start(&["--title", "work2", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("e2.txt", "y")]);
    let t2id = t2["id"].as_str().unwrap().to_string();
    w.wait_show("보고", &t2id, |v| v["report_hash"].is_string());
    w.h.ok(&["thread", "resolve", "demo", &t2id]);
    let files2 = batches(files.len() + 1);
    let (_, docs2) = all_kinds(&files2);
    let newest = docs2.last().unwrap();
    assert!(newest["from_seq"].as_u64().unwrap() > before["cursor"].as_u64().unwrap(), "{newest}");
    assert!(
        newest["events"].as_array().unwrap().iter().all(|e| e["thread"] != tid.as_str()),
        "앞 thread 사건이 다시 왔다"
    );

    // 끝내기: 상태에서 사라진다
    w.h.ok(&["evaluator", "unassign", "demo"]);
    assert!(w.h.ok(&["evaluator", "status", "demo"])["evaluators"].as_array().unwrap().is_empty());
}

/// 가짜 `gh`: 받은 인자를 남기고 `$GH_STUB`의 파일로 답한다. 계정은 thread-bot이다.
/// `pr list`는 `--author`(내 열린 PR)면 open.json, `--head=<branch>`의 list-<branch, /는 _>.json이 있으면 그것, 없으면
/// list.json이다. `pr view`는 view-<PR 번호>.json이 있으면 그것, 없으면 view.json이다.
const FAKE_GH: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$GH_STUB/calls.log"
case "$1 $2" in
  "pr list")
    case "$*" in *--author*) cat "$GH_STUB/open.json"; exit ;; esac
    head=$(printf '%s\n' "$@" | sed -n 's/^--head=//p' | tr '/' '_')
    if [ -n "$head" ] && [ -f "$GH_STUB/list-$head.json" ]; then cat "$GH_STUB/list-$head.json"; else cat "$GH_STUB/list.json"; fi ;;
  "pr view")
    n=$(printf '%s\n' "$@" | sed -n 's#.*/pull/##p')
    if [ -n "$n" ] && [ -f "$GH_STUB/view-$n.json" ]; then cat "$GH_STUB/view-$n.json"; else cat "$GH_STUB/view.json"; fi ;;
  "api user") echo thread-bot ;;
  *) echo "모르는 gh 명령: $*" >&2; exit 1 ;;
esac
"#;

/// 실험실 `root/gh`에 가짜 gh를 두고 agent가 그것을 먼저 찾게 한다(`world_setup`의 `agent_env`)
fn fake_gh(root: &Path) -> Vec<(String, String)> {
    let stub = root.join("gh");
    std::fs::create_dir_all(stub.join("bin")).unwrap();
    let gh = stub.join("bin/gh");
    std::fs::write(&gh, FAKE_GH).unwrap();
    std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    for (name, body) in [("list.json", "[]"), ("view.json", "{}"), ("open.json", "[]")] {
        std::fs::write(stub.join(name), body).unwrap();
    }
    let path = format!("{}:{}", path_str(&stub.join("bin")), std::env::var("PATH").unwrap_or_default());
    vec![("PATH".into(), path), ("GH_STUB".into(), path_str(&stub).to_string())]
}

/// `gh pr view` 답(본문 없음). `head`는 PR head commit이다.
fn pr_view_json(branch: &str, state: &str, review: &str, head: &str) -> String {
    format!(
        r#"{{"state":"{state}","reviewDecision":"{review}","headRefName":"{branch}","headRefOid":"{head}",
           "headRepository":{{"name":"app"}},"headRepositoryOwner":{{"login":"o"}}}}"#
    )
}

/// PR 추적(§8.80, herdr-projects steps.rs `pull_requests`·`pr_events`·`fire_pr_routines`): thread 머신 agent의 `gh`로
/// branch의 PR을 찾아 보고, 바뀌면 `pr` 항목과 흐름 사건을 남기고, 켜진 `on = "pr"` routine이 원하는 사건이면 thread에
/// 글을 보낸다. thread 자기 계정의 댓글은 review가 아니다. 꺼진 pr routine은 오류도 발화도 없고(t-0334 결함 5),
/// `routine toggle --on`으로 켜면 다음 사건부터 보낸다. 보고 첫 줄 `PR:`이 틀리면 알린다.
#[test]
fn pr_followup() {
    let mut w = world_setup("tick_secs = 1\npr_poll_secs = 1\n", false, fake_gh);
    let stub = w.lab.root.join("gh");
    let mut f = w.lab.follow("follow", &w.h, "demo");
    let dir = w.projects.join("demo");
    let put = |name: &str, body: &str| std::fs::write(dir.join("routines").join(format!("{name}.md")), body).unwrap();

    // 새 프로젝트에는 기본 pr-followup이 있다. 꺼진 pr routine과 함께 두고, 없는 trigger 파일이 알려지는 것으로 한 번 돈 것을 안다.
    put(
        "landing",
        "+++\n# 병합 확인\non = \"pr\"\nevents = [\"checks-failed\", \"merged\"]\nenabled = false\n+++\n병합을 확인한다\n",
    );
    put("broken", "+++\non = \"slack\"\n+++\n");
    f.until("routine_broken", T, |e| e["kind"] == "routine_broken" && e["evidence"] == "routines/broken.md");
    let errors: Vec<Value> = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["item_kind"] == "config-error")
        .cloned()
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["subject"], "broken");
    let list = String::from_utf8_lossy(&w.h.run(&["routine", "list", "demo"]).stdout).to_string();
    assert!(list.contains("landing\ton pr: checks-failed, merged\tthread에 보냄 · 꺼짐\n"), "{list}");
    assert!(list.contains("pr-followup\ton pr: checks-failed, review\tthread에 보냄\n"), "{list}");

    let task = w.task("pr.md", "PR을 연다");
    let id = w.start(&["--title", "PR", "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task])["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.until("report_ready", T, |e| e["kind"] == "report_ready" && e["thread"] == id.as_str());
    let t = w.show(&id);
    let (branch, pane) = (t["branch"].as_str().unwrap().to_string(), t["pane"].as_str().unwrap().to_string());
    assert!(t["pr"].is_null(), "origin이 GitHub가 아니면 묻지 않는다: {t}");

    // origin이 GitHub이면 branch로 찾는다(열린 PR)
    let url = "https://github.com/o/app/pull/7";
    let view = |extra: &str| {
        let body = format!(
            r#"{{"state":"OPEN","reviewDecision":"","headRefName":"{branch}","headRefOid":"abc",
               "headRepository":{{"name":"app"}},"headRepositoryOwner":{{"login":"o"}}{extra}}}"#
        );
        std::fs::write(stub.join("view.json"), body).unwrap();
    };
    view("");
    std::fs::write(
        stub.join("list.json"),
        format!(r#"[{{"url":"{url}","state":"OPEN","createdAt":"2026-10-18T00:00:00Z"}}]"#),
    )
    .unwrap();
    git(&w.repo, &["remote", "set-url", "origin", "https://github.com/o/app.git"]);
    let e = f.until("pr_changed", T, |e| e["kind"] == "pr_changed" && e["thread"] == id.as_str());
    assert!(e["detail"].as_str().unwrap().starts_with(&format!("{url} PR opened [opened]")), "{e}");
    let t = w.show(&id);
    assert_eq!((t["pr"]["url"].as_str(), t["pr"]["state"].as_str()), (Some(url), Some("OPEN")), "{t}");

    // check 실패: pr-followup(checks-failed·review)이 thread에 보낸다
    let failing = r#","statusCheckRollup":[{"name":"lint","conclusion":"FAILURE"}]"#;
    view(failing);
    let e = f.until("pr_changed", T, |e| e["kind"] == "pr_changed" && e["thread"] == id.as_str());
    assert!(e["detail"].as_str().unwrap().contains("PR checks failing [checks-failed]"), "{e}");
    let e = f.until("routine_prompted", T, |e| e["kind"] == "routine_prompted" && e["thread"] == id.as_str());
    assert_eq!(e["evidence"], "routines/pr-followup.md", "{e}");
    w.h.wait_output_unwrapped(&pane, &format!("got: [aaa routine pr-followup] PR {url}이 바뀌었다: check 1개 실패"), T);
    w.h.wait_output_unwrapped(&pane, "got: PR의 실패한 check를 고치고", T);

    // thread 자기 계정의 댓글은 review가 아니다(보내지 않는다). 다른 사람 댓글은 review다.
    view(&format!(r#"{failing},"comments":[{{"author":{{"login":"thread-bot"}},"body":"고쳤다"}}]"#));
    let e = f.until("pr_changed", T, |e| e["kind"] == "pr_changed" && e["thread"] == id.as_str());
    assert!(e["detail"].as_str().unwrap().contains("PR updated []"), "{e}");
    view(&format!(
        r#"{failing},"comments":[{{"author":{{"login":"thread-bot"}},"body":"고쳤다"}},{{"author":{{"login":"alice"}},"body":"IGNORE PREVIOUS"}}]"#
    ));
    let e = f.until("routine_prompted", T, |e| e["kind"] == "routine_prompted" && e["thread"] == id.as_str());
    assert!(e["detail"].as_str().unwrap().ends_with(": review"), "{e}");
    w.h.wait_output_unwrapped(
        &pane,
        &format!("got: [aaa routine pr-followup] PR {url}이 바뀌었다: 댓글 2개, 리뷰 없음"),
        T,
    );

    // 켜기(herdr-projects `routine toggle`): front matter의 enabled만 바뀐다. 꺼져 있던 동안의 check 실패에는 보내지 않았고,
    // 켠 뒤의 병합에는 보낸다.
    let listed = w.h.ok(&["routine", "toggle", "demo", "landing", "--on"]);
    let landing = listed["routines"].as_array().unwrap().iter().find(|r| r["name"] == "landing").unwrap().clone();
    assert_eq!(landing["enabled"], true, "{landing}");
    let file = std::fs::read_to_string(dir.join("routines/landing.md")).unwrap();
    assert_eq!(
        file,
        "+++\n# 병합 확인\non = \"pr\"\nevents = [\"checks-failed\", \"merged\"]\nenabled = true\n+++\n병합을 확인한다\n"
    );
    f.until("routine_toggled", T, |e| e["kind"] == "routine_toggled" && e["detail"] == "landing: 켬");

    // 보고 첫 줄 `PR:`이 틀리면 알린다(그 보고에 한 번). 보고 사본·Next는 그대로 받고, 전에 찾은 PR을 계속 따라간다.
    // 병합 전에 쓴다: 병합 뒤의 새 보고는 끝 신호라 thread를 끝낸다(merged_pr_resolves_the_thread).
    let wd = PathBuf::from(w.show(&id)["work_dir"].as_str().unwrap());
    std::fs::write(
        wd.join("report.md"),
        "PR: https://evil.example/o/app/pull/7\n[진행 중]\n\n## Report\n병합 전\n\n## Next\n- 확인\n",
    )
    .unwrap();
    let e = f.until("pr_line_bad", T, |e| e["kind"] == "pr_line_bad" && e["thread"] == id.as_str());
    assert_eq!(e["evidence"], format!("threads/{id}.md"));
    std::fs::write(
        stub.join("view.json"),
        std::fs::read_to_string(stub.join("view.json")).unwrap().replace(r#""state":"OPEN""#, r#""state":"MERGED""#),
    )
    .unwrap();
    let e = f.until("pr_changed", T, |e| e["kind"] == "pr_changed" && e["thread"] == id.as_str());
    assert!(e["detail"].as_str().unwrap().contains("PR merged [merged]"), "{e}");
    let e = f.until("routine_prompted", T, |e| e["kind"] == "routine_prompted" && e["thread"] == id.as_str());
    assert_eq!(
        (e["evidence"].as_str(), e["detail"].as_str().map(|d| d.ends_with(": merged"))),
        (Some("routines/landing.md"), Some(true)),
        "{e}"
    );
    w.h.wait_output_unwrapped(&pane, &format!("got: [aaa routine landing] PR {url}이 바뀌었다: 병합됐다."), T);
    let t = w.show(&id);
    assert_eq!((t["next"].clone(), t["pr"]["state"].as_str()), (serde_json::json!(["확인"]), Some("MERGED")), "{t}");
    let shown = String::from_utf8_lossy(&w.h.run(&["thread", "show", "demo", &id]).stdout).to_string();
    assert!(shown.contains(&format!("  PR {url} (MERGED)\n")), "{shown}");

    let events = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().clone();
    let prompted: Vec<&Value> = events.iter().filter(|e| e["kind"] == "routine_prompted").collect();
    let by: Vec<&str> = prompted.iter().map(|e| e["evidence"].as_str().unwrap()).collect();
    assert_eq!(by, ["routines/pr-followup.md", "routines/pr-followup.md", "routines/landing.md"], "{prompted:?}");
    let items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    let pr_events: Vec<&str> =
        items.iter().filter(|i| i["item_kind"] == "pr").map(|i| i["event"].as_str().unwrap()).collect();
    assert_eq!(
        pr_events,
        ["PR opened", "PR checks failing", "PR updated", "PR review activity", "bad PR: line", "PR merged"],
        "{items:?}"
    );
    assert!(!serde_json::to_string(&items).unwrap().contains("IGNORE PREVIOUS"), "댓글 본문은 남기지 않는다");
    // gh 인자: branch로 찾고, 주소는 `--` 뒤에
    let calls = std::fs::read_to_string(stub.join("calls.log")).unwrap();
    assert!(
        calls.contains(&format!(
            "pr list --repo o/app --head={branch} --state all --limit 20 --json url,state,createdAt\n"
        )),
        "{calls}"
    );
    assert!(calls.contains(&format!("pr view --json state,reviewDecision,statusCheckRollup,comments,reviews,headRefName,headRefOid,headRepository,headRepositoryOwner -- {url}\n")), "{calls}");
    assert!(calls.contains("api user --jq .login\n"), "{calls}");
}

/// 웹 새 세션 창의 agent 고르기(herdr-web-ui NewSessionDialog, plan §8.126): `pane_run`에 profile을 주면 그 머신 agent.toml
/// profile로 harness를 띄우고 thread처럼 상태 hook을 붙인다(hook 설정은 세션 폴더에). 없는 profile·폴더는 거부, `~/…`는 집으로 푼다.
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

/// thread-state 항목(사건, 요약) 중 이 thread 것
fn state_items(w: &World, id: &str) -> Vec<(String, String)> {
    w.h.ok(&["inbox", "list", "demo", "--peek"])["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["item_kind"] == "thread-state" && i["subject"] == id)
        .map(|i| (i["event"].as_str().unwrap().to_string(), i["summary"].as_str().unwrap().to_string()))
        .collect()
}

/// 병합(D11, herdr-projects 0.2.35 thread.rs `group` 5번 줄, steps.rs `resolve_merged`·`merged_thread_may_resolve`·
/// `other_pr_open`, threads.rs `clean`·`delete_branch`, scenarios.rs 병합 시험). aaa는 병합하지 않는다: thread가 자기 도구로
/// PR을 병합하고 aaa는 상태만 따라간다(gh는 사용자 환경의 것, 여기서는 가짜).
/// - 열리고 승인된 PR은 「병합 중」이고 전이 항목 `landing`을 낸다.
/// - 병합돼도 진행을 보고하는 agent가 100 아래인 동안은 끝내지 않는다. 끝낸 보고와 `Done`이면 마지막 사본 뒤 끝낸다.
/// - 진행을 보고하지 않는 agent는 병합 뒤 기다림(`merge_grace_secs`)이 지나면 끝내되, 보고가 이름 붙인 내 PR이 열려 있으면
///   닫힐 때까지 기다린다.
/// - branch는 끝이 병합된 PR head일 때만 지운다(그 뒤 commit이 있으면 남긴다).
#[test]
fn merged_pr_resolves_the_thread() {
    let mut w = world_setup("tick_secs = 1\npr_poll_secs = 1\nmerge_grace_secs = 1\n", false, fake_gh);
    let stub = w.lab.root.join("gh");
    let mut f = w.lab.follow("follow", &w.h, "demo");
    git(&w.repo, &["remote", "set-url", "origin", "https://github.com/o/app.git"]);
    // 처음 본 PR이 이미 승인·병합이면 사건은 `[review, merged]`다: 항목 사건 이름(`PR merged`)으로 본다
    let merged = |e: &Value, id: &str| {
        e["kind"] == "pr_changed"
            && e["thread"] == id
            && e["detail"].as_str().is_some_and(|d| d.contains(" PR merged ["))
    };

    // A: 진행을 보고하는 agent(35%로 배경 CI를 기다린다). branch로 PR #7을 찾는다: 열리고 승인됐다
    let a = w.start(&[
        "--title",
        "A",
        "--profile",
        "prog",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("a.md", "a"),
    ]);
    let a_id = a["id"].as_str().unwrap().to_string();
    f.kind("report_ready", &a_id, T);
    let a_branch = w.show(&a_id)["branch"].as_str().unwrap().to_string();
    let a_tip = git(&w.repo, &["rev-parse", &a_branch]);
    std::fs::write(stub.join("view-7.json"), pr_view_json(&a_branch, "OPEN", "APPROVED", &a_tip)).unwrap();
    std::fs::write(
        stub.join(format!("list-{}.json", a_branch.replace('/', "_"))),
        r#"[{"url":"https://github.com/o/app/pull/7","state":"OPEN","createdAt":"2026-10-18T00:00:00Z"}]"#,
    )
    .unwrap();
    let landing =
        wait_for("병합 중 항목", T, || state_items(&w, &a_id).into_iter().find(|(event, _)| event == "landing"));
    assert!(landing.1.starts_with(&format!("{a_id} \"A\": 병합 중(")), "{landing:?}");
    assert_eq!(w.show(&a_id)["group"], "landing");
    let listed = String::from_utf8_lossy(&w.h.run(&["thread", "list", "demo"]).stdout).to_string();
    assert!(listed.contains(&format!("demo/{a_id}\t병합 중\t")), "{listed}");

    // 병합됐지만 agent는 아직 35%다: ticker가 몇 번 돌아도 끝내지 않는다(기다림 1초가 지나도)
    std::fs::write(stub.join("view-7.json"), pr_view_json(&a_branch, "MERGED", "APPROVED", &a_tip)).unwrap();
    f.until("A 병합", T, |e| merged(e, &a_id));
    w.h.ticks(3);
    assert_eq!(w.show(&a_id)["status"], "open");

    // B: 진행을 보고하지 않는 agent가 commit을 남긴다. 보고 첫 줄의 PR #8이 병합됐고, 보고가 이름 붙인 내 PR #9가 열려 있다
    std::fs::write(stub.join("open.json"), r#"[{"number":9}]"#).unwrap();
    let b = w.start(&[
        "--title",
        "B",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("b.md", "commit"),
    ]);
    let b_id = b["id"].as_str().unwrap().to_string();
    f.kind("report_ready", &b_id, T);
    let bt = w.show(&b_id);
    let b_branch = bt["branch"].as_str().unwrap().to_string();
    let b_wd = PathBuf::from(bt["work_dir"].as_str().unwrap());
    std::fs::write(stub.join("view-8.json"), pr_view_json(&b_branch, "MERGED", "APPROVED", &"0".repeat(40))).unwrap();
    std::fs::write(
        b_wd.join("report.md"),
        "PR: https://github.com/o/app/pull/8\n[완료] #8을 병합했다\n\n## Report\n#9가 리뷰를 기다린다.\n\n## Next\n- #9 병합\n",
    )
    .unwrap();
    f.until("B 병합", T, |e| merged(e, &b_id));
    let asked = "pr list --repo o/app --state open --author @me --limit 100 --json number\n";
    wait_for("열린 내 PR 질문", T, || {
        std::fs::read_to_string(stub.join("calls.log")).ok().filter(|c| c.contains(asked)).map(|_| ())
    });
    w.h.ticks(2);
    assert_eq!(w.show(&b_id)["status"], "open", "#9가 열려 있다");

    // A가 끝낸 보고를 쓰고 `Done`을 알린다: 마지막 사본 뒤 끝내고, 끝이 병합된 head인 branch를 지운다
    w.message(&a_id, "finish", false);
    let e = f.kind("thread_resolved", &a_id, T);
    let detail = e["detail"].as_str().unwrap();
    assert!(detail.contains(&format!("branch {a_branch}를 지웠다(PR이 병합됐다)")), "{e}");
    assert_eq!(git(&w.repo, &["branch", "--list", &a_branch]), "");
    let copied = std::fs::read_to_string(w.projects.join(format!("demo/threads/{a_id}.md"))).unwrap();
    assert!(copied.starts_with("[완료] 끝냈다"), "{copied}");
    let a_items = state_items(&w, &a_id);
    let (_, summary) = a_items.iter().find(|(event, _)| event == "resolved").expect("resolved 항목");
    assert!(summary.starts_with(&format!("{a_id} \"A\": PR이 병합돼 저절로 끝냈다")), "{summary}");

    // #9가 닫혔다: B를 끝낸다. branch에는 병합된 PR에 없는 commit이 있어 남긴다
    std::fs::write(stub.join("open.json"), "[]").unwrap();
    let e = f.kind("thread_resolved", &b_id, T);
    assert!(
        e["detail"].as_str().unwrap().contains(&format!("branch {b_branch}를 남겼다: 병합된 PR에 없는 commit이 있다")),
        "{e}"
    );
    assert_eq!(git(&w.repo, &["branch", "--list", &b_branch, "--format=%(refname:short)"]), b_branch);
}

/// 처음 본 PR이 이미 승인·병합이면 PR 조회가 pr-followup(`review`)을 그 thread에 보낸다. 병합 뒤 끝내기는 그 글의 받음
/// 확인이 끝난 뒤에 한다(herdr-projects는 한 tick 안에서 PR 조회·routine 뒤에 `resolve_merged`). agent는 `Done` 뒤 입력을
/// 보이지 않아 받음 확인이 `accept_wait_ms`(3초) 동안 이어진다. 그동안 tick(1초)이 여러 번 돌아도 끝내지 않는다.
#[test]
fn merged_resolve_waits_for_the_routine_prompt() {
    let mut w =
        world_setup("tick_secs = 1\npr_poll_secs = 1\nmerge_grace_secs = 1\naccept_wait_ms = 3000\n", false, fake_gh);
    let stub = w.lab.root.join("gh");
    let mut f = w.lab.follow("follow", &w.h, "demo");
    git(&w.repo, &["remote", "set-url", "origin", "https://github.com/o/app.git"]);
    let t = w.start(&[
        "--title",
        "Q",
        "--profile",
        "prog",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("q.md", "q"),
    ]);
    let id = t["id"].as_str().unwrap().to_string();
    f.kind("report_ready", &id, T);
    // agent는 안내 줄을 받자마자 보고하므로, host가 전달을 확인하는 것이 보고보다 늦을 수 있다(그 사이 메시지는 「아직
    // brief를 받지 않았다」로 거절된다, 부하 아래 ci-local)
    w.wait_show("brief 전달", &id, |v| v["brief"] == "delivered");
    w.message(&id, "finish quiet", false);
    w.wait_show("Done", &id, |v| v["agent"]["percent"] == 100);
    let branch = w.show(&id)["branch"].as_str().unwrap().to_string();
    let tip = git(&w.repo, &["rev-parse", &branch]);
    std::fs::write(stub.join("view-21.json"), pr_view_json(&branch, "MERGED", "APPROVED", &tip)).unwrap();
    std::fs::write(
        stub.join(format!("list-{}.json", branch.replace('/', "_"))),
        r#"[{"url":"https://github.com/o/app/pull/21","state":"MERGED","createdAt":"2026-10-18T00:00:00Z"}]"#,
    )
    .unwrap();
    let prompted = f.until("routine 발화", T, |e| {
        e["thread"] == id.as_str() && (e["kind"] == "routine_prompted" || e["kind"] == "thread_resolved")
    });
    assert_eq!(prompted["kind"], "routine_prompted", "routine 글을 받기 전에 끝냈다: {prompted}");
    let e = f.kind("thread_resolved", &id, T);
    assert!(e["detail"].as_str().unwrap().contains(&format!("branch {branch}를 지웠다(PR이 병합됐다)")), "{e}");
    // 글은 받았는지 모르는 것으로 남는다(보이지 않는 agent). 끝내기가 session을 죽여 생긴 실패는 없다
    let cut: Vec<&Value> = f
        .seen
        .iter()
        .filter(|e| e["thread"] == id.as_str() && e["detail"].as_str().is_some_and(|d| d.contains("세션이 끝났다")))
        .collect();
    assert!(cut.is_empty(), "{cut:?}");
}

/// 사람이 끝낼 때 PR을 한 번 더 본다(herdr-projects threads.rs `last_pr_lookup`): ticker가 아직 병합을 보지 못했어도
/// 병합된 PR head와 같은 branch는 지우고, 찾은 상태를 기록과 `PR found at resolve` 항목에 남긴다.
#[test]
fn resolve_looks_up_the_pull_request_once_more() {
    // 조회는 보고가 올 때만(간격 1시간): 병합은 resolve만 본다
    let mut w = world_setup("pr_poll_secs = 3600\n", false, fake_gh);
    let stub = w.lab.root.join("gh");
    let mut f = w.lab.follow("follow", &w.h, "demo");
    git(&w.repo, &["remote", "set-url", "origin", "https://github.com/o/app.git"]);
    let (branch, base) = ("aaa/demo/t-0001-c", git(&w.repo, &["rev-parse", "origin/main"]));
    std::fs::write(stub.join("view-5.json"), pr_view_json(branch, "OPEN", "", &base)).unwrap();
    std::fs::write(
        stub.join(format!("list-{}.json", branch.replace('/', "_"))),
        r#"[{"url":"https://github.com/o/app/pull/5","state":"OPEN","createdAt":"2026-10-18T00:00:00Z"}]"#,
    )
    .unwrap();
    let c = w.start(&[
        "--title",
        "c",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("c.md", "c"),
    ]);
    assert_eq!(c["branch"], branch);
    let e = f.until("PR 열림", T, |e| e["kind"] == "pr_changed" && e["thread"] == "t-0001");
    assert!(e["detail"].as_str().unwrap().contains("PR opened"), "{e}");
    std::fs::write(stub.join("view-5.json"), pr_view_json(branch, "MERGED", "", &base)).unwrap();
    let v = w.h.ok(&["thread", "resolve", "demo", "t-0001"]);
    assert_eq!((v["status"].as_str(), v["pr"]["state"].as_str()), (Some("resolved"), Some("MERGED")), "{v}");
    assert_eq!(git(&w.repo, &["branch", "--list", branch]), "");
    let items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    let found: Vec<&Value> = items.iter().filter(|i| i["event"] == "PR found at resolve").collect();
    assert_eq!(found.len(), 1, "{items:?}");
    assert!(found[0]["summary"].as_str().unwrap().contains("상태 MERGED"), "{found:?}");
}
