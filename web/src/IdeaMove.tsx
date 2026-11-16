// 옛 아이디어 「기획으로 옮기기」(t-1894 4단계, 확정 계획 §6, 시안 library/mock7 q-3980): 다듬는 중인 아이디어를 그 저장소의 기획
// 대화에 넘긴다. host가 노트의 그림을 그 기획 branch의 위치 폴더에 commit하고 노트 본문과 옮기는 법을 대화에 보낸다(idea_move).
// 그 대화가 main에 반영되면 아이디어는 「옮김 → 경로」가 된다. 옮긴 아이디어는 읽기 전용이고 기획 쪽이 정본이다.
import { ArrowRightLeft, Lock } from "lucide-react";
import { useMemo, useState } from "react";
import { ApiError, call } from "./api";
import { SLUG } from "./designModel";
import type { DesignRepoView, IdeaNote, IdeaSummary } from "./gen/proto";

/** 저장소 미리 고르기: 제목에 저장소 이름이 낱말로 있으면 그것, 없으면 첫 저장소 */
function guessRepo(idea: IdeaSummary, repos: DesignRepoView[]): string {
  const words = new Set(idea.title.toLowerCase().split(/[^a-z0-9_-]+/).filter(Boolean));
  return (repos.find((r) => words.has(r.repo.toLowerCase())) ?? repos[0])?.repo ?? "";
}

export function IdeaMoveDialog({ idea, repos, onClose, onDone }: {
  idea: IdeaSummary;
  /** 기획 트리(아직 읽는 중이면 null) */
  repos: DesignRepoView[] | null;
  onClose: () => void;
  /** 보냈다: 옮기는 중이 된 노트 */
  onDone: (note: IdeaNote) => void;
}) {
  const usable = useMemo(() => (repos ?? []).filter((r) => r.folder), [repos]);
  // 고르기 전에는 트리가 온 뒤의 추측을 따른다(트리를 읽는 중에 창을 열 수 있다)
  const [picked, setRepoName] = useState<string | null>(null);
  const repoName = picked ?? guessRepo(idea, usable);
  const repo = usable.find((r) => r.repo === repoName) ?? null;
  const [where, setWhere] = useState("");
  // 쉬는 대화도 고를 수 있다(host가 다시 연다)
  const chats = repo?.branches ?? [];
  // 한글 제목은 branch 이름을 지을 수 없다: 아이디어 번호로 짓는다
  const [slug, setSlug] = useState(`idea-${idea.short_id}`);
  const [chat, setChat] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const taken = chat === "" && chats.some((b) => b.slug === slug);
  const ok = !!repo && !busy && (chat !== "" || (SLUG.test(slug) && !taken));
  const submit = async () => {
    if (!ok || !repo) return;
    setBusy(true);
    setProblem(null);
    try {
      const r = await call({
        op: "idea_move", idea: idea.short_id, rev: idea.rev, repo: repo.repo, slug: chat || slug,
        ...(chat ? {} : { title: `옮김: ${idea.title}` }), place: where.trim(),
      });
      if (r.kind === "idea") onDone(r.note);
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}>
      <div className="modal design-dialog design-hand" role="dialog" aria-modal="true" aria-label="기획으로 옮기기" data-testid="idea-move-dialog">
        <header className="modal-header">
          <h2 className="modal-title"><ArrowRightLeft className="i" aria-hidden="true" /><span className="design-hand-head">기획으로 옮기기</span>
            <code>{idea.short_id} 「{idea.title}」 판 {idea.rev}</code></h2>
        </header>
        <div className="modal-body design-new">
          {repos === null ? <p className="muted" role="status" data-testid="idea-move-loading">기획 트리를 읽는 중…</p>
            : usable.length === 0 && <p className="confirm-error" role="alert">기획 폴더를 정한 저장소가 없다(aaa.toml 저장소의 design)</p>}
          <div className="design-new-row">
            <label>저장소
              <select className="input" value={repoName} data-testid="idea-move-repo" onChange={(e) => { setRepoName(e.target.value); setChat(""); }}>
                {usable.map((r) => <option key={r.repo} value={r.repo}>{r.repo} ({r.folder}/)</option>)}
              </select>
            </label>
            <label>위치
              <span className="design-slug"><span>{repo?.folder ?? ""}/</span><input className="input" value={where} placeholder="비우면 기획 폴더" spellCheck={false}
                data-testid="idea-move-where" onChange={(e) => setWhere(e.target.value)} /></span>
            </label>
          </div>
          <label>대화
            <select className="input" value={chat} data-testid="idea-move-chat" onChange={(e) => setChat(e.target.value)}>
              <option value="">새 대화</option>
              {chats.map((b) => <option key={b.slug} value={b.slug}>{b.title} · design/{b.slug}{b.state === "rest" ? " (쉼)" : ""}</option>)}
            </select>
          </label>
          {chat === "" && (
            <label>branch
              <span className="design-slug"><span>design/</span><input className="input" value={slug} spellCheck={false} data-testid="idea-move-slug" onChange={(e) => setSlug(e.target.value)} /></span>
            </label>
          )}
          {chat === "" && slug !== "" && !SLUG.test(slug) && <p className="confirm-error" role="alert">영문 소문자·숫자·-</p>}
          {taken && <p className="confirm-error" role="alert">이미 있는 대화다: 「대화」에서 고른다</p>}
          <ul className="idea-move-steps" data-testid="idea-move-steps">
            <li>노트의 그림·영상은 host가 그 기획 branch의 위치 폴더에 내용 hash 이름으로 commit한다.</li>
            <li>대화는 판 {idea.rev} 노트 본문을 받아 main 코드와 견주어 트리에 녹인다: ✓는 표지 없는 본문, △는 본문과 <code>[!계획]</code>, 넘긴 할 일은 <code>[!계획] d-NNNN</code>, 미정은 <code>[!미정]</code>, 「바뀐 것」은 그대로.</li>
            <li>보내면 이 아이디어는 「옮기는 중」이 되고, 그 대화를 main에 반영하면 「옮김 → 경로」가 된다. 대화를 버리면 다시 옮길 수 있다.</li>
          </ul>
          {problem && <p className="confirm-error" role="alert" data-testid="idea-move-problem">{problem}</p>}
        </div>
        <footer className="modal-footer">
          <span className="design-hand-note"><Lock className="i" aria-hidden="true" /> 옮긴 뒤에는 기획 문서가 정본이다</span>
          <span className="spacer" />
          <button type="button" className="btn" disabled={busy} onClick={onClose}>닫기</button>
          <button type="button" className="btn primary" disabled={!ok} data-testid="idea-move-submit" onClick={() => void submit()}>{busy ? "옮기는 중…" : "옮기기"}</button>
        </footer>
      </div>
    </div>
  );
}
