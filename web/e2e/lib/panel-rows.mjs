import assert from "node:assert/strict";

// 행의 자식과 다음 행을 함께 잰다. scrollHeight만 보면 다음 선택 배경에 덮인 글을 놓친다.
export async function panelRows(page, selector) {
  const errors = await page.locator(selector).evaluateAll((rows) => {
    const errors = [];
    const visible = rows.filter((row) => row.getClientRects().length);
    for (const row of visible) {
      const r = row.getBoundingClientRect();
      if (row.scrollHeight > row.clientHeight + 1) errors.push(`${row.className}: 세로 넘침`);
      for (const child of row.querySelectorAll(".automation-card-text, .ask-row-text, .settings-tab-label, code.list-id")) {
        const b = child.getBoundingClientRect();
        if (b.top < r.top - 1 || b.bottom > r.bottom + 1) errors.push(`${row.className}: 글이 행 밖`);
        const x = Math.min(b.right - 1, r.right - 1), y = b.bottom - 1;
        if (x >= 0 && x < innerWidth && y >= 0 && y < innerHeight) {
          const hit = document.elementFromPoint(x, y);
          // 스크롤 칸의 머리/바닥에 가린 곳은 제외하고, 이웃 행이 덮은 경우만 검출한다.
          const other = hit?.closest(".ask-row, .automation-card, .report-card, .task-row, .task-log-row, .settings-tab");
          if (other && other !== row) errors.push(`${row.className}: 이웃 행이 글을 덮음`);
        }
      }
      const next = row.nextElementSibling;
      if (next?.matches(".ask-row, .automation-card, .report-card, .task-row, .task-log-row, .settings-tab") &&
          next.getBoundingClientRect().top < r.bottom - 1) errors.push(`${row.className}: 이웃 행 겹침`);
    }
    const automations = visible.filter((row) => row.matches(".automation-card"));
    const starts = automations.map((row) => {
      const id = row.querySelector("code.list-id");
      return id ? id.getBoundingClientRect().left - row.getBoundingClientRect().left : null;
    }).filter((x) => x !== null);
    if (starts.length && Math.max(...starts) - Math.min(...starts) > 1) errors.push("자동화 번호 시작 위치가 다름");
    return errors;
  });
  assert.deepEqual(errors, [], `목록 행 배치: ${selector}`);
}
