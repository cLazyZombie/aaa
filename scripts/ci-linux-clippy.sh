#!/usr/bin/env bash
# 기존 명시 설정·PATH 컴파일러를 우선하고, 없으면 repo의 Zig wrapper를 쓴다.
set -eu
cd "$(dirname "$0")/.."
export PATH="$PATH:$PWD/scripts/cross"
export CC_x86_64_unknown_linux_gnu="${CC_x86_64_unknown_linux_gnu:-${TARGET_CC:-${CC:-x86_64-linux-gnu-gcc}}}"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER:-x86_64-linux-gnu-gcc}"
exec cargo clippy --profile verify --workspace --all-targets --locked --keep-going --target x86_64-unknown-linux-gnu -- -D warnings
