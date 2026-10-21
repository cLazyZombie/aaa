#!/usr/bin/env python3
"""검증된 macOS launcher bundle을 경로 없는 고정 파일 package로 묶는다."""
import argparse
import base64
import hashlib
import json
import subprocess
from pathlib import Path

FILES = (
    "Contents/MacOS/aaa-launcher",
    "Contents/Info.plist",
    "Contents/Resources/AppIcon.icns",
    "Contents/_CodeSignature/CodeResources",
)


def pack(app: Path, binary: Path, platform: str, version: str, output: Path) -> None:
    if platform not in ("macos-aarch64", "macos-x86_64"):
        raise ValueError("macOS launcher 대상이 아니다")
    subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(app)], check=True)
    actual = {str(p.relative_to(app)) for p in app.rglob("*") if p.is_file() or p.is_symlink()}
    if actual != set(FILES) or any(p.is_symlink() for p in app.rglob("*")):
        raise ValueError("launcher bundle 파일 목록/링크가 다르다")
    data = {name: base64.b64encode((app / name).read_bytes()).decode("ascii") for name in FILES}
    if any(not value for value in data.values()):
        raise ValueError("빈 launcher 파일")
    payload = dict(schema=1, version=version, platform=platform,
                   binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), files=data)
    output.write_text(json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    pack(args.app, args.binary, args.platform, args.version, args.output)


if __name__ == "__main__":
    main()
