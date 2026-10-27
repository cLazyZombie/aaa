// browser 재사용 뒤 늦게 만들어진 자식 유실·이중 종료·정리 오류 유실을 막는다.
// 실제 WebKit 자식/실패 종료는 E2E12, 이 owner는 생성과 정리의 순서를 사건으로 제어한다.
import assert from "node:assert/strict";
import { test } from "node:test";
import { cleanupScope } from "../e2e/lib/cleanup-scope.mjs";

test("실험실은 정리 시작 뒤 만들어진 자원도 기다리고 한 번만 종료한다", async () => {
  const scope = cleanupScope();
  const late = Promise.withResolvers();
  const resource = { name: "late-browser" };
  const disposed = [];
  const owned = scope.own(() => late.promise, value => { disposed.push(value); });
  const closing = scope.close();
  // 비동기 생성이 완료되기 전에 정리를 시작한 실제 수명 경계다.
  assert.deepEqual(disposed, []);
  late.resolve(resource);
  await Promise.all([closing, owned.close(), scope.close()]);
  assert.deepEqual(disposed, [resource]);
  assert.throws(() => scope.own(() => assert.fail("정리 중 acquire"), () => {}), /정리 중/);
});

test("한 생성·정리가 실패해도 다른 자원을 종료하고 원래 오류를 모두 보존한다", async () => {
  const scope = cleanupScope();
  const acquireError = new Error("생성 경계 오류");
  const releaseError = new Error("종료 경계 오류");
  let released = 0;
  const failed = scope.own(() => { throw acquireError; }, () => assert.fail("생성 실패 자원의 종료"));
  const acquired = scope.own(() => "browser", value => {
    assert.equal(value, "browser");
    released++;
    throw releaseError;
  });
  await assert.rejects(failed.value, error => error === acquireError);
  await assert.rejects(scope.close(), error => {
    assert.ok(error instanceof AggregateError);
    assert.deepEqual(new Set(error.errors), new Set([acquireError, releaseError]));
    return true;
  });
  assert.equal(released, 1);
  await assert.rejects(acquired.close(), error => error === releaseError);
  assert.equal(released, 1);
});
