#!/bin/bash
# launcher 방식 확인(결정 14): 등록한 aaa-launcher.app을 LaunchServices로 띄우고, launcher가 자식으로 띄운
# aaa가 첫 실행 지연·전체 디스크 접근·책임 프로세스를 잰다. 재부팅·LaunchAgent 등록·시스템 설정 변경은 하지 않는다.
#
# 사용: bash p0/launcher/measure.sh <단계 이름> --aaa <aaa 실행 파일 절대 경로> [--samples 30]
#   예: before(지금 aaa) → aaa를 다시 빌드해 같은 경로에 교체 → after(재등록 없이)
# 결과: ${AAA_P0_RESULTS:-~/aaa-p0/results}/launcher-<단계>.json 과 요약
set -euo pipefail

STEP="${1:?단계 이름이 필요하다(예: before, after)}"; shift
AA=""; SAMPLES=30
while [ $# -gt 0 ]; do
  case "$1" in
    --aaa) AA="$2"; shift 2 ;;
    --samples) SAMPLES="$2"; shift 2 ;;
    *) echo "모르는 인자: $1" >&2; exit 64 ;;
  esac
done
APP="${AAA_LAUNCHER_APP:-$HOME/Applications/aaa-launcher.app}"
RES="${AAA_P0_RESULTS:-$HOME/aaa-p0/results}"
[ -x "$APP/Contents/MacOS/aaa-launcher" ] || { echo "$APP 가 없다. p0/launcher/install.sh를 먼저 한다." >&2; exit 1; }
case "$AA" in /*) ;; *) echo "--aaa에는 절대 경로가 필요하다" >&2; exit 64 ;; esac
[ -x "$AA" ] || { echo "$AA 가 없다" >&2; exit 1; }
mkdir -p "$RES"

# launcher가 등록 때와 같은지(cdhash) 확인한다. 바뀌었으면 측정이 뜻이 없다.
now=$(/usr/bin/codesign -dvvv "$APP" 2>&1 | sed -n 's/^CDHash=//p')
was=$(sed -n 's/^CDHash=//p' "$APP.txt" 2>/dev/null || true)
echo "launcher cdhash: $now (설치 때 ${was:-기록 없음})"
[ -z "$was" ] || [ "$now" = "$was" ] || { echo "launcher가 설치 뒤 바뀌었다. 측정하지 않는다." >&2; exit 1; }
echo "aaa sha256: $(shasum -a 256 "$AA" | cut -d' ' -f1)"

OUT="$RES/launcher-$STEP.json"
rm -f "$OUT"
/usr/bin/open -n -W "$APP" --args "$AA" doctor probe --samples "$SAMPLES" --fda-check --label "launcher-$STEP" --out "$OUT" || true
[ -s "$OUT" ] || { echo "결과 없음(launcher·aaa가 실행되지 않았거나 측정 실패)" >&2; exit 1; }
"$AA" doctor summary "$OUT" | tee "$RES/launcher-$STEP-summary.md"
