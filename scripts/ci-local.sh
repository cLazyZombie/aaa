#!/usr/bin/env bash
# CI 단계를 로컬에서 한 번에 돈다. 단계의 명령은 여기에만 있다: .github/workflows/ci.yml도 이 스크립트를
# `--only <단계>`로 부른다(OS별로 어느 단계를 도는지만 ci.yml이 정한다). 그래서 로컬과 CI의 명령이 어긋날 수 없다.
#
#   bash scripts/ci-local.sh                       전부(기본)
#   bash scripts/ci-local.sh --only test,e2e       고른 단계만
#   bash scripts/ci-local.sh --skip e2e            빼고 전부
#   bash scripts/ci-local.sh --logs DIR            로그 폴더(기본 target/ci-local/<시각>)
#   bash scripts/ci-local.sh --tee                 단계 출력을 로그와 함께 화면에도 낸다(CI가 쓴다)
#
# 단계: web fmt build clippy test e2e public. web·fmt·build를 차례로 돈 뒤 서로 기대지 않는 test·e2e·clippy를
# 함께 띄우고(test가 가장 긴 길이라 먼저), 끝나면 public. --tee(CI)거나 그 셋 중 하나만 돌면 차례로 돈다(출력이 섞이지 않게).
#   web        웹 front 빌드(npm ci·build)와 순수 규칙 유닛 시험(npm test, node --test). host 실행 파일에 들어간다
#   fmt        cargo fmt --all --check
#   build      cargo build --workspace --all-targets --locked
#   clippy     cargo clippy --workspace --all-targets --locked --keep-going -- -D warnings
#   test       전체 Cargo 시험을 준비한 뒤 binary·m13 분할을 병렬 실행하고 doctest도 모두 실행한다
#   e2e        웹 E2E(web/e2e/web.e2e.mjs, Playwright Chromium·WebKit, 한글 IME 기록 재생 포함). Unix만.
#              playwright는 전역 npm 모듈로 찾는다(NODE_PATH=$(npm root -g)).
#              spec 파일은 worker 여러 개가 함께 돈다(16 CPU 이상은 10개, 그 밖은 core 둘마다 하나). AAA_E2E_WORKERS=1이면 차례로.
#              병렬 단계 전에 이번 build의 실행 파일을 별도 사본으로 고정한다(test의 Cargo 링크 갱신과 겹치지 않는다).
#              아니면 스스로 빌드하고(바뀐 것이 없으면 그대로), web 없이 돌면 web/dist가 웹 소스보다 새것인지 본다.
#   public     scripts/check-public.sh(공개 저장소 검사). AAA_PUBLIC_BASE가 있으면 그 ref 이후 커밋의 메시지·author를 본다
#              (없으면 origin/main. CI는 push의 before나 PR base를 준다)
#
# 실패한 단계는 다시 돌리지 않는다. 앞 단계(web·build)가 실패하면 그것에 기대는 단계는 「앞 단계 실패」로 건너뛴다.
# 끝에 단계마다 시작·끝 시각, 걸린 시간, 결과, 실패한 단계의 로그 위치를 요약하고, 하나라도 실패하면 1로 끝난다.
# 이 스크립트는 자식 프로세스에 신호를 보내지 않는다(각 명령이 끝나기를 기다린다. 함께 띄운 단계는 모두 끝날 때까지 wait).
set -uo pipefail

ALL="web fmt build clippy test e2e public"
cd "$(dirname "$0")/.." || exit 2
ROOT=$(pwd)

ONLY=""
SKIP=""
LOGS=""
TEE=""
usage() { sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'; }
while [ $# -gt 0 ]; do
  case "$1" in
    --only) ONLY=$(echo "${2:-}" | tr ',' ' '); shift 2 ;;
    --only=*) ONLY=$(echo "${1#*=}" | tr ',' ' '); shift ;;
    --skip) SKIP=$(echo "${2:-}" | tr ',' ' '); shift 2 ;;
    --skip=*) SKIP=$(echo "${1#*=}" | tr ',' ' '); shift ;;
    --logs) LOGS="${2:-}"; shift 2 ;;
    --logs=*) LOGS="${1#*=}"; shift ;;
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
# 한 crate가 실패해도 나머지를 모두 본다: 아니면 cargo가 새 unit을 띄우지 않아 뒤 crate의 오류가 로그에 없다(10-18 windows: process.rs
# unused import가 9eeced3 뒤 main CI에서 clip.rs dead code를 가렸다)
step_clippy() { cargo clippy --workspace --all-targets --locked --keep-going -- -D warnings; }
# 한 binary가 실패해도 나머지를 모두 돈다: fail-fast면 앞 binary 실패가 뒤 crate 유닛 시험을 돌지 않게 해, main이 빨간 동안 그 결과가 어느 run에도 없다
step_test() {
  if [ "$OS" = windows ]; then cargo test --workspace --locked --no-fail-fast; return $?; fi
  python3 -m unittest scripts.test_deploy_build scripts.test_check_public scripts.test_rust_tests || return 1
  python3 scripts/rust_tests.py --logs "$LOGS/rust" || return 1
}
E2E_BIN_DIR=""
E2E_BIN=""
E2E_PREP_FAILED=""
cleanup_e2e_bin() {
  if [ -n "$E2E_BIN_DIR" ]; then
    rm -f "$E2E_BIN_DIR/aaa"
    rmdir "$E2E_BIN_DIR"
  fi
}
trap cleanup_e2e_bin EXIT
prepare_e2e_bin() {
  # Cargo는 fresh test 빌드에서도 target/debug/aaa를 지우고 다시 연결한다. 병렬 단계보다 먼저 사본을 만든다.
  if [ "$(result_of build)" != ok ]; then cargo build --locked -p aaa --bin aaa || return 1; fi
  E2E_BIN_DIR=$(mktemp -d /tmp/aaa-ci-e2e.XXXXXX) || return 1
  E2E_BIN="$E2E_BIN_DIR/aaa"
  cp "$ROOT/target/debug/aaa" "$E2E_BIN" || return 1
  cmp "$ROOT/target/debug/aaa" "$E2E_BIN" || return 1
  echo "이번 빌드의 웹 E2E 실행 사본: $E2E_BIN"
}
step_e2e() {
  local root newer
  if [ -n "$E2E_PREP_FAILED" ]; then cat "$LOGS/e2e-binary.log"; return 1; fi
  # 옛 번들·옛 binary로 시험하지 않는다(--only e2e처럼 web·build 없이 돌 때). host 실행 파일은 web/dist를 넣어 빌드된다
  if [ -z "$(result_of web)" ]; then
    [ -f web/dist/index.html ] || { echo "web/dist가 없다: web 단계를 함께 돈다(--only web,e2e)"; return 1; }
    newer=$(find web/src web/public web/patches web/index.html web/package.json web/package-lock.json web/vite.config.ts \
      web/tsconfig.json web/xterm-source.mjs -newer web/dist/index.html -print 2>/dev/null | head -1)
    [ -z "$newer" ] || { echo "web/dist가 ${newer}보다 오래됐다: web 단계를 함께 돈다(--only web,e2e)"; return 1; }
  fi
  root=$(npm root -g) || return 1
  if ! NODE_PATH="$root" node -e 'require("playwright")' 2>/dev/null; then
    echo "playwright가 전역 npm 모듈에 없다: npm install -g playwright@1.59.1 && npx playwright@1.59.1 install chromium webkit"
    return 1
  fi
  AAA_BIN="$E2E_BIN" AAA_E2E_ARTIFACTS="$LOGS/e2e-failures" NODE_PATH="$root" node web/e2e/web.e2e.mjs
}
step_public() { bash scripts/check-public.sh ${AAA_PUBLIC_BASE:+"$AAA_PUBLIC_BASE"}; }

# 이번 실행에서 그 단계가 실패했거나 앞 단계 실패로 건너뛰었다
broken() { case "$(result_of "$1")" in FAIL | 건너뜀:*) return 0 ;; esac; return 1; }

# 건너뛸 이유(없으면 빈 글)
skip_reason() {
  case "$1" in
    build) [ "$(result_of web)" = FAIL ] && echo "앞 단계 실패(web)" ;;
    clippy|test) broken build && echo "앞 단계 실패(build)" ;;
    e2e)
      if [ "$OS" = windows ]; then echo "Unix만(웹 E2E)"
      elif broken web || broken build; then echo "앞 단계 실패(web·build)"
      fi ;;
  esac
}

RESULTS=""   # 줄마다: 단계|결과|시작|끝|초|로그
result_of() { printf '%s\n' "$RESULTS" | awk -F'|' -v s="$1" '$1==s{print $2}'; }
failed=""

# 단계 하나를 돌고 결과 줄을 $LOGS/.<단계>.result에 남긴다(함께 띄운 단계는 부모 셸의 RESULTS를 바꿀 수 없다)
run_step() {
  local s=$1 log="$LOGS/$1.log" now end t0 t1 code r
  rm -f "$LOGS/.$s.result"
  now=$(date +%H:%M:%S)
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
  if [ $code -eq 0 ]; then r=ok; else r=FAIL; fi
  echo "── $s $r $end($((t1 - t0))초)"
  printf '%s|%s|%s|%s|%s|%s\n' "$s" "$r" "$now" "$end" "$((t1 - t0))" "$log" >"$LOGS/.$s.result"
}

# 끝난 단계의 결과를 RESULTS에 더한다. 실패면 로그 끝을 보이고 GitHub Actions에서는 annotation을 남긴다
collect() {
  local s=$1 log="$LOGS/$1.log"
  if [ -f "$LOGS/.$s.result" ]; then
    RESULTS="$RESULTS$(cat "$LOGS/.$s.result")
"
    rm -f "$LOGS/.$s.result"
  else
    # 결과를 남기기 전에 끝났다(함께 띄운 셸이 죽는 등): 요약에서 빠진 채 통과로 치지 않는다
    echo "── $s: 결과를 남기지 못하고 끝났다"
    RESULTS="$RESULTS$s|FAIL|-|$(date +%H:%M:%S)|0|$log
"
  fi
  [ "$(result_of "$s")" = FAIL ] || return 0
  failed="$failed $s"
  if [ -z "$TEE" ]; then
    echo "── $s 로그 끝:"
    tail -20 "$log" | sed 's/^/   │ /'
  fi
  # job 로그는 로그인해야 받지만 annotation은 누구나 읽는다.
  if [ "${GITHUB_ACTIONS:-}" = true ]; then
    { grep -E 'FAIL|panicked|^error' "$log" | tail -8; tail -5 "$log"; } |
      awk 'BEGIN { ORS = "%0A" } { gsub(/%/, "%25"); gsub(/\r/, "%0D"); print }' |
      sed "s/^/::error title=ci-local $s::/"
    echo
  fi
}

# 고른 단계면 건너뛸 이유를 RESULTS에 적고 0, 돌 단계면 1로 끝난다(고르지 않은 단계는 0, 아무것도 적지 않는다)
skipped() {
  local s=$1 why now
  wanted "$s" || return 0
  why=$(skip_reason "$s")
  [ -n "$why" ] || return 1
  now=$(date +%H:%M:%S)
  echo "── $s: 건너뜀($why)"
  RESULTS="$RESULTS$s|건너뜀: $why|$now|$now|0|
"
}

# build 뒤 서로 기대지 않는 단계. test가 가장 긴 길이라 먼저 띄우고, clippy는 cargo 잠금을 기다려도 끝이 늦어지지 않는 마지막
TOGETHER="test e2e clippy"

started=$(date +%s)
echo "ci-local: $OS, 로그 $LOGS"
for s in web fmt build; do
  skipped "$s" || { run_step "$s"; collect "$s"; }
done
go=""
for s in $TOGETHER; do skipped "$s" || go="$go $s"; done
case " $go " in
  *" e2e "*) prepare_e2e_bin >"$LOGS/e2e-binary.log" 2>&1 || E2E_PREP_FAILED=1 ;;
esac
if [ -n "$TEE" ] || [ "$(echo $go | wc -w)" -lt 2 ]; then
  for s in $ALL; do
    case " $go " in *" $s "*) run_step "$s"; collect "$s" ;; esac
  done
else
  echo "── 함께:$go"
  for s in $go; do run_step "$s" & done
  wait
  for s in $go; do collect "$s"; done
fi
for s in public; do
  skipped "$s" || { run_step "$s"; collect "$s"; }
done

echo
echo "ci-local 요약($OS, 모두 $(( $(date +%s) - started ))초, 로그 $LOGS)"
for s in $ALL; do
  printf '%s' "$RESULTS" | awk -F'|' -v s="$s" '$1==s { printf "  %-9s %-8s %s~%s %5s초  %s\n", $1, ($2=="ok"||$2=="FAIL")?$2:"-", $3, $4, $5, ($2=="ok")?"":($2=="FAIL")?$6:$2 }'
done
if [ -n "$failed" ]; then
  echo "실패:$failed"
  exit 1
fi
echo "모두 통과(건너뛴 단계는 위에 이유가 있다)"
