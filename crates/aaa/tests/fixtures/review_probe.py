"""요청 thread의 실제 자식으로 CLI를 불러 소유권과 결과를 검사한다."""
import json
import pathlib
import socket
import subprocess
import sys

aaa, root, base = sys.argv[1:4]
fixture = pathlib.Path(root)


def call(*args, fail=False):
    p = subprocess.run([aaa, "--json", "review", *args], capture_output=True, timeout=12)
    assert (p.returncode != 0) == fail, (args, p.returncode, p.stdout, p.stderr)
    return p.stderr.decode() if fail else json.loads(p.stdout)


if sys.argv[-1] == "deny":
    owner_id = (fixture / "review-owner-id").read_text()
    assert "자기 thread" in call("fetch", owner_id, fail=True)
    assert "자기 thread" in call("cancel", owner_id, fail=True)
else:
    def start(profile):
        return call("start", "--profile", profile, "--base", base)["job"]["id"]

    def wait(job):
        return call("status", job, "--wait")["job"]

    assert "profile" in call("start", "--base", base, fail=True)
    assert not (fixture / "review-runs.jsonl").exists()
    codex = start("review-codex")
    assert wait(codex)["state"] == "done"
    result = call("fetch", codex)
    assert result["job"]["account"] == "review-openai"
    assert result["job"]["target"]["base"] == base
    assert result["result"]["model"] == "review-codex-model"
    assert result["result"]["findings"][0]["path"] == "README.md"
    assert result["result"]["findings"][0]["line"] == 1
    assert result["earlier"] is False
    assert call("start", "--profile", "review-codex", "--base", base, "--id", codex)["job"]["state"] == "done"
    (fixture / "review-owner-id").write_text(codex)
    pathlib.Path("dirty.txt").write_text("live-only\n")
    assert call("fetch", codex)["earlier"] is True

    claude = start("review-claude")
    assert wait(claude)["state"] == "done"
    result = call("fetch", claude)["result"]
    assert result == {"model": "claude-review-actual", "findings": []}

    rejected = start("review-claude")
    failed = wait(rejected)
    assert failed["state"] == "failed" and "인증" in failed["reason"], failed
    assert call("start", "--profile", "review-claude", "--base", base, "--id", rejected)["job"]["state"] == "failed"

    with socket.socket() as server:
        server.bind(("127.0.0.1", 0))
        server.listen()
        server.settimeout(8)
        (fixture / "review-cancel-port").write_text(str(server.getsockname()[1]))
        cancelled = start("review-claude")
        peer, _ = server.accept()
        with peer:
            peer.settimeout(8)
            assert peer.recv(5, socket.MSG_WAITALL) == b"ready"
            call("cancel", cancelled)
            assert peer.recv(1) == b"", "실행기가 취소 뒤 종료한다"
        assert wait(cancelled)["state"] == "cancelled"

    runs = [json.loads(line) for line in (fixture / "review-runs.jsonl").read_text().splitlines()]
    assert [row["harness"] for row in runs] == ["codex", "claude", "claude", "claude"]
    assert all(not pathlib.Path(row["workspace"]).exists() for row in runs)
