import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { publishReport } from "../e2e/lib/report-file.mjs";

test("원래 writer와 교체 순서를 지켜 긴 보고서에 늦은 출력이 섞이지 않는다", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-report-race-"));
  const file = path.join(root, "report.md");
  let writer;
  const prefix = "[결정 필요] 질문 2개: 로그인 화면\n\n## Questions\n### 1. 로그인 단추 자리\n";
  const tail = "### 2. 비밀번호 눈 아이콘\n- A 넣는다 (추천)\n- B 넣지 않는다\n\n## Next\n- 답을 기다린다\n";
  const body = `[결정 필요] 본문 스크롤\n\n## Questions\n### 1. 긴 질문\n${"긴 본문과 표\n".repeat(100)}비교 16\n- A 진행 (추천)\n- B 보류\n`;
  try {
    writer = fs.openSync(file, "w");
    // 원래 fixture의 printf가 report.md를 열고 일부만 쓴 시점이다. 존재 대기는 이미 끝난다.
    fs.writeSync(writer, prefix);
    assert.equal(fs.existsSync(file), true);
    if (process.platform === "win32") {
      // E2E87도 asked-cards(printf 종료·close 뒤)를 기다린 다음 교체한다.
      // 열린 파일 교체를 재시도하거나 Windows 시험을 건너뛰지 않는다.
      fs.writeSync(writer, tail);
      fs.closeSync(writer);
      writer = undefined;
      assert.equal(fs.readFileSync(file, "utf8"), prefix + tail);
      publishReport(file, body);
    } else {
      // POSIX에서는 교체 뒤 옛 FD의 쓰기도 새 보고서를 훼손하지 않는지 계속 검사한다.
      publishReport(file, body);
      fs.writeSync(writer, tail);
      fs.closeSync(writer);
      writer = undefined;
    }
    assert.equal(fs.readFileSync(file, "utf8"), body, "늦은 fixture 출력이 긴 질문에 끼어들었다");
  } finally {
    if (writer !== undefined) fs.closeSync(writer);
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("완성된 임시 보고서만 한 번의 rename으로 공개한다", (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "aaa-report-publish-"));
  const file = path.join(root, "report.md");
  const previous = "[완료] 원래 보고서\n";
  const body = `[결정 필요] 긴 보고서\n${"긴 본문과 표\n".repeat(100)}끝\n`;
  const rename = fs.renameSync;
  const replacement = t.mock.method(fs, "renameSync", (from, to) => {
    assert.equal(path.dirname(from), root, "같은 파일시스템에서 교체해야 한다");
    assert.equal(to, file);
    assert.equal(fs.readFileSync(file, "utf8"), previous, "교체 전에는 원래 본문을 보존해야 한다");
    assert.equal(fs.readFileSync(from, "utf8"), body, "임시 보고서가 완성된 뒤 교체해야 한다");
    return rename(from, to);
  });
  try {
    fs.writeFileSync(file, previous);
    publishReport(file, body);
    assert.equal(replacement.mock.callCount(), 1);
    assert.equal(fs.readFileSync(file, "utf8"), body);
    assert.deepEqual(fs.readdirSync(root), ["report.md"]);
  } finally {
    replacement.mock.restore();
    fs.rmSync(root, { recursive: true, force: true });
  }
});
