// 기존 화면/내용 keeper의 비교 경계. 식별 주석 자체의 보존·이주·ULID 대응은 68-task-ids가 실제 bytes로 검사한다.
export const taskContent = (text) => text.replace(/ <!-- aaa-task:[A-Z0-9]{26} -->/g, "");

// 파일 직접 편집 fixture도 기존 ID를 버리지 않는다. 바꾸는 첫 task 줄의 주석을 새 줄에 그대로 옮긴다.
export function editTaskContent(text, before, after) {
  const nl = before.indexOf("\n");
  if (nl < 0 || !before.trimStart().startsWith("- [")) return text.replace(before, after);
  const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`${escape(before.slice(0, nl))}( <!-- aaa-task:[A-Z0-9]{26} -->)?${escape(before.slice(nl))}`);
  return text.replace(re, (_, marker = "") => after.replace("\n", `${marker}\n`));
}
