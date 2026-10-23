import fs from "node:fs";

export function publishReport(file, body) {
  // 호출자는 원래 writer의 완료·close를 먼저 확인한다(Windows는 열린 대상 교체가 실패한다).
  // 같은 폴더의 새 파일을 완성하고 닫은 뒤 교체해 부분 본문을 공개하지 않는다.
  const writing = `${file}.e2e.tmp`;
  fs.writeFileSync(writing, body);
  fs.renameSync(writing, file);
}
