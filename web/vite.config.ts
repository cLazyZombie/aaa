import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 결과물은 aa host 실행 파일에 포함된다(crates/aa/build.rs가 web/dist를 읽는다).
export default defineConfig({
  plugins: [react()],
  build: { outDir: "dist", emptyOutDir: true, sourcemap: false, chunkSizeWarningLimit: 1500 },
});
