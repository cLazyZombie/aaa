import { domIs, shot } from "./harness.mjs";

// 실제 목록의 ID가 보고서 inline code와 같은 색·글꼴·크기인지, 제목과 독립해 온전히 보이는지 검사한다.
export async function listIds(page, selector, ids, name) {
  await domIs(page, "목록의 고정 ID", ({ selector, ids }) => {
    const found = [...document.querySelectorAll(`${selector} code.list-id`)].map((el) => el.textContent);
    return ids.every((id) => found.includes(id)) && new Set(found).size === found.length;
  }, { selector, ids });
  const theme = await page.evaluate(() => document.documentElement.dataset.theme);
  try {
    for (const mode of ["light", "dark"]) {
      await page.evaluate((mode) => { document.documentElement.dataset.theme = mode; }, mode);
      const errors = await page.evaluate(({ selector }) => {
        const errors = [];
        const probe = document.createElement("span");
        // 보고서 줄의 기존 inline code 값. 공통 컴포넌트 class는 참조하지 않는다.
        probe.style.cssText = "font-size:var(--fs-sm);color:var(--text);position:fixed";
        const code = document.createElement("code"); probe.append(code); document.body.append(probe);
        const expected = getComputedStyle(code);
        for (const id of document.querySelectorAll(`${selector} code.list-id`)) {
          const style = getComputedStyle(id);
          for (const prop of ["fontSize", "fontFamily", "backgroundColor", "color", "paddingTop", "paddingRight", "paddingBottom", "paddingLeft", "borderRadius"]) {
            if (style[prop] !== expected[prop]) errors.push(`${id.textContent} ${prop}: ${style[prop]} != ${expected[prop]}`);
          }
          const row = id.closest("button, .report-row-main, .report-heading");
          const box = id.getBoundingClientRect(), parent = row.getBoundingClientRect();
          if (!box.width || id.scrollWidth > id.clientWidth + 1 || box.left < parent.left - 1 || box.right > parent.right + 1) errors.push(`${id.textContent}: 번호 잘림`);
          if (row.scrollWidth > row.clientWidth + 1) errors.push(`${id.textContent}: 행 넘침`);
          if (row.matches(".ask-row, .automation-card") && row.scrollHeight > row.clientHeight + 1) errors.push(`${id.textContent}: 목록 행 세로 넘침`);
        }
        probe.remove();
        // 긴 제목에서도 번호는 남고 제목만 줄여야 한다. DOM을 복원해 기존 이동·답·편집 시험을 그대로 잇는다.
        for (const row of document.querySelectorAll(selector)) {
          const title = row.querySelector(".task-title, .automation-card-title, .ask-row-title");
          if (!title) continue;
          const html = title.innerHTML;
          try {
            title.textContent = "아주 긴 목록 제목 ".repeat(100);
            if (getComputedStyle(title).textOverflow !== "ellipsis" || title.scrollWidth <= title.clientWidth) errors.push("긴 제목을 줄이지 않음");
            if (row.scrollWidth > row.clientWidth + 1) errors.push("긴 제목 행 넘침");
            for (const id of row.querySelectorAll("code.list-id")) {
              const box = id.getBoundingClientRect(), parent = row.getBoundingClientRect();
              if (!box.width || id.scrollWidth > id.clientWidth + 1 || box.left < parent.left - 1 || box.right > parent.right + 1) errors.push(`${id.textContent}: 긴 제목에서 번호 잘림`);
            }
          } finally { title.innerHTML = html; }
        }
        return errors;
      }, { selector });
      if (errors.length) throw new Error(errors.join("\n"));
      await shot(page, `${name}-${mode}`);
    }
  } finally {
    await page.evaluate((theme) => {
      if (theme === undefined) delete document.documentElement.dataset.theme;
      else document.documentElement.dataset.theme = theme;
    }, theme);
  }
}
