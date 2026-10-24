"""셸 계약 시험은 Windows에서도 WSL 대신 Git for Windows의 Bash를 쓴다."""

import os
from pathlib import Path
import shutil


def bash_executable():
    if os.name != "nt":
        return "bash"
    git = shutil.which("git")
    if git:
        directory = Path(git).resolve().parent
        for candidate in (directory / "bash.exe", directory.parent / "bin/bash.exe"):
            if candidate.is_file():
                return str(candidate)
    raise FileNotFoundError("Windows 셸 계약 시험에는 Git for Windows의 git·Bash가 필요하다")
