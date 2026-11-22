import { test } from "node:test";
import assert from "node:assert/strict";
import { FRESH_PARAM, freshUrl, withoutFresh } from "../src/freshShell.ts";
import { readFileSync } from "node:fs";

test("「앱 다시 열기」 표지는 자리 hash·다른 인자를 두고 붙였다 지운다", () => {
  const href = "https://aaa.example.ts.net/?settings=chat#pane=p1";
  const fresh = freshUrl(href);
  assert.equal(new URL(fresh).searchParams.get(FRESH_PARAM), "1");
  assert.equal(new URL(fresh).hash, "#pane=p1");
  assert.equal(withoutFresh(fresh), "/?settings=chat#pane=p1");
  assert.equal(withoutFresh(href), null);
  assert.equal(withoutFresh("https://aaa.example.ts.net/?fresh=1"), "/");
});

test("service worker와 같은 표지 이름을 쓴다", () => {
  const sw = readFileSync(new URL("../public/sw.js", import.meta.url), "utf8");
  assert.equal(/const FRESH = "([^"]+)";/.exec(sw)?.[1], FRESH_PARAM);
});
