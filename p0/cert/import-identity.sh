#!/bin/bash
# aa-codesign.p12를 이 Mac의 login 키체인에 넣고 codesign이 확인 창 없이 쓰게 한다(키체인 변경: 사용자가 실행).
# ssh에서도 된다. ssh 세션의 login 키체인은 잠겨 있으므로 먼저 사용자가 직접 잠금을 푼다:
#   security unlock-keychain ~/Library/Keychains/login.keychain-db     (비밀번호는 직접 입력)
#
# 사용: bash p0/cert/import-identity.sh <aa-codesign.p12 경로>
# 단계마다 GUI 확인 창이 뜨는지 적었다. 이 스크립트의 명령은 GUI 창 없이 끝나야 한다.
set -euo pipefail
P12="${1:?aa-codesign.p12 경로가 필요하다}"
KC="$HOME/Library/Keychains/login.keychain-db"
NAME="aa Code Signing"

if ! security show-keychain-info "$KC" >/dev/null 2>&1; then
  echo "login 키체인이 잠겨 있다. 먼저 직접 실행한다: security unlock-keychain $KC" >&2
  exit 1
fi
if security find-certificate -c "$NAME" "$KC" >/dev/null 2>&1; then
  echo "\"$NAME\" 인증서가 이미 있다. 넣지 않고 확인만 한다."
else
  # 1) 반입. -T: codesign·security가 이 키를 확인 창 없이 쓰도록 ACL에 넣는다. p12 암호를 묻는다(GUI 창 없음).
  read -r -s -p "p12 암호: " P; echo
  security import "$P12" -k "$KC" -f pkcs12 -P "$P" -T /usr/bin/codesign -T /usr/bin/security
  unset P
  # 2) partition list: macOS 10.12+는 이것이 없으면 codesign 첫 사용 때 키 접근 확인 창(GUI)이 뜬다.
  #    login 키체인 비밀번호를 묻는다(터미널 입력, GUI 창 없음).
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -l "$NAME" "$KC" >/dev/null
fi

# 3) 서명 시험(임시 파일). 신뢰 설정 없이 서명되는지 본다.
T=$(mktemp -d)
cp /usr/bin/true "$T/aa-sign-test"
if /usr/bin/codesign --force --sign "$NAME" "$T/aa-sign-test" 2>"$T/err"; then
  echo "서명 시험: 성공"
  /usr/bin/codesign -d -r- "$T/aa-sign-test" 2>&1 | grep designated || true
else
  echo "서명 시험: 실패 — $(cat "$T/err")"
  echo "신뢰 설정이 필요할 수 있다. 보고서의 「신뢰 설정」 단계를 본다."
fi
rm -rf "${T:?}"
echo "--- 코드 서명 identity 목록(신뢰되지 않으면 CSSMERR_TP_NOT_TRUSTED로 보인다)"
security find-identity -p codesigning "$KC" | sed -n '1,10p'
