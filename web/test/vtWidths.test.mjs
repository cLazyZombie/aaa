// session host 화면(들여온 vt100)의 폭 표와 aaa-vt 시험 사례가 웹 폭 규칙(src/herdr/terminalWidths.ts)에서 만든 그대로인가
// (plan §8.167). 웹 규칙이나 생성기가 바뀌었는데 표를 다시 만들지 않으면 실패한다: node web/scripts/vt-widths.ts로 다시 만든다
// (그때 모든 code point에서 표가 웹과 같은지 대조한다).
import { readFileSync } from "node:fs";
import { it } from "node:test";
import assert from "node:assert/strict";

import { generate, TABLE_PATH, CASES_PATH } from "../scripts/vt-widths.ts";

it("vt100 폭 표와 aaa-vt 사례가 웹 규칙에서 만든 그대로다", async () => {
  const { table, cases } = await generate({ verify: false });
  assert.ok(readFileSync(TABLE_PATH, "utf8") === table, "vendor/vt100/src/widths_table.rs가 오래됐다: node web/scripts/vt-widths.ts");
  assert.ok(readFileSync(CASES_PATH, "utf8") === cases, "crates/aaa-vt/src/width_cases.rs가 오래됐다: node web/scripts/vt-widths.ts");
});
