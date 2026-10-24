#!/usr/bin/env python3
"""격리 instance의 실제 setup·OS 서비스·host 연결을 검사한다. 명시적으로 실행한다.

사용 예: python3 scripts/setup-unix-e2e.py --binary target/debug/aaa
  --kind launchd --instance aaa-test-0000-m --root /tmp/aaa-test-0000-m --launcher <시험 launcher>
Linux는 격리 VM에서만 systemd-user·systemd-system을 각각 실행한다.
시간을 미리 자지 않고 콘솔 출력·파일·프로세스 종료 사건을 기다린다.
"""
import argparse
import errno
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import shutil
import signal
import subprocess
import time


def run(argv, env=None, check=True):
    result = subprocess.run(list(map(str, argv)), env=env, text=True, capture_output=True, timeout=40)
    if check and result.returncode:
        raise AssertionError(f"{argv[0]} 종료 {result.returncode}: {result.stdout}{result.stderr}")
    return result


class Console:
    def __init__(self, argv, env):
        self.master, slave = pty.openpty()
        self.child = subprocess.Popen(list(map(str, argv)), env=env, stdin=slave, stdout=slave,
                                      stderr=slave, start_new_session=True)
        os.close(slave)
        self.output = b""

    def read(self, deadline):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise AssertionError("콘솔 사건 대기 만료: " + self.text())
        ready, _, _ = select.select([self.master], [], [], min(remaining, 0.1))
        if ready:
            try:
                self.output += os.read(self.master, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise

    def text(self):
        return self.output.decode("utf-8", errors="replace")

    def until(self, pattern, timeout=40):
        deadline = time.monotonic() + timeout
        while True:
            match = re.search(pattern, self.text())
            if match:
                return match
            if self.child.poll() is not None:
                self.read(deadline)
                raise AssertionError("질문 전 콘솔 종료: " + self.text())
            self.read(deadline)

    def finish(self, expected=0):
        deadline = time.monotonic() + 110
        while self.child.poll() is None:
            self.read(deadline)
        self.read(deadline)
        if self.child.returncode != expected:
            raise AssertionError(f"setup 종료 {self.child.returncode}, 기대 {expected}: {self.text()}")
        return self.text()

    def close(self):
        if self.child.poll() is None:
            self.child.send_signal(signal.SIGTERM)
            try:
                self.child.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.child.kill()  # 이 Console이 직접 띄운 정확한 자식만 정리
                self.child.wait(timeout=5)
        os.close(self.master)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--kind", choices=["launchd", "systemd-user", "systemd-system"], required=True)
    parser.add_argument("--instance", required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--launcher", type=Path)
    args = parser.parse_args()
    assert re.fullmatch(r"aaa-test-\d{4}-[a-z]", args.instance), "시험 전용 이름만 허용한다"
    assert args.root.is_absolute() and args.root.name.startswith("aaa-test-"), "별도 시험 폴더가 필요하다"
    assert not args.root.exists(), "기존 시험 폴더를 덮어쓰지 않는다"
    if args.kind == "launchd":
        assert args.launcher and args.launcher.is_file(), "별도 시험 launcher가 필요하다"
    binary = args.binary.resolve(strict=True)
    real_home = Path.home()
    args.root.mkdir(mode=0o700)
    root = args.root.resolve()
    home, config, state = root / "home", root / "c", root / "s"
    for path in [home, config, state, root / "hc", root / "hh", root / "hs"]:
        path.mkdir(mode=0o700)
    env = {k: v for k, v in os.environ.items() if not k.startswith("AAA_") and k not in
           ["XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_DATA_HOME", "BASH_ENV", "ENV"]}
    env.update(HOME=str(home), AAA_CONFIG_DIR=str(config), AAA_STATE_DIR=str(state),
               AAA_INSTANCE=args.instance, NO_COLOR="1", RUST_LOG="aaa=info",
               AAA_USAGE_HOME=str(root / "usage"), AAA_TEST_BUILTINS="")
    if args.kind != "launchd":
        # 사용자 systemd는 로그인한 계정의 실제 unit 검색 위치를 쓴다. VM 안 시험 이름만 만든다.
        env["XDG_CONFIG_HOME"] = str(real_home / ".config")
    host_env = dict(env, HOME=str(root / "hh"), AAA_CONFIG_DIR=str(root / "hc"),
                    AAA_STATE_DIR=str(root / "hs"))
    host_env.pop("AAA_INSTANCE")
    (root / "hc" / "host.toml").write_text(
        'schema = 1\n[host]\nagent_listen = "127.0.0.1:0"\n[web]\nlisten = ""\n'
        '[local_agent]\nmode = "none"\n[projects]\nroot = ' + json.dumps(str(root / "projects")) + '\n')
    source = root / "aaa-setup"
    shutil.copy2(binary, source)
    name = f"{args.instance}-agent"
    setup = [source, "setup", "--service", args.kind, "--name", "test-agent", "--work-root", root / "work", "-y"]
    if args.launcher:
        setup += ["--launcher", args.launcher.resolve()]
    consoles = []
    traces = []
    baseline_path = os.environ.get("PATH")
    launchagents = real_home / "Library/LaunchAgents"

    def snapshot():
        files = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in launchagents.glob("*") if p.is_file()}
        loaded = run(["/bin/launchctl", "list"]).stdout if args.kind == "launchd" else ""
        return {"launchagents": files, "launchctl": loaded, "path": os.environ.get("PATH")}

    before = snapshot()
    (root / "before.json").write_text(json.dumps(before, ensure_ascii=False))
    host_log = (root / "host.log").open("wb")
    host = subprocess.Popen([str(binary), "host"], env=host_env, stdout=host_log, stderr=subprocess.STDOUT)
    installed = False
    try:
        # front socket 생성이 준비 사건이다. 아직 없는 socket은 실패한 요청 재시도가 아니다.
        deadline = time.monotonic() + 30
        while not (root / "hs/host/front.sock").exists():
            assert host.poll() is None, "host 조기 종료"
            assert time.monotonic() < deadline, "host socket 준비 만료"
            select.select([], [], [], min(0.02, deadline - time.monotonic()))
        status = json.loads(run([binary, "--json", "status"], host_env).stdout)
        address = status["agent_listen"]
        run([binary, "machine", "enroll", "--open", "10m"], host_env)
        setup += ["--host", address]

        no_tty = run(setup, env, check=False)
        assert no_tty.returncode != 0 and "터미널" in no_tty.stderr
        assert not (state / "setup.json").exists()
        blocked = Console(setup, dict(env, AAA_SESSION_ID="test-session"))
        consoles.append(blocked)
        traces.append(blocked.finish(expected=1))
        assert "aaa 세션 안" in blocked.text()

        console = Console(setup, env)
        consoles.append(console)
        console.until(r"진행할까요\?")
        os.write(console.master, b"\n")
        code = console.until(r"확인 코드\s+(\d{3})\s+(\d{3})")
        run([binary, "machine", "approve", "test-agent", "--code", code[1] + code[2]], host_env)
        traces.append(console.finish())
        assert "host에 연결됐습니다" in console.text()
        installed = True
        spec = json.loads((state / "setup-unix.json").read_text())
        service_file = Path(spec["file"])
        receipt = json.loads((state / "setup.json").read_text())
        first = json.loads((state / "setup-service.json").read_text())
        assert first["connected"]
        log = state / "agent-service.log"
        assert log.stat().st_uid == os.getuid(), "시스템 서비스 로그도 설치 사용자 소유여야 한다"
        log.read_text()
        assert Path(receipt["exe"]).stat().st_mode & 0o111
        assert "host에 연결됨" in run([source, "setup", "--status"], env).stdout
        if args.kind != "launchd":
            run(["systemd-analyze", "verify", service_file], env)

        # 정확한 시험 서비스만 멈춘 뒤 같은 설치기로 재시작한다.
        if args.kind == "launchd":
            run(["/bin/launchctl", "bootout", f"gui/{os.getuid()}/{name}"])
        else:
            command = ["/usr/bin/systemctl", "--user" if args.kind == "systemd-user" else "--system", "stop", name + ".service"]
            run(command if args.kind == "systemd-user" else ["sudo", "-n", "--"] + command, env)
        restarted = Console(setup, env)
        consoles.append(restarted)
        traces.append(restarted.finish())
        assert "확인 코드" not in restarted.text(), "재시작은 등록을 다시 묻지 않는다"
        second = json.loads((state / "setup-service.json").read_text())
        assert second["connected"] and second["pid"] != first["pid"]

        uninstall = Console([source, "setup", "--uninstall", "-y"], env)
        consoles.append(uninstall)
        traces.append(uninstall.finish())
        installed = False
        assert not service_file.exists()
        assert not (state / "setup.json").exists()
        assert (config / "agent.toml").exists() and (state / "agent").is_dir()
        assert Path(receipt["exe"]).exists()

        # 같은 이름의 다른 파일은 등록·삭제 어느 쪽으로도 덮어쓰지 않는다.
        service_file.parent.mkdir(parents=True, exist_ok=True)
        if args.kind == "systemd-system":
            sentinel = root / "foreign.service"
            sentinel.write_text("# foreign sentinel\n")
            run(["sudo", "-n", "install", "-m", "644", sentinel, service_file])
        else:
            service_file.write_text("# foreign sentinel\n")
        try:
            foreign = Console(setup, env)
            consoles.append(foreign)
            traces.append(foreign.finish(expected=1))
            assert service_file.read_text() == "# foreign sentinel\n"
        finally:
            if args.kind == "systemd-system":
                run(["sudo", "-n", "rm", "--", service_file])
            else:
                service_file.unlink()
        original_config = (config / "agent.toml").read_bytes()
        cancelled = Console(setup, env)
        consoles.append(cancelled)
        cancelled.until(r"진행할까요\?")
        cancelled.child.send_signal(signal.SIGINT)
        traces.append(cancelled.finish(expected=130))
        assert (config / "agent.toml").read_bytes() == original_config
        assert not service_file.exists()

        again = Console(setup, env)
        consoles.append(again)
        again.until(r"진행할까요\?")
        os.write(again.master, b"\n")
        traces.append(again.finish())
        installed = True
        assert "확인 코드" not in again.text() and "host에 연결됐습니다" in again.text()
        purge = Console([source, "setup", "--uninstall", "--purge", "-y"], env)
        consoles.append(purge)
        traces.append(purge.finish())
        installed = False
        assert not config.exists() and not state.exists() and not service_file.exists()
        assert Path(receipt["exe"]).exists(), "purge도 다른 명령에 쓸 실행 파일은 남긴다"
        after = snapshot()
        (root / "after.json").write_text(json.dumps(after, ensure_ascii=False))
        assert before["launchagents"] == after["launchagents"], "운영 LaunchAgents가 바뀌었다"
        assert os.environ.get("PATH") == baseline_path
        if args.kind == "launchd":
            assert name not in after["launchctl"], "시험 LaunchAgent가 남았다"
            production = lambda s: {line.split()[-1]: line.split()[:2] for line in s.splitlines()
                                    if "aaa" in line and "aaa-test-" not in line}
            assert production(before["launchctl"]) == production(after["launchctl"]), "운영 aaa 상태가 바뀌었다"
        print(json.dumps({"kind": args.kind, "installed": True, "connected": True,
                          "restarted": True, "uninstalled": True, "foreign_preserved": True,
                          "cancelled": True, "reenrolled_without_prompt": True, "purged": True,
                          "baseline_unchanged": True}))
    finally:
        for console in consoles:
            traces.append(console.text())
            console.close()
        if installed or (state / "setup-unix.json").exists():
            # 실패해도 이 실험의 정확한 이름만 내린다. 외부 서비스 검색·pattern 종료는 없다.
            spec_path = state / "setup-unix.json"
            if spec_path.exists():
                spec = json.loads(spec_path.read_text())
                assert spec["name"] == name
                file = Path(spec["file"])
                if file.exists():
                    if args.kind == "launchd":
                        run(["/bin/launchctl", "bootout", f"gui/{os.getuid()}/{name}"], check=False)
                        file.unlink()
                    else:
                        command = ["/usr/bin/systemctl", "--user" if args.kind == "systemd-user" else "--system", "disable", "--now", name + ".service"]
                        prefix = [] if args.kind == "systemd-user" else ["sudo", "-n", "--"]
                        run(prefix + command, env, check=False)
                        if prefix:
                            run(prefix + ["/usr/bin/rm", "--", file])
                        else:
                            file.unlink()
                        run(prefix + ["/usr/bin/systemctl", command[1], "daemon-reload"], env)
        if host.poll() is None:
            host.terminate()
            host.wait(timeout=15)
        host_log.close()
        (root / "console.log").write_text("\n".join(traces))
        # 공유할 로그에는 실제 사용자 경로·머신 이름·개인 주소를 남기지 않는다.
        for path in root.glob("*.log"):
            text = path.read_text(errors="replace").replace(str(real_home), "<home>").replace(os.uname().nodename, "machine-a")
            text = re.sub(r"[a-zA-Z0-9_.-]+\.ts\.net", "example.ts.net", text)
            text = re.sub(r"\b(?:\d{1,3}\.){3}\d{1,3}\b", "<address>", text)
            path.write_text(text)


if __name__ == "__main__":
    main()
