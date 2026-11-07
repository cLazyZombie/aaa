"""같은 머신의 검증 실행 단계(Rust 시험·웹 E2E)를 한 번에 하나만 돌린다.

부모 셸이 연 파일 기술자에 flock을 걸고 끝난다. 잠금은 그 열린 파일에 남으므로 부모 셸이 기술자를 닫거나
끝나면(죽어도) OS가 푼다(util-linux flock(1)의 `flock 9` 꼴, Cargo의 build 디렉터리 잠금과 같은 수명).
다른 검증이 쥐고 있으면 그 기록(누가·언제부터)을 보이고 풀릴 때까지 기다린다. 시한은 없다.

    exec 9<>"$lock"; python3 scripts/ci_machine_lock.py 9 "$LOGS"
"""

import fcntl
import os
import sys
import time


def acquire(fd, logs, say=print):
    """잠금을 얻고 기다린 초를 돌려준다. 얻은 뒤 파일에 이 실행의 기록을 남긴다."""
    started = time.monotonic()
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        holder = os.pread(fd, 4096, 0).decode(errors="replace").strip() or "기록 없음"
        say(f"── 같은 머신의 다른 검증이 실행 단계를 쓰는 중이다. 끝나기를 기다린다({holder})", flush=True)
        fcntl.flock(fd, fcntl.LOCK_EX)
    waited = time.monotonic() - started
    os.ftruncate(fd, 0)
    os.pwrite(fd, f"pid {os.getppid()} · {time.strftime('%H:%M:%S')}부터 · 로그 {logs}\n".encode(), 0)
    return waited


if __name__ == "__main__":
    waited = acquire(int(sys.argv[1]), sys.argv[2])
    if waited >= 1:
        print(f"── 같은 머신 실행 잠금을 {waited:.0f}초 기다려 얻었다", flush=True)
