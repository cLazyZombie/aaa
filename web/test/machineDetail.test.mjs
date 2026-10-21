// 설정 「머신」의 줄 요약·상세(src/machineDetail.ts, 웹 머신 추가 M0, plan §8.370). 값의 정본은 host MachineView다
// (lib.rs `machine_view_tests`: hello의 판·SHA가 host 재시작 뒤에도 남고, 판이 다르면 `software_differs`).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { ago, archLabel, machineLine, machineRows, osLabel } from "../src/machineDetail.ts";

const NOW = 10 * 86_400_000;
const base = {
  id: "m",
  name: "mac-b",
  local: false,
  connection: "connected",
  os: "macos",
  boot_id: "b",
  dev_mode: false,
  last_seen_ms: NOW - 1000,
  health: null,
  clock_skew_ms: null,
  problems: [],
};
const health = {
  at_ms: NOW,
  disk_free_bytes: 50 * 2 ** 30,
  disk_total_bytes: 500 * 2 ** 30,
  load_x100: 150,
  cpus: 8,
  on_ac: true,
  toolchain: [{ name: "git", ok: true, version: "git version 2.50" }],
  problems: [],
  warnings: [],
};
const row = (rows, label) => rows.find((r) => r.label === label);

describe("머신 상세 M0", () => {
  it("OS·CPU 이름은 사람이 아는 말로, 모르는 값은 그대로", () => {
    assert.equal(osLabel("macos"), "macOS");
    assert.equal(osLabel("windows"), "Windows");
    assert.equal(osLabel("freebsd"), "freebsd");
    assert.equal(osLabel(null), null);
    assert.equal(archLabel("aarch64"), "arm64");
    assert.equal(archLabel("x86_64"), "x86_64");
  });
  it("줄 요약은 아는 것만: 옛 host는 OS만", () => {
    assert.equal(machineLine(base), "macOS");
    assert.equal(machineLine({ ...base, arch: "aarch64", software: "aaa 0.1.2" }), "macOS · arm64 · aaa 0.1.2");
    assert.equal(machineLine({ ...base, os: null }), "");
  });
  it("판이 host와 다르면 주의, SHA는 고정폭, 모르면 「모름」", () => {
    const rows = machineRows({ ...base, arch: "aarch64", software: "aaa 0.1.1", software_differs: true, sha256: "ab".repeat(32) }, NOW);
    assert.deepEqual(row(rows, "판"), { label: "판", value: "aaa 0.1.1 (host와 다름)", warn: true });
    assert.equal(row(rows, "SHA-256").mono, true);
    assert.equal(row(machineRows(base, NOW), "판").value, "모름");
    assert.equal(row(machineRows(base, NOW), "SHA-256").value, "모름");
  });
  it("연결: host 머신·연결됨·끊김(마지막 본 때)", () => {
    assert.equal(row(machineRows({ ...base, local: true }, NOW), "연결").value, "host 머신");
    assert.equal(row(machineRows(base, NOW), "연결").warn, false);
    const off = row(machineRows({ ...base, connection: "disconnected", last_seen_ms: NOW - 3 * 3600_000 }, NOW), "연결");
    assert.deepEqual(off, { label: "연결", value: "끊김(마지막 3시간 전)", warn: true });
    assert.equal(row(machineRows({ ...base, connection: "disconnected", last_seen_ms: null }, NOW), "연결").value, "끊김(붙은 적 없음)");
  });
  it("건강은 연결 중일 때만: 디스크·부하·toolchain·배터리·시계·문제", () => {
    const rows = machineRows(
      {
        ...base,
        health: { ...health, on_ac: false, toolchain: [{ name: "cargo", ok: false, version: "없다" }], warnings: ["배터리로 돈다"] },
        clock_skew_ms: -12_000,
        problems: [{ code: "disk", text: "디스크 여유가 5GB 아래" }],
      },
      NOW,
    );
    assert.equal(row(rows, "작업 디스크").value, "50.0 GB 남음 / 500.0 GB");
    assert.equal(row(rows, "부하").value, "1.50 / CPU 8개");
    assert.deepEqual(row(rows, "전원"), { label: "전원", value: "배터리", warn: true });
    assert.deepEqual(row(rows, "cargo"), { label: "cargo", value: "없다", warn: true });
    assert.equal(row(rows, "시계 차이").value, "-12초(host 기준)");
    assert.equal(row(rows, "주의").value, "배터리로 돈다");
    assert.equal(row(rows, "문제").value, "디스크 여유가 5GB 아래");
    // 끊긴 머신의 건강은 오래된 값이라 보이지 않는다, 붙었는데 아직 보고가 없으면 기다린다고
    assert.equal(row(machineRows({ ...base, connection: "disconnected", health }, NOW), "작업 디스크"), undefined);
    assert.equal(row(machineRows(base, NOW), "건강").value, "보고를 기다린다");
    assert.equal(row(machineRows({ ...base, health }, NOW), "시계 차이"), undefined, "1초 안은 싣지 않는다");
  });
  it("시작 점검: 통과·실패 이유", () => {
    assert.deepEqual(row(machineRows({ ...base, selfcheck: { pass: true, reasons: [] } }, NOW), "시작 점검"), { label: "시작 점검", value: "통과", warn: false });
    assert.equal(row(machineRows({ ...base, selfcheck: { pass: false, reasons: ["키체인 잠김", "FDA 없음"] } }, NOW), "시작 점검").value, "실패: 키체인 잠김; FDA 없음");
    assert.equal(row(machineRows(base, NOW), "시작 점검"), undefined);
  });
  it("「몇 전」", () => {
    assert.equal(ago(NOW - 30_000, NOW), "방금");
    assert.equal(ago(NOW - 5 * 60_000, NOW), "5분 전");
    assert.equal(ago(NOW - 2 * 86_400_000, NOW), "2일 전");
    assert.equal(ago(NOW + 5000, NOW), "방금");
  });
});
