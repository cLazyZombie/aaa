// 머신 CPU·MEM(src/machineStats.ts, plan §8.243): 연결 중일 때만, 옛 agent는 값 없음, 90%부터 높음과 활동 막대 배지.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { highMachines, isHigh, machineStats } from "../src/machineStats.ts";

const GB = 2 ** 30;
const machine = (name, health, connection = "connected") => ({
  id: name, name, local: false, connection, os: "macos", boot_id: null, dev_mode: false, last_seen_ms: null, clock_skew_ms: 0, problems: [],
  health: health && { at_ms: 1_000, disk_free_bytes: null, disk_total_bytes: null, load_x100: 873, cpus: 16, on_ac: true, toolchain: [], problems: [], warnings: [], ...health },
});

describe("머신 CPU·MEM", () => {
  it("값과 title(사용 GB·부하·몇 초 전)", () => {
    const s = machineStats(machine("mac-a", { cpu_pct: 23, mem_used_bytes: 48 * GB, mem_total_bytes: 64 * GB }), 8_000);
    assert.deepEqual([s.cpu, s.mem], [23, 75]);
    assert.equal(s.detail, "CPU 23% · 메모리 48.0GB / 64.0GB 사용(75%) · 부하 8.73/16 · 7초 전");
  });
  it("옛 agent는 값 칸이 없어 둘 다 모름", () => {
    const s = machineStats(machine("mac-d", {}), 1_000);
    assert.deepEqual([s.cpu, s.mem], [null, null]);
    assert.match(s.detail, /^CPU 모름 · 메모리 모름/);
  });
  it("끊긴 머신·보고 전은 그리지 않는다(오래된 값)", () => {
    assert.equal(machineStats(machine("win-e", { cpu_pct: 99 }, "disconnected")), null);
    assert.equal(machineStats(machine("mac-f", null)), null);
  });
  it("90%부터 높음, 배지는 CPU나 MEM이 높은 연결 중 머신 수", () => {
    assert.deepEqual([isHigh(89), isHigh(90), isHigh(null)], [false, true, false]);
    const list = [
      machine("a", { cpu_pct: 23, mem_used_bytes: 10, mem_total_bytes: 100 }),
      machine("b", { cpu_pct: 94 }),
      machine("c", { cpu_pct: 12, mem_used_bytes: 92, mem_total_bytes: 100 }),
      machine("d", { cpu_pct: 99, mem_used_bytes: 99, mem_total_bytes: 100 }, "disconnected"),
    ];
    assert.equal(highMachines(list), 2);
    assert.equal(highMachines(list.slice(0, 1)), 0);
  });
});
