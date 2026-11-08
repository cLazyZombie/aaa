// 답 카드 직접 답의 줄바꿈·그림 줄(src/answerImages.ts, t-1635): 입력칸 글을 고쳐도 끝의 그림 줄과 치는 중인 줄바꿈이 그대로 남아야
// 보낸 답에서 그림이나 줄이 빠지지 않는다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { addAnswerImage, answerBody, answerImages, pickAnswerImages, projectOfBase, removeAnswerImage, withAnswerBody } from "../src/answerImages.ts";

const A = "uploads/0123456789abcdef.png";
const B = "uploads/fedcba9876543210.jpg";

describe("답 그림 줄", () => {
  it("입력칸은 그림 줄을 뺀 글이고, 고친 글 뒤에 그림 줄이 남는다(끝 줄바꿈·빈 줄을 지킨다)", () => {
    let text = addAnswerImage("첫 줄", A);
    text = addAnswerImage(text, B);
    text = addAnswerImage(text, A);
    assert.equal(text, `첫 줄\n![그림](${A})\n![그림](${B})`);
    assert.deepEqual(answerImages(text), [A, B]);
    assert.equal(answerBody(text), "첫 줄");
    for (const typed of ["첫 줄\n", "첫 줄\n\n둘째", ""]) {
      const next = withAnswerBody(text, typed);
      assert.equal(answerBody(next), typed, JSON.stringify(typed));
      assert.deepEqual(answerImages(next), [A, B]);
    }
    assert.equal(removeAnswerImage(text, A), `첫 줄\n![그림](${B})`);
    assert.equal(withAnswerBody("그림 없음", "새 글\n"), "새 글\n");
  });

  it("줄 가운데 참조·uploads 밖 경로는 글이다", () => {
    for (const line of [`보라 ![그림](${A})`, "![그림](library/a.png)", "![그림](uploads/../x.png)"]) {
      assert.deepEqual(answerImages(line), [], line);
      assert.equal(answerBody(line), line);
    }
  });

  it("파일 주소 앞부분의 프로젝트, 받는 그림 형식·크기", () => {
    assert.equal(projectOfBase("/api/v1/projects/demo/asks/01HX/"), "demo");
    assert.equal(projectOfBase("/api/v1/projects/my_proj-2/requests/i/h/"), "my_proj-2");
    assert.equal(projectOfBase(null), null);
    assert.equal(projectOfBase("/elsewhere/demo/"), null);
    const { ok, skipped } = pickAnswerImages([
      { name: "a.png", type: "image/png", size: 10 },
      { name: "plan.md", type: "text/markdown", size: 10 },
      { name: "big.png", type: "image/png", size: 16 * 1024 * 1024 + 1 },
      { name: "", type: "image/webp", size: 0 },
    ]);
    assert.deepEqual(ok.map((f) => f.name), ["a.png"]);
    assert.equal(skipped.length, 3);
  });
});
