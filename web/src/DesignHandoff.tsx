// 구현 맡기기 창(t-1894 3-2, 확정 계획 §4): 기획 branch 판의 계획 블록 여럿을 할 일 하나로 넘긴다.
// 고른 블록 표지에 같은 d-ID를 적어 그 기획 branch에 commit해 바로 올리고, 구현 thread는 그 branch 끝에서 갈라진 자기 branch에서 일한다.
import { Hammer } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { ApiError, call } from "./api";
import type { Callout } from "./designModel";
import type { DesignBranchView, DesignRepoView, OwnerChoice } from "./gen/proto";
import { ideaModelChoices } from "./ideasModel";

/** 표지 블록 하나: 덩어리 번호와, 표지 블록끼리의 차례(host `design_marks`와 같은 차례) */
export type HandoffBlock = { c: Callout; i: number; ordinal: number };

/** 넘길 수 있는 블록: 아직 넘기지 않은 계획만(미정은 정한 뒤에 넘긴다) */
export const handable = (c: Callout) => c.kind === "plan" && !c.task;

/** 줄 이름의 첫 문장(끝 마침표는 뺀다) */
const firstSentence = (t: string) => (t.split(/(?<=[.。])\s/)[0] ?? t).replace(/[.。]$/, "");

function autoTitle(picked: HandoffBlock[]): string {
  if (picked.length === 0) return "";
  if (picked.length === 1) return firstSentence(picked[0]!.c.title);
  const sections = [...new Set(picked.map((p) => p.c.section))];
  const titles = picked.map((p) => firstSentence(p.c.title)).join(" · ");
  return (sections.length === 1 ? `${sections[0]} — ${titles}` : titles).slice(0, 120);
}

function autoMemo(repo: string, path: string, picked: HandoffBlock[]): string {
  const lines: string[] = [];
  for (const section of [...new Set(picked.map((p) => p.c.section))]) {
    lines.push(`[기획 ${repo} ${path} 「${section}」](design/${repo}/${path}#${section})`);
    for (const p of picked.filter((x) => x.c.section === section)) lines.push(`- ${p.c.body.replace(/\s*\n\s*/g, " ").slice(0, 300)}`);
  }
  return lines.join("\n");
}

export function HandoffDialog({ repo, branch, path, blob, blocks, initial, onClose, onDone }: {
  repo: DesignRepoView;
  branch: DesignBranchView;
  path: string;
  /** 고를 때 읽은 판 */
  blob: string;
  /** 문서의 표지 블록 전부(차례대로) */
  blocks: HandoffBlock[];
  /** 처음 고른 블록(덩어리 번호) */
  initial: number[];
  onClose: () => void;
  /** 맡긴 뒤(경고가 없으면 창을 닫는다) */
  onDone: () => void;
}) {
  const [picks, setPicks] = useState<Set<number>>(() => new Set(initial));
  const [title, setTitle] = useState<string | null>(null);
  const [memo, setMemo] = useState<string | null>(null);
  const [profile, setProfile] = useState("");
  const [reserve, setReserve] = useState(true);
  const [owners, setOwners] = useState<OwnerChoice[]>([]);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [warning, setWarning] = useState<string | null>(null);
  useEffect(() => {
    call({ op: "assignable", check: null }).then((r) => { if (r.kind === "assignable") setOwners(r.owners); }, () => {});
  }, []);
  const names = ideaModelChoices(owners, repo.machine, "").filter(Boolean);
  const picked = useMemo(() => blocks.filter((b) => picks.has(b.i)), [blocks, picks]);
  const shownTitle = title ?? autoTitle(picked);
  const shownMemo = memo ?? autoMemo(repo.repo, path, picked);
  const flip = (i: number) => setPicks((old) => { const next = new Set(old); if (!next.delete(i)) next.add(i); return next; });
  const ok = picked.length > 0 && shownTitle.trim() !== "" && !busy && warning === null;
  const submit = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const r = await call({
        op: "design_handoff", repo: repo.repo, branch: branch.slug, path, blob, blocks: picked.map((p) => p.ordinal),
        title: shownTitle.trim(), profile: profile || null, memo: shownMemo, reserve,
      });
      if (r.kind !== "design_handoff") throw new Error("예상하지 못한 답");
      onDone();
      // 할 일은 생겼지만 표지 commit·올리기·예약 중 못한 것이 있으면 창에 남긴다
      if (r.handoff.warning) setWarning(`${r.handoff.task}를 만들었지만: ${r.handoff.warning}`);
      else onClose();
    } catch (e) {
      setProblem(e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-scrim" onMouseDown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}>
      <div className="modal design-dialog design-hand" role="dialog" aria-modal="true" aria-label="구현 맡기기" data-testid="design-hand-dialog">
        <header className="modal-header">
          <h2 className="modal-title"><Hammer className="i" aria-hidden="true" /><span className="design-hand-head">구현 맡기기</span> <code>{repo.repo} / {path}</code></h2>
        </header>
        <div className="modal-body design-new">
          <div className="design-hand-label">넘길 블록 <span>여럿을 고르면 할 일 하나</span></div>
          <ul className="design-hand-blocks" data-testid="design-hand-blocks">
            {/* 이미 넘긴 블록은 빼고, 미정은 왜 못 넘기는지 보인다 */}
            {blocks.filter(({ c }) => !c.task).map(({ c, i }) => {
              const can = handable(c);
              return (
                <li key={i} className={`design-hand-block${can ? "" : " is-off"}`}>
                  <label>
                    <input type="checkbox" checked={picks.has(i)} disabled={!can} onChange={() => flip(i)} data-testid="design-hand-pick" />
                    <span className={`design-pill is-${c.kind}`}>{c.kind === "open" ? "미정" : "계획"}</span>
                    <span className="design-hand-name"><span className="design-hand-section">{c.section}</span> · {c.title}</span>
                    {!can && <span className="design-hand-why">미정은 정한 뒤 넘긴다</span>}
                  </label>
                </li>
              );
            })}
          </ul>
          <label>제목<input className="input" value={shownTitle} data-testid="design-hand-title" onChange={(e) => setTitle(e.target.value)} /></label>
          <div className="design-new-row">
            <label>저장소<input className="input" value={`${repo.repo} (문서의 저장소)`} readOnly /></label>
            <label>모델·머신
              <select className="input" value={profile} data-testid="design-hand-model" onChange={(e) => setProfile(e.target.value)}>
                <option value="">프로젝트 기본{repo.machine ? ` · ${repo.machine}` : ""}</option>
                {names.map((n) => <option key={n} value={n}>{n}</option>)}
              </select>
            </label>
          </div>
          <label>메모<textarea className="input design-hand-memo" rows={5} value={shownMemo} spellCheck={false} data-testid="design-hand-memo" onChange={(e) => setMemo(e.target.value)} /></label>
          <label className="design-hand-check">
            <input type="checkbox" checked={reserve} onChange={(e) => setReserve(e.target.checked)} data-testid="design-hand-reserve" />
            자리가 나면 바로 시작(예약)
          </label>
          {problem && <p className="confirm-error" role="alert">{problem}</p>}
          {warning && <p className="confirm-error" role="alert" data-testid="design-hand-warning">{warning}</p>}
        </div>
        <footer className="modal-footer">
          <span className="design-hand-note">블록 {picked.length}개 표지에 d-ID를 적어 <code>design/{branch.slug}</code>에 commit하고 바로 올린다.
            구현 thread는 그 끝(<code>{(branch.head ?? "").slice(0, 7)}</code>)에서 갈라진 자기 branch에서 일한다</span>
          <span className="spacer" />
          <button type="button" className="btn" disabled={busy} onClick={onClose}>닫기</button>
          <button type="button" className="btn primary" disabled={!ok} data-testid="design-hand-submit" onClick={() => void submit()}>할 일 1개 만들기</button>
        </footer>
      </div>
    </div>
  );
}
