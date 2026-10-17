#!/bin/bash
# P0: aaa.app 번들을 지금 소스로 빌드하고 지정한 identity로 서명한다. 시스템 설정은 바꾸지 않는다.
#
# 사용:
#   p0/build-app.sh --identity "aaa Code Signing" [--name aaa] [--bundle-id dev.aaa] [--salt S] [--out DIR] [--launchagents]
#   --identity -          adhoc 서명(음성 대조: 업그레이드 뒤 등록이 풀려야 정상)
#   --salt S              실행 파일 bytes를 바꿔 「다시 빌드한 판」을 만든다(업그레이드 모의)
#   --launchagents        Contents/Library/LaunchAgents에 p0/launchagents/*.plist를 넣는다(SMAppService용)
# 결과: <out>/<name>.app 과 <out>/<name>.build.txt(서명·designated requirement·cdhash)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
IDENTITY=""
NAME="aaa"
BUNDLE_ID="dev.aaa"
SALT=""
OUT="$ROOT/target/p0"
LAUNCHAGENTS=0
while [ $# -gt 0 ]; do
  case "$1" in
    --identity) IDENTITY="$2"; shift 2 ;;
    --name) NAME="$2"; shift 2 ;;
    --bundle-id) BUNDLE_ID="$2"; shift 2 ;;
    --salt) SALT="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --launchagents) LAUNCHAGENTS=1; shift ;;
    *) echo "모르는 인자: $1" >&2; exit 64 ;;
  esac
done
[ -n "$IDENTITY" ] || { echo "--identity가 필요하다(adhoc은 -)" >&2; exit 64; }

cd "$ROOT"
echo "== 빌드(release, AAA_BUILD_SALT=${SALT:-없음})"
AAA_BUILD_SALT="$SALT" nice -n 15 cargo build --release -p aaa --bin aaa

APP="$OUT/$NAME.app"
mkdir -p "$OUT"
STAGE="$OUT/.$NAME.app.building"
rm -rf "${STAGE:?}"
mkdir -p "$STAGE/Contents/MacOS" "$STAGE/Contents/Resources"
cp "$ROOT/target/release/aaa" "$STAGE/Contents/MacOS/aaa"
# 앱 아이콘: launcher와 같은 것(web/public/icon.svg에서 p0/launcher/make-icon.mjs로 만듦)
cp "$ROOT/p0/launcher/AppIcon.icns" "$STAGE/Contents/Resources/AppIcon.icns"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
cat > "$STAGE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>$BUNDLE_ID</string>
	<key>CFBundleName</key>
	<string>$NAME</string>
	<key>CFBundleExecutable</key>
	<string>aaa</string>
	<key>CFBundleIconFile</key>
	<string>AppIcon</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$VERSION</string>
	<key>CFBundleVersion</key>
	<string>${SALT:-1}</string>
	<key>LSUIElement</key>
	<true/>
	<key>LSMinimumSystemVersion</key>
	<string>13.0</string>
</dict>
</plist>
EOF
plutil -lint "$STAGE/Contents/Info.plist" >/dev/null
if [ "$LAUNCHAGENTS" = 1 ]; then
  mkdir -p "$STAGE/Contents/Library/LaunchAgents"
  for f in "$ROOT"/p0/launchagents/*.plist; do
    sed "s/@BUNDLE_ID@/$BUNDLE_ID/" "$f" > "$STAGE/Contents/Library/LaunchAgents/$(basename "$f")"
    plutil -lint "$STAGE/Contents/Library/LaunchAgents/$(basename "$f")" >/dev/null
  done
fi

echo "== 서명(identity: $IDENTITY)"
# 번들 전체를 서명한다. hardened runtime·공증은 쓰지 않는다(결정 9).
/usr/bin/codesign --force --sign "$IDENTITY" --timestamp=none "$STAGE"
/usr/bin/codesign --verify --strict --verbose=2 "$STAGE"

rm -rf "${APP:?}"
mv "$STAGE" "$APP"
INFO="$OUT/$NAME.build.txt"
{
  echo "built_at=$(date '+%Y-%m-%dT%H:%M:%S%z')"
  echo "git=$(git -C "$ROOT" rev-parse --short HEAD)$(git -C "$ROOT" diff --quiet || echo '+dirty')"
  echo "salt=$SALT"
  echo "identity=$IDENTITY"
  echo "sha256=$(shasum -a 256 "$APP/Contents/MacOS/aaa" | cut -d' ' -f1)"
  /usr/bin/codesign -dvvv "$APP" 2>&1 | grep -E '^(Identifier|Authority|TeamIdentifier|Signature|CDHash)='
  echo "designated=$(/usr/bin/codesign -d -r- "$APP" 2>&1 | sed -n "s/^#* *designated => //p")"
} > "$INFO"
cat "$INFO"
echo "== 완료: $APP"
