#!/bin/bash
# 공개 저장소 push 전 검사(결정 11). 착지마다 push 직전에 돌린다.
#
#   bash scripts/check-public.sh [기준 ref, 기본 origin/main]   작업 트리 + 기준 ref 이후 커밋 메시지
#   bash scripts/check-public.sh --history [--tags] [rev...]     rev(기본 HEAD)에서 닿는 모든 커밋의 파일·메시지.
#                                                                --tags면 origin의 공개 tag(land-lock 같은 고아 commit)도 본다
#                                                                (로컬 tag를 만들지 않고 받아 본다)
# CI(환경 변수 CI)는 기준 ref를 받는다(ci.yml: push면 그 전 tip, PR이면 base). 기준 ref가 없으면 건너뛰지 않고 실패한다.
# --history는 이미 공개된 옛 이력의 항목 중 scripts/public-history-known.txt에 적은 것만 넘긴다(이력 재작성은 사람이 정한다).
# 그 파일에는 내용 대신 「경로<TAB>걸린 줄」(세션 id는 「session<TAB>id」)의 `git hash-object` 값과 이유만 적는다.
#
# 검사하는 것
#   1) 추적 파일 전체: 금지 확장자(p12·key·pem 등), 1MiB 넘는 파일
#   2) 추적 파일 내용: 개인 키·인증서 블록, 알려진 token 형식(Telegram 봇 token 포함), Web Push 구독 주소,
#      실제 *.ts.net 주소, IP 주소(Tailscale IPv6 fd7a:115c:a1e0: 포함), 사용자 홈·외장 볼륨 경로(/Volumes/<이름>),
#      이메일, 대문자 hex 지문(40자 이상, 전부 0인 run은 뺀다)
#   3) 기준 ref 이후 커밋 메시지: 위 내용 패턴
#   4) 저장소 밖 목록의 실제 이름(머신 hostname·tailnet·사람·회사 이름):
#      `$(git rev-parse --git-common-dir)/info/private-words`에 한 줄에 하나씩 둔다(이 파일은 커밋되지 않는다).
#      이 파일이 없는 clone(다른 머신·CI)에서는 4)를 건너뛴다. 그때의 「통과」는 1)~3)·5)만 본 결과다.
#   5) 기준 ref 이후(--history면 모든) 커밋의 author·committer(결정 D4): email은 GitHub noreply
#      (…@users.noreply.github.com, noreply@github.com)만 쓴다. 이름은 4)의 목록으로도 본다.
#   6) 실제 세션 id(로컬 전용): 추적 파일·커밋 메시지의 UUID가 이 머신의 Claude Code·Codex 세션 기록 이름
#      (~/.claude/projects·~/.codex/sessions, CHECK_PUBLIC_SESSION_DIRS로 바꾼다. `:`로 잇는다)과 같으면 막는다.
#      기록 폴더가 없는 clone(CI)에서는 건너뛴다. 찾은 id는 앞 8자만 보인다.
#
# 허용: example.ts.net·<이름>.ts.net, 127.0.0.1·0.0.0.0·문서용 IP(192.0.2.x, 198.51.100.x, 203.0.113.x),
#       noreply@anthropic.com·git@github.com·GitHub noreply(5)와 같다)·*@example.com과 그 하위 이름(token@GitHub.example.com),
#       /Users/<이름>·/Users/me/·/Users/u/·/home/me/·/Volumes/<이름> 같은 자리표시자(me·u는 참고 프로젝트와 옛 시험의 예시),
#       Tailscale 고정 주소 fd7a:115c:a1e0::53(MagicDNS)·fd7a:115c:a1e0::/48(대역),
#       앞뒤가 hex가 아닌 0만의 run(git의 빈 sha 0×40. 0이 아닌 hex가 한 글자라도 붙으면 막는다).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 2
HISTORY=0
if [ "${1:-}" = "--history" ]; then
  HISTORY=1
  shift
  TAGREVS=()
  if [ "${1:-}" = "--tags" ]; then
    shift
    # 공개 tag는 main에서 닿지 않을 수 있다. 로컬 refs/tags를 만들거나 덮지 않게 sha로만 받는다
    TAGS=$(git ls-remote --tags origin) || exit 2
    while read -r sha _; do [ -n "$sha" ] && TAGREVS+=("$sha"); done <<<"$TAGS"
    if [ ${#TAGREVS[@]} -gt 0 ]; then git fetch -q --no-tags origin "${TAGREVS[@]}" || exit 2; fi
    echo "공개 tag ${#TAGREVS[@]}개도 본다"
  fi
  REVS=$(git rev-list "${@:-HEAD}" ${TAGREVS[@]+"${TAGREVS[@]}"}) || exit 2
  echo "이력 검사: 커밋 $(wc -l <<<"$REVS" | tr -d ' ')개"
else
  BASE="${1:-origin/main}"
  REVS=""
fi
fail=0
report() { echo "[막음] $1"; fail=1; }
KNOWN_FILE=scripts/public-history-known.txt
KNOWN_HASHES=""
if [ "$HISTORY" = 1 ] && [ -f "$KNOWN_FILE" ]; then KNOWN_HASHES=$(sed -E -e '/^(#|$)/d' -e 's/[[:space:]].*//' "$KNOWN_FILE"); fi
KNOWN_SKIPPED=0
# 알려진 이력 항목의 열쇠: 「경로<TAB>줄」(rev·줄 번호는 커밋마다 달라 뺀다)
known() { [ -n "$KNOWN_HASHES" ] && grep -q -x -F -e "$(printf '%s\t%s' "$1" "$2" | git hash-object --stdin)" <<<"$KNOWN_HASHES"; }
# 검사 명령이 돌지 못한 것(정규식·rev 오류)을 「찾은 것 없음」으로 치지 않는다: grep·git grep은 0 찾음, 1 없음, 그 밖은 오류다.
ERR=$(mktemp) || exit 2
trap 'rm -f "$ERR"' EXIT
broke() { echo "[오류] $1: $(head -c 300 "$ERR")"; fail=1; }

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

# 2·3) 내용 패턴: 이름, 정규식, 허용 정규식(빈 값이면 허용 없음), 이 검사에서 뺄 파일 정규식(없으면 빈 값)을 같은 순서로 둔다.
NAMES=() RES=() ALLOWS=() SKIPS=()
add() { NAMES+=("$1"); RES+=("$2"); ALLOWS+=("$3"); SKIPS+=("${4:-}"); }
add '개인 키·인증서 블록' '-----BEGIN [A-Z ]*(PRIVATE KEY|CERTIFICATE)-----' ''
add 'token 형식' '(tskey-[a-z]+-[A-Za-z0-9]{6,}|gh[pousr]_[A-Za-z0-9]{20,}|sk-ant-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|xox[abpr]-[A-Za-z0-9-]{10,}|[0-9]{8,10}:AA[A-Za-z0-9_-]{30,})' ''
# 기기가 준 구독 주소의 끝은 그 기기로 push를 보내는 열쇠다. 시험의 짧은 가짜 주소(…/fcm/send/abc)는 걸리지 않는다
add 'Web Push 구독 주소' '(fcm\.googleapis\.com/(fcm/send|wp)/|push\.services\.mozilla\.com/wpush/v[0-9]+/|web\.push\.apple\.com/|notify\.windows\.com(:[0-9]+)?/w/[?]token=)[A-Za-z0-9_%=:.-]{40,}' ''
add 'tailnet 주소' '[A-Za-z0-9-]+\.ts\.net' '(example\.ts\.net|>\.ts\.net|[`*]\.ts\.net|이름\.ts\.net)'
# agentSvgMarks.ts는 참고에서 옮긴 SVG path 숫자(점으로 이은 좌표)뿐이라 IP 검사에서만 뺀다(plan §8.56)
add 'IP 주소' '(^|[^0-9.])([0-9]{1,3}\.){3}[0-9]{1,3}([^0-9.]|$)' '(127\.0\.0\.1|0\.0\.0\.0|192\.0\.2\.|198\.51\.100\.|203\.0\.113\.|255\.255\.)' '^([0-9a-f]{40}:)?web/src/herdr/agentSvgMarks\.ts:'
# Tailscale IPv6(fd7a:115c:a1e0::/48)의 기기 주소. 대역 표기와 MagicDNS 고정 주소만 허용한다
add 'Tailscale IPv6 주소' '[fF][dD]7[aA]:115[cC]:[aA]1[eE]0::?[0-9a-fA-F]{1,4}' '(fd7a:115c:a1e0::/48|fd7a:115c:a1e0::53([^0-9a-fA-F:]|$))'
add '사용자 홈·볼륨 경로' '(/Users/|/home/|/Volumes/)[A-Za-z0-9._-]+' '(/Users/<|/home/<|/Volumes/<|/Users/me/|/Users/u/|/home/me/|/Users/Shared|/Users/runner|/home/runner)'
add '이메일' '[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}' '(noreply@anthropic\.com|git@github\.com|noreply@github\.com|[A-Za-z0-9._%+-]+@users\.noreply\.github\.com|@([A-Za-z0-9-]+\.)*example\.(com|org|net))'
# 0만의 run은 정보가 없다(git의 빈 sha 0×40). 허용은 그 run과 앞뒤 경계 글자만 지우므로 0이 아닌 hex는 남는다.
# 경계 글자 하나로만 떨어진 0 run 여럿(「<빈 sha> <빈 sha>」)도 한 번에 지운다
add '인증서 지문(대문자 hex)' '[0-9A-F]{40,}' '(^|[^0-9A-F])0{40,}([^0-9A-F]0{40,})*([^0-9A-F]|$)'

# 정규식 자체 검사: 깨진 패턴은 git grep·sed가 오류로 끝나 아무것도 막지 못한다(통과로 보인다)
for i in "${!NAMES[@]}"; do
  for re in "${RES[$i]}" "${ALLOWS[$i]}" "${SKIPS[$i]}"; do
    [ -n "$re" ] || continue
    grep -E -e "$re" </dev/null >/dev/null 2>"$ERR"
    [ $? -le 1 ] || broke "${NAMES[$i]}: grep 정규식 오류"
    sed -E "s~${re}~~g" </dev/null >/dev/null 2>"$ERR" || broke "${NAMES[$i]}: sed 정규식 오류"
  done
done
[ "$fail" = 0 ] || { echo "공개 저장소 검사 실패: 검사 패턴을 고친다."; exit 2; }

# $1=설명, $2..=검사할 rev(없으면 작업 트리의 추적 파일). 커밋 메시지는 COMMITS 변수로 검사한다.
scan() {
  local label="$1" i hits
  shift
  for i in "${!NAMES[@]}"; do
    if [ "$label" = "files" ]; then
      hits=$(git grep -I -n -E -e "${RES[$i]}" "$@" -- . ':!scripts/check-public.sh' 2>"$ERR")
    else
      hits=$(grep -n -E -e "${RES[$i]}" <<<"$COMMITS" 2>"$ERR")
    fi
    [ $? -le 1 ] || { broke "${NAMES[$i]} ($label) 검사가 돌지 못했다"; continue; }
    # 허용 문자열만 지운 뒤에도 패턴이 남는 줄만 남긴다(한 줄에 허용·금지가 섞여도 놓치지 않는다)
    [ -n "${ALLOWS[$i]}" ] && hits=$(printf '%s\n' "$hits" | sed -E "s~${ALLOWS[$i]}~~g" | grep -E -e "${RES[$i]}")
    [ -n "${SKIPS[$i]}" ] && [ "$label" = "files" ] && hits=$(printf '%s\n' "$hits" | grep -v -E -e "${SKIPS[$i]}")
    hits=$(printf '%s\n' "$hits" | sed '/^$/d')
    if [ -n "$hits" ] && [ -n "$KNOWN_HASHES" ] && [ "$label" = "files" ]; then
      local line rest path kept="" n=0
      while IFS= read -r line; do
        rest=${line#*:}
        path=${rest%%:*}
        rest=${rest#*:}
        if known "$path" "${rest#*:}"; then n=$((n + 1)); else kept="${kept}${line}"$'\n'; fi
      done <<<"$hits"
      KNOWN_SKIPPED=$((KNOWN_SKIPPED + n))
      hits=$(sed '/^$/d' <<<"$kept")
    fi
    if [ -n "$hits" ]; then
      report "${NAMES[$i]} ($label)"
      head -20 <<<"$hits" | cut -c1-200 | sed 's/^/    /'
    fi
  done
}
# shellcheck disable=SC2086
scan files $REVS
COMMITS=""
if [ "$HISTORY" = 1 ]; then
  COMMITS=$(git log --no-walk=unsorted --format='%H %B' $REVS 2>"$ERR") || broke "커밋 메시지를 읽지 못했다"
  COMMITS=$(grep -v -E '^Co-Authored-By: ' <<<"$COMMITS")
  scan "커밋 메시지(이력)"
elif git rev-parse -q --verify "$BASE" >/dev/null; then
  COMMITS=$(git log --format='%H %B' "$BASE..HEAD" 2>"$ERR") || broke "커밋 메시지를 읽지 못했다"
  COMMITS=$(grep -v -E '^Co-Authored-By: ' <<<"$COMMITS")
  [ -n "$COMMITS" ] && scan "커밋 메시지 $BASE..HEAD"
elif [ -n "${CI:-}" ]; then
  # CI의 통과가 커밋 메시지·author를 본 결과이게 한다(얕은 checkout이면 기준이 없어 늘 건너뛰었다)
  report "기준 ref ${BASE}가 없다: CI는 그 push·PR의 커밋을 볼 수 있게 이력을 받는다(ci.yml checkout fetch-depth)"
else
  echo "기준 ref ${BASE}가 없어 커밋 메시지는 건너뛴다"
fi

# 5) 커밋 author·committer. 공개 이력은 메타데이터도 그대로 공개된다. 출력에는 email의 domain만 보인다(CI annotation도 공개다)
IDENTS=""
if [ "$HISTORY" = 1 ]; then
  # shellcheck disable=SC2086
  IDENTS=$(git log --no-walk=unsorted --format='%h|%an|%ae|%cn|%ce' $REVS 2>"$ERR") || broke "author·committer를 읽지 못했다"
elif git rev-parse -q --verify "$BASE" >/dev/null; then
  IDENTS=$(git log --format='%h|%an|%ae|%cn|%ce' "$BASE..HEAD" 2>"$ERR") || broke "author·committer를 읽지 못했다"
fi
noreply() { case "$1" in *@users.noreply.github.com | noreply@github.com) return 0 ;; esac; return 1; }
bad=""
while IFS='|' read -r h _ ae _ ce; do
  [ -n "$h" ] || continue
  noreply "$ae" || bad="${bad}${h} author …@${ae#*@}"$'\n'
  noreply "$ce" || bad="${bad}${h} committer …@${ce#*@}"$'\n'
done <<<"$IDENTS"
if [ -n "$bad" ]; then
  report "커밋 author·committer email이 GitHub noreply가 아니다(git config user.email을 …@users.noreply.github.com으로 둔다)"
  printf '%s' "$bad" | head -10 | sed 's/^/    /'
fi
IDENT_NAMES=$(cut -d'|' -f1,2,4 <<<"$IDENTS")

# 4) 저장소 밖 목록의 실제 이름(대소문자 무시)
WORDS="$(git rev-parse --git-common-dir)/info/private-words"
if [ -f "$WORDS" ]; then
  while IFS= read -r w; do
    if [ -z "$w" ] || [ "${w#\#}" != "$w" ]; then continue; fi
    # shellcheck disable=SC2086
    hits=$(git grep -I -n -i -F -e "$w" $REVS -- . ':!scripts/check-public.sh' 2>"$ERR")
    [ $? -le 1 ] || broke "실제 이름 검사가 돌지 못했다"
    [ -n "$hits" ] && { report "실제 이름(private-words의 한 항목)"; printf '%s\n' "$hits" | head -10 | cut -c1-200 | sed 's/^/    /'; }
    if [ -n "$COMMITS" ] && grep -q -i -F -e "$w" <<<"$COMMITS"; then
      report "실제 이름이 커밋 메시지에 있다(private-words의 한 항목)"
      grep -i -F -e "$w" <<<"$COMMITS" | head -5 | cut -c1-200 | sed 's/^/    /'
    fi
    if [ -n "$IDENTS" ] && grep -q -i -F -e "$w" <<<"$IDENT_NAMES"; then
      report "실제 이름이 커밋 author·committer 이름에 있다(private-words의 한 항목)"
      grep -i -F -e "$w" <<<"$IDENT_NAMES" | head -5 | cut -d'|' -f1 | sed 's/^/    /'
    fi
  done <"$WORDS"
else
  echo "참고: ${WORDS}가 없어 실제 이름 검사는 건너뛴다(머신 hostname·tailnet 이름을 한 줄씩 적어 둔다)"
fi

# 6) 실제 세션 id(로컬 전용). 시험 값에 실제 Codex 세션 id를 옮겨 넣은 일이 있었다(10-18 감사, chat.rs).
#    id만으로 기록에 닿을 수는 없지만, 그 머신에서 그날 그 도구를 썼다는 것이 드러난다.
UUID_RE='[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}'
SESSION_DIRS="${CHECK_PUBLIC_SESSION_DIRS-$HOME/.claude/projects:$HOME/.codex/sessions}"
SDIRS=()
IFS=: read -r -a SDIRS <<<"$SESSION_DIRS"
KNOWN="" SEEN_DIRS=""
for d in ${SDIRS[@]+"${SDIRS[@]}"}; do
  [ -d "$d" ] || continue
  SEEN_DIRS="${SEEN_DIRS} ${d}"
  names=$(find "$d" -print 2>"$ERR") || { broke "세션 기록 이름을 읽지 못했다(${d})"; continue; }
  ids=$(grep -o -E -e "$UUID_RE" <<<"$names" 2>"$ERR")
  [ $? -le 1 ] || { broke "세션 기록 이름 검사가 돌지 못했다"; continue; }
  KNOWN="${KNOWN}${ids}"$'\n'
done
if [ -n "$SEEN_DIRS" ]; then
  # shellcheck disable=SC2086
  ids=$(git grep -I -h -o -E -e "$UUID_RE" $REVS -- . ':!scripts/check-public.sh' 2>"$ERR")
  [ $? -le 1 ] || broke "세션 id 검사(files)가 돌지 못했다"
  msg_ids=$(grep -o -E -e "$UUID_RE" <<<"$COMMITS" 2>"$ERR")
  [ $? -le 1 ] || broke "세션 id 검사(커밋 메시지)가 돌지 못했다"
  norm() { tr 'A-F' 'a-f' | sed '/^$/d' | LC_ALL=C sort -u; }
  hits=$(LC_ALL=C comm -12 <(printf '%s\n%s\n' "$ids" "$msg_ids" | norm) <(printf '%s\n' "$KNOWN" | norm))
  if [ -n "$hits" ] && [ -n "$KNOWN_HASHES" ]; then
    kept=""
    while IFS= read -r id; do
      if known session "$id"; then KNOWN_SKIPPED=$((KNOWN_SKIPPED + 1)); else kept="${kept}${id}"$'\n'; fi
    done <<<"$hits"
    hits=$(sed '/^$/d' <<<"$kept")
  fi
  if [ -n "$hits" ]; then
    report "실제 세션 id(이 머신의 Claude Code·Codex 세션 기록 이름과 같다): 지어낸 id로 바꾼다"
    while IFS= read -r id; do
      # shellcheck disable=SC2086
      where=$(git grep -I -l -i -F -e "$id" $REVS -- . ':!scripts/check-public.sh' 2>/dev/null | head -5 | tr '\n' ' ')
      grep -q -i -F -e "$id" <<<"$COMMITS" && where="${where}(커밋 메시지)"
      echo "    ${id:0:8}…: ${where}"
    done <<<"$hits"
  fi
else
  echo "참고: 세션 기록 폴더(${SESSION_DIRS})가 없어 실제 세션 id 검사는 건너뛴다"
fi

[ "$KNOWN_SKIPPED" -eq 0 ] || echo "참고: 이미 공개된 이력 항목 ${KNOWN_SKIPPED}건은 ${KNOWN_FILE}에 있어 넘겼다"
if [ "$fail" -ne 0 ]; then
  echo "공개 저장소 검사 실패: 위 항목을 일반 이름·자리표시자로 바꾼 뒤 다시 돌린다."
  exit 1
fi
echo "공개 저장소 검사 통과"
