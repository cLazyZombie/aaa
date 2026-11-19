import { Pencil } from "lucide-react";
import { useState } from "react";
import { call } from "./api";
import { mdBlocks } from "./designModel";
import type { DesignDoc, TaskView } from "./gen/proto";
import { noteLines } from "./tasksModel";

const quote = (text: string) => text.split("\n").map((line) => `> ${line}`).join("\n");

export function DesignEditRequest({ doc, machine, block, restore, onClose }: {
  doc: DesignDoc;
  machine: string | null;
  block?: string;
  restore?: string;
  onClose: () => void;
}) {
  const blocks = mdBlocks(doc.text);
  const headings = blocks.flatMap((text, index) => /^#{1,6}\s/.test(text) ? [{ text: text.split("\n")[0]!, index }] : []);
  const [scope, setScope] = useState(block ? "block" : "all");
  const [request, setRequest] = useState(restore ? `이 문서를 ${restore} 판으로 복원해 주세요.` : "");
  const [created, setCreated] = useState<TaskView | null>(null);
  const [reserved, setReserved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const heading = headings.find((h) => String(h.index) === scope);
  const end = heading && headings.find((h) => h.index > heading.index);
  const original = scope === "block" ? block : heading ? blocks.slice(heading.index, end?.index).join("\n\n") : "";
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      let task = created;
      if (!task) {
        const title = `${request.trim().split("\n")[0]!.slice(0, 90)} · 기획 편집`;
        const notes = noteLines([
          `기획 문서 ${doc.repo}/${doc.path} · 기준 판 ${doc.commit}`,
          doc.id && `기획서 ID ${doc.id}를 보존한다. 파일을 옮기면 ID 표지도 함께 옮긴다.`,
          `편집 범위: ${heading?.text ?? (block && scope === "block" ? "선택한 블록" : "문서 전체")}`,
          original && `선택한 원문\n${quote(original)}`, `편집 요청\n${quote(request.trim())}`,
          "작업 branch에서 커밋하고 aaa plan preview로 기준 판과 비교한 문서 전체와 그림·GIF를 library에 담아 보고한다. 사용자 승인 뒤 일반 착지로 진행한다.",
        ].filter(Boolean).join("\n\n"));
        const result = await call({ op: "task_edit", project: doc.project, base_sha256: null, ops: [{
          op: "add", list: "Backlog", title, repo: doc.repo, machine, profile: null, owner: null,
          person: false, notes, parent: null, before: null, thread: null,
        }] });
        if (result.kind !== "task_edited") throw new Error("할 일 생성 결과를 읽지 못했습니다.");
        if (result.conflict) throw new Error(result.conflict.reason);
        task = result.board.tasks.find((t) => t.title === title && t.repo === doc.repo) ?? null;
        if (!task?.short_id) throw new Error("만든 할 일의 번호를 찾지 못했습니다.");
        setCreated(task);
      }
      const id = task.short_id!;
      const preview = await call({ op: "thread_start_preview", project: doc.project, from_task: id, task_block: task.block_sha256 });
      if (preview.kind !== "thread_start_preview") throw new Error("작업 시작 정보를 읽지 못했습니다.");
      await call({ op: "task_queue", project: doc.project, action: {
        op: "reserve", task: id, block: preview.task_block, profile: preview.profile, profile_revision: preview.profile_revision,
      } });
      setReserved(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return <div className="modal-scrim">
    <div className="modal design-dialog" role="dialog" aria-modal="true" aria-label="편집 요청">
      <header className="modal-header"><h2 className="modal-title"><Pencil className="i" />편집 요청</h2></header>
      <div className="modal-body design-new">
        <span className="design-crumb">{doc.repo} / {doc.path} · {doc.commit.slice(0, 7)}</span>
        <label>편집할 부분<select className="input" disabled={busy || !!created || !!restore} value={scope} onChange={(e) => setScope(e.target.value)}>
          <option value="all">문서 전체</option>
          {block && <option value="block">선택한 블록</option>}
          {headings.map((h) => <option key={h.index} value={h.index}>{h.text.replace(/^#+\s+/, "")}</option>)}
        </select></label>
        {original && <blockquote className="design-edit-original">{original}</blockquote>}
        <label>요청 내용<textarea className="input" rows={4} value={request} autoFocus disabled={busy || !!created} onChange={(e) => setRequest(e.target.value)} /></label>
        {created && <span role="status"><a href={`#task=${created.short_id}`}>{created.short_id}</a> · {reserved ? "시작 예약됨" : "할 일 생성됨"}</span>}
        {error && <span role="alert" className="confirm-error">{error}</span>}
      </div>
      <footer className="modal-footer"><span className="spacer" />
        <button className="btn" disabled={busy} onClick={onClose}>{created ? "닫기" : "취소"}</button>
        {!reserved && <button className="btn primary" disabled={busy || !request.trim()} onClick={() => void submit()}>{created ? "시작 예약" : "작업 요청"}</button>}
      </footer>
    </div>
  </div>;
}
