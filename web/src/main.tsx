import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@xterm/xterm/css/xterm.css";
import "./style.css";
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
