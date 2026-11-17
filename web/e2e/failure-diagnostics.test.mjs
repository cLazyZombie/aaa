import { test } from "node:test";
import path from "node:path";
import { createRequire } from "node:module";
import { launchChromium } from "./lib/chromium-launch.mjs";
import { failureDiagnosticsCase } from "./lib/failure-diagnostics-case.mjs";
const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), "noop.js"));
const { chromium, webkit } = require("playwright");
for (const type of ["chromium", "webkit"]) test(`${type}: 닫힌 context의 locator·원인 stack·DOM·화면·trace 보존`, async () => {
  const browser = type === "chromium" ? await launchChromium(chromium) : await webkit.launch();
  try { await failureDiagnosticsCase(browser, type); }
  finally { await browser.close(); }
});
