#!/bin/bash
# P0: 빌드한 .app을 /Applications의 고정 경로로 「rename 교체」한다(제자리 쓰기 금지, 기획서 §11).
# 이전 판은 /Applications/.<name>.app.old-<시각>으로 남긴다(되돌리기용).
#
# 사용: p0/install-app.sh <빌드한 .app 경로>   예) p0/install-app.sh target/p0/aa.app
set -euo pipefail

SRC="${1:?빌드한 .app 경로가 필요하다}"
SRC=$(cd "$(dirname "$SRC")" && pwd)/$(basename "$SRC")
NAME=$(basename "$SRC" .app)
DEST="/Applications/$NAME.app"
STAGE="/Applications/.$NAME.app.new"

/usr/bin/codesign --verify --strict "$SRC"
rm -rf "${STAGE:?}"
/usr/bin/ditto --noqtn "$SRC" "$STAGE"
# updater가 직접 옮긴 파일에는 quarantine이 없어야 한다(§10.9, E7)
if /usr/bin/xattr -r "$STAGE" 2>/dev/null | grep -q com.apple.quarantine; then
  echo "quarantine 속성이 붙어 있다. 중단한다." >&2
  exit 1
fi
/usr/bin/codesign --verify --strict "$STAGE"
if [ -e "$DEST" ]; then
  OLD="/Applications/.$NAME.app.old-$(date +%Y%m%d%H%M%S)"
  mv "$DEST" "$OLD"
  echo "이전 판: $OLD"
fi
mv "$STAGE" "$DEST"
echo "설치: $DEST"
/usr/bin/codesign -dvvv "$DEST" 2>&1 | grep -E '^(Identifier|Authority|CDHash)='
echo "designated=$(/usr/bin/codesign -d -r- "$DEST" 2>&1 | sed -n "s/^#* *designated => //p")"
