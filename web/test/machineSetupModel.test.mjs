// 「SSH로 추가」 화면 규칙(src/machineSetupModel.ts, 웹 머신 추가 M2, plan §8.375). 대상 검증·감지의 정본은 host(machine_setup.rs)다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { busy, canSsh, detectedLine, needsPerson, PHASE, sshLine, TARGETS, targetLabel, targetProblem } from "../src/machineSetupModel.ts";

describe("SSH로 추가 M2", () => {
  it("1.91 이상 host만", () => {
    for (const v of [undefined, null, "1.90"]) assert.equal(canSsh(v), false, String(v));
    assert.equal(canSsh("1.91"), true);
  });
  it("대상 앞 검사: 별칭·user@host:port는 되고 공백·옵션·비밀번호는 막는다", () => {
    for (const ok of ["mac-b", "me@mac-b.example.com:22", "[fe80::1%en0]:22"]) assert.equal(targetProblem(ok), null, ok);
    assert.match(targetProblem("-oProxyCommand=x"), /-로 시작/);
    assert.match(targetProblem("mac b"), /공백/);
    assert.match(targetProblem("me:pw@mac-b"), /비밀번호/);
    assert.match(targetProblem("  "), /넣는다/);
  });
  it("단계 이름과 사람이 할 단계", () => {
    assert.equal(PHASE.trust, "지문 확인 필요");
    assert.deepEqual(Object.keys(PHASE).filter(needsPerson), ["trust", "choose"]);
    assert.deepEqual(Object.keys(PHASE).filter(busy), ["connecting", "detecting"]);
  });
  it("release 대상 다섯", () => {
    assert.deepEqual(TARGETS.map(targetLabel), ["macOS · arm64", "macOS · x86_64", "Linux · x86_64", "Linux · arm64", "Windows · x86_64"]);
  });
  it("감지 한 줄·접속 줄", () => {
    const d = { os: "macos", arch: "aarch64", raw: "Darwin arm64", chosen: false, home: "/Users/<me>", shell: "/bin/zsh", aaa: null };
    assert.equal(detectedLine(d), "macOS · arm64 · /Users/<me> · zsh · aaa 없음");
    assert.equal(detectedLine({ ...d, chosen: true, home: null, shell: null, aaa: "aaa 0.1.2" }), "macOS · arm64 · 직접 고름 · aaa 0.1.2 있음");
    const v = { ssh: { user: "me", host: "::1", port: 22, proxy_jump: true } };
    assert.equal(sshLine(v), "me@[::1]:22 (ProxyJump 거침)");
    assert.equal(sshLine({ ssh: null }), null);
  });
});
