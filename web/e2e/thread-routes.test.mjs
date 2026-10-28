// 기존 E12 본문 수명 keeper를 두 실제 엔진에서 따로 확인한다.
import { createRequire } from "node:module";
import path from "node:path";
import { test } from "node:test";
import { threadRoutesLifetime } from "./lib/thread-routes-lifetime.mjs";
import { launchChromium } from "./lib/chromium-launch.mjs";

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), "noop.js"));
const { chromium, webkit } = require("playwright");
for (const engine of [chromium, webkit]) test(`${engine.name()}: 두 보류 응답을 마친 뒤 route를 해제한다`, { timeout: 5000 }, async () => {
  const browser = await (engine === chromium ? launchChromium(engine) : engine.launch({ headless: true }));
  try { await threadRoutesLifetime(browser); }
  finally { await browser.close(); }
});
