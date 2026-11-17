// 기획 대화 사건 줄 자리(t-1950): 짝 말을 못 찾은 옛 사건이 맨 끝에 몰려 방금 일처럼 보였다(사용자 11-17 화면).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { markPlaces } from "../src/chatMarks.ts";

const user = (text) => ({ role: "user", text });
const bot = (text) => ({ role: "assistant", text });

describe("기획 대화 사건 자리", () => {
  it("짝 말이 있으면 그 말의 답 뒤, after가 없으면 맨 앞", () => {
    const msgs = [user("첫 말"), bot("답"), user("둘째 말"), bot("답")];
    assert.deepEqual(markPlaces([{ after: null }, { after: "첫 말" }, { after: "둘째" }], msgs, []), [-1, 0, 2]);
  });

  it("짝 말을 불러온 기록에서 못 찾은 사건은 바로 앞 사건 자리(시각 차례)다", () => {
    // 옮기기 말은 앞 쪽이라 기록에 없다. 그 뒤 사건(올림·저장·반영 요청)은 앞 사건(새 대화) 자리에 남는다
    const msgs = [user("계속 잇는중?"), bot("네"), user("(보는 문서: docs/design/game-loop.md)\n왜 main에 반영되지 않지?"), bot("답")];
    const marks = [{ after: null }, { after: "옛 아이디어 n-0009" }, { after: "옛 아이디어 n-0009" }, { after: "(보는 문서: docs/design/game-loop.md) 왜 main" }, { after: "없는 말" }];
    assert.deepEqual(markPlaces(marks, msgs, []), [-1, -1, -1, 2, 2]);
  });

  it("보내는 중이라 아직 기록에 없는 말의 사건만 맨 끝이고, 그 뒤 사건도 끝이다", () => {
    const msgs = [user("첫 말"), bot("답")];
    const marks = [{ after: "첫 말" }, { after: "방금 보낸 말" }, { after: "기록에 없는 옛 말" }];
    assert.deepEqual(markPlaces(marks, msgs, [{ text: "방금 보낸 말 이어서" }]), [0, Number.POSITIVE_INFINITY, Number.POSITIVE_INFINITY]);
    assert.deepEqual(markPlaces([{ after: "기록에 없는 옛 말" }], msgs, []), [-1], "앞 사건이 없으면 맨 앞");
  });
});
