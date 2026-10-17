import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@xterm/xterm/css/xterm.css";
import "./style.css";
// herdr-web-ui lib/viewport.ts: 화면 키보드 위로 화면을 맞춘다(--app-height, data-keyboard)
import "./herdr/viewport.ts";
import { App } from "./App";
import { registerServiceWorker } from "./push";
import { initTheme } from "./theme";

initTheme();
registerServiceWorker();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
