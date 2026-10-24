#!/usr/bin/env python3
"""공개 release의 SHA256SUMS에 고정한 Pages 설치 입구를 만든다. 게시하지 않는다."""

import argparse
import json
import re
import urllib.error
import urllib.request
from pathlib import Path

if __package__:
    from .release import ASSETS, LAUNCHERS, RELEASES, ROOT, render_windows_entry
else:
    from release import ASSETS, LAUNCHERS, RELEASES, ROOT, render_windows_entry


def read_url(url: str) -> bytes:
    try:
        with urllib.request.urlopen(url, timeout=30) as response:
            return response.read()
    except (urllib.error.URLError, OSError) as error:
        # 리디렉션의 서명 URL은 로그에 남기지 않는다.
        cause = getattr(error, 'reason', error)
        raise ValueError(f"release 다운로드 실패: {url} ({type(error).__name__}, "
                         f"http={getattr(error, 'code', None)}, cause={type(cause).__name__}, "
                         f"errno={getattr(cause, 'errno', None)})") from None


def render(manifest: dict, checksums: str, output: Path) -> str:
    version = manifest.get("version", "")
    if not isinstance(version, str) or not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("release 판은 x.y.z여야 한다")
    if tuple(map(int, version.split('.'))) < (0, 1, 6):
        raise ValueError("세 OS의 setup이 있는 v0.1.6 이상이 필요하다")
    sums = {}
    for line in checksums.splitlines():
        match = re.fullmatch(r"([0-9a-fA-F]{64})  ([A-Za-z0-9_.-]+)", line)
        if not match or match[2] in sums:
            raise ValueError("SHA256SUMS 형식 오류 또는 중복 자산")
        sums[match[2]] = match[1].lower()
    required = {}
    for group, names in (("assets", ASSETS), ("launchers", LAUNCHERS)):
        for platform, name in names.items():
            asset = manifest.get(group, {}).get(platform, {})
            if (asset.get("url") != f"{RELEASES}/v{version}/{name}"
                    or not sums.get(name) or asset.get("sha256") != sums[name]):
                raise ValueError(f"manifest·SHA256SUMS 불일치: {name}")
            required[name] = sums[name]
    source = (ROOT / "scripts/bootstrap/install-unix.sh").read_text(encoding="utf-8")
    substitutions = {
        "@AAA_VERSION@": version,
        "@AAA_CHECKSUMS@": '\n'.join(
            f"            {name}) printf '%s\\n' '{digest}' ;;"
            for name, digest in required.items() if not name.endswith('.exe')),
    }
    for key, value in substitutions.items():
        if source.count(key) != 1:
            raise ValueError(f"Unix 입구의 결속 위치가 바뀌었다: {key}")
        source = source.replace(key, value)
    # Windows 구현은 release와 동일한 정본 하나다. SHA256SUMS의 값이 실제 PE 검증까지 전달된다.
    windows = render_windows_entry(version, required[ASSETS['windows-x86_64']])
    output.mkdir(parents=True, exist_ok=True)
    (output / "install.sh").write_text(source, encoding="utf-8", newline="\n")
    (output / "install.ps1").write_bytes(windows)
    (output / ".nojekyll").touch()
    (output / "release.json").write_text(json.dumps({"version": version, "sha256": required}, indent=2) + '\n')
    return version


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--metadata-dir", type=Path, help="격리 시험용 manifest.json·SHA256SUMS 폴더")
    args = parser.parse_args()
    try:
        if args.metadata_dir:
            manifest = json.loads((args.metadata_dir / "manifest.json").read_text(encoding="utf-8"))
            sums = (args.metadata_dir / "SHA256SUMS").read_text(encoding="utf-8")
        else:
            latest = json.loads(read_url("https://api.github.com/repos/cLazyZombie/aaa/releases/latest"))
            tag = latest["tag_name"]
            if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag) or latest["draft"] or latest["prerelease"]:
                raise ValueError("공개 stable release가 아니다")
            # latest를 한 번만 해석한다. 이후 파일들은 같은 불변 tag에서 받는다.
            manifest = json.loads(read_url(f"{RELEASES}/{tag}/manifest.json"))
            if tag != 'v' + manifest.get('version', ''):
                raise ValueError("release tag와 manifest 판이 다르다")
            sums = read_url(f"{RELEASES}/{tag}/SHA256SUMS").decode('utf-8')
        version = render(manifest, sums, args.output)
        print(f"Pages 설치 입구 준비: v{version} (install.sh, install.ps1)")
    except (ValueError, OSError, KeyError, TypeError) as error:
        parser.exit(1, f"Pages 준비 실패: {error}\n")


if __name__ == "__main__":
    main()
