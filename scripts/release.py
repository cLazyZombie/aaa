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
LAUNCHERS = {"macos-aarch64": "aaa-launcher-macos-aarch64.json", "macos-x86_64": "aaa-launcher-macos-x86_64.json"}


def version_for_tag(tag: str) -> str:
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag):
        raise ValueError("tag는 vx.y.z(앞자리 0 없는 숫자 셋)여야 한다")
    if tag != f"v{version}":
        raise ValueError(f"tag {tag}와 Cargo 버전 {version}이 다르다")
    return version


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
    output.mkdir(parents=True, exist_ok=True)
    (output / "manifest.json").write_text(
        json.dumps({"version": version, "notes": f"aaa {tag}", "assets": assets, "launchers": launchers}, indent=2) + "\n",
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
