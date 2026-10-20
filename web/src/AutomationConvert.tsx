// 옛 .md 규칙·임무를 새 TOML로 바꾸기(관리 GUI W1, plan §8.303): 미리보기(만들 파일·생기는 규칙·보존 사본) → 실행 → 결과.
// 미리보기는 host가 쓰지 않고 짠 계획이며(`automation_convert` dry_run), 실행은 본 계획의 hash를 실어 그 사이 바뀐 계획을 거부한다.
// 변환은 프로젝트 단위다(host가 옛 파일 전부를 한 번에 검증·변환하고 실패하면 이번 파일만 되돌린다).
import { useEffect, useState } from "react";
import { call } from "./api";
import { AutomationSheet } from "./AutomationSheet";
import { archiveDirs, convertGroups } from "./automationModel";

type Phase = { kind: "loading" } | { kind: "plan"; files: string[]; sha: string } | { kind: "running"; files: string[]; sha: string } | { kind: "done"; files: string[] } | { kind: "error"; message: string; files?: string[]; sha?: string };

function Plan({ files }: { files: string[] }) {
  return <>{convertGroups(files).map((g) => <section key={g.key} className="automation-section" aria-label={g.title} data-group={g.key}>
    <h3>{g.title} · {g.files.length}</h3>
    <ul className="automation-list automation-files">{g.files.map((f) => <li key={f}><code>{f}</code></li>)}</ul>
  </section>)}</>;
}

export function ConvertSheet({ project, canControl, onClose, onDone }: { project: string; canControl: boolean; onClose: () => void; onDone: () => Promise<void> }) {
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });
  useEffect(() => {
    let live = true;
    call({ op: "automation_convert", project, dry_run: true })
      .then((r) => {
        if (!live) return;
        if (r.kind !== "automation_converted" || !r.dry_run) throw new Error("미리보기 응답이 아니다. host가 미리보기를 모르면 변환하지 않는다");
        setPhase({ kind: "plan", files: r.files, sha: r.plan_sha256 });
      })
      .catch((e: unknown) => live && setPhase({ kind: "error", message: e instanceof Error ? e.message : String(e) }));
    return () => { live = false; };
  }, [project]);
  const run = async (files: string[], sha: string) => {
    setPhase({ kind: "running", files, sha });
    try {
      const r = await call({ op: "automation_convert", project, plan_sha256: sha });
      if (r.kind !== "automation_converted") throw new Error("변환 응답이 아니다");
      setPhase({ kind: "done", files: r.files });
      await onDone();
    } catch (e) {
      setPhase({ kind: "error", message: e instanceof Error ? e.message : String(e), files, sha });
    }
  };
  const busy = phase.kind === "running";
  const footer = phase.kind === "plan" && phase.files.length > 0 && canControl
    ? <><button type="button" className="btn" onClick={onClose}>취소</button><button type="button" className="btn btn-primary" onClick={() => void run(phase.files, phase.sha)}>변환</button></>
    : phase.kind === "running" ? <button type="button" className="btn btn-primary" disabled>변환 중…</button>
    : <button type="button" className="btn" onClick={onClose}>닫기</button>;
  return <AutomationSheet title={phase.kind === "done" ? "새 형식으로 바꿨습니다" : "새 형식으로 바꾸기"} subtitle={<span className="automation-dim">{project}</span>} onClose={onClose} busy={busy} footer={footer} testid="automation-convert">
    {phase.kind === "loading" && <p role="status">변환 계획을 짜는 중…</p>}
    {phase.kind === "error" && <p role="alert">{phase.message}</p>}
    {(phase.kind === "plan" || phase.kind === "running") && (phase.files.length === 0
      ? <p data-testid="convert-empty">바꿀 옛 파일이 없습니다.</p>
      : <>
        <p>아래 파일을 새로 만들고 옛 파일은 <code>.converted/</code>로 옮겨 보존합니다. 값(조건·동작·승인 hash)이 같은지 host가 검증하고, 하나라도 실패하면 이번 파일만 되돌립니다. 이미 있는 파일은 덮어쓰지 않습니다.</p>
        {!canControl && <p className="automation-dim">보기 권한은 미리보기만 볼 수 있습니다.</p>}
        <Plan files={phase.files} />
      </>)}
    {phase.kind === "error" && phase.files && <Plan files={phase.files} />}
    {phase.kind === "done" && (phase.files.length === 0 ? <p>바꿀 옛 파일이 없었습니다.</p> : <>
      <p data-testid="convert-archived">옛 파일 보존 위치: {archiveDirs(phase.files).map((d) => <code key={d}>{d} </code>)}</p>
      <p className="automation-dim">새 파일과 보존 사본을 coordinator 폴더 git에 한 커밋으로 남기세요.</p>
      <Plan files={phase.files} />
    </>)}
  </AutomationSheet>;
}
