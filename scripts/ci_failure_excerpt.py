"""공개 CI annotation에 실패 표지와 panic 다음 오류 한 줄을 함께 남긴다."""

from collections import deque
import os
from pathlib import Path
import re
import sys


def excerpt(lines, env):
    entries = deque(maxlen=8)
    tail = deque(maxlen=5)
    pending = None
    # runner가 실제로 받은 인증값은 공개 annotation에서 가린다. 로그 파일은 바꾸지 않는다.
    hidden = sorted({value for key, value in env.items() if len(value) >= 8 and
                     re.search(r"TOKEN|SECRET|PASSWORD|API_KEY|PRIVATE_KEY", key, re.I)},
                    key=len, reverse=True)

    def clean(line):
        line = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", line.rstrip("\r\n"))
        for value in hidden:
            line = line.replace(value, "<redacted>")
        return line

    for raw in lines:
        line = clean(raw)
        tail.append(line)
        if pending is not None:
            # 첫 줄은 표지와 한 묶음으로 보관한다. 역추적 길이와 tail 상한에 밀리지 않는다.
            entries[pending] += "\n" + line
            pending = None
        if re.search(r"FAIL|panicked|^error", line):
            entries.append(line)
            if "panicked at " in line:
                pending = len(entries) - 1
    return "\n".join([*entries, *tail]) + "\n"


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    with Path(sys.argv[1]).open(encoding="utf-8", errors="replace") as log:
        sys.stdout.write(excerpt(log, os.environ))
