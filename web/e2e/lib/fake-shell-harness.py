#!/usr/bin/env python3
"""hook 없는 셸 harness: 실제 PID·앞 그룹·FD·Claude PID 등록 경계를 시험한다."""
import json
import os
from pathlib import Path
import sys
import time
import re
import termios
import tty
from collections import deque

root, harness = Path(sys.argv[1]), Path(sys.argv[0]).name
args = sys.argv[2:]
mode = args.pop(0) if args[0] in ("resume", "--resume", "--continue") else None
last = root / f"{harness}-last-session"
session = last.read_text() if mode == "--continue" else args.pop(0)
inline = "--inline" in args
mouse = "--mouse" in args
wheel = 0
started = int(time.time() * 1000)
record = None
path = None
registry = root / "claude/sessions" / f"{os.getpid()}.json"
input_events = deque(maxlen=64)


def input_event(kind, **fields):
    # 시험 입력의 종류만 남긴다. 원문 글·좌표·환경값은 저장하지 않는다.
    input_events.append({"at": int(time.time() * 1000), "kind": kind, **fields})
    folder = root / "shell-input"
    folder.mkdir(exist_ok=True)
    (folder / f"{os.getpid()}.json").write_text(json.dumps({
        "pid": os.getpid(), "harness": harness, "mode": mode, "session": session,
        "events": list(input_events),
    }))


def line(text, n):
    if harness == "codex":
        return {"type": "response_item", "payload": {"type": "message", "role": "assistant" if n % 2 else "user", "content": [{"type": "output_text" if n % 2 else "input_text", "text": text}]}}
    return {"type": "assistant" if n % 2 else "user", "message": {"content": [{"type": "text", "text": text}]}}


def append(text, n):
    if record:
        record.write(json.dumps(line(text, n)) + "\n")
        record.flush()
    else:
        with path.open("a") as out:
            out.write(json.dumps(line(text, n)) + "\n")


def open_record(resume=False, messages=360):
    global record, path
    if record:
        record.close()
    if harness == "codex":
        path = root / "codex/sessions/2026/10/23" / f"rollout-shell-{session}_01900000-0000-7000-9000-000000000001.jsonl"
    else:
        path = root / "claude/projects/shell" / f"{session}.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    if resume:
        assert path.is_file(), "이전 대화 기록이 없다"
    else:
        path.write_text("")
    record = path.open("a") if harness == "codex" else None
    if record and not resume:
        record.write(json.dumps({"type": "session_meta", "payload": {"id": session, "source": "cli", "cwd": str(root)}}) + "\n")
    if harness == "claude":
        registry.parent.mkdir(parents=True, exist_ok=True)
        registry.write_text(json.dumps({"pid": os.getpid(), "sessionId": session, "kind": "interactive", "cwd": str(root), "startedAt": started}))
    if not resume:
        for n in range(messages):
            append(f"{session} 기록 {n} · " + "긴 대화 줄 " * 8, n)
    last.write_text(session)


def screen():
    if inline:
        # Codex inline insert_history와 같은 DECSTBM+LF. 입력줄 밑은 그대로 둔다.
        import shutil
        rows = shutil.get_terminal_size().lines
        print(f"\033[{rows};1Hfixed-composer>\033[1;{rows-2}r\033[1;1H", end="")
        for n in range(80):
            print(f"terminal-history-{n}\r")
        print(f"\033[r\033[{rows-1};1Hforeground-harness-ready", flush=True)
        return
    if mouse:
        import shutil
        rows = shutil.get_terminal_size().lines
        print(f"\033[?1049h\033[?1000h\033[?1006h\033[2J\033[Happ-history-{wheel}\033[2;1Hforeground-harness-ready\033[{rows};1Hfixed-composer>", end="", flush=True)
        return
    print("\033[?1049h\033[2J\033[H", end="")
    print("foreground-harness-ready")
    if harness == "codex":
        print("› Ask Codex to do anything\n\n  GPT-6.1-Sol low · Context 97% left\n  ? for shortcuts")
    else:
        print("────────────────\n❯ \n────────────────\n  ? for shortcuts")
    sys.stdout.flush()


def commands():
    if not mouse:
        yield from sys.stdin
        return
    pending = ""
    control = ""
    while True:
        char = sys.stdin.read(1)
        if not char:
            return
        if char == "\033":
            control = char
        elif control:
            control += char
            if control == "\033[":
                continue
            if control.startswith("\033[") and not ("@" <= char <= "~"):
                continue
            # CSI는 마지막 글자에서 끝난다. 클릭·focus 보고까지 소비해야 다음 quit을
            # 삼키지 않는다. 휠 누름만 화면을 굴리고 뗌·다른 보고는 화면을 안 바꾼다.
            mouse_report = re.fullmatch(r"\033\[<(\d+);\d+;\d+([Mm])", control)
            button = int(mouse_report[1]) if mouse_report else None
            is_wheel = button in (64, 65) and mouse_report[2] == "M"
            input_event("control", button=button, final=char, wheel=is_wheel)
            control = ""
            if is_wheel:
                yield "wheel"
        elif char in "\r\n":
            input_event("command", command=pending if pending in ("quit", "open", "append") else "other")
            yield pending.strip()
            pending = ""
        else:
            pending += char


old_tty = termios.tcgetattr(sys.stdin) if mouse else None
try:
    if mouse:
        tty.setraw(sys.stdin)
    if mode:
        open_record(resume=True)
    input_event("ready")
    screen()
    for command in commands():
        command = command.strip()
        if command == "open":
            open_record()
        elif command == "append":
            append(f"{session} 새 답변", 361)
        elif command.startswith("switch "):
            session = command.split()[1]
            open_record()
        elif command.startswith("switch-paused "):
            session = command.split()[1]
            open_record(messages=300)
            print("foreground-history-paused", flush=True)
        elif command == "finish-history":
            for n in range(300, 360):
                append(f"{session} 기록 {n} · " + "긴 대화 줄 " * 8, n)
            print("foreground-history-complete", flush=True)
        elif command == "quit":
            break
        elif command == "wheel":
            wheel += 1
        screen()
finally:
    if old_tty:
        termios.tcsetattr(sys.stdin, termios.TCSANOW, old_tty)
    if record:
        record.close()
    if registry.exists():
        registry.unlink()
    print("\033[?1000l\033[?1006l\033[?1049l\nforeground-harness-exited", flush=True)
    input_event("exited")
