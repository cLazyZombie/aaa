"""가짜 Claude 메뉴: 입력 묶음을 다 처리한 뒤 화면을 한 번 그린다."""

import os
import select
import sys
import termios


title, selected, first, second, footer, *prefix = sys.argv[1:]
selected = int(selected)
options = (first, second)
rule = "─" * 40
working = "✻ Working… (1s · esc to interrupt)"
clear = f"\x1b[{title.count(chr(10)) + 6}A\r\x1b[J"
before = prefix[0] if prefix else ""
mode = termios.tcgetattr(0)
mode[3] &= ~(termios.ICANON | termios.ECHO)
mode[6][termios.VMIN] = 1
mode[6][termios.VTIME] = 0
termios.tcsetattr(0, termios.TCSANOW, mode)


def write(text):
    # fd 3은 부모 셸의 PTY, stdout은 선택 결과를 받는 command substitution이다.
    data = text.encode()
    assert os.write(3, data) == len(data)


def draw():
    rows = [f" {'❯' if n == selected else ' '} {text}" for n, text in enumerate(options)]
    write(f"{before}{rule}\n{title}\n{rows[0]}\n{rows[1]}\n\n {footer}\n")


draw()
before = clear
pending = b""
while True:
    if not pending:
        pending = os.read(0, 4096)
        if not pending:
            raise EOFError("메뉴 답 전에 PTY 입력이 닫혔다")
    while pending:
        if pending.startswith((b"\x1b[A", b"\x1b[B")):
            selected = 0 if pending[2:3] == b"A" else 1
            pending = pending[3:]
        elif pending[:1] in (b"\r", b"\n"):
            write(before)
            sys.stdout.write(options[selected])
            sys.exit(0)
        elif pending[:1] == b"\x1b":
            # Esc 단독과 나눠 도착한 화살표를 구별하는 동안 이전 질문은 즉시 지운다.
            write(f"{before}{working}\n")
            before = "\x1b[1A\r\x1b[J"
            while pending in (b"\x1b", b"\x1b[") and select.select([0], [], [], 0.2)[0]:
                more = os.read(0, 4096)
                if not more:
                    break
                pending += more
            if pending.startswith((b"\x1b[A", b"\x1b[B")):
                continue
            write(before)
            sys.stdout.write("cancel")
            sys.exit(0)
        else:
            pending = pending[1:]
    # 이미 도착한 Enter가 있으면 중간 선택 화면을 다시 그리지 않는다.
    if select.select([0], [], [], 0)[0]:
        pending = os.read(0, 4096)
        if not pending:
            raise EOFError("메뉴 답 전에 PTY 입력이 닫혔다")
        continue
    draw()
    before = clear
