import type { LibraryReport } from "./gen/proto";

type ReportRepo = Pick<LibraryReport, "repo">;
/** 탭을 바꿔도 선택지는 유지한다. 출처 없는 사용자 보고·옛 host 응답은 정하지 않음이다. */
export function reportRepoChoices(reports: readonly ReportRepo[]) {
  const counts = new Map<string, number>();
  for (const r of reports) counts.set(r.repo ?? "", (counts.get(r.repo ?? "") ?? 0) + 1);
  return [...counts.keys()].sort((a, b) => a === "" ? 1 : b === "" ? -1 : a.localeCompare(b))
    .map((key) => ({ key, label: key || "정하지 않음", n: counts.get(key)! }));
}
export function filterReports<T extends ReportRepo>(reports: readonly T[], repo: string | null): readonly T[] {
  return repo === null ? reports : reports.filter((r) => (r.repo ?? "") === repo);
}
