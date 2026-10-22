import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@xterm/xterm/css/xterm.css";
import "./herdr/tokens.css";
import "./herdr/modal.css";
import "./style.css";
import "./github-themes.css";
// herdr-web-ui lib/viewport.ts: 화면 키보드 위로 화면을 맞춘다(--app-height, data-keyboard)
import "./herdr/viewport.ts";
import { App } from "./App";
import "./PanelRows.css";
import { registerServiceWorker } from "./push";
import { initTheme } from "./theme";
import { initDeviceFonts } from "./settings";
// herdr-web-ui main.tsx: 휴대폰 sheet(설정·확인 창)가 닫힐 때 내려가는 움직임
import { watchSheetExits } from "./herdr/sheets.ts";

initTheme();
initDeviceFonts();
watchSheetExits();
registerServiceWorker();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
