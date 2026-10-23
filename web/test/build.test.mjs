import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { buildWeb, syncDist } from "../scripts/build.mjs";

function snapshot(root, result = new Map(), relative = "") {
  const path = join(root, relative);
  result.set(relative, { mtime: statSync(path, { bigint: true }).mtimeNs,
    bytes: statSync(path).isFile() ? readFileSync(path).toString("base64") : null });
  if (statSync(path).isDirectory()) {
    for (const name of readdirSync(path).sort()) snapshot(root, result, join(relative, name));
  }
  return result;
}

test("같은 내용 재생성은 모든 파일·폴더 mtime을 보존한다", (t) => {
  const root = mkdtempSync(join(tmpdir(), "aaa-dist-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const source = join(root, "source");
  const dist = join(root, "dist");
  mkdirSync(join(source, "assets"), { recursive: true });
  writeFileSync(join(source, "assets", "app.js"), "same bytes");
  writeFileSync(join(source, "index.html"), "same html");
  syncDist(source, dist);
  // 시간을 기다리지 않고 재쓰기를 구별할 수 있는 과거 시각을 지정한다.
  for (const name of snapshot(dist).keys()) utimesSync(join(dist, name), 1, 1);
  const before = snapshot(dist);
  writeFileSync(join(source, "assets", "app.js"), "same bytes");
  syncDist(source, dist);
  assert.deepEqual(snapshot(dist), before);

  // 길이·mtime이 같아도 내용 차이, 삭제·추가와 파일/폴더 교체를 반영한다.
  writeFileSync(join(source, "assets", "app.js"), "next bytes");
  utimesSync(join(source, "assets", "app.js"), 1, 1);
  rmSync(join(source, "index.html"));
  writeFileSync(join(source, "new.html"), "new html");
  syncDist(source, dist);
  assert.equal(readFileSync(join(dist, "assets", "app.js"), "utf8"), "next bytes");
  assert.deepEqual(readdirSync(dist).sort(), ["assets", "new.html"]);
  // 대소문자를 구별하지 않는 파일 시스템에서도 새 이름을 지우지 않는다.
  rmSync(join(source, "new.html"));
  writeFileSync(join(source, "NEW.html"), "renamed");
  syncDist(source, dist);
  assert.equal(readFileSync(join(dist, "NEW.html"), "utf8"), "renamed");
  rmSync(join(source, "assets"), { recursive: true });
  writeFileSync(join(source, "assets"), "now a file");
  rmSync(join(source, "NEW.html"));
  mkdirSync(join(source, "new.html"));
  writeFileSync(join(source, "new.html", "child"), "now a directory");
  syncDist(source, dist);
  assert.equal(readFileSync(join(dist, "assets"), "utf8"), "now a file");
  assert.equal(readFileSync(join(dist, "new.html", "child"), "utf8"), "now a directory");
});

test("실제 Vite 빌드의 동일 출력·변경·삭제와 실패 때 이전 dist 보존", async (t) => {
  const root = mkdtempSync(join(tmpdir(), "aaa-vite-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "public"));
  writeFileSync(join(root, "index.html"), '<script type="module" src="/main.js"></script>');
  writeFileSync(join(root, "main.js"), 'document.title = "first";');
  writeFileSync(join(root, "public", "old.txt"), "remove me");
  const dist = join(root, "dist");
  await buildWeb(root);
  for (const name of snapshot(dist).keys()) utimesSync(join(dist, name), 1, 1);
  const before = snapshot(dist);
  await buildWeb(root);
  assert.deepEqual(snapshot(dist), before);

  writeFileSync(join(root, "main.js"), 'document.title = "after";');
  rmSync(join(root, "public", "old.txt"));
  writeFileSync(join(root, "public", "new.txt"), "added");
  await buildWeb(root);
  const after = snapshot(dist);
  assert.notEqual(after.get("index.html").bytes, before.get("index.html").bytes);
  assert.ok(!after.has("old.txt"));
  assert.ok(after.has("new.txt"));
  const bundles = readdirSync(join(dist, "assets"));
  assert.equal(bundles.length, 1);
  assert.match(readFileSync(join(dist, "assets", bundles[0]), "utf8"), /after/);

  const stamp = join(root, "node_modules", ".cache", "aaa-web-build.stamp");
  utimesSync(stamp, 1, 1);
  writeFileSync(join(root, "main.js"), "invalid javascript = ;");
  await assert.rejects(buildWeb(root));
  assert.deepEqual(snapshot(dist), after);
  assert.equal(statSync(stamp).mtimeMs, 1000);
  assert.ok(!readdirSync(join(root, "node_modules", ".cache")).some((name) => name.startsWith("aaa-dist-")));
});
