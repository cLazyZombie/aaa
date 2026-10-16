#!/bin/bash
# 자체 서명 코드 서명 인증서를 만든다(결정 9 1순위). 키체인은 바꾸지 않는다: 파일만 만든다.
# 한 번만 만들고 세 Mac이 같은 인증서를 쓴다(업그레이드 뒤에도 같은 인증서로 다시 서명).
#
# 사용(mac-a 터미널): bash p0/cert/make-selfsigned.sh [출력 폴더, 기본 ~/aa-codesign]
# 만드는 것: cert.pem(공개 인증서), aa-codesign.p12(개인 키+인증서, 암호 보호), fingerprint.txt
# 개인 키 평문(key.pem)은 p12를 만든 뒤 지운다. p12와 암호는 사용자가 보관한다.
# p12·암호·지문(fingerprint.txt)은 git·보고서·채팅에 넣지 않는다(공개 저장소, 결정 11).
set -euo pipefail
OUT="${1:-$HOME/aa-codesign}"
NAME="aa Code Signing"
OPENSSL=/usr/bin/openssl   # macOS 기본 LibreSSL. p12를 macOS 키체인이 읽는 형식으로 만든다.
if [ -e "$OUT/aa-codesign.p12" ]; then echo "$OUT/aa-codesign.p12가 이미 있다. 다시 만들지 않는다." >&2; exit 1; fi
# 개인 키는 저장소 밖에만 둔다(공개 저장소, 결정 11): 출력 폴더나 그 위가 git 작업 트리면 멈춘다.
ANC="$OUT"; while [ ! -d "$ANC" ]; do ANC="$(dirname "$ANC")"; done
if git -C "$ANC" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "${OUT}은 git 작업 트리 안이다. 개인 키를 저장소 밖(기본 ~/aa-codesign)에 만든다." >&2; exit 1
fi
mkdir -p "$OUT"; chmod 700 "$OUT"
cd "$OUT"
umask 077
"$OPENSSL" req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 3650 \
  -subj "/CN=$NAME" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" \
  -addext "basicConstraints=critical,CA:false"
echo "p12 암호를 정한다(다른 Mac에 넣을 때 필요, 스크립트에 저장하지 않는다)."
read -r -s -p "p12 암호: " P1; echo
read -r -s -p "p12 암호 확인: " P2; echo
[ "$P1" = "$P2" ] && [ -n "$P1" ] || { echo "암호가 비었거나 다르다"; rm -f key.pem; exit 1; }
"$OPENSSL" pkcs12 -export -inkey key.pem -in cert.pem -name "$NAME" -out aa-codesign.p12 -passout fd:3 3<<<"$P1"
unset P1 P2
rm -f key.pem
{
  echo "name=$NAME"
  "$OPENSSL" x509 -in cert.pem -noout -enddate
  echo "sha1=$("$OPENSSL" x509 -in cert.pem -noout -fingerprint -sha1 | cut -d= -f2 | tr -d :)"
  echo "sha256=$("$OPENSSL" x509 -in cert.pem -noout -fingerprint -sha256 | cut -d= -f2 | tr -d :)"
} > fingerprint.txt
cat fingerprint.txt
ls -l "$OUT"
