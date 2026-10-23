#!/usr/bin/env python3
"""이미지 E2E의 가짜 Codex. 인증/선택/전송은 제품 owner가 처리한다."""
import base64
import hashlib
import json
import os
from pathlib import Path
import socket
import stat
import struct
import sys
import tomllib
import urllib.request
import zlib

root = Path(__file__).resolve().parent
control = json.loads((root / "image-control.json").read_text())
home = Path(os.environ["CODEX_HOME"])
if sys.argv[1:] == ["--version"]:
    assert not (home / "auth.json").exists(), "unsupported binary received credentials"
    print(control.get("version", "codex-cli 0.163.0"))
    sys.exit(0)
assert sys.argv[1:] == ["app-server", "--stdio", "--strict-config"]
assert os.environ["HOME"] == str(home)
for key in ["OPENAI_API_KEY", "OPENAI_BASE_URL", "CLAUDE_CODE_OAUTH_TOKEN", "AAA_HOOK_TOKEN", "AAA_SESSION_ID"]:
    assert key not in os.environ, key
assert stat.S_IMODE(home.stat().st_mode) == 0o700
assert stat.S_IMODE((home / "auth.json").stat().st_mode) == 0o600
auth = json.loads((home / "auth.json").read_text())
config = tomllib.loads((home / "config.toml").read_text())
assert config["features"]["shell_tool"] is False
assert config["features"]["image_generation"] is True
assert config["model_providers"]["aaa-image"]["request_max_retries"] == 0
assert config["model_providers"]["aaa-image"]["stream_max_retries"] == 0
assert config["model_providers"]["aaa-image"]["requires_openai_auth"] is True


def send(value):
    print(json.dumps(value), flush=True)


for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    if method == "initialize":
        send(dict(id=request["id"], result={}))
    elif method == "initialized":
        pass
    elif method == "thread/start":
        assert request["params"]["sandbox"] == "read-only"
        assert request["params"]["model"] == config["model"]
        assert request["params"]["ephemeral"] is True
        send(dict(id=request["id"], result=dict(thread=dict(id="image-thread"), model=config["model"], modelProvider="aaa-image")))
    elif method == "turn/start":
        params = request["params"]
        inputs = [i for i in params["input"] if i["type"] == "image"]
        for i in inputs:
            data = base64.b64decode(i["url"].split(",", 1)[1])
            assert data.startswith(b"\x89PNG\r\n\x1a\n")
        proof = dict(home=str(home), inputs=len(inputs), access_hash=hashlib.sha256(auth["tokens"]["access_token"].encode()).hexdigest())
        with (root / "image-attempts.jsonl").open("a") as f:
            f.write(json.dumps(proof) + "\n")
        if control.get("gate"):
            host, port = control["gate"].split(":")
            with socket.create_connection((host, int(port)), timeout=30) as gate:
                gate.sendall((json.dumps(proof) + "\n").encode())
                assert gate.recv(1) == b"1"
        if control.get("refresh"):
            body = json.dumps(dict(grant_type="refresh_token")).encode()
            with urllib.request.urlopen(urllib.request.Request(os.environ["CODEX_REFRESH_TOKEN_URL_OVERRIDE"], data=body), timeout=15) as r:
                new = json.load(r)
            assert new["access_token"] != auth["tokens"]["access_token"]
        if control.get("mode") == "failed":
            send(dict(id=request["id"], result=dict(turn=dict(id="turn-image"))))
            send(dict(method="turn/completed", params=dict(threadId="image-thread", turn=dict(status="failed", items=[]))))
            continue
        if control.get("mode") == "request":
            send(dict(id=request["id"], result=dict(turn=dict(id="turn-image"))))
            send(dict(id="untrusted-request", method="item/commandExecution/requestApproval", params={}))
            continue
        def chunk(kind, payload):
            body = kind + payload
            return struct.pack(">I", len(payload)) + body + struct.pack(">I", zlib.crc32(body))

        # 실제 PNG(887×1774 RGBA, 544 KiB). 등록한 가짜 인증 조각과
        # base64 글자만 겹치고 원시 바이트는 겹치지 않게 한다. 일반 글 가림은
        # 첫 조각의 offset 3787에 '*'를 넣어 Invalid symbol 42를 재현한다.
        header = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 887, 1774, 8, 6, 0, 0, 0))
        pixels = chunk(b"IDAT", zlib.compress((b"\0" + b"\x20\x80\x40\xff" * 887) * 1774))
        padding = bytearray(544 * 1024 - len(header) - len(pixels) - 24)
        collision = base64.b64decode("AAAcG5nMDkxA")
        for offset in (2838, 65520, 65539, 8 * 65536 + 2838):
            start = offset - len(header) - 8
            padding[start:start + len(collision)] = collision
        data = header + chunk(b"aaAa", padding) + pixels + chunk(b"IEND", b"")
        assert len(data) == 544 * 1024
        # 각 전송 조각은 base64 정렬을 다시 시작한다. 경계 양쪽과 마지막
        # 조각에 의도한 충돌이 실제로 있는지 준비 단계에서 확인한다.
        for offset in (0, 65536, 8 * 65536):
            assert b"cG5nMDkx" in base64.b64encode(data[offset:offset + 65536])
        images = home / "generated_images"
        images.mkdir()
        output = images / "result.png"
        output.write_bytes(data)
        item = dict(id="image-one", type="imageGeneration", status="completed", savedPath=str(output), result="")
        send(dict(method="thread/tokenUsage/updated", params=dict(threadId="image-thread", tokenUsage=dict(total=dict(totalTokens=123)))))
        send(dict(method="item/completed", params=dict(threadId="image-thread", item=item)))
        # RPC 응답보다 앞선 사건도 제품 reader가 보존해야 한다.
        send(dict(id=request["id"], result=dict(turn=dict(id="turn-image"))))
        send(dict(method="turn/completed", params=dict(threadId="image-thread", turn=dict(status="completed", items=[item]))))
    else:
        raise AssertionError(method)
