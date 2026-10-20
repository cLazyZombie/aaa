// 크게 보기 손짓(src/lightboxGesture.ts, plan §8.353): 두 손가락 확대는 두 점 가운데 아래 자리를 지키고, 배율은 1~6에 묶이며,
// 키운 그림은 무대 밖으로 빠지지 않는다. 맞춘 크기에서 옆으로 쓸면 넘기고 아래로 쓸면 닫는다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { FIT, MAX_SCALE, TAP_SCALE, bound, doubleTap, pinch, swipeOf, wheel, zoomAt } from "../src/lightboxGesture.ts";

const IMAGE = { width: 300, height: 200 };
const STAGE = { width: 400, height: 800 };
const close = (a, b) => assert.ok(Math.abs(a - b) < 1e-9, `${a} ≠ ${b}`);

describe("두 손가락", () => {
  it("벌린 만큼 키우고 가운데 아래 그림 자리는 그대로다", () => {
    const v = pinch(FIT, { x: 40, y: 10 }, { x: 80, y: 10 }, { x: 20, y: 10 }, { x: 100, y: 10 });
    close(v.scale, 2);
    // 가운데(60,10) 아래 그림 자리 (60,10)/1이 키운 뒤에도 (60,10)에 있다
    close(v.x + 60 * v.scale, 60);
    close(v.y + 10 * v.scale, 10);
  });

  it("함께 옮기면 따라간다", () => {
    const v = pinch(FIT, { x: -10, y: 0 }, { x: 10, y: 0 }, { x: 20, y: 30 }, { x: 40, y: 30 });
    assert.deepEqual(v, { scale: 1, x: 30, y: 30 });
  });

  it("배율은 1~6이다", () => {
    close(pinch(FIT, { x: 0, y: 0 }, { x: 10, y: 0 }, { x: 0, y: 0 }, { x: 1000, y: 0 }).scale, MAX_SCALE);
    close(pinch({ scale: 2, x: 0, y: 0 }, { x: 0, y: 0 }, { x: 100, y: 0 }, { x: 0, y: 0 }, { x: 10, y: 0 }).scale, 1);
  });

  it("두 손가락이 겹친 채 시작하면 바꾸지 않는다", () => {
    const start = { scale: 2, x: 5, y: 5 };
    assert.equal(pinch(start, { x: 0, y: 0 }, { x: 0, y: 0 }, { x: 0, y: 0 }, { x: 50, y: 0 }), start);
  });
});

describe("묶기", () => {
  it("맞춘 크기면 늘 제자리", () => {
    assert.deepEqual(bound({ scale: 1, x: 80, y: -40 }, IMAGE, STAGE), FIT);
    assert.deepEqual(bound({ scale: 0.5, x: 80, y: -40 }, IMAGE, STAGE), FIT);
  });

  it("키운 그림은 가장자리까지만 옮기고 무대보다 작은 쪽은 가운데", () => {
    // 3배: 900×600. 가로 남는 폭 (900-400)/2=250, 세로는 600<800이라 0
    assert.deepEqual(bound({ scale: 3, x: 1000, y: 300 }, IMAGE, STAGE), { scale: 3, x: 250, y: 0 });
    assert.deepEqual(bound({ scale: 3, x: -1000, y: -300 }, IMAGE, STAGE), { scale: 3, x: -250, y: 0 });
    assert.deepEqual(bound({ scale: 9, x: 0, y: 0 }, IMAGE, STAGE).scale, MAX_SCALE);
  });
});

describe("두 번 누름·바퀴", () => {
  it("맞춘 크기면 누른 자리를 키우고, 키운 것은 맞춘 크기로", () => {
    const v = doubleTap(FIT, { x: 50, y: -20 });
    close(v.scale, TAP_SCALE);
    close(v.x + 50 * v.scale, 50);
    assert.deepEqual(doubleTap(v, { x: 0, y: 0 }), FIT);
  });

  it("바퀴는 위로 키우고 아래로 줄인다", () => {
    assert.ok(wheel(FIT, { x: 0, y: 0 }, -100).scale > 1);
    assert.equal(wheel(FIT, { x: 0, y: 0 }, 100).scale, 1);
    const z = zoomAt({ scale: 2, x: 10, y: 0 }, { x: 10, y: 0 }, 4);
    assert.deepEqual(z, { scale: 4, x: 10, y: 0 });
  });
});

describe("쓸기", () => {
  it("옆으로 넉넉히 쓸면 넘기고, 아래로 쓸면 닫는다", () => {
    assert.equal(swipeOf(-120, 10, 400), "next");
    assert.equal(swipeOf(120, -10, 400), "prev");
    assert.equal(swipeOf(5, 120, 400), "close");
  });

  it("짧거나 비스듬하거나 위로 쓴 것은 아무것도 아니다", () => {
    assert.equal(swipeOf(-30, 0, 400), null);
    assert.equal(swipeOf(-100, 90, 400), null);
    assert.equal(swipeOf(0, -200, 400), null);
    assert.equal(swipeOf(0, 60, 400), null);
    // 넓은 화면은 너비의 12%를 넘어야 넘긴다
    assert.equal(swipeOf(-100, 0, 1280), null);
    assert.equal(swipeOf(-160, 0, 1280), "next");
  });
});
