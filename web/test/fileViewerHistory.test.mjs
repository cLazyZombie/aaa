import assert from "node:assert/strict";
import test from "node:test";
import { filePreviewEntry, filePreviewState } from "../src/herdr/useFileViewer.ts";

test("업로드 중인 그림은 파일과 이름을 기록하지 않고 닫을 칸만 표시한다", () => {
  const image = new File(["그림 바이트"], "draft.png", { type: "image/png" });
  const entry = filePreviewState({ route: "chat" }, { path: image.name, pane: "a", image });
  assert.deepEqual(structuredClone(entry), { route: "chat", "aaa:file-preview": true });
  assert.equal(filePreviewEntry(entry), true);
  assert.equal(filePreviewEntry({ route: "chat" }), null);
});

test("서버 파일은 기록을 거친 뒤에도 원래 출처에서 다시 연다", () => {
  for (const preview of [
    { path: "/uploads/sent.png", pane: "a" },
    { path: "page.md", pane: null, library: { project: "demo", thread: "t-1234" } },
    { path: "image.png", pane: null, worktree: { project: "demo", thread: "t-1234" } },
    { path: "report.html", pane: null, view: "/v/example/" },
  ]) {
    assert.deepEqual(filePreviewEntry(structuredClone(filePreviewState(null, preview))), preview);
  }
});
