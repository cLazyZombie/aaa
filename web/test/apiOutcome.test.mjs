import assert from "node:assert/strict";
import { test } from "node:test";
import { ApiError, readApiResponse } from "../src/apiOutcome.ts";

test("서버의 원인 요약을 상태 코드와 무관하게 보존한다", async () => {
  const body = { code: "unavailable", message: "실행 실패: PermissionDenied (os error 5)" };
  await assert.rejects(readApiResponse(new Response(JSON.stringify({ err: body }), { status: 503 })), (error) => {
    assert.ok(error instanceof ApiError);
    assert.deepEqual(error.body, body);
    return true;
  });
  assert.deepEqual(await readApiResponse(new Response('{"ok":{"count":2}}')), { count: 2 });
});

test("비 JSON·손상 envelope는 단계와 HTTP 상태만 보이고 응답 원문은 숨긴다", async () => {
  const privateText = "private-response-marker";
  for (const text of [`<html>${privateText}</html>`, "null", '{"err":null}', '{"ok":1,"err":{}}']) {
    await assert.rejects(readApiResponse(new Response(text, { status: 502 })), (error) => {
      assert.ok(error instanceof ApiError);
      assert.match(error.message, /HTTP 502.*(JSON|host 응답)/);
      assert.ok(!error.message.includes(privateText));
      return true;
    });
  }
  await assert.rejects(readApiResponse(new Response('{"ok":null}', { status: 503 })), /HTTP 503/);
});

test("읽기 도중 끊긴 본문은 JSON 문법 오류와 구분한다", async () => {
  const response = new Response(new ReadableStream({ start(controller) { controller.error(new Error("private-marker")); } }));
  await assert.rejects(readApiResponse(response), (error) => {
    assert.match(error.message, /HTTP 200: 응답 본문 읽기 실패/);
    assert.ok(!error.message.includes("private-marker"));
    return true;
  });
});
