#!/bin/bash
# P0: 설치한 /Applications/<name>.app의 계보별 측정(E1·E2). 시스템 설정은 바꾸지 않는다.
#
# 사용: p0/measure.sh <단계 이름> [--name aa] [--samples 30] [--volume-dir DIR] [--no-launchagent] [--no-shell]
#   단계 이름 예: before(등록 직후), after(업그레이드 뒤)
# 측정 조건
#   ls        LaunchServices로 띄운 앱 자신(open -n -W). 앱이 자기 책임 프로세스다.
#   launchagent-standard / launchagent-interactive
#             SMAppService로 등록된 LaunchAgent(dev.aa.p0probe.*)를 kickstart해서 잰다(등록돼 있을 때만).
#   shell     이 터미널 계보(WezTerm·Terminal·ssh 등). 기준선 비교용
# 결과: ${AA_P0_RESULTS:-~/aa-p0/results}/<단계>-<name>-<조건>.json 과 요약 표
set -euo pipefail

STEP="${1:?단계 이름이 필요하다(예: before, after)}"; shift
NAME="aa"; SAMPLES=30; LAUNCHAGENT=1; SHELLRUN=1; VOLUME_DIR=""
while [ $# -gt 0 ]; do
  case "$1" in
    --name) NAME="$2"; shift 2 ;;
    --samples) SAMPLES="$2"; shift 2 ;;
    --no-launchagent) LAUNCHAGENT=0; shift ;;
    --no-shell) SHELLRUN=0; shift ;;
    --volume-dir) VOLUME_DIR="$2"; shift 2 ;;
    *) echo "모르는 인자: $1" >&2; exit 64 ;;
  esac
done

APP="/Applications/$NAME.app"
AA="$APP/Contents/MacOS/aa"
RES="${AA_P0_RESULTS:-$HOME/aa-p0/results}"
mkdir -p "$RES"
[ -x "$AA" ] || { echo "$AA 가 없다. install-app.sh를 먼저 실행한다." >&2; exit 1; }
COMMON=(doctor probe --samples "$SAMPLES" --fda-check --keychain-service aa-p0-probe)
[ -n "$VOLUME_DIR" ] && COMMON+=(--volume-dir "$VOLUME_DIR")
done_files=()

echo "== [$STEP/$NAME] ls(앱 자신이 책임 프로세스)"
OUT="$RES/$STEP-$NAME-ls.json"
rm -f "$OUT"
/usr/bin/open -n -W "$APP" --args "${COMMON[@]}" --label "$STEP-$NAME-ls" --out "$OUT" || true
if [ -s "$OUT" ]; then done_files+=("$OUT"); else echo "  결과 없음(앱이 실행되지 않았거나 측정 실패)"; fi

if [ "$LAUNCHAGENT" = 1 ]; then
  STATE_RESULTS="$HOME/Library/Application Support/aa/probe/results"
  for kind in standard interactive; do
    LABEL="dev.aa.p0probe.$kind"
    status=$("$AA" service status --label "$LABEL" 2>/dev/null | awk '{print $2}') || status="?"
    if [ "$status" != "enabled" ]; then
      echo "== [$STEP/$NAME] launchagent-$kind: 건너뜀(상태 $status)"
      continue
    fi
    echo "== [$STEP/$NAME] launchagent-$kind(kickstart)"
    mark=$(date +%s)
    launchctl kickstart "gui/$(id -u)/$LABEL"
    found=""
    for _ in $(seq 1 180); do
      sleep 1
      f=$(ls -t "$STATE_RESULTS"/launchagent-$kind-*.json 2>/dev/null | head -1 || true)
      if [ -n "$f" ] && [ "$(stat -f %m "$f")" -ge "$mark" ]; then found="$f"; break; fi
    done
    if [ -z "$found" ]; then echo "  180초 안에 결과가 없다(실패로 기록)"; continue; fi
    cp "$found" "$RES/$STEP-$NAME-launchagent-$kind.json"
    done_files+=("$RES/$STEP-$NAME-launchagent-$kind.json")
  done
fi

if [ "$SHELLRUN" = 1 ]; then
  echo "== [$STEP/$NAME] shell(이 터미널 계보)"
  OUT="$RES/$STEP-$NAME-shell.json"
  "$AA" "${COMMON[@]}" --label "$STEP-$NAME-shell" --out "$OUT" >/dev/null || true
  [ -s "$OUT" ] && done_files+=("$OUT")
fi

echo
if [ ${#done_files[@]} -gt 0 ]; then
  "$AA" doctor summary "${done_files[@]}" | tee "$RES/$STEP-$NAME-summary.md"
fi
