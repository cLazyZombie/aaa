#!/usr/bin/env bash
# CI 단계를 로컬에서 한 번에 돈다. 단계의 명령은 여기에만 있다: .github/workflows/ci.yml도 이 스크립트를
# `--only <단계>`로 부른다(OS별로 어느 단계를 도는지만 ci.yml이 정한다). 그래서 로컬과 CI의 명령이 어긋날 수 없다.
#
#   bash scripts/ci-local.sh                       전부(기본)
#   bash scripts/ci-local.sh --only test,e2e       고른 단계만
#   bash scripts/ci-local.sh --skip e2e            빼고 전부
#   bash scripts/ci-local.sh --logs DIR            로그 폴더(기본 target/ci-local/<시각>)
#   bash scripts/ci-local.sh --tee                 단계 출력을 로그와 함께 화면에도 낸다(CI가 쓴다)
#   bash scripts/ci-local.sh --only linux-clippy   Zig로 Linux strict cross 검사(선택 단계)
#   bash scripts/ci-local.sh --only web,e2e --spec 23,109   개발 중 관련 E2E만
#
# 기본: web fmt build test e2e public. test·e2e가 CPU 예산을 나눠 함께 돈다.
# clippy는 개발 owner·CI가 명시해서 실행한다. 함께 고르면 준비 뒤 먼저 끝내 Cargo 잠금을 나눈다.
# 1 CPU는 test·e2e도 직렬 실행하며 끝나면 public이다.
#   web        웹 front 빌드(npm ci·build)와 순수 규칙 유닛 시험(npm test, node --test). host 실행 파일에 들어간다
#   fmt        cargo fmt --all --check
#   build      verify 실행 파일·unit 사전 컴파일(Windows release cfg도 check)
#   clippy     cargo clippy --profile verify --workspace --all-targets --locked --keep-going -- -D warnings
#   test       Python 검사와 전체 Rust 시험을 함께 실행한다. 독립 owner 분할과 doctest도 유지한다
#   e2e        웹 E2E(web/e2e/web.e2e.mjs, Playwright Chromium·WebKit, 한글 IME 기록 재생 포함). Unix만.
#              playwright는 전역 npm 모듈로 찾는다(NODE_PATH=$(npm root -g)).
#              전체 실행은 Rust와 CPU를 나누며, E2E만 실행하면 core 둘마다 하나·최대6 worker다.
#              AAA_E2E_WORKERS=1이면 차례로.
#              병렬 단계 전에 이번 build의 실행 파일을 별도 사본으로 고정한다(test의 Cargo 링크 갱신과 겹치지 않는다).
#              아니면 스스로 빌드하고(바뀐 것이 없으면 그대로), web 없이 돌면 web/dist가 웹 소스보다 새것인지 본다.
#   public     scripts/check-public.sh(공개 저장소 검사). AAA_PUBLIC_BASE가 있으면 그 ref 이후 커밋의 메시지·author를 본다
#              (없으면 origin/main. CI는 push의 before나 PR base를 준다)
#
# 실패한 단계는 다시 돌리지 않는다. 앞 단계(web·build)가 실패하면 그것에 기대는 단계는 「앞 단계 실패」로 건너뛴다.
# 끝에 단계마다 시작·끝 시각, 걸린 시간, 결과, 실패한 단계의 로그 위치를 요약하고, 하나라도 실패하면 1로 끝난다.
# 이 스크립트는 자식 프로세스에 신호를 보내지 않는다(각 명령이 끝나기를 기다린다. 함께 띄운 단계는 모두 끝날 때까지 wait).
set -uo pipefail

ALL="web fmt build test e2e public"
KNOWN="$ALL clippy linux-clippy"
cd "$(dirname "$0")/.." || exit 2
ROOT=$(pwd)
# 고정 binary 입력 경로와 Cargo 출력을 맞춰 외부 target의 낡은 파일을 쓰지 않는다.
export CARGO_TARGET_DIR="$ROOT/target"

ONLY=""
SKIP=""
LOGS=""
TEE=""
E2E_SPEC=""
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
    --spec) E2E_SPEC="${2:-}"; [ -n "$E2E_SPEC" ] || { echo '--spec 뒤에 번호가 필요하다' >&2; exit 2; }; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "모르는 인자: $1" >&2; usage >&2; exit 2 ;;
  esac
done
if [ -n "$E2E_SPEC" ]; then
  case " $ONLY " in *" e2e "*) ;; *) echo '--spec은 --only e2e를 명시한 개발 검사에서만 쓴다' >&2; exit 2 ;; esac
  echo "개발 관련 검사: E2E $E2E_SPEC (전체 gate 아님)"
fi
for s in $ONLY $SKIP; do
  case " $KNOWN " in *" $s "*) ;; *) echo "모르는 단계: $s (단계: $KNOWN)" >&2; exit 2 ;; esac
done
# cross 도구·target이 없는 기본 CI에는 추가하지 않는다. 명시한 실행만 기존 집계에 넣는다.
case " $ONLY " in *" linux-clippy "*) ALL="$ALL linux-clippy" ;; esac
case " $ONLY " in *" clippy "*) ALL="$ALL clippy" ;; esac
[ -n "$LOGS" ] || LOGS="target/ci-local/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$LOGS" || exit 2
LOGS=$(cd "$LOGS" && pwd)

case "$(uname -s)" in
  Darwin) OS=macos ;;
  Linux) OS=linux ;;
  MINGW*|MSYS*|CYGWIN*) OS=windows ;;
  *) OS=other ;;
esac

# 격리한 시험 CLI·host·agent마다 머신 전체 코어 수의 runtime을 만들지 않는다.
# Windows native owner의 기존 환경은 유지한다.
if [ "$OS" != windows ]; then export TOKIO_WORKER_THREADS=${TOKIO_WORKER_THREADS:-2}; fi

wanted() {
  local s=$1
  case " $ALL " in *" $s "*) ;; *) return 1 ;; esac
  if [ -n "$ONLY" ]; then case " $ONLY " in *" $s "*) ;; *) return 1 ;; esac; fi
  case " $SKIP " in *" $s "*) return 1 ;; esac
  return 0
}

# Cargo 빌드의 병렬도는 바꾸지 않는다. 실행 단계만 같은 CPU를 중복 예약하지 않게 나눈다.
CI_CPUS=1
if wanted test || wanted e2e || { wanted web && wanted fmt; }; then
  ci_rust=0; wanted test && ci_rust=1
  ci_web=0; wanted e2e && [ "$OS" != windows ] && ci_web=1
  python3 scripts/ci_resources.py "$ci_rust" "$ci_web" >"$LOGS/resources.tsv" || exit 2
  IFS='|' read -r CI_CPUS ci_rust_slots ci_web_workers <"$LOGS/resources.tsv"
  if [ "$ci_rust" -eq 1 ]; then
    export AAA_RUST_TEST_CPU_BUDGET=${AAA_RUST_TEST_CPU_BUDGET:-$ci_rust_slots}
    export AAA_RUST_TEST_WORKERS=${AAA_RUST_TEST_WORKERS:-$CI_CPUS}
    if [ "$OS" != windows ]; then export RUST_TEST_THREADS=${RUST_TEST_THREADS:-1}; fi
  fi
  if [ "$ci_web" -eq 1 ]; then export AAA_E2E_WORKERS=${AAA_E2E_WORKERS:-$ci_web_workers}; fi
fi

# ── 단계 명령(여기가 유일한 정의다) ──
step_web() { python3 scripts/check_fixture_secrets.py && (cd web && npm ci --no-audit --no-fund && npm run build && npm test); }
step_fmt() { cargo fmt --all --check; }
# Windows release 전용 cfg는 check로 확인하고 내부 실행 파일은 verify로 빌드한다.
# 실제 배포 최적화·PDB 검증은 release workflow가 담당한다.
step_build() {
  if [ "$OS" = windows ]; then
    cargo check --profile release --locked -p aaa --bin aaa --target x86_64-pc-windows-msvc || return $?
  fi
  # macOS 전체 실행은 integration test의 CLI를 공유한다. 단독 build와 다른 OS의
  # SSH/native owner가 요구하는 기존 e2e 산출물은 같은 명령으로 만든다.
  if [ "$OS" != macos ] || ! wanted test || ! wanted e2e; then
    cargo build --profile e2e --locked -p aaa --bin aaa || return $?
  fi
  # integration test가 쓰는 실제 verify/aaa를 웹에도 고정 복사한다. 같은 CLI를 두 profile로 만들지 않는다.
  # cold unit codegen을 headless와 겹치지 않는다. test의 no-run은 산출물 목록만 다시 모은다.
  cargo test --profile verify --workspace --locked --no-fail-fast --no-run
}
# 한 crate가 실패해도 나머지를 모두 본다: 아니면 cargo가 새 unit을 띄우지 않아 뒤 crate의 오류가 로그에 없다(10-18 windows: process.rs
# unused import가 9eeced3 뒤 main CI에서 clip.rs dead code를 가렸다)
step_clippy() { cargo clippy --profile verify --workspace --all-targets --locked --keep-going -- -D warnings; }
step_linux_clippy() { bash scripts/ci-linux-clippy.sh; }
# 한 binary가 실패해도 나머지를 모두 돈다: fail-fast면 앞 binary 실패가 뒤 crate 유닛 시험을 돌지 않게 해, main이 빨간 동안 그 결과가 어느 run에도 없다
step_test() {
  python3 scripts/check_fixture_secrets.py || return $?
  if [ "$OS" = windows ]; then
    pwsh -NoProfile -File scripts/test-windows-sshd-command.ps1 || return $?
    cargo test --profile verify --workspace --locked --no-fail-fast; return $?
  fi
  local python_pid python_code=0 rust_code=0
  # 서로 산출물을 쓰지 않는 검사다. 어느 쪽이 실패해도 둘 다 끝까지 확인한다.
  python3 -m unittest scripts.test_fixture_secrets scripts.test_deploy_build scripts.test_check_public scripts.test_rust_tests scripts.test_build_watch scripts.test_release scripts.test_install_site scripts.test_ci_local scripts.test_ci_resources scripts.test_workspace_metrics scripts.test_workspace_record scripts.test_linux_cross scripts.test_short_ids scripts.test_case_phases scripts.test_verify scripts.test_windows_production >"$LOGS/python.log" 2>&1 &
  python_pid=$!
  python3 scripts/rust_tests.py --logs "$LOGS/rust" || rust_code=$?
  wait "$python_pid" || python_code=$?
  # 실패를 흉내 낸 fixture도 출력한다. 바깥 suite의 실제 종료 코드를 각 줄에 붙여
  # 실패 수집기와 annotation 재출력에서도 성공한 Python 시험의 자식 출력을 구분한다.
  awk -v code="$python_code" '{ print "[python exit=" code "] " $0 }' "$LOGS/python.log"
  # 긴 다른 쪽의 출력 뒤에도 실패한 검사를 CI annotation에서 찾을 수 있게 남긴다.
  if [ "$python_code" -ne 0 ]; then echo "FAIL python: exit=$python_code, 로그 $LOGS/python.log"; fi
  if [ "$rust_code" -ne 0 ]; then echo "FAIL test: exit=$rust_code, 로그 $LOGS/rust"; fi
  [ "$python_code" -eq 0 ] && [ "$rust_code" -eq 0 ]
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
  local source="$ROOT/target/e2e/aaa"
  if [ "$OS" = macos ] && [ "$(result_of build)" = ok ] && wanted test; then source="$ROOT/target/verify/aaa"; fi
  # 병렬 검증의 작업 파일과 분리해 이번 E2E가 실행할 바이트를 고정한다.
  if [ "$(result_of build)" != ok ]; then
    cargo build --locked --profile e2e -p aaa --bin aaa || return 1
    source="$ROOT/target/e2e/aaa"
  fi
  E2E_BIN_DIR=$(mktemp -d "$ROOT/target/aaa-ci-e2e.XXXXXX") || return 1
  E2E_BIN="$E2E_BIN_DIR/aaa"
  cp "$source" "$E2E_BIN" || return 1
  cmp "$source" "$E2E_BIN" || return 1
  echo "이번 빌드의 웹 E2E 실행 사본: $E2E_BIN"
}
step_e2e() {
  local root newer built
  set --
  if [ -n "$E2E_SPEC" ]; then set -- --spec "$E2E_SPEC"; fi
  if [ -n "$E2E_PREP_FAILED" ]; then cat "$LOGS/e2e-binary.log"; return 1; fi
  # 옛 번들·옛 binary로 시험하지 않는다(--only e2e처럼 web·build 없이 돌 때). host 실행 파일은 web/dist를 넣어 빌드된다
  if [ -z "$(result_of web)" ]; then
    [ -f web/dist/index.html ] || { echo "web/dist가 없다: web 단계를 함께 돈다(--only web,e2e)"; return 1; }
    built=web/node_modules/.cache/aaa-web-build.stamp
    [ -f "$built" ] || built=web/dist/index.html
    newer=$(find web/src web/public web/patches web/scripts web/index.html web/package.json web/package-lock.json web/vite.config.ts \
      web/tsconfig.json web/xterm-source.mjs -newer "$built" -print 2>/dev/null | head -1)
    [ -z "$newer" ] || { echo "web/dist가 ${newer}보다 오래됐다: web 단계를 함께 돈다(--only web,e2e)"; return 1; }
  fi
  root=${NODE_PATH:-$(npm root -g)} || return 1
  if ! NODE_PATH="$root" node -e 'const v = require("playwright/package.json").version; console.log(`Playwright ${v}`); process.exit(v === "1.64.1" ? 0 : 1)' 2>/dev/null; then
    echo "Playwright 1.64.1이 필요하다: npm install -g playwright@1.64.1 && npx playwright@1.64.1 install chromium webkit"
    return 1
  fi
  AAA_BIN="$E2E_BIN" AAA_E2E_ARTIFACTS="$LOGS/e2e-failures" NODE_PATH="$root" node web/e2e/web.e2e.mjs "$@"
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
    "step_${s//-/_}" 2>&1 | tee "$log"
    code=$?
  else
    "step_${s//-/_}" >"$log" 2>&1
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
    python3 scripts/ci_failure_excerpt.py "$log" |
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

# clippy가 끝난 뒤 실행만 겹친다. Rust 준비·doctest가 Cargo 잠금을 다투지 않는다.
TOGETHER="test e2e"

started=$(date +%s)
python3 scripts/workspace_metrics.py start "$LOGS" || true
echo "ci-local: $OS, 로그 $LOGS"
printf '%s\n' "$E2E_SPEC" >"$LOGS/e2e-selection.txt"
# 포맷은 읽기 전용이며 npm의 출력과 독립이다. 1 CPU에서는 기존처럼 직렬 실행한다.
if [ "$CI_CPUS" -gt 1 ] && wanted web && wanted fmt; then
  run_step fmt &
  fmt_pid=$!
  run_step web; collect web
  wait "$fmt_pid"
  collect fmt
else
  for s in web fmt; do skipped "$s" || { run_step "$s"; collect "$s"; }; done
fi
for s in build clippy; do skipped "$s" || { run_step "$s"; collect "$s"; }; done
go=""
for s in $TOGETHER; do skipped "$s" || go="$go $s"; done
unset AAA_RUST_TEST_RELEASE_FILE
case " $go " in
  *" test e2e "*) export AAA_RUST_TEST_RELEASE_FILE="$LOGS/.e2e.result" ;;
esac
# 같은 로그 폴더를 다시 써도 이전 완료를 자원 반환으로 보지 않는다.
for s in $go; do rm -f "$LOGS/.$s.result"; done
case " $go " in
  *" e2e "*) prepare_e2e_bin >"$LOGS/e2e-binary.log" 2>&1 || E2E_PREP_FAILED=1 ;;
esac
if [ "$CI_CPUS" -eq 1 ] || [ "$(echo $go | wc -w)" -lt 2 ]; then
  for s in $ALL; do
    case " $go " in *" $s "*) run_step "$s"; collect "$s" ;; esac
  done
else
  echo "── 함께:$go"
  # 이전 실행의 로그·완료 표지를 reader가 읽지 않게 먼저 초기화한다.
  stream_done="$LOGS/.stream-done"
  rm -f "$stream_done"
  for s in $go; do : >"$LOGS/$s.log"; done
  stream_pid=""
  if [ -n "$TEE" ]; then
    python3 scripts/ci_stream.py "$LOGS" "$stream_done" $go &
    stream_pid=$!
  fi
  # 명령의 stdout/stderr는 로컬 로그에만 쓴다. 화면의 속도·종료가 검증 프로세스에 영향을 주지 않는다.
  step_pids=""
  for s in $go; do
    TEE="" run_step "$s" &
    step_pids="$step_pids $!"
  done
  for step_pid in $step_pids; do wait "$step_pid"; done
  : >"$stream_done"
  if [ -n "$stream_pid" ]; then
    wait "$stream_pid" || failed="$failed 출력"
  fi
  rm -f "$stream_done"
  for s in $go; do collect "$s"; done
fi
for s in linux-clippy public; do
  skipped "$s" || { run_step "$s"; collect "$s"; }
done

echo
finished=$(date +%s)
echo "ci-local 요약($OS, 모두 $(( finished - started ))초, 로그 $LOGS)"
# 이미 메모리에 있는 종료 결과만 보존한다. 수집·전송·추가 명령은 검증 완료 조건이 아니다.
metric_outcome=succeeded
[ -z "$failed" ] || metric_outcome=failed
{ printf 'v1|%s|%s|%s\n' "$started" "$((finished - started))" "$metric_outcome"; printf '%s' "$RESULTS"; } > "$LOGS/workspace-summary.tsv" || true
python3 scripts/workspace_metrics.py finish "$LOGS" || true
for s in $ALL; do
  printf '%s' "$RESULTS" | awk -F'|' -v s="$s" '$1==s { printf "  %-9s %-8s %s~%s %5s초  %s\n", $1, ($2=="ok"||$2=="FAIL")?$2:"-", $3, $4, $5, ($2=="ok")?"":($2=="FAIL")?$6:$2 }'
done
if [ -n "$failed" ]; then
  echo "실패:$failed"
  exit 1
fi
echo "모두 통과(건너뛴 단계는 위에 이유가 있다)"
