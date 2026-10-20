// 대화 글의 링크 중 휴대폰에서 앱 안으로 여는 것(src/reportLinks.ts, plan §8.299). 같은 host의 보고서 보기 경로·앱 주소만 고르고,
// 다른 host·내려받기·파일 읽기 경로·API는 새 창 그대로다. 보조 키·가운데 단추는 브라우저 몫이다(t3code `resolveLinkTarget`과 같다).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { browserClick, inAppLink } from "../src/reportLinks.ts";

const O = "https://mac-a.example.ts.net:8443";

describe("앱 안에서 여는 링크", () => {
  it("보기 경로: 맨 위와 그 안 경로(풀어 쓴 것)", () => {
    assert.deepEqual(inAppLink(`${O}/v/abc/`, O), { kind: "view", base: "/v/abc/", path: "" });
    assert.deepEqual(inAppLink(`${O}/v/abc/%EB%B3%B4%EA%B3%A0/a%20b.md`, O), { kind: "view", base: "/v/abc/", path: "보고/a b.md" });
    assert.deepEqual(inAppLink("/v/abc/index.html", O), { kind: "view", base: "/v/abc/", path: "index.html" });
  });
  it("앱 보고서 화면 주소는 그 보고서, 그 밖의 앱 주소는 주소 그대로", () => {
    assert.deepEqual(inAppLink(`${O}/#report=demo/t-0448`, O), { kind: "report", project: "demo", name: "t-0448" });
    assert.deepEqual(inAppLink(`${O}/#report=demo%2Fpage`, O), { kind: "report", project: "demo", name: "page" });
    assert.deepEqual(inAppLink(`${O}/#thread=t-0001`, O), { kind: "app", hash: "#thread=t-0001" });
  });
  it("다른 host·내려받기·다른 경로·깨진 주소는 새 창 그대로", () => {
    for (const href of [
      "https://other.example.ts.net:8443/v/abc/",
      "http://mac-a.example.ts.net:8770/t-0001/index.html",
      `${O}/v/abc/a.png?download=1`,
      `${O}/f/cap/a.png`,
      `${O}/api/v1/me`,
      `${O}/`,
      `${O}/v/abc/%E0%A4%A`,
      `${O}/#report=demo`,
      `${O}/#report=%E0%A4%A`,
      "http://[",
    ])
      assert.equal(inAppLink(href, O), null, href);
  });
});

describe("브라우저 몫의 누름", () => {
  const plain = { button: 0, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false };
  it("보통 누름만 앱 몫", () => {
    assert.equal(browserClick(plain), false);
    for (const k of ["metaKey", "ctrlKey", "shiftKey", "altKey"]) assert.equal(browserClick({ ...plain, [k]: true }), true, k);
    assert.equal(browserClick({ ...plain, button: 1 }), true);
  });
});
