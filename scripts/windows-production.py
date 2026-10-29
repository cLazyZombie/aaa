#!/usr/bin/env python3
"""최종 SHA의 Windows release를 한 번 빌드하고 검사한 exe/PDB를 그대로 보존한다.

build는 설치·전송하지 않는다. native는 격리한 Windows 폴더에서 실행한다.
verify는 전달 전후의 출처·해시·PE/PDB 연결·줄 정보·native 영수증을 확인하며 빌드하지 않는다.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"


def output(command, **kwargs):
    return subprocess.check_output(command, text=True, **kwargs).strip()


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def llvm(name, directory):
    path = directory / name if directory else shutil.which(name)
    if not path:
        raise ValueError(f"{name}이 필요하다. --llvm-bin으로 도구 폴더를 지정한다")
    return str(path)


def symbol_identity(artifact, directory):
    pe = output([llvm("llvm-readobj", directory), "--coff-debug-directory", str(artifact / "aaa.exe")])
    guid = re.search(r"PDBGUID: (\{[A-Fa-f0-9-]+\})", pe)
    age = re.search(r"PDBAge: (\d+)", pe)
    if "Format: COFF-x86-64" not in pe or not guid or not age:
        raise ValueError("x86_64 PE의 CodeView 연결 정보가 없다")
    # 실제 줄 표를 읽되 원문에 들어 있는 빌드 절대 경로를 보관하지 않는다.
    found = {}
    lines = False
    with subprocess.Popen([llvm("llvm-pdbutil", directory), "dump", "--summary", "-l", str(artifact / "aaa.pdb")],
                          stdout=subprocess.PIPE, text=True) as process:
        for line in process.stdout:
            field = re.match(r"\s*(GUID|Age|Has Debug Info|Is stripped): (.+?)\s*$", line)
            if field:
                found[field[1]] = field[2]
            lines |= bool(re.search(r"line/addr entries = [1-9]\d*", line))
        if process.wait():
            raise ValueError(f"PDB 해석 실패: exit={process.returncode}")
    if (found.get("GUID", "").upper(), found.get("Age")) != (guid[1].upper(), age[1]):
        raise ValueError("exe와 PDB의 GUID/age가 다르다")
    if found.get("Has Debug Info") != "true" or found.get("Is stripped") != "false" or not lines:
        raise ValueError("PDB의 debug 정보 또는 줄 표가 없다")
    return dict(guid=guid[1].upper(), age=int(age[1]), line_tables=True)


def read_artifact(artifact, commit):
    data = json.loads((artifact / "provenance.json").read_text())
    if (data["format"], data["commit"], data["target"], data["profile"], data["salt"]) != (
            1, commit, TARGET, "release", commit[:12]):
        raise ValueError("요청한 최종 SHA와 Windows release 출처가 다르다")
    if not data["software"].endswith("+" + commit[:12]):
        raise ValueError("출처의 판에 최종 SHA salt가 없다")
    for name in ("aaa.exe", "aaa.pdb"):
        path = artifact / name
        if path.is_symlink() or not path.is_file() or not path.stat().st_size or digest(path) != data["sha256"][name]:
            raise ValueError(f"산출물 해시 불일치 또는 일반 파일 아님: {name}")
    return data


def verify(artifact, commit, directory):
    data = read_artifact(artifact, commit)
    if symbol_identity(artifact, directory) != data["symbols"]:
        raise ValueError("보존한 PE/PDB 연결 정보와 다르다")
    native = json.loads((artifact / "native-check.json").read_text())
    expected = dict(commit=commit, sha256=data["sha256"], software=data["software"], config_check_exit=0)
    if native != expected:
        raise ValueError("같은 exe/PDB의 Windows native 검사 영수증이 아니다")
    return data


def native(artifact, commit):
    if sys.platform != "win32":
        raise ValueError("native 검사는 Windows에서 실행한다")
    data = read_artifact(artifact, commit)
    # 운영 설정·상태·인증을 읽지 않는다. 검사가 만든 폴더는 성공/실패와 무관하게 지운다.
    with tempfile.TemporaryDirectory(prefix="native-", dir=artifact) as temporary:
        env = {k: v for k, v in os.environ.items() if not k.startswith("AAA_")}
        binary = str((artifact / "aaa.exe").resolve())
        software = output([binary, "--version"], env=env, timeout=15)
        if software != data["software"]:
            raise ValueError(f"native --version 불일치: {software}")
        subprocess.run([binary, "--config-dir", str(Path(temporary) / "config"),
                        "--state-dir", str(Path(temporary) / "state"), "config", "check"],
                       env=env, check=True, timeout=15)
    # 검사 중 바이트가 바뀌어도 영수증을 만들지 않는다.
    read_artifact(artifact, commit)
    receipt = dict(commit=commit, sha256=data["sha256"], software=software, config_check_exit=0)
    (artifact / "native-check.json").write_text(json.dumps(receipt, indent=2) + "\n")


def build(artifact, commit, directory):
    import tomllib

    def source():
        head = output(["git", "rev-parse", "HEAD"], cwd=ROOT)
        if head != commit or output(["git", "status", "--porcelain"], cwd=ROOT):
            raise ValueError("요청한 최종 SHA와 같은 깨끗한 worktree가 필요하다")
        return output(["git", "rev-parse", "HEAD^{tree}"], cwd=ROOT)

    tree = source()
    if artifact.exists():
        raise ValueError("출력 폴더가 이미 있다. 기존 후보를 덮어쓰거나 다시 빌드하지 않는다")
    # 도구 누락은 production 컴파일보다 먼저 보고한다.
    for tool in ("llvm-readobj", "llvm-pdbutil"):
        output([llvm(tool, directory), "--version"])
    started = time.monotonic()
    env = {**os.environ, "AAA_BUILD_SALT": commit[:12], "CARGO_TARGET_DIR": str(ROOT / "target")}
    subprocess.run(["npm", "ci", "--no-audit", "--no-fund"], cwd=ROOT / "web", env=env, check=True)
    subprocess.run(["npm", "run", "build"], cwd=ROOT / "web", env=env, check=True)
    command = ["cargo"] + ([] if sys.platform == "win32" else ["xwin"])
    # test/all-targets와 합치지 않는다. dev 의존성 없는 실제 release 전용 호출이다.
    command += ["build", "--release", "--locked", "-p", "aaa", "--bin", "aaa", "--target", TARGET]
    subprocess.run(command, cwd=ROOT, env=env, check=True)
    if source() != tree:
        raise ValueError("빌드 중 source tree가 바뀌었다")
    artifact.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".windows-production-", dir=artifact.parent) as temporary:
        stage = Path(temporary) / "artifact"
        stage.mkdir()
        for name in ("aaa.exe", "aaa.pdb"):
            shutil.copyfile(ROOT / "target" / TARGET / "release" / name, stage / name)
        symbols = symbol_identity(stage, directory)
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        data = dict(format=1, commit=commit, tree=tree, target=TARGET, profile="release", salt=commit[:12],
                    software=f"aaa {version}+{commit[:12]}", symbols=symbols,
                    sha256={name: digest(stage / name) for name in ("aaa.exe", "aaa.pdb")},
                    build_elapsed_s=round(time.monotonic() - started, 3))
        (stage / "provenance.json").write_text(json.dumps(data, indent=2) + "\n")
        stage.rename(artifact)
    print(json.dumps(data, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("build", "native", "verify"))
    parser.add_argument("--commit", required=True, help="최종 통합 SHA 40자리")
    parser.add_argument("--artifact", type=Path, required=True, help="보존할/보존한 exe·PDB·출처 폴더")
    parser.add_argument("--llvm-bin", type=Path)
    args = parser.parse_args()
    try:
        if not re.fullmatch("[0-9a-f]{40}", args.commit):
            raise ValueError("--commit은 최종 SHA 40자리다")
        artifact = args.artifact.resolve()
        if args.action == "native":
            native(artifact, args.commit)
        elif args.action == "verify":
            verify(artifact, args.commit, args.llvm_bin)
        else:
            build(artifact, args.commit, args.llvm_bin)
        print(f"Windows production {args.action} 통과: {args.commit}")
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Windows production {args.action} 실패: {error}\n")


if __name__ == "__main__":
    main()
