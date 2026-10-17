#!/bin/bash
# [결정 14로 보류] launcher 방식(p0/launcher/)이 실측에서 실패할 때만 쓴다. 절차는 docs/p0-runbook.md.
# P0 E1(서명 identity 유지)·E2 일부를 mac-a GUI 터미널에서 순서대로 진행한다.
# 시스템 설정 화면 등록은 사용자가 하고, 이 스크립트는 그 앞에서 멈춘다(Enter로 계속).
#
# 세 앱을 나란히 시험한다(번들 id가 달라 등록이 서로 섞이지 않는다).
#   aaa.app          자체 서명 "aaa Code Signing"(1순위), SMAppService LaunchAgent plist 포함
#   aaa-appledev.app 이 Mac의 무료 Apple Development 인증서(2순위)
#   aaa-adhoc.app    adhoc(음성 대조: 업그레이드 뒤 등록이 풀려야 측정이 믿을 만하다)
#
# 사용: bash p0/e1.sh [phase]
#   phase: all(기본) | build | register | before | upgrade | after | summary
# 환경: AAA_P0_VOLUME_DIR(외장 볼륨 읽기·쓰기 확인 폴더, 예: /Volumes/<볼륨 이름>. 비우면 볼륨 확인을 건너뛴다), AAA_P0_SAMPLES(기본 30)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PHASE="${1:-all}"
RES="${AAA_P0_RESULTS:-$HOME/aaa-p0/results}"
VOL="${AAA_P0_VOLUME_DIR:-}"
SAMPLES="${AAA_P0_SAMPLES:-30}"
SELF_ID="aaa Code Signing"
mkdir -p "$RES"

appledev_id() {
  # 같은 이름의 폐기된 인증서가 함께 있을 수 있어 「유효한」 것의 SHA-1을 쓴다.
  security find-identity -v -p codesigning | awk '/Apple Development/ {print $2; exit}'
}

pause() {
  echo
  echo ">>> $1"
  read -r -p ">>> 끝났으면 Enter(중단은 Ctrl-C): " _
}

preflight() {
  echo "== 사전 확인"
  security show-keychain-info "$HOME/Library/Keychains/login.keychain-db" >/dev/null 2>&1 \
    || { echo "login 키체인이 잠겨 있다. GUI 터미널에서 실행하거나 security unlock-keychain을 먼저 한다."; exit 1; }
  security find-identity -p codesigning | grep -q "\"$SELF_ID\"" \
    || { echo "\"$SELF_ID\" identity가 없다. p0/cert/import-identity.sh를 먼저 한다."; exit 1; }
  [ -n "$(appledev_id)" ] || echo "주의: 유효한 Apple Development identity가 없다. aaa-appledev는 건너뛴다."
  command -v cargo >/dev/null || { echo "cargo가 없다"; exit 1; }
  echo "자체 서명: $SELF_ID / Apple Development: $(appledev_id) / 결과: $RES / 볼륨: ${VOL:-확인 안 함}"
}

build_all() {
  "$ROOT/p0/build-app.sh" --identity "$SELF_ID" --name aaa --bundle-id dev.aaa --launchagents
  "$ROOT/p0/install-app.sh" "$ROOT/target/p0/aaa.app"
  if [ -n "$(appledev_id)" ]; then
    "$ROOT/p0/build-app.sh" --identity "$(appledev_id)" --name aaa-appledev --bundle-id dev.aaa.appledev
    "$ROOT/p0/install-app.sh" "$ROOT/target/p0/aaa-appledev.app"
  fi
  "$ROOT/p0/build-app.sh" --identity - --name aaa-adhoc --bundle-id dev.aaa.adhoc
  "$ROOT/p0/install-app.sh" "$ROOT/target/p0/aaa-adhoc.app"
}

register() {
  if ! security find-generic-password -s aaa-p0-probe -a probe >/dev/null 2>&1; then
    echo "== 키체인 실제 항목 읽기용 시험 항목(aaa-p0-probe)을 만든다(키체인 변경)."
    read -r -p "만들까? [y/N] " yn
    if [ "$yn" = y ]; then
      # 값은 의미 없는 난수다. -T: 세 앱의 실행 파일이 확인 창 없이 읽도록 허용한다.
      security add-generic-password -s aaa-p0-probe -a probe -w "$(openssl rand -hex 16)" \
        -T /Applications/aaa.app/Contents/MacOS/aaa \
        -T /Applications/aaa-appledev.app/Contents/MacOS/aaa \
        -T /Applications/aaa-adhoc.app/Contents/MacOS/aaa
    fi
  fi
  cat <<'EOF'

== 시스템 설정에서 등록한다(이 Mac 화면에서, 한 번만)
 1) 시스템 설정 > 개인정보 보호 및 보안 > 개발자 도구
    「+」 → /Applications/aaa.app, aaa-appledev.app, aaa-adhoc.app 을 추가하고 스위치를 켠다.
 2) 시스템 설정 > 개인정보 보호 및 보안 > 전체 디스크 접근 권한
    「+」 → 같은 세 앱을 추가하고 스위치를 켠다.
 3) 아래 Enter 뒤 이 스크립트가 LaunchAgent 두 개를 SMAppService로 등록한다
    (aaa service register). 「백그라운드 항목이 추가됨」 알림이 뜬다.
    상태가 requires_approval이면 시스템 설정 > 일반 > 로그인 항목 및 확장 프로그램 >
    「백그라운드에서 허용」에서 aaa를 켠다.
EOF
  pause "1)·2) 등록을 마쳤는가?"
  for kind in standard interactive; do
    /Applications/aaa.app/Contents/MacOS/aaa service register --label "dev.aaa.p0probe.$kind" || true
  done
  for kind in standard interactive; do
    /Applications/aaa.app/Contents/MacOS/aaa service status --label "dev.aaa.p0probe.$kind"
  done
  pause "LaunchAgent 상태가 둘 다 enabled인가?(아니면 로그인 항목 화면에서 켠다)"
}

measure_all() {
  local step="$1"
  "$ROOT/p0/measure.sh" "$step" --name aaa --samples "$SAMPLES" --volume-dir "$VOL"
  [ -d /Applications/aaa-appledev.app ] && "$ROOT/p0/measure.sh" "$step" --name aaa-appledev --samples "$SAMPLES" --volume-dir "$VOL" --no-launchagent --no-shell
  "$ROOT/p0/measure.sh" "$step" --name aaa-adhoc --samples "$SAMPLES" --volume-dir "$VOL" --no-launchagent --no-shell
}

upgrade_all() {
  "$ROOT/p0/upgrade-sim.sh" --identity "$SELF_ID" --name aaa --bundle-id dev.aaa --launchagents
  [ -d /Applications/aaa-appledev.app ] && "$ROOT/p0/upgrade-sim.sh" --identity "$(appledev_id)" --name aaa-appledev --bundle-id dev.aaa.appledev
  "$ROOT/p0/upgrade-sim.sh" --identity - --name aaa-adhoc --bundle-id dev.aaa.adhoc
}

summary() {
  /Applications/aaa.app/Contents/MacOS/aaa doctor summary "$RES"/before-*.json "$RES"/after-*.json | tee "$RES/E1-summary.md"
  echo "결과: $RES (thread에 알려 주면 ssh로 가져가 분석한다)"
}

case "$PHASE" in
  all) preflight; build_all; register; measure_all before; upgrade_all; measure_all after; summary ;;
  build) preflight; build_all ;;
  register) register ;;
  before) measure_all before ;;
  upgrade) preflight; upgrade_all ;;
  after) measure_all after ;;
  summary) summary ;;
  *) echo "모르는 phase: $PHASE" >&2; exit 64 ;;
esac
