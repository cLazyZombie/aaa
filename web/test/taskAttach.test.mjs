// task 첨부 줄 규칙(src/tasksModel.ts, plan §8.319): 메모의 한 줄이 첨부 하나다(그림·올린 문서·보고서 문서·바깥 주소).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { addAttachment, attachmentOfLine, attachmentsOf, fileKind, linkFromText, memoText, pickAttachable, removeAttachment, reportDoc, reportUrlOf, withMemoText, withoutAttachments } from "../src/tasksModel.ts";

const IMG = "uploads/0123456789abcdef.png";
const DOC = "uploads/fedcba9876543210.md";

describe("task 첨부 줄", () => {
  it("줄 하나가 첨부다: 그림·올린 문서·보고서 문서·주소. 줄 가운데 참조·다른 꼴은 메모 글이다", () => {
    assert.deepEqual(attachmentOfLine(`![그림](${IMG})`), { kind: "image", target: IMG, label: "그림" });
    assert.deepEqual(attachmentOfLine(`  [계획](${DOC})  `), { kind: "upload", target: DOC, label: "계획" });
    assert.equal(attachmentOfLine("[계획서](library/t-0482/plan.md)")?.kind, "report");
    assert.equal(attachmentOfLine("[보고서](library/automation-design/index.html)")?.kind, "report");
    assert.equal(attachmentOfLine("[spec](https://example.com/a?b=1)")?.kind, "url");
    for (const not of [
      `보라 ![그림](${IMG})`,
      "[숨김](library/t-0001/.git/x.md)",
      "[위로](library/t-0001/../x.md)",
      "[그림 파일](library/t-0001/shot.png)",
      "[로컬](file:///etc/passwd)",
      "[대문자](uploads/0123456789ABCDEF.png)",
      "[](https://example.com)",
    ])
      assert.equal(attachmentOfLine(not), null, not);
  });

  it("같은 대상은 한 번, 더하기는 끝에, 떼기는 그 줄만(다른 메모·첨부는 그대로)", () => {
    const notes = `버튼이 잘린다\n![그림](${IMG})\n[계획](${DOC})\n![다시](${IMG})`;
    assert.deepEqual(attachmentsOf(notes).map((a) => a.target), [IMG, DOC]);
    assert.equal(addAttachment(notes, { kind: "image", target: IMG, label: "" }), notes);
    assert.equal(addAttachment("", { kind: "url", target: "https://x.test/a", label: "x [1]\n" }), "[x 1](https://x.test/a)");
    assert.equal(removeAttachment(notes, IMG), `버튼이 잘린다\n[계획](${DOC})`);
    assert.equal(withoutAttachments(notes), "버튼이 잘린다");
  });

  it("입력칸 글: 첨부 줄을 빼고 보이며, 고친 글 뒤에 첨부 줄을 둔다(치는 중인 빈 줄을 지킨다)", () => {
    const notes = `하나\n![그림](${IMG})\n둘`;
    assert.equal(memoText(notes), "하나\n둘");
    assert.equal(withMemoText(notes, "하나\n둘\n"), `하나\n둘\n\n![그림](${IMG})`);
    assert.equal(memoText(withMemoText(notes, "하나\n둘\n")), "하나\n둘\n");
    assert.equal(withMemoText(notes, ""), `![그림](${IMG})`);
    assert.equal(withMemoText("글만", "바꾼 글"), "바꾼 글");
  });

  it("붙인 글·파일: 주소와 보고서 경로만 링크, 그림·md·html만 받고 16MiB까지", () => {
    assert.deepEqual(linkFromText(" https://example.com/spec/ "), { kind: "url", target: "https://example.com/spec/", label: "example.com/spec/" });
    assert.deepEqual(linkFromText("library/t-0482/plan.md"), { kind: "report", target: "library/t-0482/plan.md", label: "plan.md" });
    assert.equal(linkFromText("그냥 글"), null);
    assert.deepEqual(reportDoc("library/t-0482/docs/plan.md"), { name: "t-0482", path: "docs/plan.md" });
    assert.equal(fileKind({ name: "plan.md", type: "" }), "md");
    assert.equal(fileKind({ name: "IMG.HEIC", type: "image/heic" }), null);
    assert.equal(fileKind({ name: "x", type: "image/webp" }), "image");
    const r = pickAttachable([
      { name: "a.png", type: "image/png", size: 10 },
      { name: "b.txt", type: "text/plain", size: 10 },
      { name: "c.html", type: "", size: 17 * 1024 * 1024 },
    ]);
    assert.deepEqual(r.ok, [0]);
    assert.equal(r.skipped.length, 2);
  });

  it("보고서 주소: 보기 경로는 어느 host 이름이든 cap으로, 앱 보고서 주소는 이 앱 것만", () => {
    const cap = "0123456789abcdef".repeat(3);
    const origin = "https://mac-a.example.ts.net";
    assert.deepEqual(reportUrlOf(`https://other.example.ts.net/v/${cap}/docs/%EA%B3%84%ED%9A%8D.md?download=1`, origin), { kind: "view", cap, path: "docs/계획.md" });
    assert.deepEqual(reportUrlOf(`${origin}/v/${cap}/`, origin), { kind: "view", cap, path: "" });
    assert.deepEqual(reportUrlOf(`${origin}/#report=projects%2Fweek`, origin), { kind: "report", project: "projects", name: "week" });
    assert.equal(reportUrlOf(`https://other.example.ts.net/#report=projects%2Fweek`, origin), null, "다른 host의 앱 주소");
    assert.equal(reportUrlOf(`${origin}/v/short/a.md`, origin), null, "cap 꼴이 아니다");
    assert.equal(reportUrlOf(`${origin}/#thread=t-0001`, origin), null);
    assert.equal(reportUrlOf(`${origin}/v/${cap}/%E0%A4%A.md`, origin), null, "풀 수 없는 경로");
  });
});
