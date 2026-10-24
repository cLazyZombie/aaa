import assert from "node:assert/strict";
import { test } from "node:test";
import { reportRepoChoices, filterReports } from "../src/reportRepos.ts";

test("보고서 저장소: 세 종류·옛 응답·전체·정하지 않음", () => {
  const rows = [{ repo: "rc", archived: true }, { repo: "aaa", to_user: true }, { repo: "aaa" }, {}, { repo: undefined }];
  assert.deepEqual(reportRepoChoices(rows), [{ key: "aaa", label: "aaa", n: 2 }, { key: "rc", label: "rc", n: 1 }, { key: "", label: "정하지 않음", n: 2 }]);
  assert.equal(filterReports(rows, null), rows);
  assert.deepEqual(filterReports(rows, "aaa"), rows.slice(1, 3));
  assert.deepEqual(filterReports(rows, ""), rows.slice(3));
  assert.deepEqual(filterReports(rows, "없음"), []);
  assert.deepEqual(reportRepoChoices([]), []);
});
