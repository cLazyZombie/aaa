// 붙여넣기 꼬리표 걷기(src/pastedContent.ts): Claude Code가 기록에 남기는 `<pasted_content id>` 감싸기만 걷고 글은 그대로 둔다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { unwrapPasted } from "../src/pastedContent.ts";

describe("붙여넣기 꼬리표", () => {
  it("글 전체를 감싼 여는·닫는 줄(같은 id)만 걷고 안의 글은 바꾸지 않는다", () => {
    assert.equal(unwrapPasted('<pasted_content id="7f06">\n첫 줄\n  둘째 줄 \n</pasted_content id="7f06">'), "첫 줄\n  둘째 줄 ");
    assert.equal(unwrapPasted('<pasted_content id="a1">한 줄</pasted_content id="a1">\n'), "한 줄");
    assert.equal(unwrapPasted('<pasted_content id="b2">\r\nwin\r\n</pasted_content id="b2">'), "win");
  });
  it("감싸지 않았거나 id가 다르거나 한쪽만 있으면 그대로다(사람이 쓴 같은 모양 글)", () => {
    const keep = [
      "그냥 글",
      '<pasted_content id="a">one</pasted_content id="a">\n<pasted_content id="a">two</pasted_content id="a">',
      '앞말 <pasted_content id="7f06">\n안\n</pasted_content id="7f06">',
      '<pasted_content id="7f06">\n안\n</pasted_content id="7f06">\n뒷말',
      '<pasted_content id="7f06">\n안\n</pasted_content id="9999">',
      '<pasted_content id="7f06">\n안',
      "<pasted_content>\n안\n</pasted_content>",
    ];
    for (const text of keep) assert.equal(unwrapPasted(text), text);
  });
});
