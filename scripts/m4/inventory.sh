#!/usr/bin/env bash
# M4 읽기 전용 inventory(plan §15 1, docs/migration.md). herdr-projects root가 있는 머신(home)에서 돈다.
# 머신마다 `herdr api snapshot`을 모으고, 로컬 thread가 쓰는 profile을 `herdr-projects profile resolve`로 풀어
# `aaa migrate inventory`에 넘긴다. 운영 쪽에는 읽기 명령만 쓴다. 결과에는 홈 경로·머신 이름·세션 ID가 들어가므로
# --out은 저장소 밖에 둔다.
#
# 쓰기 예(home 머신 mac-a에서):
#   scripts/m4/inventory.sh --aaa ~/.local/bin/aaa --project projects --home mac-a \
#     --herdr mac-a=local --herdr mac-b=mac-b --out /abs/m4-inventory
set -euo pipefail

aaa="" project="" home="" out="" root="${HERDR_PROJECTS_ROOT:-$HOME/.herdr-projects}"
herdr=()
while [ $# -gt 0 ]; do
  case "$1" in
    --aaa) aaa="$2"; shift 2 ;;
    --project) project="$2"; shift 2 ;;
    --home) home="$2"; shift 2 ;;
    --root) root="$2"; shift 2 ;;
    --herdr) herdr+=("$2"); shift 2 ;;     # 이름=ssh대상|local
    --out) out="$2"; shift 2 ;;
    *) echo "모르는 인자: $1" >&2; exit 2 ;;
  esac
done
[ -n "$aaa" ] && [ -n "$project" ] && [ -n "$home" ] && [ -n "$out" ] || { echo "필요: --aaa --project --home --out" >&2; exit 2; }
case "$out" in /*) ;; *) echo "--out은 절대 경로" >&2; exit 2 ;; esac
mkdir -p "$out/snap"

args=()
for spec in "${herdr[@]}"; do
  name="${spec%%=*}" target="${spec#*=}"
  if [ "$target" = local ]; then
    herdr api snapshot >"$out/snap/$name.json"
  else
    ssh -o BatchMode=yes -o ConnectTimeout=10 "$target" 'herdr api snapshot' >"$out/snap/$name.json"
  fi
  args+=(--snapshot "$name=$out/snap/$name.json")
done

# 로컬(기록에 머신이 없는) thread가 쓰는 profile만 푼다
: >"$out/snap/profiles.jsonl"
for p in $(grep -l '^machine = ""' "$root/$project"/threads/*.toml 2>/dev/null | xargs grep -h '^profile = ' | sort -u | sed 's/^profile = "\(.*\)"$/\1/'); do
  [ -n "$p" ] && herdr-projects --root "$root" profile resolve "$p" >>"$out/snap/profiles.jsonl" || true
done

nice -n 15 "$aaa" migrate inventory --from "$root" --project "$project" --out "$out" --home-machine "$home" \
  --source-version "$(herdr-projects --version)" --home-profiles "$out/snap/profiles.jsonl" ${args[@]+"${args[@]}"}
