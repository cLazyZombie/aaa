// service worker 빌드 도장(web/scripts/service-worker-build.ts). herdr-web-ui(aa624cb) scripts/service-worker-build.test.ts의
// 사례를 node:test로 옮겼다(bun:test expect → node:assert, 고지는 web/src/herdr/README.md). aaa 파일 이름으로 바꾼 것만 다르다.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { buildFiles, buildId, stampServiceWorker } from "../scripts/service-worker-build.ts";

const source = readFileSync(join(fileURLToPath(new URL(".", import.meta.url)), "..", "public", "sw.js"), "utf8");

describe("service worker build stamp", () => {
  it("names a build after its hashed files only, in any order", () => {
    const files = ["assets/index-a1.js", "assets/katex-b2.js", "index.html", "sw.js"];
    assert.match(buildId(files), /^[0-9a-f]{12}$/);
    assert.equal(buildId([...files].reverse()), buildId(files));
    assert.equal(buildId([...files, "offline.html"]), buildId(files));
    assert.notEqual(buildId(["assets/index-c3.js", "assets/katex-b2.js"]), buildId(files));
  });

  it("stamps the one BUILD line of public/sw.js and nothing else", () => {
    const stamped = stampServiceWorker(source, "0123456789ab");
    assert.ok(stamped.includes('const BUILD = "0123456789ab";'));
    assert.equal(stamped.replace('const BUILD = "0123456789ab";', 'const BUILD = "";'), source);
  });

  it("refuses a worker it cannot stamp, and a build id that is not one", () => {
    assert.throws(() => stampServiceWorker(source.replace('const BUILD = "";', ""), "0123456789ab"), /exactly one/);
    assert.throws(() => stampServiceWorker(source.replace("const BUILD_FILES = null;", ""), "0123456789ab"), /exactly one/);
    assert.throws(() => stampServiceWorker(source, 'x"; evil()'), /not a build id/);
    assert.throws(() => stampServiceWorker(source, "0123456789ab", ['/assets/a.js"]; evil(); //']), /not a build file/);
    assert.throws(() => stampServiceWorker(source, "0123456789ab", ["/assets/../sw.js"]), /not a build file/);
    assert.ok(stampServiceWorker(source, "0123456789ab", ["/assets/nested/index-a1.js"]).includes('["/assets/nested/index-a1.js"]'));
  });

  it("lists the build's own files the page asks for, without typefaces or precompressed copies", () => {
    const files = buildFiles(["assets/katex-b2.js", "assets/index-a1.js", "assets/index-a1.css", "assets/index-a1.js.br", "assets/index-a1.js.gz",
      "assets/Meslotendard-Regular-abc.woff2", "assets/Some-x.woff", "assets/Some-x.ttf", "index.html", "sw.js"]);
    assert.deepEqual(files, ["/assets/index-a1.css", "/assets/index-a1.js", "/assets/katex-b2.js"]);
    const stamped = stampServiceWorker(source, "0123456789ab", files);
    assert.ok(stamped.includes(`const BUILD_FILES = ${JSON.stringify(files)};`));
    assert.ok(stamped.includes('const BUILD = "0123456789ab";'));
    assert.ok(!stamped.includes("const BUILD_FILES = null;"));
  });
});
