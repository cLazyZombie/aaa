// 실제 host의 상태 표지·예약·하위 일·첨부·thread 칩을 서로 다른 목록 폭에서 검사한다.
import fs from "node:fs";
import assert from "node:assert/strict";
import { root, aaa, until, step, browser, webkit, pairPage, domIs, shot } from "../lib/harness.mjs";

export default async function () {
  await step("할 일 열 정렬: iPad 양 방향·데스크톱·휴대폰에서 표지와 칩이 겹치거나 잘리지 않는다", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    aaa("project", "set", "demo", "max_parallel_threads", "1");
    const titles = ["일반 할 일의 충분히 긴 제목", "병합 대기 할 일의 충분히 긴 제목", "연결된 할 일의 충분히 긴 제목"];
    // 한 실제 thread를 여러 task가 참조해도 행별 메타데이터는 그대로 재현된다.
    fs.writeFileSync(`${root}/layout.md`, "보고를 작성하고 기다린다");
    const id = aaa("thread", "start", "demo", "--title", "레이아웃 확인", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/layout.md`).id;
    const t = aaa("thread", "show", "demo", id);
    await until("가짜 harness의 첫 보고", () => fs.existsSync(`${t.work_dir}/report.md`));
    fs.writeFileSync(`${t.work_dir}/report.md.tmp`, "[병합 준비됨] 레이아웃 확인\n\n## Next\n- 기다린다\n");
    fs.renameSync(`${t.work_dir}/report.md.tmp`, `${t.work_dir}/report.md`);
    await until("host 병합 대기 표지", () => aaa("thread", "show", "demo", id).stage === "merge_ready");
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, `## Backlog\n` + titles.map((title, i) => `- [ ] ${title} (fake@labA) [작업 폴더]${i ? ` · ${id}` : ""}\n  메모\n  ![첨부](uploads/0123456789abcdef.png)\n`).join("") + "  - [ ] 하위 할 일 (fake@labA) [작업 폴더]\n    하위 메모\n- [ ] 예약할 일 (fake@labA) [작업 폴더]\n  예약 내용\n");
    fs.mkdirSync(`${root}/projects/demo/uploads`, { recursive: true });
    fs.copyFileSync(`${root}/pic.png`, `${root}/projects/demo/uploads/0123456789abcdef.png`);
    aaa("task", "list");
    // 단계 제외 슬롯을 별도 대기 thread가 차지해 예약이 기동하지 않게 한다.
    fs.writeFileSync(`${root}/layout-busy.md`, "사람을 기다림");
    aaa("thread", "start", "demo", "--title", "자리 보유", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/layout-busy.md`);
    aaa("task", "reserve", "예약할 일");
    const errors = [];
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, name, width, height] of [[wk, "ipad-landscape", 1366, 1024], [wk, "ipad-portrait", 1024, 1366], [browser, "desktop-1280", 1280, 900], [browser, "desktop-2000", 2000, 1080], [wk, "phone", 390, 844], [wk, "phone-320", 320, 740], [browser, "phone-chromium", 390, 844], [browser, "phone-320-chromium", 320, 740]]) {
        const { page: p } = await pairPage(engine, aaa("web", "pair").code, { width, height }, { hash: "#tasks", touch: engine === wk });
        await domIs(p, "할 일과 단계 준비", () => document.querySelectorAll('.task-row[data-kind]').length === 6 && document.querySelector('.task-row [data-stage="merge_ready"]'));
        await p.evaluate(() => document.fonts.ready);
        assert.equal(await p.locator(".task-row .task-attach-mark").count(), 3, "첨부 표지가 빠졌다");
        if (engine === browser) await p.locator('.task-row').filter({ hasText: titles[0] }).hover();
        await shot(p, `task-layout-${name}`);
        const problems = await p.evaluate(() => {
          const problems = [];
          const rows = [...document.querySelectorAll('.task-row[data-kind]')];
          const columns = new Map();
          const titleStart = row => {
            const title = row.querySelector('.task-title');
            return title.getBoundingClientRect().left + parseFloat(getComputedStyle(title).paddingLeft);
          };
          for (const row of rows) {
            const r = row.getBoundingClientRect();
            const grip = row.querySelector('.task-grip')?.getBoundingClientRect();
            const stateBox = row.querySelector('.task-state, .task-reserved-dot').getBoundingClientRect();
            if (grip && (stateBox.left - grip.right > 16 || stateBox.left < grip.right)) problems.push(`${row.dataset.title}: 손잡이와 완료 원 사이 빈 열 ${stateBox.left - grip.right}px`);
            const titleX = titleStart(row);
            const chips = row.querySelector('.task-row-chips');
            const firstChip = [...chips.children].find(e => e.getBoundingClientRect().width > 0);
            if (firstChip && Math.abs(firstChip.getBoundingClientRect().left - titleX) > 1) problems.push(`${row.dataset.title}: 제목/칩 시작점 ${titleX}/${firstChip.getBoundingClientRect().left}px`);
            if (innerWidth <= 480) {
              const state = row.querySelector('.task-state, .task-reserved-dot').getBoundingClientRect();
              const title = row.querySelector('.task-title').getBoundingClientRect();
              if (state.left - r.left > 40) problems.push(`${row.dataset.title}: 휴대폰 왼쪽 빈 열`);
              if (title.width < 100) problems.push(`${row.dataset.title}: 휴대폰 제목 폭 ${title.width}`);
              if (r.height > 76) problems.push(`${row.dataset.title}: 휴대폰 행 높이 ${r.height}`);
              const chips = row.querySelector('.task-row-chips');
              const hasChips = [...chips.children].some(e => e.getBoundingClientRect().height > 0);
              if (!hasChips && r.height > 40) problems.push(`${row.dataset.title}: 메모만 있는 행이 두 줄`);
              const note = row.querySelector('.task-notes-mobile')?.getBoundingClientRect();
              const pick = row.querySelector('.task-pick').getBoundingClientRect();
              if (note && (note.top < pick.top || note.bottom > pick.bottom)) problems.push(`${row.dataset.title}: 메모 별도 줄`);
              const grip = row.querySelector('.task-grip')?.getBoundingClientRect();
              if (grip && (grip.top < title.top - 8 || grip.bottom > title.bottom + 8)) problems.push(`${row.dataset.title}: 손잡이 별도 줄`);
            }
            const leaves = [...row.querySelectorAll('.thread-stage, .task-state, .task-reserved-dot, .list-id, .task-title, .task-chip, .task-notes-mark, .task-notes-mobile, .task-attach-mark, .task-menu-button')].filter(e => e.getBoundingClientRect().width > 0);
            for (const e of leaves) {
              const b = e.getBoundingClientRect();
              if (b.left < r.left - 1 || b.right > r.right + 1 || b.top < r.top - 1 || b.bottom > r.bottom + 1) problems.push(`${row.dataset.title}: ${e.className} 행 밖`);
              if (e.matches('.thread-stage') && e.scrollWidth > e.clientWidth + 1) problems.push(`${row.dataset.title}: 표지 잘림`);
            }
            for (let i = 0; i < leaves.length; i++) for (let j = i + 1; j < leaves.length; j++) {
              const a = leaves[i].getBoundingClientRect(), b = leaves[j].getBoundingClientRect();
              if (Math.min(a.right, b.right) - Math.max(a.left, b.left) > 1 && Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > 1) problems.push(`${row.dataset.title}: ${leaves[i].className} / ${leaves[j].className} 겹침`);
            }
            for (const selector of ['.task-state, .task-reserved-dot', '.list-id', '.task-title', '.task-menu-button']) {
              const e = row.querySelector(selector);
              if (!e) continue;
              const box = e.getBoundingClientRect();
              // 예약 점은 12px+양쪽 여백 2px, 상태 SVG는 16px다. 같은 열의 중심을 비교한다.
              const x = selector.startsWith('.task-state') ? box.left + box.width / 2 : box.left;
              if (columns.has(selector) && Math.abs(columns.get(selector) - x) > 1) problems.push(`${row.dataset.title}: ${selector} 열 어긋남`);
              else columns.set(selector, x);
            }
          }
          const child = rows.find(row => row.dataset.title === '하위 할 일');
          const parent = rows.find(row => row.dataset.title === '연결된 할 일의 충분히 긴 제목');
          if (!child || !parent || titleStart(child) - titleStart(parent) < 8) problems.push('하위 제목 들여쓰기 소실');
          const scroll = document.querySelector('.tasks-scroll');
          if (scroll.scrollWidth > scroll.clientWidth + 1) problems.push('목록 가로 넘침');
          return problems;
        });
        errors.push(...problems.map(s => `${name}: ${s}`));
        // 제목과 메뉴는 바뀐 배치에서도 누를 수 있다.
        await p.locator('.task-row').filter({ hasText: titles[0] }).locator('.task-pick').click();
        await p.getByTestId('task-detail').waitFor();
        assert.equal(await p.locator('.task-row.is-selected').getAttribute('data-title'), titles[0]);
        await p.getByTestId('task-detail-close').click();
        if (width === 320) {
          for (const resized of [820, 320]) {
            await p.setViewportSize({ width: resized, height });
            await p.locator('.task-row').filter({ hasText: titles[1] }).locator('[data-stage="merge_ready"]').waitFor();
            assert.equal(await p.locator('.task-row').filter({ hasText: titles[1] }).locator('[data-stage="merge_ready"]').count(), 1, '폭 변경 뒤 단계 표지 중복');
          }
        }
        await p.close();
      }
    } finally { await wk.close(); }
    assert.deepEqual(errors, [], errors.join('\n'));
  });
}
