// 완료 목록의 날짜 묶음(src/taskArchiveModel.ts, plan §8.494). 묶음 경계는 기기 시간대의 자정이다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { archiveGroups, dayLabel, rowTime } from "../src/taskArchiveModel.ts";

const at = (y, mo, d, h = 12, mi = 0) => new Date(y, mo - 1, d, h, mi).getTime();
const row = (n, completed_ms, started_ms) => ({ task_id: `id-${n}`, task: { short_id: `d-${n}` }, completed_ms, ...(started_ms === undefined ? {} : { started_ms }) });

describe("완료 목록 날짜 묶음", () => {
  const now = at(2026, 10, 26, 9);
  it("host 차례를 지키며 같은 날끼리 묶고, 시작 기록 없는 일은 끝 묶음이다", () => {
    const rows = [row(4, at(2026, 10, 26, 0, 1), at(2026, 10, 25, 23, 59)), row(3, at(2026, 10, 25, 23, 59), at(2026, 10, 25, 8)), row(2, at(2025, 12, 31, 18)), row(1, at(2025, 12, 31, 7))];
    const completed = archiveGroups(rows, "completed", now);
    assert.deepEqual(completed.map((g) => [g.label, g.entries.map((e) => e.task.short_id)]), [
      ["오늘 · 10월 26일 월요일", ["d-4"]],
      ["어제 · 10월 25일 일요일", ["d-3"]],
      ["2025년 12월 31일 수요일", ["d-2", "d-1"]],
    ]);
    const started = archiveGroups(rows, "started", now);
    assert.deepEqual(started.map((g) => [g.label, g.entries.length]), [["어제 · 10월 25일 일요일", 2], ["시작 기록 없음", 2]]);
    assert.deepEqual(archiveGroups(rows, "number", now).map((g) => [g.label, g.entries.length]), [[null, 4]]);
  });

  it("행 시각은 24시간제이고 번호순만 날짜를 붙인다", () => {
    assert.equal(rowTime(at(2026, 10, 25, 7, 5), false, now), "07:05");
    assert.equal(rowTime(at(2026, 10, 25, 17, 5), true, now), "10.25 17:05");
    assert.equal(rowTime(at(2025, 3, 2, 17, 5), true, now), "2025.3.2 17:05");
    assert.equal(dayLabel(at(2026, 9, 1), now), "9월 1일 화요일");
  });
});
