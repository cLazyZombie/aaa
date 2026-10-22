#!/usr/bin/env python3
"""herdr release의 Cargo/tag 검사와 OS/arch별 URL·SHA-256 manifest를 만든다. 업로드는 workflow가 한다."""

import argparse
import hashlib
import json
import re
import tomllib
from pathlib import Path

if __package__:
    from .launcher_asset import FILES
else:
    from launcher_asset import FILES


ROOT = Path(__file__).resolve().parents[1]
RELEASES = "https://github.com/cLazyZombie/aaa/releases/download"
ASSETS = {
    "linux-x86_64": "aaa-linux-x86_64",
    "linux-aarch64": "aaa-linux-aarch64",
    "macos-x86_64": "aaa-macos-x86_64",
    "macos-aarch64": "aaa-macos-aarch64",
    "windows-x86_64": "aaa-windows-x86_64.exe",
}
BOOTSTRAP = "aaa-install-windows.ps1"
WINDOWS_ENTRY = "install.ps1"
LAUNCHERS = {"macos-aarch64": "aaa-launcher-macos-aarch64.json", "macos-x86_64": "aaa-launcher-macos-x86_64.json"}
SYMBOLS = {
    "windows-x86_64": "aaa-windows-x86_64.pdb.gz",
    "macos-aarch64": "aaa-macos-aarch64.dSYM.tar.gz",
    "macos-x86_64": "aaa-macos-x86_64.dSYM.tar.gz",
}


def version_for_tag(tag: str) -> str:
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag):
        raise ValueError("tag는 vx.y.z(앞자리 0 없는 숫자 셋)여야 한다")
    if tag != f"v{version}":
        raise ValueError(f"tag {tag}와 Cargo 버전 {version}이 다르다")
    return version


def render_windows_entry(version: str, binary_sha256: str) -> bytes:
    """검토한 bootstrap owner에 해당 release의 기본 판·독립 payload SHA만 결속한다."""
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("bootstrap 판이 올바르지 않다")
    if not re.fullmatch(r"[a-f0-9]{64}", binary_sha256):
        raise ValueError("bootstrap payload SHA가 올바르지 않다")
    source = (ROOT / "scripts/bootstrap/install-windows.ps1").read_text()
    defaults = {
        "    [string]$Version,": f"    [string]$Version = '{version}',",
        "    [string]$ExpectedBinarySha256,": f"    [string]$ExpectedBinarySha256 = '{binary_sha256}',",
    }
    for original, rendered in defaults.items():
        if source.count(original) != 1:
            raise ValueError("bootstrap의 release 기본값 결속 위치가 바뀌었다")
        source = source.replace(original, rendered, 1)
    # irm | iex runs in the caller's scope. Keep functions/strict mode/preferences local,
    # without exiting a saved caller that itself used iex. The legacy -File bootstrap keeps its exit code.
    entry = "#Requires -Version 5.1\n& {\n" + source + "\n}\n"
    return entry.encode("utf-8")


def build_manifest(tag: str, artifacts: Path, output: Path) -> None:
    version = version_for_tag(tag)
    assets = {}
    checksums = []
    for platform, name in ASSETS.items():
        path = artifacts / name
        if path.is_symlink() or not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"release 실행 파일이 없거나 비었다: {name}")
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        assets[platform] = {"url": f"{RELEASES}/{tag}/{name}", "sha256": digest}
        checksums.append(f"{digest}  {name}\n")
    symbols = {}
    for platform, name in SYMBOLS.items():
        path = artifacts / name
        if path.is_symlink() or not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"release 심볼 파일이 없거나 비었다: {name}")
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        symbols[platform] = {"url": f"{RELEASES}/{tag}/{name}", "sha256": digest,
                             "binary_sha256": assets[platform]["sha256"]}
        checksums.append(f"{digest}  {name}\n")
    launchers = {}
    for platform, name in LAUNCHERS.items():
        path = artifacts / name
        if path.is_symlink() or not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"release launcher package가 없다: {name}")
        package = json.loads(path.read_text())
        if (package.get("schema") != 1
                or not isinstance(package.get("files"), dict)
                or set(package["files"]) != set(FILES)
                or package.get("version") != version
                or package.get("platform") != platform
                or package.get("binary_sha256") != assets[platform]["sha256"]):
            raise ValueError(f"launcher와 같은 release 실행 파일 결속이 다르다: {name}")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        launchers[platform] = {"url": f"{RELEASES}/{tag}/{name}", "sha256": digest}
        checksums.append(f"{digest}  {name}\n")
    bootstrap = artifacts / BOOTSTRAP
    if (bootstrap.is_symlink() or not bootstrap.is_file()
            or bootstrap.read_bytes() != (ROOT / "scripts/bootstrap/install-windows.ps1").read_bytes()):
        raise ValueError("Windows bootstrap은 같은 source의 검토한 파일이어야 한다")
    digest = hashlib.sha256(bootstrap.read_bytes()).hexdigest()
    checksums.append(f"{digest}  {BOOTSTRAP}\n")
    bootstraps = {"windows-x86_64": {"url": f"{RELEASES}/{tag}/{BOOTSTRAP}", "sha256": digest}}
    entry = render_windows_entry(version, assets["windows-x86_64"]["sha256"])
    entry_sha256 = hashlib.sha256(entry).hexdigest()
    checksums.append(f"{entry_sha256}  {WINDOWS_ENTRY}\n")
    bootstraps["windows-one-line"] = {"url": f"{RELEASES}/{tag}/{WINDOWS_ENTRY}", "sha256": entry_sha256}
    output.mkdir(parents=True, exist_ok=True)
    (output / WINDOWS_ENTRY).write_bytes(entry)
    (output / "manifest.json").write_text(
        json.dumps({"version": version, "notes": f"aaa {tag}", "assets": assets, "launchers": launchers, "bootstraps": bootstraps, "symbols": symbols}, indent=2) + "\n",
        encoding="utf-8",
    )
    (output / "SHA256SUMS").write_text("".join(checksums), encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["check-tag", "manifest"])
    parser.add_argument("--tag", required=True)
    parser.add_argument("--artifacts", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        version = version_for_tag(args.tag)
        if args.command == "manifest":
            if args.artifacts is None or args.output is None:
                parser.error("manifest에는 --artifacts와 --output이 필요하다")
            build_manifest(args.tag, args.artifacts, args.output)
        print(f"release 버전 확인: {version}")
    except (ValueError, OSError, KeyError, tomllib.TOMLDecodeError) as error:
        parser.exit(1, f"release 준비 실패: {error}\n")


if __name__ == "__main__":
    main()
