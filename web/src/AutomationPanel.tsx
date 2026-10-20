// 정의·승인·기록의 정본은 host. 이 패널은 목록·enabled 편집·자동 시작 범위 승인과, 줄마다 상세(AutomationDetail)·프로젝트마다
// 옛 형식 변환(AutomationConvert)을 연다(관리 GUI W1, plan §8.303). 줄은 t3code ScheduledTaskRow처럼 짧게: 이름·배지·켜기,
// 조건 → 동작, 지난·다음 한 줄. 기록·원문·대상은 상세에 있다.
import { useCallback, useEffect, useRef, useState } from "react";
import { call, me } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import type { RoutineRun, RoutineView, ThreadView } from "./gen/proto";
import { RuleDetail, ThreadLink } from "./AutomationDetail";
import { ConvertSheet } from "./AutomationConvert";
import { canPreviewConvert, legacyFiles, legacyRule, ruleTone, runWord, stuckBatch } from "./automationModel";
import "./AutomationPanel.css";

const time = (at: number) => new Date(at).toLocaleString();
function Result({ run, onThread }: { run: RoutineRun; onThread: (id: string) => void }) {
  return <span>{time(run.at_ms)} · {runWord[run.status] ?? run.status}: {run.detail}{run.thread && <> · <ThreadLink id={run.thread} onThread={onThread} /></>}</span>;
}
function Closing({ at }: { at: number }) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => { const timer = window.setInterval(() => setNow(Date.now()), 30_000); return () => window.clearInterval(timer); }, [at]);
  const minutes = Math.ceil(Math.max(0, at - now) / 60_000);
  return <span className="automation-dim" data-testid="automation-closing">{minutes > 0 ? `${minutes}분 뒤 끝남` : "곧 끝남"}</span>;
}
type Row = { project: string; rules: RoutineView[] };
type Open = { kind: "rule"; project: string; name: string } | { kind: "convert"; project: string };

/**
 * `threads`: 열린 thread(사이드바 목록과 같은 것). 규칙 줄의 「열림」 칩이 그 규칙이 띄운 열린 thread를 보인다(plan t-0475 A1).
 * `ask`: thread 줄 「자동 · 규칙」 표지가 열어 달라고 한 규칙 상세. 받으면 `onAsked`로 지운다.
 */
export function AutomationPanel({ canControl, threads = [], onThread = () => {}, ask = null, onAsked = () => {} }: {
  canControl: boolean;
  threads?: ThreadView[];
  onThread?: (project: string, id: string) => void;
  ask?: { project: string; name: string } | null;
  onAsked?: () => void;
}) {
  const [rows, setRows] = useState<Row[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [approval, setApproval] = useState<{ project: string; rule: RoutineView } | null>(null);
  const [open, setOpen] = useState<Open | null>(null);
  // undefined: 아직 묻는 중(그동안 변환 단추도 안내 글도 그리지 않는다), null: 옛 host이거나 묻지 못함
  const [frontApi, setFrontApi] = useState<string | null | undefined>(undefined);
  const [loading, setLoading] = useState(true);
  const generation = useRef(0);
  const load = useCallback(async () => {
    const ticket = ++generation.current;
    try {
      const projects = await call({ op: "projects" });
      if (projects.kind !== "projects") throw new Error("프로젝트 목록 응답이 아니다");
      const next = await Promise.all(projects.projects.map(async (p) => {
        const result = await call({ op: "routines", project: p.name });
        if (result.kind !== "routines") throw new Error("자동화 목록 응답이 아니다");
        return { project: p.name, rules: result.routines };
      }));
      if (ticket === generation.current) { setRows(next); setError(null); window.dispatchEvent(new Event("aaa:automations-changed")); }
    } catch (e) {
      if (ticket === generation.current) setError(String(e));
    } finally {
      if (ticket === generation.current) setLoading(false);
    }
  }, []);
  useEffect(() => {
    void load();
    // 옛 host는 판을 알리지 않는다: 미리보기(dry_run)를 버리고 변환하므로 그때는 변환 단추를 두지 않는다
    me().then((m) => setFrontApi(m?.front_api ?? null)).catch(() => setFrontApi(null));
    const refresh = () => { if (!document.hidden) void load(); };
    const timer = window.setInterval(refresh, 30_000);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => { ++generation.current; clearInterval(timer); window.removeEventListener("focus", refresh); document.removeEventListener("visibilitychange", refresh); };
  }, [load]);
  // 보고·Idle·입력 처리 사건으로 남은 시간을 다시 읽는다. 시계 표시는 아래 Closing이 갱신한다.
  const threadStamp = threads.map((t) => `${t.id}:${t.updated_at_ms}:${t.report_hash}:${t.group}:${t.agent?.activity}`).join("|");
  useEffect(() => { void load(); }, [load, threadStamp]);
  const toggle = async (project: string, rule: RoutineView) => {
    setBusy(`${project}/${rule.name}`);
    try {
      await call({ op: "routine_toggle", project, name: rule.name, on: !rule.enabled });
      await load();
    } catch (e) { setError(String(e)); }
    finally { setBusy(null); }
  };
  useEffect(() => {
    if (!ask) return;
    setOpen({ kind: "rule", project: ask.project, name: ask.name });
    onAsked();
  }, [ask, onAsked]);
  const openRule = open?.kind === "rule" ? rows.find((r) => r.project === open.project)?.rules.find((r) => r.name === open.name) : undefined;
  return <section className="automations" aria-label="자동화 목록" data-testid="automations">
    <div className="automation-tools"><span>조건 → 동작</span><button type="button" onClick={() => void load()}>다시 읽기</button></div>
    {error && <p role="alert">{error}</p>}
    {loading && <p role="status">읽는 중…</p>}
    {!loading && rows.every((p) => p.rules.length === 0) && <p>자동화가 없습니다.</p>}
    {rows.filter((p) => p.rules.length > 0).map(({ project, rules }) => {
      const legacy = legacyFiles(rules).length;
      return <section key={project} aria-label={`${project} 자동화`}>
        <h3>{project} · {rules.length}개</h3>
        {legacy > 0 && <div className="automation-legacy" data-testid={`automation-legacy-${project}`}>
          <span><span className="automation-badge is-legacy">이전 형식</span> {legacy}개 · 웹은 새 형식(TOML)만 고친다</span>
          {canPreviewConvert(frontApi)
            ? <button type="button" className="btn" onClick={() => setOpen({ kind: "convert", project })}>새 형식으로 바꾸기</button>
            : frontApi !== undefined && <span className="automation-dim">host를 1.58 이상으로 올리면 여기서 바꿀 수 있다</span>}
        </div>}
        {rules.map((r) => {
          const tone = ruleTone(r);
          const stuck = stuckBatch(r);
          const title = r.title || r.name;
          const detail = () => setOpen({ kind: "rule", project, name: r.name });
          // 이 규칙이 띄워 아직 열린 thread(목록 줄에 호박색 막대)
          const running = threads.filter((t) => t.project === project && t.automation === r.name && t.group !== "resolved" && t.status !== "failed");
          return <article className="automation-row" key={r.name} data-testid={`automation-${project}-${r.name}`} data-status={tone === "stuck" ? "stuck" : !r.enabled ? "off" : r.error ? "failed" : r.last_result?.status ?? "ready"}>
            <div className="automation-heading"><span className="automation-indicator" role="img" aria-label={!r.enabled ? "꺼짐" : r.error ? "실패" : stuck ? "막힘" : r.last_result ? runWord[r.last_result.status] ?? r.last_result.status : "기록 없음"} />
              <button type="button" className="automation-name" onClick={detail}>{title}</button>
              {legacyRule(r) && <span className="automation-badge is-legacy">이전 형식</span>}

              <button type="button" role="switch" aria-checked={r.enabled} aria-label={`${project}/${r.name} 켜기`} disabled={!canControl || busy !== null || r.error != null} onClick={() => void toggle(project, r)}>{r.enabled ? "켬" : "끔"}</button>
            </div>
            <p className="automation-summary">{r.trigger || r.schedule} → {r.action || "동작 정보 없음"}</p>
            {running.length > 0 && <p className="automation-open">{running.map((t) => <span key={t.id}><button type="button" className="automation-open-chip" data-testid={`automation-open-${project}-${t.id}`} title={`${t.id} · ${t.title} 열기`} onClick={() => onThread(project, t.id)}>열림 {t.id}</button>{r.closing?.filter((c) => c.thread === t.id).map((c) => <Closing key={c.thread} at={c.close_at_ms} />)}</span>)}</p>}

            {r.error ? <p role="alert">{r.error}</p> : <>
              {stuck && <p className="automation-stuck-line" role="alert">막힘 · 입력 전달 모름 · 확인 필요{stuck.thread ? ` (${stuck.thread})` : ""}</p>}
              {!r.enabled && <p>꺼짐</p>}

              {r.start_approved === false && <p className="automation-approval">자동 시작 승인 필요 {canControl && <button type="button" disabled={busy !== null} onClick={() => setApproval({ project, rule: r })}>승인 검토</button>}</p>}
              {r.command && (!r.approved || !r.commands_allowed) && <p>{!r.approved ? "승인 필요" : "routine_commands 꺼짐"}</p>}
              <p>지난 {r.last_result ? <Result run={r.last_result} onThread={(id) => onThread(project, id)} /> : "기록 없음"}</p>
              <p>다음: {r.enabled ? r.next_run_ms === null ? "사건 오면" : time(r.next_run_ms) : "꺼짐"} · 오늘 {r.today ?? "—"}{`/${r.max_per_day || "무제한"}`}</p>
              {r.last_skip && <p className="automation-skip">마지막 건너뜀: {r.last_skip.detail}</p>}
            </>}
            <div className="automation-foot"><span className="automation-file">{r.file || `routines/${r.name}.md`}</span><button type="button" className="btn btn-ghost" aria-label={`${title} 자세히`} onClick={detail}>자세히</button></div>
          </article>;
        })}
      </section>;
    })}
    {open?.kind === "rule" && openRule && <RuleDetail project={open.project} rule={openRule} canControl={canControl} covered={approval !== null} onChanged={load} onApprove={() => setApproval({ project: open.project, rule: openRule })} onThread={(id) => { setOpen(null); onThread(open.project, id); }} onClose={() => setOpen(null)} />}
    {open?.kind === "rule" && !openRule && !loading && <ConfirmDialog title="자동화가 없습니다" body={`${open.project}/${open.name}은 지워졌거나 이름이 바뀌었습니다.`} confirmLabel="닫기" onClose={() => setOpen(null)} onConfirm={async () => setOpen(null)} />}
    {open?.kind === "convert" && <ConvertSheet project={open.project} canControl={canControl} onClose={() => setOpen(null)} onDone={load} />}
    {approval && <ConfirmDialog title={`${approval.rule.title || approval.rule.name} 자동 시작 범위`} body={`${approval.rule.start_scope || ""}\n\n처음·넓힌 조건과 대상에만 승인합니다. 작업 본문과 선택 예산은 승인 범위에 포함하지 않습니다.`} confirmLabel="승인" onClose={() => setApproval(null)} onConfirm={async () => {
      if (!approval.rule.start_approval_sha256) throw new Error("승인할 범위 hash가 없습니다.");
      await call({ op: "automation_approve", project: approval.project, name: approval.rule.name, approval_sha256: approval.rule.start_approval_sha256 });
      setApproval(null); await load();
    }} />}
  </section>;
}
