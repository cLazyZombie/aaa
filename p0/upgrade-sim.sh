#!/bin/bash
# P0 E1 ④: 같은 identity로 다시 빌드·다시 서명해 /Applications/<name>.app을 rename 교체한다.
# 재등록 없이 measure.sh <단계> --name <name>으로 다시 잰다.
#
# 사용: p0/upgrade-sim.sh --identity "<같은 identity>" [--name aaa] [--bundle-id dev.aaa] [--launchagents]
# 확인: designated requirement는 같아야 하고 cdhash·실행 파일 sha256은 달라야 한다.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
IDENTITY=""; NAME="aaa"; BUNDLE_ID="dev.aaa"; EXTRA=()
while [ $# -gt 0 ]; do
  case "$1" in
    --identity) IDENTITY="$2"; shift 2 ;;
    --name) NAME="$2"; shift 2 ;;
    --bundle-id) BUNDLE_ID="$2"; shift 2 ;;
    --launchagents) EXTRA+=(--launchagents); shift ;;
    *) echo "모르는 인자: $1" >&2; exit 64 ;;
  esac
done
[ -n "$IDENTITY" ] || { echo "--identity가 필요하다" >&2; exit 64; }
DEST="/Applications/$NAME.app"
[ -d "$DEST" ] || { echo "$DEST 가 없다" >&2; exit 1; }

dr() { /usr/bin/codesign -d -r- "$1" 2>&1 | sed -n "s/^#* *designated => //p"; }
cdh() { /usr/bin/codesign -dvvv "$1" 2>&1 | sed -n 's/^CDHash=//p'; }
DR0=$(dr "$DEST"); CD0=$(cdh "$DEST"); SHA0=$(shasum -a 256 "$DEST/Contents/MacOS/aaa" | cut -d' ' -f1)

SALT="upg-$(date +%Y%m%d%H%M%S)"
"$ROOT/p0/build-app.sh" --identity "$IDENTITY" --name "$NAME" --bundle-id "$BUNDLE_ID" --salt "$SALT" ${EXTRA[@]+"${EXTRA[@]}"}
"$ROOT/p0/install-app.sh" "$ROOT/target/p0/$NAME.app"

DR1=$(dr "$DEST"); CD1=$(cdh "$DEST"); SHA1=$(shasum -a 256 "$DEST/Contents/MacOS/aaa" | cut -d' ' -f1)
echo
echo "designated 이전: $DR0"
echo "designated 이후: $DR1"
echo "cdhash: $CD0 → $CD1"
echo "sha256: $SHA0 → $SHA1"
ok=1
[ "$DR0" = "$DR1" ] || { echo "주의: designated requirement가 바뀌었다(등록 유지 기대 불가)"; ok=0; }
[ "$CD0" != "$CD1" ] || { echo "오류: cdhash가 같다(다시 빌드되지 않음)"; ok=0; }
[ "$SHA0" != "$SHA1" ] || { echo "오류: 실행 파일이 같다"; ok=0; }
[ "$ok" = 1 ] && echo "업그레이드 모의 정상: 같은 요구 사항, 다른 실행 파일. 재등록하지 말고 measure.sh로 잰다."
