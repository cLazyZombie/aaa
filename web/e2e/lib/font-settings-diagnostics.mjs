import fs from "node:fs";
import path from "node:path";

// 원래 E2E06은 실패한 기기의 context를 먼저 닫아 공통 진단에 다른 화면만 남았다.
// 입력 순서는 건드리지 않고 최근 사건과 그 순간의 저장값만 메모리에 남긴다.
export function traceFontSettings() {
  const keys = { "화면 글꼴": "aaa:ui-font-family", "고정폭 글꼴": "aaa:mono-font-family", "터미널 글꼴": "aaa:terminal-font-family" };
  window.__fontSettingsEvents = [];
  for (const phase of ["capture", "bubble"]) for (const type of ["focus", "blur", "input", "keydown", "keyup", "compositionstart", "compositionend"]) {
    document.addEventListener(type, event => {
      const input = event.target;
      if (!(input instanceof HTMLInputElement)) return;
      const label = input.getAttribute("aria-label"), key = keys[label];
      if (!key) return;
      window.__fontSettingsEvents.push({ type, phase, label, value: input.value, rendered: input.defaultValue,
        saved: localStorage.getItem(key), key: event.key, keyCode: event.keyCode, composing: event.isComposing,
        active: document.activeElement === input });
      if (window.__fontSettingsEvents.length > 64) window.__fontSettingsEvents.shift();
    }, phase === "capture");
  }
}

export async function saveFontSettingsFailure(root, page, device, error) {
  const state = await page.evaluate(() => ({
    viewport: [innerWidth, innerHeight],
    active: document.activeElement?.getAttribute("aria-label") ?? document.activeElement?.tagName,
    fields: [...document.querySelectorAll(".settings-font-input")].map(input => ({
      label: input.getAttribute("aria-label"), value: input.value, rendered: input.defaultValue,
    })),
    saved: Object.fromEntries(["aaa:ui-font-family", "aaa:mono-font-family", "aaa:terminal-font-family"].map(key => [key, localStorage.getItem(key)])),
    events: window.__fontSettingsEvents ?? [],
  }));
  const dir = path.join(root, "diagnostics");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, `font-settings-${device}.json`), JSON.stringify({ device, error: String(error), ...state }, null, 2) + "\n");
}
