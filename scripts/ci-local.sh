#!/usr/bin/env bash
# CI 단계를 로컬에서 한 번에 돈다. 단계의 명령은 여기에만 있다: .github/workflows/ci.yml도 이 스크립트를
# `--only <단계>`로 부른다(OS별로 어느 단계를 도는지만 ci.yml이 정한다). 그래서 로컬과 CI의 명령이 어긋날 수 없다.
#
#   bash scripts/ci-local.sh                       전부(기본)
#   bash scripts/ci-local.sh --only test,e2e       고른 단계만
#   bash scripts/ci-local.sh --skip e2e            빼고 전부
#   bash scripts/ci-local.sh --logs DIR            로그 폴더(기본 target/ci-local/<시각>)
#   bash scripts/ci-local.sh --native-clipboard    clipboard 단계를 이 머신의 실제 clipboard로 돈다
#   bash scripts/ci-local.sh --tee                 단계 출력을 로그와 함께 화면에도 낸다(CI가 쓴다)
#
# 단계(순서대로): web fmt build clippy test clipboard e2e public
#   web        웹 front 빌드(npm ci·build)와 순수 규칙 유닛 시험(npm test, node --test). host 실행 파일에 들어간다
#   fmt        cargo fmt --all --check
#   build      cargo build --workspace --all-targets --locked
#   clippy     cargo clippy --workspace --all-targets --locked -- -D warnings
#   test       cargo test --workspace --locked
#   clipboard  실제 시스템 clipboard 시험(§8.28). 사람의 clipboard를 덮어쓰므로 AAA_CLIP_NATIVE=1(CI)이나
#              --native-clipboard일 때만 돈다. 아니면 「건너뜀」으로 요약에 남긴다. Linux는 xvfb-run 위에서.
#   e2e        웹 E2E(web/e2e/web.e2e.mjs, Playwright Chromium·WebKit, 한글 IME 기록 재생 포함). Unix만.
#              playwright는 전역 npm 모듈로 찾는다(NODE_PATH=$(npm root -g)).
#              target/debug/aaa는 스스로 빌드하고(바뀐 것이 없으면 그대로), web 없이 돌면 web/dist가 웹 소스보다 새것인지 본다.
#   public     scripts/check-public.sh(공개 저장소 검사)
#
# 실패한 단계는 다시 돌리지 않는다. 앞 단계(web·build)가 실패하면 그것에 기대는 단계는 「앞 단계 실패」로 건너뛴다.
# 끝에 단계마다 시작·끝 시각, 걸린 시간, 결과, 실패한 단계의 로그 위치를 요약하고, 하나라도 실패하면 1로 끝난다.
# 이 스크립트는 자식 프로세스에 신호를 보내지 않는다(각 명령은 앞에서 돌고 끝나기를 기다린다).
set -uo pipefail

ALL="web fmt build clippy test clipboard e2e public"
cd "$(dirname "$0")/.." || exit 2
ROOT=$(pwd)

ONLY=""
SKIP=""
LOGS=""
NATIVE_CLIP="${AAA_CLIP_NATIVE:-}"
TEE=""
usage() { sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'; }
while [ $# -gt 0 ]; do
  case "$1" in
    --only) ONLY=$(echo "${2:-}" | tr ',' ' '); shift 2 ;;
    --only=*) ONLY=$(echo "${1#*=}" | tr ',' ' '); shift ;;
    --skip) SKIP=$(echo "${2:-}" | tr ',' ' '); shift 2 ;;
    --skip=*) SKIP=$(echo "${1#*=}" | tr ',' ' '); shift ;;
    --logs) LOGS="${2:-}"; shift 2 ;;
    --logs=*) LOGS="${1#*=}"; shift ;;
    --native-clipboard) NATIVE_CLIP=1; shift ;;
    --tee) TEE=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "모르는 인자: $1" >&2; usage >&2; exit 2 ;;
  esac
done
for s in $ONLY $SKIP; do
  case " $ALL " in *" $s "*) ;; *) echo "모르는 단계: $s (단계: $ALL)" >&2; exit 2 ;; esac
done
[ -n "$LOGS" ] || LOGS="target/ci-local/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOGS" || exit 2
LOGS=$(cd "$LOGS" && pwd)

case "$(uname -s)" in
  Darwin) OS=macos ;;
  Linux) OS=linux ;;
  MINGW*|MSYS*|CYGWIN*) OS=windows ;;
  *) OS=other ;;
esac

wanted() {
  local s=$1
  if [ -n "$ONLY" ]; then case " $ONLY " in *" $s "*) ;; *) return 1 ;; esac; fi
  case " $SKIP " in *" $s "*) return 1 ;; esac
  return 0
}

# ── 단계 명령(여기가 유일한 정의다) ──
step_web() { (cd web && npm ci --no-audit --no-fund && npm run build && npm test); }
step_fmt() { cargo fmt --all --check; }
step_build() { cargo build --workspace --all-targets --locked; }
step_clippy() { cargo clippy --workspace --all-targets --locked -- -D warnings; }
step_test() { cargo test --workspace --locked; }
step_clipboard() {
  local run=()
  [ "$OS" = linux ] && run=(xvfb-run -a)
  # -p aaa-tui만 고르면 의존 crate의 feature 조합이 test 단계(--workspace)와 달라 다시 컴파일한다. test 단계와 같은 선택에 이름만 거른다
  AAA_CLIP_NATIVE=1 "${run[@]+"${run[@]}"}" cargo test --locked --workspace native_clipboard_roundtrip -- --nocapture || return 1
  # TUI E2E는 Unix 전용이다
  [ "$OS" = windows ] && return 0
  AAA_CLIP_NATIVE=1 "${run[@]+"${run[@]}"}" cargo test --locked -p aaa --test tui ctrl_v_pastes_clipboard_image -- --nocapture
}
step_e2e() {
  local root newer
  # 옛 번들·옛 binary로 시험하지 않는다(--only e2e처럼 web·build 없이 돌 때). host 실행 파일은 web/dist를 넣어 빌드된다
  if [ -z "$(result_of web)" ]; then
    [ -f web/dist/index.html ] || { echo "web/dist가 없다: web 단계를 함께 돈다(--only web,e2e)"; return 1; }
    newer=$(find web/src web/public web/patches web/index.html web/package.json web/package-lock.json web/vite.config.ts \
      web/tsconfig.json web/xterm-source.mjs -newer web/dist/index.html -print 2>/dev/null | head -1)
    [ -z "$newer" ] || { echo "web/dist가 ${newer}보다 오래됐다: web 단계를 함께 돈다(--only web,e2e)"; return 1; }
  fi
  # e2e는 target/debug/aaa만 쓴다. build 단계 뒤라면 다시 컴파일하지 않는다(같은 binary)
  cargo build --locked -p aaa --bin aaa || return 1
  root=$(npm root -g) || return 1
  if ! NODE_PATH="$root" node -e 'require("playwright")' 2>/dev/null; then
    echo "playwright가 전역 npm 모듈에 없다: npm install -g playwright@1.59.1 && npx playwright@1.59.1 install chromium webkit"
    return 1
  fi
  NODE_PATH="$root" node web/e2e/web.e2e.mjs
}
step_public() { bash scripts/check-public.sh; }

# 이번 실행에서 그 단계가 실패했거나 앞 단계 실패로 건너뛰었다
broken() { case "$(result_of "$1")" in FAIL | 건너뜀:*) return 0 ;; esac; return 1; }

# 건너뛸 이유(없으면 빈 글)
skip_reason() {
  case "$1" in
    build) [ "$(result_of web)" = FAIL ] && echo "앞 단계 실패(web)" ;;
    clippy|test) broken build && echo "앞 단계 실패(build)" ;;
    clipboard)
      if broken build; then echo "앞 단계 실패(build)"
      elif [ -z "$NATIVE_CLIP" ]; then echo "사람의 clipboard를 덮어쓴다(--native-clipboard나 AAA_CLIP_NATIVE=1로 켠다)"
      fi ;;
    e2e)
      if [ "$OS" = windows ]; then echo "Unix만(웹 E2E)"
      elif broken web || broken build; then echo "앞 단계 실패(web·build)"
      fi ;;
  esac
}

RESULTS=""   # 줄마다: 단계|결과|시작|끝|초|로그
result_of() { printf '%s\n' "$RESULTS" | awk -F'|' -v s="$1" '$1==s{print $2}'; }
failed=""
started=$(date +%s)
echo "ci-local: $OS, 로그 $LOGS"
for s in $ALL; do
  wanted "$s" || continue
  why=$(skip_reason "$s")
  now=$(date +%H:%M:%S)
  if [ -n "$why" ]; then
    echo "── $s: 건너뜀($why)"
    RESULTS="$RESULTS$s|건너뜀: $why|$now|$now|0|
"
    continue
  fi
  log="$LOGS/$s.log"
  echo "── $s 시작 $now(로그 $log)"
  t0=$(date +%s)
  if [ -n "$TEE" ]; then
    "step_$s" 2>&1 | tee "$log"
    code=$?
  else
    "step_$s" >"$log" 2>&1
    code=$?
  fi
  t1=$(date +%s)
  end=$(date +%H:%M:%S)
  if [ $code -eq 0 ]; then
    r=ok
  else
    r=FAIL
    failed="$failed $s"
    [ -n "$TEE" ] || tail -20 "$log" | sed 's/^/   │ /'
    # GitHub Actions: 실패 줄을 annotation으로 남긴다. job 로그는 로그인해야 받지만 annotation은 누구나 읽는다.
    if [ "${GITHUB_ACTIONS:-}" = true ]; then
      { grep -E 'FAIL|panicked|^error' "$log" | tail -8; tail -5 "$log"; } |
        awk 'BEGIN { ORS = "%0A" } { gsub(/%/, "%25"); gsub(/\r/, "%0D"); print }' |
        sed "s/^/::error title=ci-local $s::/"
      echo
    fi
  fi
  echo "── $s $r $end($((t1 - t0))초)"
  RESULTS="$RESULTS$s|$r|$now|$end|$((t1 - t0))|$log
"
done

echo
echo "ci-local 요약($OS, 모두 $(( $(date +%s) - started ))초, 로그 $LOGS)"
printf '%s' "$RESULTS" | awk -F'|' 'NF>1 { printf "  %-9s %-8s %s~%s %5s초  %s\n", $1, ($2=="ok"||$2=="FAIL")?$2:"-", $3, $4, $5, ($2=="ok")?"":($2=="FAIL")?$6:$2 }'
if [ -n "$failed" ]; then
  echo "실패:$failed"
  exit 1
fi
echo "모두 통과(건너뛴 단계는 위에 이유가 있다)"
