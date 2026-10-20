// 보고서 그림 크게 보기(src/reportImages.ts, plan §8.349): 보고서 iframe이 보낸 그림 누름 알림은 믿지 않는다. 그 보고서 보기 경로
// `/v/<cap>/` 아래 같은 origin 주소만 받고, 누른 그림이 그 보고서 것이 아니면 열지 않는다. 보고 글 그림은 `library/…`만 보기 경로로 옮긴다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { MAX_REPORT_IMAGES, reportImageUrl, reportImages, reportMarkdownImage, viewRoot } from "../src/reportImages.ts";

const ORIGIN = "https://mac-a.example.ts.net";
const ROOT = "/v/cap1/";
const msg = (images, at = 0) => ({ type: "aaa:report-image", at, images });

describe("보고서 보기 경로 뿌리", () => {
  it("같은 origin 보기 경로의 `/v/<cap>/`만", () => {
    assert.equal(viewRoot("/v/cap1/index.html", ORIGIN), ROOT);
    assert.equal(viewRoot(`${ORIGIN}/v/cap1/a/b.html`, ORIGIN), ROOT);
    assert.equal(viewRoot("/v/cap1/", ORIGIN), ROOT);
    for (const bad of ["/f/cap1/index.html", "/api/v1/me", "https://other.example.ts.net/v/cap1/index.html", "/v/", "/v/cap1"])
      assert.equal(viewRoot(bad, ORIGIN), null, bad);
  });
});

describe("그림 주소 거르기", () => {
  it("그 보고서 아래 같은 origin 파일만 받고 조각(#)은 버린다", () => {
    assert.equal(reportImageUrl(`${ORIGIN}/v/cap1/shot.png`, ROOT, ORIGIN), `${ORIGIN}/v/cap1/shot.png`);
    assert.equal(reportImageUrl(`${ORIGIN}/v/cap1/a/b%20c.png?v=2#x`, ROOT, ORIGIN), `${ORIGIN}/v/cap1/a/b%20c.png?v=2`);
    for (const bad of [
      "data:image/png;base64,AAAA",
      "blob:https://mac-a.example.ts.net/1234",
      "https://other.example.ts.net/v/cap1/shot.png",
      `${ORIGIN}/v/cap2/shot.png`,
      `${ORIGIN}/v/cap1/../cap2/shot.png`,
      `${ORIGIN}/v/cap1/%2e%2e/cap2/shot.png`,
      `${ORIGIN}/api/v1/me`,
      `${ORIGIN}/v/cap1/`,
      "javascript:alert(1)",
      "",
      42,
      null,
    ])
      assert.equal(reportImageUrl(bad, ROOT, ORIGIN), null, String(bad));
  });

  it("뿌리가 보기 경로가 아니면 아무것도 받지 않는다", () => {
    assert.equal(reportImageUrl(`${ORIGIN}/api/v1/me`, "/api/", ORIGIN), null);
  });
});

describe("그림 누름 알림", () => {
  const good = (n) => ({ src: `${ORIGIN}/v/cap1/${n}.png`, alt: `그림 ${n}` });

  it("그 보고서 그림만 남기고 누른 자리를 맞춘다", () => {
    const got = reportImages(msg([good(1), { src: "data:image/png;base64,AA", alt: "x" }, good(2), { src: `${ORIGIN}/v/cap2/z.png` }, good(3)], 2), ROOT, ORIGIN);
    assert.deepEqual(got, { items: [good(1), good(2), good(3)], at: 1 });
  });

  it("누른 그림이 그 보고서 것이 아니거나 꼴이 틀리면 열지 않는다", () => {
    assert.equal(reportImages(msg([good(1), { src: `${ORIGIN}/api/v1/me` }], 1), ROOT, ORIGIN), null);
    for (const bad of [null, "x", {}, { type: "other", at: 0, images: [good(1)] }, msg([good(1)], 1), msg([good(1)], -1), msg([good(1)], 0.5), { type: "aaa:report-image", at: 0, images: "x" }, msg([null, good(1)], 0)])
      assert.equal(reportImages(bad, ROOT, ORIGIN), null, JSON.stringify(bad));
  });

  it("alt가 없거나 길면 비우거나 자른다", () => {
    const got = reportImages(msg([{ src: good(1).src }, { src: good(2).src, alt: "가".repeat(400) }]), ROOT, ORIGIN);
    assert.equal(got.items[0].alt, "");
    assert.equal(got.items[1].alt.length, 300);
  });

  it("그림이 많으면 누른 그림 둘레만 넘긴다", () => {
    const many = Array.from({ length: MAX_REPORT_IMAGES + 100 }, (_, i) => good(i));
    const got = reportImages(msg(many, MAX_REPORT_IMAGES + 90), ROOT, ORIGIN);
    assert.equal(got.items.length, MAX_REPORT_IMAGES);
    assert.equal(got.items[got.at].src, good(MAX_REPORT_IMAGES + 90).src);
    const first = reportImages(msg(many, 0), ROOT, ORIGIN);
    assert.equal(first.at, 0);
    assert.equal(first.items.length, MAX_REPORT_IMAGES);
  });
});

describe("보고 글 그림", () => {
  it("`library/…`만 그 보고서 보기 경로로", () => {
    assert.equal(reportMarkdownImage(ROOT, "library/shot.png"), "/v/cap1/shot.png");
    assert.equal(reportMarkdownImage(ROOT, "./library/시안/a b.png"), "/v/cap1/%EC%8B%9C%EC%95%88/a%20b.png");
    for (const bad of ["shot.png", "library/../brief.md", "library/.env", "../library/a.png", "https://example.ts.net/a.png", "report.md", "library/"])
      assert.equal(reportMarkdownImage(ROOT, bad), null, bad);
  });
});
