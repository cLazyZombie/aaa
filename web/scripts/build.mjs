// Vite는 임시 폴더에 완성하고, 바뀐 파일만 dist에 반영한다.
// 같은 빌드로 dist의 파일·폴더 mtime을 바꾸면 Cargo가 host·실행 파일까지 다시 컴파일한다.
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "vite";

/** 추가·삭제와 파일/폴더 교체도 반영하되 같은 bytes는 다시 쓰지 않는다. */
export function syncDist(source, destination) {
  const entries = readdirSync(source, { withFileTypes: true });
  let previous;
  try {
    previous = new Map(readdirSync(destination, { withFileTypes: true }).map((entry) => [entry.name, entry]));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    mkdirSync(destination, { recursive: true });
    previous = new Map();
  }
  // Windows에서 대소문자만 바꾼 이름도 옛 항목을 먼저 지우고 새 이름으로 만든다.
  const names = new Set(entries.map((entry) => entry.name));
  for (const name of previous.keys()) {
    if (!names.has(name)) {
      rmSync(join(destination, name), { recursive: true });
      previous.delete(name);
    }
  }
  for (const entry of entries) {
    const from = join(source, entry.name);
    const to = join(destination, entry.name);
    const old = previous.get(entry.name);
    if (!entry.isFile() && !entry.isDirectory()) throw new Error(`웹 빌드에 일반 파일·폴더가 아닌 항목: ${from}`);
    if (old && (old.isDirectory() !== entry.isDirectory() || old.isSymbolicLink())) {
      rmSync(to, { recursive: true });
      previous.delete(entry.name);
    }
    if (entry.isDirectory()) {
      syncDist(from, to);
    } else {
      const bytes = readFileSync(from);
      if (!previous.has(entry.name) || !readFileSync(to).equals(bytes)) writeFileSync(to, bytes);
    }
  }
}

export async function buildWeb(root) {
  // Cargo가 감시하는 dist 밖이며 npm ci가 지워도 되는 이 worktree 전용 폴더다.
  const cache = join(root, "node_modules", ".cache");
  mkdirSync(cache, { recursive: true });
  const staging = mkdtempSync(join(cache, "aaa-dist-"));
  try {
    await build({ root, build: { outDir: staging, emptyOutDir: true } });
    syncDist(staging, join(root, "dist"));
    // dist/index.html이 그대로여도 --only e2e는 마지막으로 성공한 웹 빌드 시각을 확인한다.
    writeFileSync(join(cache, "aaa-web-build.stamp"), "");
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await buildWeb(fileURLToPath(new URL("..", import.meta.url)));
}
