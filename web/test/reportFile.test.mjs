import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { publishReport } from "../e2e/lib/report-file.mjs";

test("가짜 agent가 생성한 보고서에 쓰는 중에도 긴 보고서 공개 뒤 내용이 섞이지 않는다", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-report-race-"));
  const file = path.join(root, "report.md");
  const writer = fs.openSync(file, "w");
  const prefix = "[결정 필요] 질문 2개: 로그인 화면\n\n## Questions\n### 1. 로그인 단추 자리\n";
  const tail = "### 2. 비밀번호 눈 아이콘\n- A 넣는다 (추천)\n- B 넣지 않는다\n\n## Next\n- 답을 기다린다\n";
  const body = `[결정 필요] 본문 스크롤\n\n## Questions\n### 1. 긴 질문\n${"긴 본문과 표\n".repeat(100)}비교 16\n- A 진행 (추천)\n- B 보류\n`;
  try {
    // 원래 fixture의 printf가 report.md를 열고 일부만 쓴 시점이다. 존재 대기는 이미 끝난다.
    fs.writeSync(writer, prefix);
    assert.equal(fs.existsSync(file), true);
    publishReport(file, body);
    // 대체한 뒤 원래 FD가 남은 printf 출력을 쓴다. sleep·실행 속도에 의존하지 않는다.
    fs.writeSync(writer, tail);
  } finally {
    fs.closeSync(writer);
  }
  try {
    assert.equal(fs.readFileSync(file, "utf8"), body, "늦은 fixture 출력이 긴 질문에 끼어들었다");
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
