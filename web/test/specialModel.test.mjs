// 특수 패널(src/specialModel.ts, plan §8.258): 줄 색·첫 낱말과 활동 막대 배지(막힘·오류·승인 필요 임무 + 확인창 걸린 평가자).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { evaluatorState, missionTone, missionWord, specialAttention } from "../src/specialModel.ts";

const mission = (over) => ({ project: "p", name: "m", title: "임무", thread: null, state: "대기", approved: true, error: null, ...over });
const evaluator = (over) => ({ project: "p", thread: { pane: "e1" }, running: true, evaluations: 3, undecided: 2, ...over });
const pane = (id, activity) => ({ id, state: "running", agent: activity ? { activity } : null });

describe("특수 패널", () => {
  it("임무 줄: 오류·막힘은 빨강, 승인 필요는 사람 차례, 일함·시작 중은 파랑, 나머지는 회색", () => {
    assert.equal(missionTone(mission({ error: "깨진 파일" })), "blocked");
    assert.equal(missionTone(mission({ state: "막힘" })), "blocked");
    assert.equal(missionTone(mission({ approved: false, state: "승인 대기" })), "need");
    assert.equal(missionTone(mission({ state: "일함" })), "working");
    assert.equal(missionTone(mission({ state: "시작 중" })), "working");
    for (const state of ["대기", "멈춤", "예산 대기", "사용량 대기", "끝남"]) assert.equal(missionTone(mission({ state })), "quiet", state);
    assert.equal(missionWord(mission({ error: "x", approved: false })), "오류");
    assert.equal(missionWord(mission({ approved: false, state: "승인 대기" })), "승인 필요");
    assert.equal(missionWord(mission({ state: "멈춤" })), "멈춤");
  });

  it("평가자 줄: 확인창이면 막힘, 일하면 평가 중, pane이 없거나 끝나면 세션 끝남", () => {
    assert.deepEqual(evaluatorState(evaluator(), [pane("e1", "question")]), { tone: "blocked", word: "막힘" });
    assert.deepEqual(evaluatorState(evaluator(), [pane("e1", "working")]), { tone: "working", word: "평가 중" });
    assert.deepEqual(evaluatorState(evaluator(), [pane("e1", "idle")]), { tone: "quiet", word: "대기" });
    assert.deepEqual(evaluatorState(evaluator(), []), { tone: "quiet", word: "세션 끝남" });
    assert.deepEqual(evaluatorState(evaluator({ running: false }), [pane("e1", "question")]), { tone: "quiet", word: "세션 끝남" });
  });

  it("배지: 막힘·오류·승인 필요 임무와 확인창 걸린 평가자만 센다(정할 개선안·일함·멈춤은 세지 않는다)", () => {
    const missions = [
      mission({ name: "a", state: "막힘" }),
      mission({ name: "b", error: "x" }),
      mission({ name: "c", approved: false }),
      mission({ name: "d", state: "일함" }),
      mission({ name: "e", state: "멈춤" }),
    ];
    assert.equal(specialAttention(missions, [evaluator()], [pane("e1", "working")]), 3);
    assert.equal(specialAttention(missions, [evaluator()], [pane("e1", "question")]), 4);
    assert.equal(specialAttention([], [evaluator({ undecided: 9 })], [pane("e1", "idle")]), 0);
  });
});
