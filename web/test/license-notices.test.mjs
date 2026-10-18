// 웹 빌드가 함께 넣은 패키지 찾기(scripts/license-notices.ts, plan §8.175). 빌드가 실제로 낸 고지(따로 받는 조각·패치한
// xterm 소스·글을 싣지 않은 패키지)는 웹 E2E 16 「긴 소스 가상화」가 host에서 받아 본다.
import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { packageDir } from "../scripts/license-notices.ts";

describe("packageDir", () => {
  it("is the package folder under the last node_modules, scoped or not", () => {
    assert.equal(packageDir("/w/web/node_modules/diff/libesm/index.js"), "/w/web/node_modules/diff");
    assert.equal(packageDir("/w/web/node_modules/@pierre/diffs/dist/react/index.js"), "/w/web/node_modules/@pierre/diffs");
    assert.equal(packageDir("/w/web/node_modules/a/node_modules/@s/b/x.js?commonjs-proxy"), "/w/web/node_modules/a/node_modules/@s/b");
  });
  it("is no package for the app's own sources, virtual modules and dot folders, unless a source names its package", () => {
    assert.equal(packageDir("/w/web/src/App.tsx"), null);
    assert.equal(packageDir("\0commonjsHelpers.js"), null);
    assert.equal(packageDir("/w/web/node_modules/.cache/aaa-xterm/common/Types.js"), null);
    const sources = [["/w/web/node_modules/.cache/aaa-xterm/", "/w/web/node_modules/@xterm/xterm"]];
    assert.equal(packageDir("/w/web/node_modules/.cache/aaa-xterm/common/Types.js", sources), "/w/web/node_modules/@xterm/xterm");
  });
});
