// 옛 아이디어 「기획으로 옮기기」(t-1894 4단계, 확정 계획 §6): 다듬는 중인 아이디어를 그 저장소의 기획 대화에 넘긴다.
// 대화가 노트(`aaa idea show`)를 읽어 main 코드와 견주어 기획 트리에 녹인다. 노트의 그림은 host가 그 기획 branch에 commit한다.
// 옮긴 아이디어는 읽기 전용이고 기획 쪽이 정본이다.
import { ArrowRightLeft, Lock } from "lucide-react";
import { useMemo, useState } from "react";
import { SLUG } from "./designModel";
import type { DesignRepoView, IdeaSummary } from "./gen/proto";

/** 옛 아이디어의 상태: 다듬는 중(옮길 수 있다)·옮기는 중·옮김·완료 */
export type IdeaLegacy = { kind: "open" | "moving" | "moved" | "done"; label: string };

export function ideaLegacy(idea: Pick<IdeaSummary, "status" | "moved">): IdeaLegacy {
  const m = idea.moved;
  if (m?.done) return { kind: "moved", label: `옮김 → ${m.repo} ${m.path}` };
  if (m) return { kind: "moving", label: `옮기는 중 → ${m.repo} · design/${m.branch}` };
  if (idea.status === "done") return { kind: "done", label: "완료" };
  return { kind: "open", label: "다듬는 중" };
}

/** 저장소 미리 고르기: 제목에 저장소 이름이 낱말로 있으면 그것, 없으면 첫 저장소 */
function guessRepo(idea: IdeaSummary, repos: DesignRepoView[]): string {
  const words = new Set(idea.title.toLowerCase().split(/[^a-z0-9_-]+/).filter(Boolean));
  return (repos.find((r) => words.has(r.repo.toLowerCase())) ?? repos[0])?.repo ?? "";
}

export function IdeaMoveDialog({ idea, repos, onClose }: { idea: IdeaSummary; repos: DesignRepoView[]; onClose: () => void }) {
  const usable = useMemo(() => repos.filter((r) => r.folder), [repos]);
  const [repoName, setRepoName] = useState(() => guessRepo(idea, usable));
  const repo = usable.find((r) => r.repo === repoName) ?? null;
  const [where, setWhere] = useState("");
  const chats = (repo?.branches ?? []).filter((b) => b.state === "open");
  // 한글 제목은 branch 이름을 지을 수 없다: 아이디어 번호로 짓는다
  const [slug, setSlug] = useState(`idea-${idea.short_id}`);
  const [chat, setChat] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const ok = !!repo && (chat !== "" || SLUG.test(slug));
  return (
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="modal design-dialog design-hand" role="dialog" aria-modal="true" aria-label="기획으로 옮기기" data-testid="idea-move-dialog">
        <header className="modal-header">
          <h2 className="modal-title"><ArrowRightLeft className="i" aria-hidden="true" /><span className="design-hand-head">기획으로 옮기기</span>
            <code>{idea.short_id} 「{idea.title}」 판 {idea.rev}</code></h2>
        </header>
        <div className="modal-body design-new">
          <div className="design-new-row">
            <label>저장소
              <select className="input" value={repoName} data-testid="idea-move-repo" onChange={(e) => { setRepoName(e.target.value); setChat(""); }}>
                {usable.map((r) => <option key={r.repo} value={r.repo}>{r.repo} ({r.folder}/)</option>)}
              </select>
            </label>
            <label>위치
              <span className="design-slug"><span>{repo?.folder ?? ""}/</span><input className="input" value={where} placeholder="비우면 대화가 정한다" spellCheck={false}
                data-testid="idea-move-where" onChange={(e) => setWhere(e.target.value)} /></span>
            </label>
          </div>
          <label>대화
            <select className="input" value={chat} data-testid="idea-move-chat" onChange={(e) => setChat(e.target.value)}>
              <option value="">새 대화</option>
              {chats.map((b) => <option key={b.slug} value={b.slug}>{b.title} · design/{b.slug}</option>)}
            </select>
          </label>
          {chat === "" && (
            <label>branch
              <span className="design-slug"><span>design/</span><input className="input" value={slug} spellCheck={false} data-testid="idea-move-slug" onChange={(e) => setSlug(e.target.value)} /></span>
            </label>
          )}
          {chat === "" && slug !== "" && !SLUG.test(slug) && <p className="confirm-error" role="alert">영문 소문자·숫자·-</p>}
          <ul className="idea-move-steps" data-testid="idea-move-steps">
            <li>노트의 그림·영상은 host가 그 기획 branch의 위치 폴더에 내용 hash 이름으로 commit한다.</li>
            <li>대화는 판 {idea.rev} 노트를 <code>aaa idea show {idea.short_id}</code>로 읽고 main 코드와 견주어 트리에 녹인다: ✓는 표지 없는 본문, △는 본문과 <code>[!계획]</code>, 넘긴 할 일은 <code>[!계획] d-NNNN</code>, 미정은 <code>[!미정]</code>, 「바뀐 것」은 그대로.</li>
            <li>보내면 이 아이디어는 읽기 전용(옮기는 중)이 되고, 그 대화를 main에 반영하면 「옮김 → 경로」가 된다.</li>
          </ul>
          {problem && <p className="confirm-error" role="alert">{problem}</p>}
        </div>
        <footer className="modal-footer">
          <span className="design-hand-note"><Lock className="i" aria-hidden="true" /> 옮긴 뒤에는 기획 문서가 정본이다</span>
          <span className="spacer" />
          <button type="button" className="btn" onClick={onClose}>닫기</button>
          <button type="button" className="btn primary" disabled={!ok} data-testid="idea-move-submit"
            onClick={() => setProblem("시안: 아직 동작하지 않는다(검수 뒤 만든다)")}>옮기기</button>
        </footer>
      </div>
    </div>
  );
}
