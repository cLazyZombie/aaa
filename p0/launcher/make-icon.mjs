// aaa-launcher.app 아이콘(AppIcon.icns)을 web/public/icon.svg(F3 a 블록 커서)에서 만든다. 개발 머신에서 한 번 돌리고 결과를 커밋한다.
// macOS 앱 아이콘 격자: 1024 칸 안 가운데 824 둥근 사각형(반지름 185). 크기마다 SVG를 다시 그려 작게 줄인 흐림이 없다.
//
// 사용: NODE_PATH=$(npm root -g) node p0/launcher/make-icon.mjs   (playwright 전역 설치, iconutil은 macOS 기본)
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");
const here = path.dirname(new URL(import.meta.url).pathname);
const svg = fs.readFileSync(path.join(here, "../../web/public/icon.svg"), "utf8");
const set = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-icon-")) + "/AppIcon.iconset";
fs.mkdirSync(set);
const sizes = [16, 32, 128, 256, 512];
const browser = await chromium.launch();
try {
  for (const base of sizes) {
    for (const scale of [1, 2]) {
      const px = base * scale;
      const page = await browser.newPage({ viewport: { width: px, height: px } });
      const inset = (100 / 1024) * px;
      const side = (824 / 1024) * px;
      const radius = (185 / 1024) * px;
      await page.setContent(`<body style="margin:0;background:transparent">
        <div style="position:absolute;left:${inset}px;top:${inset}px;width:${side}px;height:${side}px;border-radius:${radius}px;overflow:hidden">
          ${svg.replace("<svg ", `<svg style="display:block;width:${side}px;height:${side}px" `)}
        </div></body>`);
      const name = scale === 1 ? `icon_${base}x${base}.png` : `icon_${base}x${base}@2x.png`;
      await page.screenshot({ path: path.join(set, name), omitBackground: true });
      await page.close();
    }
  }
} finally {
  await browser.close();
}
execFileSync("iconutil", ["-c", "icns", "-o", path.join(here, "AppIcon.icns"), set]);
fs.rmSync(path.dirname(set), { recursive: true, force: true });
console.log(`만들었다: ${path.join(here, "AppIcon.icns")}`);
