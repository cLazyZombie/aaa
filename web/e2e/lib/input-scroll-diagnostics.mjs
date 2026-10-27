import fs from "node:fs";
import path from "node:path";
import { inTime } from "./harness.mjs";

// 페이지를 닫거나 실패 진단용 resize를 하기 전에 저장한다. 성공 경로는 건드리지 않는다.
export async function saveInputScrollFailure(root, page, label, error, extra = {}) {
  const dir = path.join(root, "diagnostics", label);
  fs.mkdirSync(dir, { recursive: true });
  const write = (name, value) => fs.writeFileSync(path.join(dir, name), value);
  write("failure.json", JSON.stringify({ error: String(error), stack: error.stack, ...extra }, null, 2));
  for (const role of ["host", "agent"]) {
    write(`${role}.log`, fs.readFileSync(path.join(root, `${role}.log`), "utf8")
      .replace(/\bfp=[0-9a-f]+\b/g, "fp=<redacted>"));
  }
  await Promise.all([
    inTime(page.evaluate(() => {
      const el = document.querySelector('.term:not([hidden]) [data-testid="terminal"]');
      const vp = el?.querySelector(".xterm-viewport");
      const active = document.activeElement;
      return { hash: location.hash, active: { tag: active?.tagName, kind: active?.className,
        pane: active?.closest(".term")?.dataset.pane }, focus: window.chatFocusLog,
        scroll: window.takeScrollStates?.(), term: el && { ...el.dataset },
        native: vp && { top: vp.scrollTop, height: vp.scrollHeight, client: vp.clientHeight },
        lines: el?.querySelector(".xterm-rows")?.textContent, log: window.__aaaTermLog };
    }), 5000, "입력·스크롤 실패 상태 저장")
      .then(state => write("state.json", JSON.stringify(state, null, 2)))
      .catch(cause => write("state-error.txt", String(cause))),
    page.screenshot({ path: path.join(dir, "screen.png"), timeout: 5000 })
      .catch(cause => write("screen-error.txt", String(cause))),
  ]);
  console.log(`입력·스크롤 실패 진단: ${dir}`);
}
