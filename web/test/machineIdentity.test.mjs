import assert from "node:assert/strict";
import { test } from "node:test";
import { migrateMachineDrafts } from "../src/machineIdentity.ts";
function storage(entries) {
  const map = new Map(entries);
  return { get length() { return map.size; }, key: i => [...map.keys()][i] ?? null,
    getItem: key => map.get(key) ?? null, setItem: (key, value) => map.set(key, value), removeItem: key => map.delete(key) };
}
test("이름이 바뀐 채 다시 열어도 pane ID로 두 터미널 초안을 옮긴다", () => {
  const s=storage([["herdr-web-ui:terminal-line:mac-b:pane-a","보내지 않은 글"],["herdr-web-ui:terminal-draft:mac-b:pane-a",'{"text":"보류"}'],["herdr-web-ui:terminal-line:mac-b:pane-b","다른 pane"]]);
  migrateMachineDrafts("m-a","pane-a",s);
  assert.equal(s.getItem("herdr-web-ui:terminal-line:m-a:pane-a"),"보내지 않은 글");
  assert.equal(s.getItem("herdr-web-ui:terminal-draft:m-a:pane-a"),'{"text":"보류"}');
  assert.equal(s.getItem("herdr-web-ui:terminal-line:mac-b:pane-a"),null);
  assert.equal(s.getItem("herdr-web-ui:terminal-line:mac-b:pane-b"),"다른 pane");
});
test("새 키와 모호한 옛 키는 덮거나 임의로 고르지 않는다", () => {
  const s=storage([["herdr-web-ui:terminal-line:mac-b:pane-a","옛 글"],["herdr-web-ui:terminal-line:mac-c:pane-a","다른 옛 글"],["herdr-web-ui:terminal-draft:m-a:pane-a","새 글"],["herdr-web-ui:terminal-draft:mac-b:pane-a","옛 보류"]]);
  migrateMachineDrafts("m-a","pane-a",s);
  assert.equal(s.getItem("herdr-web-ui:terminal-line:m-a:pane-a"),null);
  assert.equal(s.getItem("herdr-web-ui:terminal-line:mac-b:pane-a"),"옛 글");
  assert.equal(s.getItem("herdr-web-ui:terminal-draft:m-a:pane-a"),"새 글");
  assert.equal(s.getItem("herdr-web-ui:terminal-draft:mac-b:pane-a"),"옛 보류");
});
