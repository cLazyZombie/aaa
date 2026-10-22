#!/usr/bin/env bash
# main 조상 커밋을 깨끗한 detached worktree에서 한 번 빌드한다. 운영 파일·서비스는 건드리지 않는다.
set -euo pipefail
usage() { echo '사용법: scripts/deploy-build.sh <commit> --out <빈 출력 폴더>'; }
[ "$#" = 3 ] && [ "$2" = --out ] || { usage >&2; exit 2; }
repo=$(git rev-parse --show-toplevel)
commit=$(git rev-parse --verify "$1^{commit}")
out=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$3")
[ ! -e "$out" ] || { echo '출력 폴더가 이미 있다: 덮어쓰지 않는다' >&2; exit 1; }
git fetch --no-tags origin main
git merge-base --is-ancestor "$commit" origin/main || { echo 'origin/main의 조상 커밋만 빌드한다' >&2; exit 1; }
origin_main=$(git rev-parse origin/main)
short=${commit:0:12}
# macOS system temp에서 build-script 실행이 SIGKILL된 적이 있다. TMPDIR·Cargo의
# 공용 target 설정과 무관하게 저장소의 무시된 target 아래에 매번 새 공간을 만든다.
# 다른 디스크가 필요하면 AAA_DEPLOY_BUILD_ROOT로 부모 폴더를 지정한다.
build_parent=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "${AAA_DEPLOY_BUILD_ROOT:-$repo/target/deploy-build}")
mkdir -p "$build_parent"
build_root=$(mktemp -d "$build_parent/aaa-deploy-build.XXXXXX")
cleanup() {
  git -C "$repo" worktree remove --force "$build_root/source" >/dev/null 2>&1 || true
  rm -rf "${build_root:?}"
}
trap cleanup EXIT
git worktree add --detach "$build_root/source" "$commit"
(
  cd "$build_root/source/web"
  npm ci --no-audit --no-fund
  npm run build
)
(
  cd "$build_root/source"
  # kache의 dSYM 수집 전에 rustc가 object를 지우지 않도록 unpacked를 유지한다.
  if [ "$(uname -s)" = Darwin ]; then export CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=unpacked; fi
  AAA_BUILD_SALT="$short" CARGO_TARGET_DIR="$build_root/target" cargo build --release --locked -p aaa --bin aaa
)
# 캐시 도구가 없는 환경에서는 남은 object로 dSYM을 직접 만든다. 실행 파일은 수정하지 않는다.
if [ "$(uname -s)" = Darwin ] && [ ! -d "$build_root/target/release/aaa.dSYM" ]; then
  (cd "$build_root/target/release" && dsymutil aaa) || { echo 'macOS 배포 후보 dSYM 생성 실패' >&2; exit 1; }
fi
mkdir -p "$out"
cp "$build_root/target/release/aaa" "$out/aaa"
# macOS 줄 정보도 임시 worktree를 지우기 전에 보존한다.
# 실행 파일을 수정하거나 운영 머신에 설치하지 않는다. Cargo의 dSYM symlink는 실제 폴더로 복사한다.
if [ "$(uname -s)" = Darwin ] && [ ! -d "$build_root/target/release/aaa.dSYM" ]; then
  echo 'macOS 배포 후보의 dSYM이 없다' >&2
  exit 1
fi
if [ -d "$build_root/target/release/aaa.dSYM" ]; then
  cp -RL "$build_root/target/release/aaa.dSYM" "$out/aaa.dSYM"
fi
# provenance는 기계 이름·절대 빌드 경로를 담지 않는다.
python3 - "$out" "$commit" "$short" "$origin_main" <<'PY'
import datetime, hashlib, json, pathlib, subprocess, sys
out, commit, short, main = sys.argv[1:]
bin = pathlib.Path(out) / 'aaa'
version = subprocess.check_output([str(bin), '--version'], text=True).strip()
if not version.endswith('+' + short):
    raise SystemExit('후보 --version에 커밋 표시가 없다: ' + version)
data = dict(commit=commit, origin_main=main, sha256=hashlib.sha256(bin.read_bytes()).hexdigest(), software=version,
            built_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
            tools={k: subprocess.check_output(args, text=True).strip() for k,args in
                   dict(rustc=['rustc','--version'], cargo=['cargo','--version'], node=['node','--version'], npm=['npm','--version']).items()})
(out := pathlib.Path(out)).joinpath('provenance.json').write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
print(json.dumps(data, ensure_ascii=False))
PY
