import { fileURLToPath } from "node:url";
import { defineConfig, normalizePath } from "vite";
import react from "@vitejs/plugin-react";
import { buildXtermSource } from "./xterm-source.mjs";
import { serviceWorkerBuild } from "./scripts/service-worker-build.ts";

// IME 패치를 붙인 xterm 소스(herdr-web-ui와 같은 방식, patches/README.md)
const xtermSource = normalizePath(buildXtermSource(fileURLToPath(new URL(".", import.meta.url))) as string);

// 결과물은 aaa host 실행 파일에 포함된다(crates/aaa-host/build.rs가 web/dist를 읽는다).
export default defineConfig({
  // dist/sw.js에 이 빌드의 이름과 파일을 찍는다(herdr-web-ui scripts/service-worker-build.ts, plan §8.103)
  plugins: [react(), serviceWorkerBuild()],
  build: { outDir: "dist", emptyOutDir: true, sourcemap: false, chunkSizeWarningLimit: 1500 },
  resolve: {
    alias: [
      { find: /^@xterm\/xterm$/, replacement: `${xtermSource}/browser/public/Terminal.js` },
      { find: /^browser\//, replacement: `${xtermSource}/browser/` },
      { find: /^common\//, replacement: `${xtermSource}/common/` },
    ],
  },
});
