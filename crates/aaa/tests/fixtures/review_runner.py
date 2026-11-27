#!/usr/bin/env python3
"""실제 agent가 시작하는 review.v1 가짜 harness."""
import hashlib
import json
import os
import pathlib
import socket
import sys

fixture = pathlib.Path(__file__).resolve().parent
proof = json.loads((fixture / "review-fixture.json").read_text())


def matches_secret(value, key):
    assert hashlib.sha256(value.encode()).hexdigest() == proof[key]


def check_candidate(cwd, prompt):
    tree = pathlib.Path(cwd)
    assert tree != pathlib.Path(proof["original"])
    assert (tree / "README.md").read_text() == "candidate-only\n"
    assert "candidate-only" in prompt and "live-only" not in prompt
    assert not (tree / "dirty.txt").exists()
    assert all(not key.startswith("AAA_") for key in os.environ)
    assert "OPENAI_API_KEY" not in os.environ
    assert "REVIEW_PROFILE_ENV" not in os.environ
    assert "--profile-argument" not in sys.argv


def record(lane, cwd):
    with (fixture / "review-runs.jsonl").open("a") as out:
        print(json.dumps({"harness": lane, "workspace": cwd}), file=out)


def reply(message):
    print(json.dumps(message), flush=True)


if sys.argv[1] == "app-server":
    cfg = (pathlib.Path(os.environ["CODEX_HOME"]) / "config.toml").read_text()
    assert "shell_tool = false" in cfg and "hooks = false" in cfg
    assert not (pathlib.Path(os.environ["CODEX_HOME"]) / "auth.json").exists()
    for line in sys.stdin:
        message = json.loads(line)
        method, params = message["method"], message["params"]
        if method == "initialized":
            continue
        if method == "initialize":
            assert params["capabilities"]["experimentalApi"]
            response = {}
        elif method == "account/login/start":
            assert params["type"] == "chatgptAuthTokens"
            matches_secret(params["accessToken"], "codex")
            assert "refreshToken" not in params
            response = {"type": "chatgptAuthTokens"}
        elif method == "thread/start":
            assert params["model"] == "review-codex-model"
            assert params["sandbox"] == "read-only" and params["approvalPolicy"] == "never"
            cwd = params["cwd"]
            response = {"thread": {"id": "review-thread"}, "model": params["model"], "modelProvider": "aaa-review"}
        else:
            assert method == "turn/start" and params["effort"] == "high"
            assert params["outputSchema"]["required"] == ["findings"]
            check_candidate(cwd, params["input"][0]["text"])
            record("codex", cwd)
            reply({"id": message["id"], "result": {"turn": {"id": "review-turn"}}})
            finding = dict(path="README.md", line=1, severity="P2", title="fixture finding",
                           evidence="candidate-only", failure_path="new value reaches caller",
                           confidence=0.9, verification="read the caller")
            reply(dict(method="item/completed", params=dict(
                threadId="review-thread", turnId="review-turn",
                item=dict(type="agentMessage", text=json.dumps(dict(findings=[finding]))))))
            reply(dict(method="turn/completed", params=dict(
                threadId="review-thread", turn=dict(id="review-turn", status="completed"))))
            break
        reply({"id": message["id"], "result": response})
else:
    argv = sys.argv[1:]
    assert argv[0] == "-p" and "--restricted" in argv and "--safe-mode" in argv
    assert argv[argv.index("--permission-mode") + 1] == "plan"
    assert argv[argv.index("--tools") + 1] == "Read,Glob,Grep"
    assert argv[argv.index("--model") + 1] == "review-claude-model"
    matches_secret(os.environ["CLAUDE_CODE_OAUTH_TOKEN"], "claude")
    cwd = os.getcwd()
    check_candidate(cwd, sys.stdin.read())
    runs = (fixture / "review-runs.jsonl").read_text().count('"claude"')
    record("claude", cwd)
    if runs == 0:
        reply(dict(type="result", subtype="success", is_error=False, structured_output=dict(findings=[]),
                   modelUsage={"claude-review-actual": {"outputTokens": 32}}))
    elif runs == 1:
        # exit 0과 JSON의 실패가 함께 오면 실패였다.
        reply(dict(type="result", subtype="success", is_error=True, result="401 authentication"))
    else:
        port = int((fixture / "review-cancel-port").read_text())
        with socket.create_connection(("127.0.0.1", port), timeout=10) as peer:
            peer.sendall(b"ready")
            raise AssertionError(peer.recv(1))
