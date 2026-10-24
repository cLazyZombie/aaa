#!/bin/sh
# Pages 빌드가 공개 release의 판·SHA256SUMS를 결속한다. 설치는 aaa setup이 맡는다.
main() (
    set -eu
    version='@AAA_VERSION@'
    base="https://github.com/cLazyZombie/aaa/releases/download/v$version"
    fail() { printf 'aaa bootstrap: %s\n' "$*" >&2; exit 2; }
    need() { command -v "$1" >/dev/null 2>&1 || fail "필요한 도구가 없습니다: $1"; }
    os=$(uname -s)
    case "$os" in
        Darwin) os=macos ;;
        Linux)
            [ "$(uname -o 2>/dev/null || true)" != Android ] || fail '지원하지 않는 OS: Android'
            os=linux ;;
        *) fail "지원하지 않는 OS: $os" ;;
    esac
    arch=$(uname -m)
    case "$arch" in
        x86_64|amd64) arch=x86_64 ;;
        arm64|aarch64) arch=aarch64 ;;
        *) fail "지원하지 않는 CPU: $arch" ;;
    esac
    need curl
    need awk
    if command -v sha256sum >/dev/null 2>&1; then
        digest() { sha256sum < "$1" | awk '{print $1}'; }
    elif command -v shasum >/dev/null 2>&1; then
        digest() { shasum -a 256 < "$1" | awk '{print $1}'; }
    else
        fail 'SHA-256 검증에 sha256sum 또는 shasum이 필요합니다'
    fi
    # curl | sh의 stdin은 스크립트다. setup 질문은 원래 터미널에서 받는다.
    if [ ! -t 0 ]; then
        if ( : </dev/tty ) 2>/dev/null; then
            exec </dev/tty
        else
            fail '설치할 사용자 계정의 대화형 터미널에서 실행하세요 (SSH는 ssh -t)'
        fi
    fi
    umask 077
    temp=$(mktemp -d "${TMPDIR:-/tmp}/aaa-bootstrap.XXXXXXXX")
    case "$temp" in /*) ;; *) fail '임시 폴더는 절대 경로여야 합니다' ;; esac
    trap 'rm -rf "${temp:?}"' 0
    trap 'exit 130' INT
    trap 'exit 143' TERM
    trap 'exit 129' HUP
    download() {
        curl --fail --show-error --silent --location --proto '=https' --proto-redir '=https' \
            --connect-timeout 15 --max-time 300 "$base/$1" --output "$2" || {
                code=$?
                fail "다운로드 실패: $1 (curl 종료 코드 $code)"
            }
    }
    pinned() {
        case "$1" in
@AAA_CHECKSUMS@
            *) fail "release 자산이 없습니다: $1" ;;
        esac
    }
    verified_download() {
        expected=$(pinned "$1")
        listed=$(awk -v name="$1" '$2 == name { count++; value=$1; if (NF != 2) bad=1 }
            END { if (count != 1 || bad || length(value) != 64 || value ~ /[^0-9a-fA-F]/) exit 1; print tolower(value) }' "$temp/SHA256SUMS") \
            || fail "SHA256SUMS 항목 오류: $1"
        [ "$listed" = "$expected" ] || fail "공개판 SHA256SUMS가 고정 SHA와 다릅니다: $1"
        download "$1" "$2"
        actual=$(digest "$2")
        [ "$actual" = "$expected" ] || fail "SHA-256 불일치: $1"
    }
    printf 'aaa %s (%s/%s)를 확인합니다.\n' "$version" "$os" "$arch"
    download SHA256SUMS "$temp/SHA256SUMS"
    verified_download "aaa-$os-$arch" "$temp/aaa"
    if [ "$os" = macos ]; then
        launcher="aaa-launcher-macos-$arch.json"
        verified_download "$launcher" "$temp/$launcher"
    fi
    chmod 700 "$temp/aaa"
    actual_version=$("$temp/aaa" --version) || fail '실행 파일의 판을 확인하지 못했습니다'
    [ "$actual_version" = "aaa $version" ] || fail "실행 파일 판 불일치: $actual_version"
    # 실행 파일·launcher 배치, 등록, 서비스와 PATH 판단은 모두 기존 setup의 책임이다.
    "$temp/aaa" setup "$@"
)

# 본문을 모두 읽은 뒤 시작하여 잘린 다운로드를 실행하지 않는다.
main "$@"
