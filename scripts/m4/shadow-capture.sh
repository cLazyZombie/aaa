#!/usr/bin/env bash
# M4 shadow 수집(plan §15 2, docs/migration.md). 지금 운영(herdr·herdr-projects 0.2)과 aaa host가 같은 시각에
# 보는 것을 원자료 그대로 tick 폴더에 남긴다. 해석은 하지 않는다(`aaa migrate shadow`가 나중에 다시 읽는다).
#
# - 운영 쪽에는 읽기 명령만 쓴다: `herdr-projects thread list --json`, `herdr api snapshot`, inbox 폴더 목록,
#   `aaa thread list --json`, `aaa inbox list --peek --all --json`(--peek: 「봤다」 표시도 하지 않는다), `aaa pane list --json`.
#   pane에 입력하지 않는다.
# - 원자료에는 홈 경로·머신 이름·세션 ID가 들어간다. 출력 폴더는 저장소 밖(또는 git 제외 폴더)에 둔다.
#
# 쓰기 예:
#   scripts/m4/shadow-capture.sh --out /abs/ticks --hp-host mac-a --hp-project projects \
#     --herdr mac-a=mac-a --herdr mac-b=local --aaa-host mac-a --aaa-project pilot --interval 600 --count 36
set -u

out="" hp_host="" hp_project="" aaa_host="" interval=600 count=1
herdr=() aaa_projects=()
while [ $# -gt 0 ]; do
  case "$1" in
    --out) out="$2"; shift 2 ;;
    --hp-host) hp_host="$2"; shift 2 ;;          # herdr-projects root가 있는 머신(ssh 대상, local이면 이 머신)
    --hp-project) hp_project="$2"; shift 2 ;;
    --herdr) herdr+=("$2"); shift 2 ;;            # 이름=ssh대상|local, herdr 서버가 도는 머신마다
    --aaa-host) aaa_host="$2"; shift 2 ;;         # aaa host 머신(ssh 대상, local이면 이 머신)
    --aaa-project) aaa_projects+=("$2"); shift 2 ;;
    --interval) interval="$2"; shift 2 ;;
    --count) count="$2"; shift 2 ;;
    *) echo "모르는 인자: $1" >&2; exit 2 ;;
  esac
done
[ -n "$out" ] && [ -n "$hp_host" ] && [ -n "$hp_project" ] && [ -n "$aaa_host" ] || {
  echo "필요: --out --hp-host --hp-project --aaa-host" >&2; exit 2; }
case "$out" in /*) ;; *) echo "--out은 절대 경로" >&2; exit 2 ;; esac

# 한 명령을 대상 머신에서 돌려 파일에 담는다. 실패도 기록한다(빈 파일과 구별).
on() { # on <대상> <파일> <명령…>
  local target="$1" file="$2"; shift 2
  local rc
  if [ "$target" = local ]; then
    nice -n 15 bash -lc "$*" >"$file" 2>"$file.err"; rc=$?
  else
    ssh -o BatchMode=yes -o ConnectTimeout=10 "$target" "nice -n 15 bash -lc $(printf %q "$*")" >"$file" 2>"$file.err"; rc=$?
  fi
  [ -s "$file.err" ] || rm -f "$file.err"
  echo "$rc" >"$file.rc"
}

i=0
while :; do
  ts=$(date -u +%Y%m%dT%H%M%SZ)
  dir="$out/$ts"
  mkdir -p "$dir"
  start=$(date +%s)
  hp="herdr-projects"
  on "$hp_host" "$dir/hp-threads.json" "$hp thread list $(printf %q "$hp_project") --json"
  # inbox: 끝내지 않은 항목 = inbox/ 바로 아래 .md(done/ 제외). 본문(front matter)까지 담는다.
  on "$hp_host" "$dir/hp-inbox.txt" \
    "d=\"\${HERDR_PROJECTS_ROOT:-\$HOME/.herdr-projects}/$hp_project/inbox\"; for f in \"\$d\"/*.md; do [ -f \"\$f\" ] || continue; echo \"=== \$(basename \"\$f\")\"; cat \"\$f\"; done"
  # 열린 thread의 집 보고 사본(threads/<id>.md): sha256과 첫 줄(상태 태그). thread list --json은 기록의 hash만 준다.
  on "$hp_host" "$dir/hp-reports.txt" \
    "d=\"\${HERDR_PROJECTS_ROOT:-\$HOME/.herdr-projects}/$hp_project/threads\"; for t in \"\$d\"/*.toml; do grep -q '^status = \"open\"' \"\$t\" || continue; id=\$(basename \"\$t\" .toml); f=\"\$d/\$id.md\"; [ -f \"\$f\" ] || continue; printf '%s\t%s\t%s\n' \"\$id\" \"\$(shasum -a 256 \"\$f\" | cut -d' ' -f1)\" \"\$(grep -m1 -v '^[[:space:]]*\$' \"\$f\")\"; done"
  for spec in "${herdr[@]}"; do
    name="${spec%%=*}" target="${spec#*=}"
    on "$target" "$dir/herdr-$name.json" "herdr api snapshot"
    # herdr-projects 진행 보고 기록(pane마다, group의 self_working·self_waiting 입력)
    on "$target" "$dir/progress-$name.txt" \
      "for f in \"\${HERDR_PROJECTS_ROOT:-\$HOME/.herdr-projects}\"/.progress/*.json; do [ -f \"\$f\" ] || continue; echo \"=== \$(basename \"\$f\")\"; cat \"\$f\"; echo; done"
  done
  aaa='$HOME/.local/bin/aaa'
  on "$aaa_host" "$dir/aaa-threads.json" "$aaa thread list --json"
  on "$aaa_host" "$dir/aaa-panes.json" "$aaa pane list --json"
  # 고른 aaa 프로젝트의 열린 thread pane 화면(aaa 상태가 실제 화면과 맞는지 보는 근거). pane read는 읽기만 한다.
  for p in "${aaa_projects[@]}"; do
    on "$aaa_host" "$dir/aaa-screens-$p.txt" \
      "$aaa thread list -p $(printf %q "$p") --json | python3 -c 'import json,sys
for t in json.load(sys.stdin).get(\"threads\",[]):
    if t.get(\"status\")==\"open\" and t.get(\"pane\"): print(t[\"id\"], t[\"pane\"])' | while read -r id pane; do echo \"=== \$id \$pane\"; $aaa pane read \"\$pane\"; done"
  done
  for p in "${aaa_projects[@]}"; do
    on "$aaa_host" "$dir/aaa-inbox-$p.json" "$aaa inbox list $(printf %q "$p") --peek --all --json"
  done
  printf '{"tick":"%s","started":%s,"finished":%s}\n' "$ts" "$start" "$(date +%s)" >"$dir/tick.json"
  i=$((i + 1))
  [ "$i" -ge "$count" ] && break
  sleep "$interval"
done
