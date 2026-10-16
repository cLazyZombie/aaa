#!/bin/bash
# 공개 저장소 push 전 검사(결정 11). 착지마다 push 직전에 돌린다.
#
#   bash scripts/check-public.sh [기준 ref, 기본 origin/main]   작업 트리 + 기준 ref 이후 커밋 메시지
#   bash scripts/check-public.sh --history [rev...]              rev(기본 HEAD)에서 닿는 모든 커밋의 파일·메시지
#
# 검사하는 것
#   1) 추적 파일 전체: 금지 확장자(p12·key·pem 등), 1MiB 넘는 파일
#   2) 추적 파일 내용: 개인 키·인증서 블록, 알려진 token 형식, 실제 *.ts.net 주소, IP 주소,
#      사용자 홈 경로, 이메일, 대문자 hex 지문(40자 이상)
#   3) 기준 ref 이후 커밋 메시지: 위 내용 패턴
#   4) 저장소 밖 목록의 실제 이름(머신 hostname·tailnet·사람·회사 이름):
#      `$(git rev-parse --git-common-dir)/info/private-words`에 한 줄에 하나씩 둔다(이 파일은 커밋되지 않는다).
#
# 허용: example.ts.net·<이름>.ts.net, 127.0.0.1·0.0.0.0·문서용 IP(192.0.2.x, 198.51.100.x, 203.0.113.x),
#       noreply@anthropic.com·git@github.com·*@example.com, /Users/<이름> 같은 자리표시자.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 2
HISTORY=0
if [ "${1:-}" = "--history" ]; then
  HISTORY=1
  shift
  REVS=$(git rev-list "${@:-HEAD}") || exit 2
  echo "이력 검사: 커밋 $(wc -l <<<"$REVS" | tr -d ' ')개"
else
  BASE="${1:-origin/main}"
  REVS=""
fi
fail=0
report() { echo "[막음] $1"; fail=1; }

# 1) 금지 확장자·큰 파일(이력 검사면 모든 커밋의 파일 이름)
if [ "$HISTORY" = 1 ]; then
  FILES=$(for r in $REVS; do git ls-tree -r --name-only "$r"; done | sort -u | tr '\n' '\0')
else
  FILES=$(git ls-files -z | tr '\0' '\n' | tr '\n' '\0')
fi
while IFS= read -r -d '' f; do
  case "$f" in
    *.p12 | *.pfx | *.key | *.pem | *.cer | *.der | *.crt | *.keychain | *.keychain-db | *.mobileprovision | *.sqlite | *.db | *.local.toml | *.env | .env)
      report "금지 확장자: $f" ;;
  esac
  if [ "$HISTORY" = 0 ] && [ -f "$f" ] && [ "$(wc -c <"$f")" -gt 1048576 ]; then report "1MiB 넘는 파일: $f"; fi
done < <(printf '%s' "$FILES")

# 2·3) 내용 패턴: 이름, 정규식, 허용 정규식(빈 값이면 허용 없음)을 같은 순서로 둔다.
NAMES=() RES=() ALLOWS=()
add() { NAMES+=("$1"); RES+=("$2"); ALLOWS+=("$3"); }
add '개인 키·인증서 블록' '-----BEGIN [A-Z ]*(PRIVATE KEY|CERTIFICATE)-----' ''
add 'token 형식' '(tskey-[a-z]+-[A-Za-z0-9]{6,}|gh[pousr]_[A-Za-z0-9]{20,}|sk-ant-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|xox[abpr]-[A-Za-z0-9-]{10,})' ''
add 'tailnet 주소' '[A-Za-z0-9-]+\.ts\.net' '(example\.ts\.net|>\.ts\.net|[`*]\.ts\.net|이름\.ts\.net)'
add 'IP 주소' '(^|[^0-9.])([0-9]{1,3}\.){3}[0-9]{1,3}([^0-9.]|$)' '(127\.0\.0\.1|0\.0\.0\.0|192\.0\.2\.|198\.51\.100\.|203\.0\.113\.|255\.255\.)'
add '사용자 홈 경로' '(/Users/|/home/)[A-Za-z0-9._-]+' '(/Users/<|/home/<|/Users/Shared|/Users/runner|/home/runner)'
add '이메일' '[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}' '(noreply@anthropic\.com|git@github\.com|@example\.(com|org|net))'
add '인증서 지문(대문자 hex)' '[0-9A-F]{40,}' ''

# $1=설명, $2..=검사할 rev(없으면 작업 트리의 추적 파일). 커밋 메시지는 COMMITS 변수로 검사한다.
scan() {
  local label="$1" i hits
  shift
  for i in "${!NAMES[@]}"; do
    if [ "$label" = "files" ]; then
      hits=$(git grep -I -n -E -e "${RES[$i]}" "$@" -- . ':!Cargo.lock' ':!web/package-lock.json' ':!scripts/check-public.sh' 2>/dev/null)
    else
      hits=$(grep -n -E -e "${RES[$i]}" <<<"$COMMITS" 2>/dev/null)
    fi
    # 허용 문자열만 지운 뒤에도 패턴이 남는 줄만 남긴다(한 줄에 허용·금지가 섞여도 놓치지 않는다)
    [ -n "${ALLOWS[$i]}" ] && hits=$(printf '%s\n' "$hits" | sed -E "s~${ALLOWS[$i]}~~g" | grep -E -e "${RES[$i]}")
    hits=$(printf '%s\n' "$hits" | sed '/^$/d')
    if [ -n "$hits" ]; then
      report "${NAMES[$i]} ($label)"
      printf '%s\n' "$hits" | head -20 | cut -c1-200 | sed 's/^/    /'
    fi
  done
}
# shellcheck disable=SC2086
scan files $REVS
COMMITS=""
if [ "$HISTORY" = 1 ]; then
  COMMITS=$(git log --no-walk=unsorted --format='%H %B' $REVS | grep -v -E '^Co-Authored-By: ')
  scan "커밋 메시지(이력)"
elif git rev-parse -q --verify "$BASE" >/dev/null; then
  COMMITS=$(git log --format='%H %B' "$BASE..HEAD" | grep -v -E '^Co-Authored-By: ')
  [ -n "$COMMITS" ] && scan "커밋 메시지 $BASE..HEAD"
else
  echo "기준 ref $BASE가 없어 커밋 메시지는 건너뛴다"
fi

# 4) 저장소 밖 목록의 실제 이름(대소문자 무시)
WORDS="$(git rev-parse --git-common-dir)/info/private-words"
if [ -f "$WORDS" ]; then
  while IFS= read -r w; do
    if [ -z "$w" ] || [ "${w#\#}" != "$w" ]; then continue; fi
    # shellcheck disable=SC2086
    hits=$(git grep -I -n -i -F -e "$w" $REVS -- . ':!scripts/check-public.sh' 2>/dev/null | head -10)
    [ -n "$hits" ] && { report "실제 이름(private-words의 한 항목)"; printf '%s\n' "$hits" | cut -c1-200 | sed 's/^/    /'; }
    if [ -n "$COMMITS" ] && grep -q -i -F -e "$w" <<<"$COMMITS"; then
      report "실제 이름이 커밋 메시지에 있다(private-words의 한 항목)"
      grep -i -F -e "$w" <<<"$COMMITS" | head -5 | cut -c1-200 | sed 's/^/    /'
    fi
  done <"$WORDS"
else
  echo "참고: $WORDS가 없어 실제 이름 검사는 건너뛴다(머신 hostname·tailnet 이름을 한 줄씩 적어 둔다)"
fi

if [ "$fail" -ne 0 ]; then
  echo "공개 저장소 검사 실패: 위 항목을 일반 이름·자리표시자로 바꾼 뒤 다시 돌린다."
  exit 1
fi
echo "공개 저장소 검사 통과"
