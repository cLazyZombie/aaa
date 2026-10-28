// 전체 맥락의 양쪽 원문·EOF 보존, 구판/잘림 거부와 읽는 사이 바뀐 hunk 혼합 방지.
import assert from "node:assert/strict";
import { test } from "node:test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { parsePatchFiles } from "@pierre/diffs";
import { diffContextFiles } from "../src/diffContext.ts";

function fixture(run) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-diff-context-"));
  try {
    const old = Array.from({ length: 90 }, (_, i) => `원문 ${i}\r\n`).join("") + "끝";
    const next = old.replace("원문 40\r\n", "고친 줄\r\n").replace("원문 60\r\n", "다른 hunk\r\n");
    fs.writeFileSync(path.join(dir, "old.txt"), old);
    fs.writeFileSync(path.join(dir, "new.txt"), next);
    const patch = (lines) => {
      try {
        return execFileSync("git", ["-c", "core.autocrlf=false", "diff", "--no-index", "--no-ext-diff", "--no-textconv", `-U${lines}`, "old.txt", "new.txt"], { cwd: dir, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
      } catch (e) {
        if (e.status === 1) return e.stdout;
        throw e;
      }
    };
    const partial = parsePatchFiles(patch(3), undefined, true)[0].files[0];
    const view = { full_context: true, truncated: false, base_sha: "base", patch: patch(2147483647) };
    run({ partial, view, old, next, dir, patch });
  } finally {
    fs.rmSync(dir, { recursive: true });
  }
}

test("맥락 원문은 CRLF·마지막 개행 없음과 처음·마지막 줄을 보존한다", () => fixture(({ partial, view, old, next }) => {
  const loaded = diffContextFiles(partial, "base", view);
  assert.equal(loaded.oldFile.contents, old);
  assert.equal(loaded.newFile.contents, next);
  assert.equal(partial.isPartial, true); // 실제 metadata 갱신은 라이브러리의 책임이다.
}));

test("구판·잘림·기준 변경은 불완전한 원문을 hydration에 넘기지 않는다", () => fixture(({ partial, view }) => {
  assert.throws(() => diffContextFiles(partial, "base", { ...view, full_context: false }), /agent.*지원/);
  assert.throws(() => diffContextFiles(partial, "base", { ...view, truncated: true }), /너무 커서/);
  assert.throws(() => diffContextFiles(partial, "other", view), /파일이 바뀌었다/);
}));

test("새 변경이 기존 hunk 밖에 생겨도 옛 비교와 섞지 않는다", () => fixture(({ partial, view, next, dir, patch }) => {
  fs.writeFileSync(path.join(dir, "new.txt"), next.replace("원문 80\r\n", "추가 변경\r\n"));
  assert.throws(() => diffContextFiles(partial, "base", { ...view, patch: patch(2147483647) }), /파일이 바뀌었다/);
}));
