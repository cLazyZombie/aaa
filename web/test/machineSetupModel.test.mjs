// 「SSH로 추가」 화면 규칙(src/machineSetupModel.ts, 웹 머신 추가 M2, plan §8.384). 대상 검증·감지의 정본은 host(machine_setup.rs)다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  busy,
  canCancel,
  canInstall,
  canSsh,
  defaultSettings,
  loginHarnesses,
  detectedLine,
  needsPerson,
  notInstallable,
  operationLabel,
  PHASE,
  settingsProblem,
  shortHash,
  sshLine,
  TARGETS,
  targetLabel,
  targetProblem,
} from "../src/machineSetupModel.ts";

describe("SSH로 추가 M2", () => {
  it("1.93 이상 host만", () => {
    for (const v of [undefined, null, "1.90", "1.92"]) assert.equal(canSsh(v), false, String(v));
    assert.equal(canSsh("1.93"), true);
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
    assert.deepEqual(Object.keys(PHASE).filter(needsPerson), ["trust", "choose", "review", "needs_user"]);
    assert.deepEqual(Object.keys(PHASE).filter(busy), ["connecting", "detecting", "planning", "installing"]);
    assert.deepEqual(Object.keys(PHASE).filter((p) => !canCancel(p)), ["installing"]);
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

describe("SSH 설치 M3", () => {
  it("감독자까지 끝나는 macOS만 설치한다", () => {
    assert.equal(notInstallable({ os: "macos" }), null);
    for (const os of ["linux", "windows"]) assert.match(notInstallable({ os }) ?? "", /macOS부터/);
  });
  it("1.103 이상 host만", () => {
    for (const v of [undefined, null, "1.93", "1.102"]) assert.equal(canInstall(v), false, String(v));
    assert.equal(canInstall("1.103"), true);
  });
  const d = { os: "macos", arch: "aarch64", raw: "Darwin arm64", chosen: false, home: "/Users/<me>/", shell: "/bin/zsh", aaa: null };
  it("기본값: 이름은 대상 별칭, 작업 root는 HOME/aaa-work", () => {
    assert.deepEqual(defaultSettings("me@mac-b.example.com:22", d, "mac-a.example.ts.net:7430"), {
      name: "mac-b",
      host: "mac-a.example.ts.net:7430",
      work_root: "/Users/<me>/aaa-work",
      work_mount: null,
      harness: [],
      login: "defer",
    });
    assert.equal(defaultSettings("[fe80::1%en0]:22", null, "").name, "fe80--1-en0");
    assert.equal(defaultSettings("lab", null, "").work_root, "");
  });
  it("설정 앞 검사는 host check_settings와 같다", () => {
    const ok = defaultSettings("mac-b", d, "mac-a:7430");
    assert.equal(settingsProblem(ok), null);
    assert.match(settingsProblem({ ...ok, name: "mac b" }), /머신 이름/);
    assert.match(settingsProblem(ok, ["mac-b"]), /이미 있는/);
    assert.match(settingsProblem({ ...ok, host: "a b" }), /host 주소/);
    assert.match(settingsProblem({ ...ok, work_root: "aaa-work" }), /절대 경로/);
    assert.match(settingsProblem({ ...ok, work_root: "/a/../b" }), /절대 경로/);
    assert.match(settingsProblem({ ...ok, work_mount: "Work" }), /mount/);
  });
  it("로그인 pane은 고른 harness, 없으면 둘 다", () => {
    assert.deepEqual(loginHarnesses({ settings: { harness: ["codex"] } }), ["codex"]);
    assert.deepEqual(loginHarnesses({ settings: { harness: ["none"] } }), ["claude", "codex"]);
    assert.deepEqual(loginHarnesses({}), ["claude", "codex"]);
  });
  it("승인 범위 이름과 hash 앞부분", () => {
    assert.equal(operationLabel("enroll_agent"), "이 host에 등록");
    assert.equal(operationLabel("new_op"), "new_op");
    assert.equal(shortHash("0123456789abcdef"), "0123456789ab");
  });
});
