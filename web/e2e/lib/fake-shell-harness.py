#!/usr/bin/env python3
"""hook 없는 셸 harness: 실제 PID·앞 그룹·FD·Claude PID 등록 경계를 시험한다."""
import json
import os
from pathlib import Path
import sys
import time

root, harness, session = Path(sys.argv[1]), Path(sys.argv[0]).name, sys.argv[2]
started = int(time.time() * 1000)
record = None
path = None
registry = root / "claude/sessions" / f"{os.getpid()}.json"


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


def open_record():
    global record, path
    if record:
        record.close()
    if harness == "codex":
        path = root / "codex/sessions/2026/10/23" / f"rollout-shell-{session}.jsonl"
    else:
        path = root / "claude/projects/shell" / f"{session}.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("")
    record = path.open("a") if harness == "codex" else None
    if record:
        record.write(json.dumps({"type": "session_meta", "payload": {"id": session, "source": "cli", "cwd": str(root)}}) + "\n")
    else:
        registry.parent.mkdir(parents=True, exist_ok=True)
        registry.write_text(json.dumps({"pid": os.getpid(), "sessionId": session, "kind": "interactive", "cwd": str(root), "startedAt": started}))
    for n in range(360):
        append(f"{session} 기록 {n} · " + "긴 대화 줄 " * 8, n)


def screen():
    print("\033[?1049h\033[2J\033[H", end="")
    print("foreground-harness-ready")
    if harness == "codex":
        print("› Ask Codex to do anything\n\n  GPT-6.1-Sol low · Context 97% left\n  ? for shortcuts")
    else:
        print("────────────────\n❯ \n────────────────\n  ? for shortcuts")
    sys.stdout.flush()


try:
    screen()
    for command in sys.stdin:
        command = command.strip()
        if command == "open":
            open_record()
        elif command == "append":
            append(f"{session} 새 답변", 361)
        elif command.startswith("switch "):
            session = command.split()[1]
            open_record()
        elif command == "quit":
            break
        screen()
finally:
    if record:
        record.close()
    if registry.exists():
        registry.unlink()
    print("\033[?1049l\nforeground-harness-exited", flush=True)
