#!/bin/bash
# aa-launcher를 한 번 빌드해 최소 .app으로 감싸 고정 경로에 둔다(결정 14). 시스템 설정은 바꾸지 않는다.
# 이미 있으면 덮어쓰지 않는다: launcher가 바뀌면 개발자 도구·전체 디스크 접근 등록을 다시 해야 한다.
#
# 사용: bash p0/launcher/install.sh [--dest ~/Applications] [--replace]
#   --replace  있는 launcher를 바꾼다(등록을 다시 해야 한다는 것을 알고 쓸 때만)
# 결과: <dest>/aa-launcher.app, <dest>/aa-launcher.app.txt(cdhash 기록)
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
DEST="$HOME/Applications"
REPLACE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dest) DEST="$2"; shift 2 ;;
    --replace) REPLACE=1; shift ;;
    *) echo "모르는 인자: $1" >&2; exit 64 ;;
  esac
done
APP="$DEST/aa-launcher.app"
if [ -e "$APP" ] && [ "$REPLACE" != 1 ]; then
  echo "이미 있다: $APP (바꾸지 않는다)"
  cat "$APP.txt" 2>/dev/null || true
  exit 0
fi
command -v cc >/dev/null || { echo "cc가 없다(Xcode Command Line Tools)"; exit 1; }

STAGE=$(mktemp -d)
trap 'rm -rf "${STAGE:?}"' EXIT
mkdir -p "$STAGE/aa-launcher.app/Contents/MacOS"
cc -O2 -Wall -Wextra -o "$STAGE/aa-launcher.app/Contents/MacOS/aa-launcher" "$HERE/aa-launcher.c"
cat > "$STAGE/aa-launcher.app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>dev.aa.launcher</string>
	<key>CFBundleName</key>
	<string>aa-launcher</string>
	<key>CFBundleExecutable</key>
	<string>aa-launcher</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>1</string>
	<key>CFBundleVersion</key>
	<string>1</string>
	<key>LSUIElement</key>
	<true/>
	<key>LSMinimumSystemVersion</key>
	<string>13.0</string>
</dict>
</plist>
PLIST
plutil -lint "$STAGE/aa-launcher.app/Contents/Info.plist" >/dev/null
# adhoc 서명. 다시 빌드하지 않으므로 cdhash가 그대로다(결정 9의 인증서는 보류).
/usr/bin/codesign --force --sign - --timestamp=none "$STAGE/aa-launcher.app"
/usr/bin/codesign --verify --strict "$STAGE/aa-launcher.app"
mkdir -p "$DEST"
rm -rf "${APP:?}"
/usr/bin/ditto --noqtn "$STAGE/aa-launcher.app" "$APP"
{
  echo "installed_at=$(date '+%Y-%m-%dT%H:%M:%S%z')"
  echo "source_sha256=$(shasum -a 256 "$HERE/aa-launcher.c" | cut -d' ' -f1)"
  /usr/bin/codesign -dvvv "$APP" 2>&1 | grep -E '^(Identifier|Signature|CDHash)='
} > "$APP.txt"
cat "$APP.txt"
echo "== 설치: $APP"
echo "다음: 시스템 설정 > 개인정보 보호 및 보안 > 개발자 도구, 전체 디스크 접근 권한에 이 앱을 넣고 켠다."
