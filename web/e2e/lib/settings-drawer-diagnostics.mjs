// E2E39: 공통 진단이 화면을 움직이거나 context가 닫히기 전에 원래 서랍 상태를 보관한다.
import fs from "node:fs";
import path from "node:path";

export class SettingsDrawerDiagnostics {
  constructor(root) {
    this.dir = path.join(root, "diagnostics", "settings-drawer");
    this.active = null;
  }
  watch(page, engine, close) {
    this.active = { page, engine, close, phase: "첫 pane 선택 준비" };
  }
  phase(phase) { if (this.active) this.active.phase = phase; }
  async save(error) {
    try { await this.capture(error); }
    catch (captureError) { console.log(`설정 서랍 진단 저장 실패: ${captureError}`); }
  }
  async capture(error) {
    const active = this.active;
    if (!active || active.saved) return;
    active.saved = true;
    const { page, engine, close, phase } = active;
    fs.mkdirSync(this.dir, { recursive: true });
    const stem = path.join(this.dir, `${engine}-${close}`);
    const result = { engine, close, phase, error: String(error), stack: error.stack };
    try {
      result.state = await page.evaluate(() => {
        const describe = (el) => {
          if (!el) return null;
          const style = getComputedStyle(el), box = el.getBoundingClientRect();
          return { id: el.id, testid: el.getAttribute("data-testid"), class: el.className,
            hidden: el.hidden, display: style.display, visibility: style.visibility, transform: style.transform,
            current: el.getAttribute("aria-current"), expanded: el.getAttribute("aria-expanded"),
            box: { x: box.x, y: box.y, width: box.width, height: box.height },
            animations: el.getAnimations().map((a) => ({ state: a.playState, time: a.currentTime })) };
        };
        return { hash: location.hash, settings: new URLSearchParams(location.search).get("settings"),
          length: history.length, settingsEntry: history.state?.["aaa:settings"] ?? null,
          rewind: window.__settingsRewind, trace: window.__settingsDrawerTrace, viewport: [innerWidth, innerHeight],
          drawer: describe(document.getElementById("workspace-drawer")),
          panes: ["bk1", "bk2"].map((id) => describe(document.querySelector(`[data-testid="pane-${id}"]`))),
          toggles: [...document.querySelectorAll(".view [data-testid='drawer-toggle']")].map(describe),
          panels: [...document.querySelectorAll("[data-panel]")].map(describe) };
      });
    } catch (captureError) { result.captureError = String(captureError); }
    fs.writeFileSync(`${stem}.json`, JSON.stringify(result, null, 2));
    try { await page.screenshot({ path: `${stem}.png`, timeout: 5000 }); }
    catch (captureError) { console.log(`설정 서랍 화면 저장 실패: ${captureError}`); }
    console.log(`설정 서랍 실패 ${engine}/${close}/${phase}: ${JSON.stringify(result.state ?? result.captureError)}`);
  }
}
