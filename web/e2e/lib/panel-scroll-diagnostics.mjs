import fs from "node:fs";
import path from "node:path";

// 관측만 한다. 스크롤·높이·휠 전달·대기 조건을 바꾸지 않는다.
export function installPanelScrollTrace() {
  const events = [];
  const ids = new WeakMap(); let nextId = 0;
  let selected = null, phase = "entry", resize = null, mutation = null;
  const read = (details = false) => {
    const element = selected && document.querySelector(selected.selector);
    if (!element) return { selected, present: false };
    if (!ids.has(element)) ids.set(element, ++nextId);
    const state = { selected, present: true, elementId: ids.get(element), connected: element.isConnected, scrollTop: element.scrollTop,
      clientHeight: element.clientHeight, scrollHeight: element.scrollHeight, bottomGap: element.scrollHeight - element.clientHeight - element.scrollTop,
    };
    if (details) {
      const box = element.getBoundingClientRect(), rows = element.querySelectorAll(selected.rowSelector), last = rows[rows.length - 1], end = last?.getBoundingClientRect();
      Object.assign(state, { overflow: getComputedStyle(element).overflowY, scrollBehavior: getComputedStyle(element).scrollBehavior,
        scrollSnapType: getComputedStyle(element).scrollSnapType, fonts: document.fonts.status,
        hiddenBy: element.closest("[hidden]")?.getAttribute("data-testid") ?? element.closest("[hidden]")?.id ?? null,
        rows: rows.length, top: box.top, bottom: box.bottom, last: end ? { top: end.top, bottom: end.bottom } : null });
    }
    return state;
  };
  const mark = (event, detail = {}, sample = true) => {
    events.push({ at: performance.now(), phase, event, hash: location.hash, ...detail, ...(sample ? { state: read() } : {}) });
    if (events.length > 3000) events.shift();
  };
  for (const type of ["hashchange", "popstate"]) window.addEventListener(type, () => mark(type, {}, false));
  for (const type of ["wheel", "scroll", "scrollend"]) document.addEventListener(type, event => {
    if (!selected) return;
    const target = event.target instanceof Element ? event.target : null;
    mark(type, { target: target?.getAttribute("data-testid") ?? target?.className,
      onSelected: target === document.querySelector(selected.selector), position: type === "wheel" ? undefined : target?.scrollTop,
      deltaX: event.deltaX, deltaY: event.deltaY, trusted: event.isTrusted }, false);
  }, { capture: true, passive: true });
  window.__panelScrollTrace = {
    mark,
    phase: value => { phase = value; mark("phase", {}, false); },
    begin: value => {
      resize?.disconnect(); mutation?.disconnect(); selected = value; phase = "geometry";
      const element = document.querySelector(value.selector);
      if (element) {
        resize = new ResizeObserver(() => mark("resize", {}, false)); resize.observe(element);
        mutation = new MutationObserver(records => mark("mutation", { records: records.length }, false));
        mutation.observe(element, { subtree: true, childList: true, attributes: true, characterData: true });
      }
      mark("begin", { selected }, false);
    },
    snapshot: () => ({ selected, phase, current: read(true), events }),
  };
}

export class PanelScrollDiagnostics {
  constructor(root, page, engine, name) {
    this.root = root;
    this.page = page;
    this.dir = path.join(root, "diagnostics", "panel-scroll");
    this.label = `${engine}-${name}`;
  }
  async save(error) {
    fs.mkdirSync(this.dir, { recursive: true });
    const stem = path.join(this.dir, this.label);
    const state = await this.page.evaluate(() => window.__panelScrollTrace?.snapshot());
    fs.writeFileSync(`${stem}.json`, JSON.stringify({ label: this.label, error: error ? String(error) : null, stack: error?.stack, ...state }, null, 2));
    if (error || process.env.AAA_E2E_PANEL_TRACE) {
      try { await this.page.screenshot({ path: `${stem}.png`, timeout: 5000 }); }
      catch (captureError) { console.error("목록 진단 화면 저장 오류", String(captureError)); }
    }
    if (error) {
      console.error("panel-scroll-failure", JSON.stringify({ label: this.label, error: String(error), selected: state?.selected, phase: state?.phase, current: state?.current }));
    }
    if (process.env.AAA_E2E_PANEL_TRACE && process.env.AAA_E2E_ARTIFACTS) {
      fs.cpSync(this.dir, path.join(process.env.AAA_E2E_ARTIFACTS, path.basename(this.root), "panel-scroll"), { recursive: true });
    }
  }
}
