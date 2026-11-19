// 구현할 계획 블록을 일반 할 일로 넘긴다. 표지는 구현 branch에서 적는다.
import { Hammer } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";
import { ApiError, call, me } from "./api";
import { apiAtLeast } from "./automationModel";
import { modelChoices, type Callout } from "./designModel";
import type { DesignRepoView, OwnerChoice } from "./gen/proto";

/** 표지 블록 하나: 덩어리 번호, 표지 블록끼리의 차례(host `design_marks`와 같은 차례)와 첫 줄(host가 같은 블록인지 다시 본다) */
export type HandoffBlock = { c: Callout; i: number; ordinal: number; head: string };

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

export function HandoffDialog({ repo, path, blob, blocks, initial, onClose, onDone }: {
  repo: DesignRepoView;
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
  const [owners, setOwners] = useState<OwnerChoice[] | "failed" | null>(null);
  const modelStatusId = useId();
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [warning, setWarning] = useState<string | null>(null);
  useEffect(() => {
    call({ op: "assignable", check: null }).then(
      (r) => setOwners(r.kind === "assignable" ? r.owners : "failed"),
      () => setOwners("failed"),
    );
  }, []);
  const names = modelChoices(Array.isArray(owners) ? owners : [], repo.machine, "").filter(Boolean);
  let modelStatus = "";
  if (owners === null) modelStatus = "모델 불러오는 중";
  else if (owners === "failed") modelStatus = "모델 목록을 불러오지 못했습니다";
  else if (names.length === 0) modelStatus = "선택할 수 있는 모델이 없습니다";
  const selectedProfile = names.includes(profile) ? profile : "";
  const picked = useMemo(() => blocks.filter((b) => picks.has(b.i)), [blocks, picks]);
  const shownTitle = title ?? autoTitle(picked);
  const shownMemo = memo ?? autoMemo(repo.repo, path, picked);
  const flip = (i: number) => setPicks((old) => { const next = new Set(old); if (!next.delete(i)) next.add(i); return next; });
  const ok = picked.length > 0 && shownTitle.trim() !== "" && !busy && warning === null;
  const submit = async () => {
    setBusy(true);
    setProblem(null);
    try {
      if (!apiAtLeast((await me())?.front_api, 1, 189)) throw new Error("작업 branch에서 구현을 맡기려면 host를 갱신해 주세요.");
      const r = await call({
        op: "design_handoff", repo: repo.repo, branch: "", path, blob, blocks: picked.map((p) => ({ ordinal: p.ordinal, head: p.head })),
        title: shownTitle.trim(), profile: selectedProfile || null, memo: shownMemo, reserve,
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
            <div>
              <label>모델·머신
                <select className="input" value={selectedProfile} aria-describedby={modelStatusId} data-testid="design-hand-model" onChange={(e) => setProfile(e.target.value)}>
                  <option value="">프로젝트 기본{repo.machine ? ` · ${repo.machine}` : ""}</option>
                  {names.map((n) => <option key={n} value={n}>{n}</option>)}
                </select>
              </label>
              <span id={modelStatusId} className="design-hand-model-status" role="status">{modelStatus}</span>
            </div>
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
          <span className="design-hand-note" data-testid="design-hand-main">선택한 블록 {picked.length}개</span>
          <span className="spacer" />
          <button type="button" className="btn" disabled={busy} onClick={onClose}>닫기</button>
          <button type="button" className="btn primary" disabled={!ok} data-testid="design-hand-submit" onClick={() => void submit()}>할 일 1개 만들기</button>
        </footer>
      </div>
    </div>
  );
}
