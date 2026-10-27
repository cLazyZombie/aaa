import assert from "node:assert/strict";
import { test } from "node:test";
import { readReportMediaStatus, reportMediaTarget } from "../src/reportMediaProbe.ts";

test("매체 진단은 보고서 파일만 받고 표시할 경로에서 권한·query를 제거한다", () => {
  const origin = "https://example.com";
  assert.deepEqual(reportMediaTarget("/v/private-cap/sub/a%20b.png?private=1#x", origin), { url: `${origin}/v/private-cap/sub/a%20b.png?private=1`, path: "sub/a b.png" });
  for (const src of ["/api/v1/me", "/v/cap/", "data:image/png,1", "https://other.example.ts.net/v/cap/a.png", "https://name:secret@example.com/v/cap/a.png"])
    assert.equal(reportMediaTarget(src, origin), null);
});

test("사용자 진단은 cookie 없는 HEAD 한 번이며 HTTP 원인만 남긴다", async () => {
  const controller = new AbortController();
  let count = 0;
  const read = async (_url, options) => {
    count++;
    assert.equal(options.method, "HEAD"); assert.equal(options.credentials, "omit"); assert.equal(options.redirect, "error"); assert.equal(options.cache, "no-store");
    return new Response(null, { status: 401, headers: { location: "/v/private-cap/file.png" } });
  };
  const got = await readReportMediaStatus("https://example.ts.net/v/private-cap/file.png", controller.signal, read);
  assert.equal(count, 1); assert.equal(got.status, 401); assert.match(got.message, /권한.*끝/);
  assert(!JSON.stringify(got).includes("private-cap"));
  const failed = await readReportMediaStatus("/v/cap/file.png", controller.signal, async () => { throw Error("secret url"); });
  assert.equal(failed.status, null); assert(!JSON.stringify(failed).includes("secret url"));
});

test("화면이 바뀐 HEAD는 취소되고 원문 오류를 결과로 남기지 않는다", async () => {
  const controller = new AbortController();
  const pending = readReportMediaStatus("/v/cap/a.png", controller.signal, async (_url, options) => new Promise((_resolve, reject) => {
    options.signal.addEventListener("abort", () => reject(new DOMException("private response", "AbortError")), { once: true });
  }));
  controller.abort();
  await assert.rejects(pending, { name: "AbortError" });
});
