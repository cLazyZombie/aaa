#!/usr/bin/env python3
"""macOS의 격리 host와 SSH Windows 시험 서비스로 혼합 rollout을 검증한다.

운영 서비스·host·PATH·sshd는 쓰지 않는다. 같은 commit salt로 빌드한 두 OS binary가 필요하다.
Windows 준비·전체 시스템 기준 목록 대조는 실행자가 별도 수행한다. 서비스 이름은 시험 접두만 받는다.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import shutil
import subprocess as sp
import tarfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--windows-binary", required=True)
    parser.add_argument("--failure-binary", help="self-check는 통과하고 서비스 시작은 실패하는 시험 PE")
    parser.add_argument("--windows-root", required=True)
    parser.add_argument("--instance", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    assert re.fullmatch(r"aaa-test-\d{4}-[a-z]", args.instance)
    assert re.fullmatch(r"C:\\aaa-test\\t-\d{4}", args.windows_root)
    assert args.instance.split("-")[2] == args.windows_root[-4:]
    assert re.fullmatch(r"[0-9a-f]{40}", args.commit)
    assert args.windows_binary.startswith(args.windows_root + "\\")
    binary = args.binary.resolve()
    root = Path("/tmp") / f"awr-{os.getpid()}"
    root.mkdir(mode=0o700)
    args.output.mkdir(parents=True, exist_ok=True)
    service = args.instance + "-agent"
    remote = args.windows_root + "\\lab"
    processes = {}
    logs = []
    env = {k: v for k, v in os.environ.items() if not k.startswith("AAA_")}
    env.update(AAA_CONFIG_DIR=str(root / "config"), AAA_STATE_DIR=str(root / "state"),
               AAA_TEST_BUILTINS="", AAA_USAGE_HOME=str(root / "usage"))
    for name in ["config", "state", "home/Library/LaunchAgents", "bin", "mac", "windows", "remote/config", "remote/state"]:
        (root / name).mkdir(parents=True, exist_ok=True)
    calls = root / "calls"
    calls.touch()
    (root / "bin/launchctl").write_text(
        '#!/bin/sh\ncase "$1" in\nprint) printf "state = running\\npid = 100\\n" ;;\n'
        'kickstart) printf "%s\\n" "$3" >> "$ROLLOUT_CALLS" ;;\n*) exit 1 ;;\nesac\n')
    (root / "bin/launchctl").chmod(0o755)
    for role in ["host", "agent"]:
        (root / f"home/Library/LaunchAgents/dev.aaa.{role}.plist").write_text("<plist/>")
    env.update(HOME=str(root / "home"), PATH=str(root / "bin") + ":" + env["PATH"], ROLLOUT_CALLS=str(calls))
    installed = root / "mac/aaa"
    shutil.copy2(binary, installed)
    candidate = root / "mac/candidate/aaa"
    candidate.parent.mkdir()
    shutil.copy2(binary, candidate)
    sp.run(["codesign", "--force", "--sign", "-", "--identifier", "rollout-candidate", str(candidate)], check=True)
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    mac_sha = sha(candidate)
    (candidate.parent / "provenance.json").write_text(json.dumps({"sha256": mac_sha, "commit": args.commit}))

    def ps(script, check=True):
        encoded = base64.b64encode(("$ErrorActionPreference='Stop';\n" + script).encode("utf-16le")).decode()
        return sp.run(["ssh", "-o", "BatchMode=yes", args.ssh, "powershell", "-NoProfile", "-EncodedCommand", encoded],
                      check=check, stdout=sp.PIPE, stderr=sp.PIPE, text=True)

    def cli(*argv, check=True, context=env):
        result = sp.run([str(binary), "--json", *argv], env=context, capture_output=True, text=True, timeout=30)
        if check and result.returncode:
            raise AssertionError(result.stderr)
        return result

    def spawn(tag, argv):
        log = (root / f"{tag}-{len(logs)}.log").open("w")
        logs.append(log)
        processes[tag] = sp.Popen(argv, env=env, stdin=sp.DEVNULL, stdout=log, stderr=sp.STDOUT)

    def stop(tag):
        child = processes.pop(tag, None)
        if child is not None:
            child.terminate()
            child.wait(timeout=30)

    def until(what, fn, seconds=30):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            value = fn()
            if value:
                return value
        raise AssertionError(f"{what}: 사건 대기 {seconds}초 초과")

    def connected(name):
        result = cli("machine", "list", check=False)
        if result.returncode:
            return False
        return next((m for m in json.loads(result.stdout)["machines"]
                     if m["name"] == name and m["connection"] == "connected"), None)

    def terminal(argv, prompt=None, seconds=30, expected_exit=0):
        pid, fd = pty.fork()
        if pid == 0:
            os.execve(str(binary), [str(binary), "--json", *argv], env)
        text = b""
        answered = False
        deadline = time.monotonic() + seconds
        reaped = False
        try:
            while time.monotonic() < deadline:
                ready, _, _ = select.select([fd], [], [], max(0, deadline - time.monotonic()))
                if not ready:
                    raise AssertionError("배포 시작 응답 한도")
                try:
                    chunk = os.read(fd, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                text += chunk
                if prompt is not None and not answered and prompt.encode() in text:
                    os.write(fd, b"yes\n")
                    answered = True
            else:
                raise AssertionError("배포 응답 한도")
            waited, status = os.waitpid(pid, 0)
            reaped = True
            assert waited == pid and os.waitstatus_to_exitcode(status) == expected_exit, text.decode(errors="replace")
            decoded = text.decode()
            return json.JSONDecoder().raw_decode(decoded[decoded.index('{'):])[0]
        finally:
            os.close(fd)
            if not reaped:
                waited, _ = os.waitpid(pid, os.WNOHANG)
                if not waited:
                    os.kill(pid, 15)
                    os.waitpid(pid, 0)

    created = False
    queue = None
    try:
        host_config = root / "config/host.toml"
        host_config.write_text(f'schema = 1\n[host]\nagent_listen = "127.0.0.1:0"\n'
                               f'[web]\nlisten = ""\n[local_agent]\nmode = "none"\n'
                               f'[projects]\nroot = "{root}/projects"\n')
        (root / "config/agent.toml").write_text(f'schema = 1\n[agent]\nwork_root = "{root}/work"\n'
                                               '[reconnect]\nmax_backoff_s = 1\n')
        spawn("host", [str(installed), "host"])
        status = until("시험 host 준비", lambda: (r := cli("status", check=False)).returncode == 0 and json.loads(r.stdout))
        addr = status["agent_listen"]
        host_config.write_text(host_config.read_text().replace("127.0.0.1:0", addr))
        spawn("agent", [str(installed), "agent", "--host", "local", "--name", "mac-a"])
        until("로컬 agent 연결", lambda: connected("mac-a"))
        cli("project", "create", "demo")
        cli("machine", "enroll", "--open", "5m")
        remote_env = dict(env, AAA_CONFIG_DIR=str(root / "remote/config"), AAA_STATE_DIR=str(root / "remote/state"))
        enrollment = sp.Popen([str(binary), "agent", "enroll", addr, "--name", "win-a"], env=remote_env,
                              stdin=sp.DEVNULL, stdout=sp.PIPE, stderr=sp.STDOUT, text=True)
        processes["enroll"] = enrollment
        code = None
        for line in enrollment.stdout:
            if "--code" in line:
                code = line.split("--code", 1)[1].strip()
                break
        assert code
        cli("machine", "approve", "win-a", "--code", code)
        assert enrollment.wait(timeout=30) == 0
        processes.pop("enroll")
        tunnel_log = (root / "tunnel.log").open("w")
        logs.append(tunnel_log)
        tunnel = sp.Popen(["ssh", "-o", "BatchMode=yes", "-o", "ExitOnForwardFailure=yes", "-N", "-R",
                           f"0:127.0.0.1:{addr.rsplit(':', 1)[1]}", args.ssh], stderr=tunnel_log)
        processes["tunnel"] = tunnel
        def tunnel_port():
            match = re.search(r"Allocated port (\d+)", (root / "tunnel.log").read_text())
            return match and match[1]
        port = until("SSH 역방향 시험 포트", tunnel_port)
        # 등록 키는 이 시험 host 전용이다. 새 서비스 SID에 소유권을 넘기며 로그에는 쓰지 않는다.
        (root / "remote/config/agent.toml").write_text(
            'schema = 1\n[agent]\n' + f'host = "127.0.0.1:{port}"\nname = "win-a"\n'
            + f'work_root = {json.dumps(remote + chr(92) + "work")}\n[reconnect]\nmax_backoff_s = 1\n')
        archive = root / "state.tar"
        with tarfile.open(archive, "w") as tar:
            tar.add(root / "remote/config", arcname="config")
            tar.add(root / "remote/state", arcname="state")
        ps(f"New-Item -ItemType Directory -Path '{remote}' | Out-Null")
        sp.run(["scp", str(archive), args.ssh + ":" + remote.replace("\\", "/") + "/state.tar"], check=True)
        ps(f"tar -xf '{remote}\\state.tar' -C '{remote}'; if ($LASTEXITCODE) {{ throw 'tar failed' }}; "
           f"New-Item -ItemType Directory -Path '{remote}\\bin' | Out-Null; "
           f"Copy-Item '{args.windows_binary}' '{remote}\\bin\\aaa.exe'")
        command = f'{remote}\\bin\\aaa.exe --instance {args.instance} --config-dir {remote}\\config --state-dir {remote}\\state setup-service'
        # CreateService의 password NULL과 같은 암호 없는 전용 가상 계정.
        ps(f"New-Service -Name '{service}' -BinaryPathName '{command}' -StartupType Manual | Out-Null")
        created = True
        ps(f"sc.exe config '{service}' obj= 'NT SERVICE\\{service}' | Out-Null; if ($LASTEXITCODE) {{ throw 'account failed' }}")
        ps(f"$sid = ([System.Security.Principal.NTAccount]'NT SERVICE\\{service}').Translate([System.Security.Principal.SecurityIdentifier]).Value; "
           f"icacls '{remote}' /grant ('*'+$sid+':(OI)(CI)F') /T /Q | Out-Null; if ($LASTEXITCODE) {{ throw 'grant failed' }}; "
           f"icacls '{remote}\\state' /setowner ('*'+$sid) /T /Q | Out-Null; if ($LASTEXITCODE) {{ throw 'owner failed' }}; "
           f"$sd = (sc.exe sdshow '{service}' | Where-Object {{ $_ -like 'D:*' }}) -join ''; "
           "$ace = '(A;;CCLCSWRPWPDTLOCRRC;;;'+$sid+')'; $idx=$sd.IndexOf('S:'); "
           "if ($idx -lt 0) { $sd += $ace } else { $sd=$sd.Insert($idx,$ace) }; "
           f"sc.exe sdset '{service}' $sd | Out-Null; if ($LASTEXITCODE) {{ throw 'service ACL failed' }}; "
           f"Start-Service '{service}'")
        before = until("Windows 서비스 TLS 연결", lambda: connected("win-a"), 60)
        assert "windows_scm_rollout.v1" in before["features"]
        windows_candidate = root / "windows/aaa.exe"
        sp.run(["scp", args.ssh + ":" + args.windows_binary.replace("\\", "/"), str(windows_candidate)], check=True)
        with windows_candidate.open("ab") as f:
            f.write(b"aaa-rollout-windows-candidate")
        win_sha = sha(windows_candidate)
        (windows_candidate.parent / "provenance.json").write_text(json.dumps({"sha256": win_sha, "commit": args.commit}))
        artifacts = root / "artifacts.json"
        artifacts.write_text(json.dumps([dict(os="windows", arch="x86_64", source_machine="mac-a",
                                             **{"from": str(windows_candidate)}, sha256=win_sha)]))
        queue = select.kqueue()
        descriptor = calls.open()
        queue.control([select.kevent(descriptor.fileno(), filter=select.KQ_FILTER_VNODE,
                                     flags=select.KQ_EV_ADD | select.KQ_EV_CLEAR, fflags=select.KQ_NOTE_WRITE)], 0)
        response = terminal(["deploy", "start", "--file", f"mac-a:{candidate}", "--sha256", mac_sha,
                             "--commit", args.commit, "--machines", "win-a", "--artifacts", str(artifacts)],
                            "yes를 입력한다:")
        rollout = response["rollout"]
        for role in ["host", "agent"]:
            def restart_request():
                lines = calls.read_text().splitlines()
                if any(line.endswith("." + role) for line in lines):
                    return True
                assert queue.control(None, 1, 120), f"{role} 재시작 요청 한도"
                return False
            until(role + " 재시작 요청", restart_request, 120)
            stop(role)
            spawn(role, [str(installed), role] + (["--host", "local", "--name", "mac-a"] if role == "agent" else []))
        completed = terminal(["deploy", "status", rollout["id"], "--wait"], seconds=120)["rollout"]
        assert completed["state"] == "done", completed
        after = until("Windows 새 SHA", lambda: (m := connected("win-a")) and m["sha256"] == win_sha and m)
        assert before["sha256"] != after["sha256"]
        assert [s["job_id"] for s in completed["steps"]] == [s["job_id"] for s in rollout["steps"]]
        (args.output / "success.json").write_text(json.dumps(completed, indent=2))
        print("mixed rollout done: target SHA and stable job IDs verified", flush=True)
        if args.failure_binary:
            assert args.failure_binary.startswith(args.windows_root + "\\")
            failure = root / "failure/aaa.exe"
            failure.parent.mkdir()
            sp.run(["scp", args.ssh + ":" + args.failure_binary.replace("\\", "/"), str(failure)], check=True)
            failure_sha = sha(failure)
            (failure.parent / "provenance.json").write_text(json.dumps({"sha256": failure_sha, "commit": args.commit}))
            payload = json.loads(artifacts.read_text())
            payload[0].update(sha256=failure_sha, **{"from": str(failure)})
            artifacts.write_text(json.dumps(payload))
            calls.write_text("")
            failed_start = terminal(["deploy", "start", "--file", f"mac-a:{candidate}", "--sha256", mac_sha,
                                     "--commit", args.commit, "--machines", "win-a", "--artifacts", str(artifacts)],
                                    "yes를 입력한다:")["rollout"]
            for role in ["host", "agent"]:
                def next_request():
                    if any(line.endswith("." + role) for line in calls.read_text().splitlines()):
                        return True
                    assert queue.control(None, 1, 120), f"{role} 재시작 요청 한도"
                    return False
                until(role + " 재시작 요청", next_request, 120)
                stop(role)
                spawn(role, [str(installed), role] + (["--host", "local", "--name", "mac-a"] if role == "agent" else []))
            until("서비스 교체 뒤 Windows 단절", lambda: not connected("win-a"), 120)
            stop("host")
            # host가 없는데도 SCM 상태·실제 파일 SHA가 baseline으로 돌아와야 한다.
            def restored_without_host():
                status = ps(f"$s=Get-CimInstance Win32_Service -Filter \"Name='{service}'\"; "
                            f"@{{ state=$s.State; pid=$s.ProcessId; sha=(Get-FileHash '{remote}\\bin\\aaa.exe').Hash.ToLower() }} | ConvertTo-Json -Compress")
                observed = json.loads(status.stdout)
                return observed["state"] == "Running" and observed["sha"] == win_sha and observed
            native_restored = until("host 없이 Windows baseline 복구", restored_without_host, 120)
            assert "host" not in processes
            spawn("host", [str(installed), "host"])
            rolled_back = terminal(["deploy", "status", failed_start["id"], "--wait"], seconds=120, expected_exit=1)["rollout"]
            assert rolled_back["state"] == "stopped_rolled_back", rolled_back
            assert rolled_back["recovery"]["phase"] == "restored"
            assert rolled_back["recovery"]["installed_sha256"] == win_sha
            assert [s["job_id"] for s in rolled_back["steps"]] == [s["job_id"] for s in failed_start["steps"]]
            assert sha(installed) == mac_sha, "앞서 성공한 macOS를 되돌리면 안 된다"
            (args.output / "recovery.json").write_text(json.dumps(rolled_back, indent=2))
            print("independent Windows recovery verified while host stopped; stopped_rolled_back", flush=True)
    finally:
        cleanup_errors = []
        def cleanup(action):
            try:
                action()
            except Exception as error:
                cleanup_errors.append(str(error))
        if queue is not None:
            queue.close()
        if created:
            cleanup(lambda: ps(f"Stop-Service '{service}' -ErrorAction SilentlyContinue; "
                               f"sc.exe delete '{service}' | Out-Null; if ($LASTEXITCODE) {{ throw 'delete failed' }}"))
            cleanup(lambda: ps(f"Get-CimInstance Win32_Process | Where-Object {{ $_.ExecutablePath -like '{remote}\\state\\agent\\deployments\\*.guard.bin' -and $_.CommandLine -like '*deployment-guard*' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction Stop }}"))
        for record in (root / "state/agent/deployments").glob("*.guard.json"):
            worker_pid = json.loads(record.read_text())["worker_pid"]
            command_line = sp.run(["ps", "-p", str(worker_pid), "-o", "command="], capture_output=True, text=True).stdout
            if str(root) in command_line and "deployment-guard" in command_line:
                cleanup(lambda: os.kill(worker_pid, 15))
        for tag in list(processes):
            cleanup(lambda: stop(tag))
        for log in logs:
            log.close()
        for log in root.glob("*.log"):
            shutil.copy2(log, args.output / log.name)
        if cleanup_errors:
            raise RuntimeError("시험 정리 실패: " + "; ".join(cleanup_errors))
        print("isolated service removed; diagnostic root: " + str(root), flush=True)


if __name__ == "__main__":
    main()
