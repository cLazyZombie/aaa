// herdr-web-ui 시험(bun:test)을 node:test로 옮길 때 본문을 참고와 같게 두려고 쓰는 `expect`·`each`(bun `it.each`).
// 참고 시험이 쓰는 비교만 둔다. toBe는 Object.is(assert.strictEqual), toEqual은 깊은 같음이다.
import assert from "node:assert/strict";
import { it } from "node:test";

export function expect(actual) {
  return {
    toBe: (expected) => assert.strictEqual(actual, expected),
    toEqual: (expected) => assert.deepStrictEqual(actual, expected),
    toBeNull: () => assert.strictEqual(actual, null),
    toBeUndefined: () => assert.strictEqual(actual, undefined),
    toHaveLength: (length) => assert.strictEqual(actual.length, length),
    toBeLessThan: (bound) => assert.ok(actual < bound, `${actual} < ${bound}`),
    toBeGreaterThan: (bound) => assert.ok(actual > bound, `${actual} > ${bound}`),
    not: {
      toBe: (expected) => assert.notStrictEqual(actual, expected),
      toBeNull: () => assert.notStrictEqual(actual, null),
    },
  };
}

/** bun `it.each(table)(name, fn)`: 줄마다 단계 하나, 이름의 `%s`·`%j`는 차례로 그 줄의 값 */
export function each(table) {
  return (name, fn) => {
    for (const row of table) {
      let at = 0;
      const title = name.replace(/%[sj]/g, (spec) => {
        const value = row[at++];
        return spec === "%j" ? JSON.stringify(value) : String(value);
      });
      it(title, () => fn(...row));
    }
  };
}
