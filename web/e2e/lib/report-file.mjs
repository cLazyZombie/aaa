import fs from "node:fs";

export function publishReport(file, body) {
  // 존재하는 보고서도 원래 fixture의 printf가 아직 쓰는 중일 수 있다.
  // 새 파일을 완성한 뒤 교체하면 열린 옛 FD의 나머지 쓰기가 새 보고서에 섞이지 않는다.
  const writing = `${file}.e2e.tmp`;
  fs.writeFileSync(writing, body);
  fs.renameSync(writing, file);
}
