#!/usr/bin/env python3
"""실제 Git 응답과 소켓 진입 사이를 관찰하는 m13 실험실 래퍼."""

import os
import pathlib
import socket
import subprocess
import sys
import time


args = sys.argv[1:]
root = pathlib.Path(os.environ["GIT_GATE_ROOT"])
# agent의 호출은 git -C <repo> <명령> 꼴이다. 경로·인자값은 기록하지 않는다.
command = args[2] if len(args) > 2 and args[0] == "-C" else "other"
action = args[3] if command == "worktree" and len(args) > 3 else ""
# 관측하는 동안 선행 rev-parse·fetch 등의 실패도 같은 기록에 남긴다.
observe = (root / "gate-add").exists() or (root / "gate-remove").exists()
gate = root / ("gate-" + action)


def record(event, **fields):
    if observe:
        detail = " ".join(f"{key}={value}" for key, value in fields.items())
        line = f"{time.time_ns() // 1_000_000} shim={os.getpid()} {command} {action} {event} {detail}"
        try:
            with (root / "git-gate.events").open("a", encoding="utf-8") as log:
                log.write(line + "\n")
        except OSError as error:
            print(f"Git gate 기록 오류 errno={error.errno}: {line}", file=sys.stderr)


stage = "spawn"
try:
    record("spawn-attempt")
    with subprocess.Popen([os.environ["REAL_GIT"], *args]) as child:
        record("git-started", pid=child.pid)
        stage = "wait"
        rc = child.wait()
    record("git-exited", code=rc)
    if rc == 0 and action and gate.exists():
        stage = "connect"
        record("connect-attempt")
        addr = (os.environ["GIT_GATE_HOST"], int(os.environ["GIT_GATE_PORT"]))
        with socket.create_connection(addr, timeout=60) as conn:
            record("connected")
            stage = "send"
            conn.sendall((action + "\n").encode())
            record("signal-sent")
            stage = "receive"
            if conn.recv(1) != b"!":
                record("release-missing")
                sys.exit(42)
except OSError as error:
    record("error", stage=stage, kind=type(error).__name__, errno=error.errno)
    raise
sys.exit(rc)
