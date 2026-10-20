// 정의·승인·기록의 정본은 host. 이 패널은 목록·enabled 편집·자동 시작 범위 승인과, 줄마다 상세(AutomationDetail)·프로젝트마다
// 옛 형식 변환(AutomationConvert)을 연다(관리 GUI W1, plan §8.302). 줄은 t3code ScheduledTaskRow처럼 짧게: 이름·배지·켜기,
// 조건 → 동작, 지난·다음 한 줄. 기록·원문·대상은 상세에 있다.
import { useCallback, useEffect, useRef, useState } from "react";
import { call, me } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import type { MissionView, RoutineRun, RoutineView } from "./gen/proto";
import { MissionDetail, RuleDetail } from "./AutomationDetail";
import { ConvertSheet } from "./AutomationConvert";
import { canPreviewConvert, legacyFiles, legacyMission, legacyRule, ruleTone, runWord, stuckBatch } from "./automationModel";
import { missionWord } from "./specialModel";
import "./AutomationPanel.css";

const time = (at: number) => new Date(at).toLocaleString();
function Result({ run }: { run: RoutineRun }) {
  return <span>{time(run.at_ms)} · {runWord[run.status] ?? run.status}: {run.detail}{run.thread && ` · ${run.thread}`}</span>;
}
type Row = { project: string; rules: RoutineView[]; missions: MissionView[] };
type Open = { kind: "rule"; project: string; name: string } | { kind: "mission"; project: string; name: string } | { kind: "convert"; project: string };

export function AutomationPanel({ canControl }: { canControl: boolean }) {
  const [rows, setRows] = useState<Row[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [approval, setApproval] = useState<{ project: string; rule: RoutineView } | null>(null);
  const [open, setOpen] = useState<Open | null>(null);
  const [frontApi, setFrontApi] = useState<string | null>(null);
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
        const missions = await call({ op: "mission_list", project: p.name });
        return { project: p.name, rules: result.routines, missions: missions.kind === "missions" ? missions.missions : [] };
      }));
      if (ticket === generation.current) { setRows(next); setError(null); }
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
  const toggle = async (project: string, rule: RoutineView) => {
    setBusy(`${project}/${rule.name}`);
    try {
      await call({ op: "routine_toggle", project, name: rule.name, on: !rule.enabled });
      await load();
    } catch (e) { setError(String(e)); }
    finally { setBusy(null); }
  };
  const openRule = open?.kind === "rule" ? rows.find((r) => r.project === open.project)?.rules.find((r) => r.name === open.name) : undefined;
  return <section className="automations" aria-label="자동화 목록" data-testid="automations">
    <div className="automation-tools"><span>조건 → 동작</span><button type="button" onClick={() => void load()}>다시 읽기</button></div>
    {error && <p role="alert">{error}</p>}
    {loading && <p role="status">읽는 중…</p>}
    {!loading && rows.every((p) => p.rules.length === 0 && p.missions.length === 0) && <p>자동화가 없습니다.</p>}
    {rows.filter((p) => p.rules.length > 0 || p.missions.length > 0).map(({ project, rules, missions }) => {
      const legacy = legacyFiles(rules, missions).length;
      return <section key={project} aria-label={`${project} 자동화`}>
        <h3>{project} · {rules.length}개</h3>
        {legacy > 0 && <div className="automation-legacy" data-testid={`automation-legacy-${project}`}>
          <span><span className="automation-badge is-legacy">이전 형식</span> {legacy}개 · 웹은 새 형식(TOML)만 고친다</span>
          {canPreviewConvert(frontApi)
            ? <button type="button" className="btn" onClick={() => setOpen({ kind: "convert", project })}>새 형식으로 바꾸기</button>
            : frontApi !== null || !loading ? <span className="automation-dim">host를 1.58 이상으로 올리면 여기서 바꿀 수 있다</span> : null}
        </div>}
        {rules.map((r) => {
          const tone = ruleTone(r);
          const stuck = stuckBatch(r);
          const title = r.title || r.name.replace(/^mission:/, "");
          const detail = () => setOpen(r.origin === "mission" ? { kind: "mission", project, name: r.name.replace(/^mission:/, "") } : { kind: "rule", project, name: r.name });
          return <article className="automation-row" key={r.name} data-testid={`automation-${project}-${r.name}`} data-status={tone === "stuck" ? "stuck" : !r.enabled ? "off" : r.error ? "failed" : r.last_result?.status ?? "ready"}>
            <div className="automation-heading"><span className="automation-indicator" role="img" aria-label={!r.enabled ? "꺼짐" : r.error ? "실패" : stuck ? "막힘" : r.last_result ? runWord[r.last_result.status] ?? r.last_result.status : "기록 없음"} />
              <button type="button" className="automation-name" onClick={detail}>{title}</button>
              {legacyRule(r) && <span className="automation-badge is-legacy">이전 형식</span>}
              {r.origin === "mission" && <span>임무 파일에서</span>}
              <button type="button" role="switch" aria-checked={r.enabled} aria-label={`${project}/${r.name} 켜기`} disabled={!canControl || busy !== null || r.error != null} onClick={() => void toggle(project, r)}>{r.enabled ? "켬" : "끔"}</button>
            </div>
            <p className="automation-summary">{r.trigger || r.schedule} → {r.action || "동작 정보 없음"}</p>
            {r.origin === "mission" && <p>임무 파일의 켜짐 상태입니다. 적용 사본이 다르면 특수 패널에서 다시 읽기를 확인하세요.</p>}
            {r.error ? <p role="alert">{r.error}</p> : <>
              {stuck && <p className="automation-stuck-line" role="alert">막힘 · 입력 전달 모름 · 사람 확인 필요{stuck.thread ? ` (${stuck.thread})` : ""}</p>}
              {!r.enabled && <p>꺼짐</p>}
              {r.origin === "mission" && !r.approved && <p className="automation-approval">임무 승인 필요</p>}
              {r.start_approved === false && <p className="automation-approval">자동 시작 승인 필요 {canControl && <button type="button" disabled={busy !== null} onClick={() => setApproval({ project, rule: r })}>승인 검토</button>}</p>}
              {r.command && (!r.approved || !r.commands_allowed) && <p>{!r.approved ? "승인 필요" : "routine_commands 꺼짐"}</p>}
              <p>지난 {r.last_result ? <Result run={r.last_result} /> : "기록 없음"}</p>
              <p>다음: {r.enabled ? r.next_run_ms === null ? "사건 오면" : time(r.next_run_ms) : "꺼짐"} · 오늘 {r.today ?? "—"}{`/${r.max_per_day || "무제한"}`}</p>
              {r.last_skip && <p className="automation-skip">마지막 건너뜀: {r.last_skip.detail}</p>}
            </>}
            <div className="automation-foot"><span className="automation-file">{r.file || `routines/${r.name}.md`}</span><button type="button" className="btn btn-ghost" aria-label={`${title} 자세히`} onClick={detail}>자세히</button></div>
          </article>;
        })}
        {missions.length > 0 && <>
          <h4 className="automation-subhead">임무 · {missions.length}개</h4>
          {missions.map((m) => <article className="automation-row" key={`mission:${m.name}`} data-testid={`automation-mission-${project}-${m.name}`}>
            <div className="automation-heading">
              <button type="button" className="automation-name" onClick={() => setOpen({ kind: "mission", project, name: m.name })}>{m.title || m.name}</button>
              {legacyMission(m) && <span className="automation-badge is-legacy">이전 형식</span>}
            </div>
            <p className="automation-summary">{missionWord(m)} · 깨움 오늘 {m.wakes_today} · 대기 입력 {m.pending}</p>
            <div className="automation-foot"><span className="automation-file">{m.thread ? `${m.thread.id} · ${m.thread.machine}` : "시작 안 함"}</span><button type="button" className="btn btn-ghost" aria-label={`${m.title || m.name} 자세히`} onClick={() => setOpen({ kind: "mission", project, name: m.name })}>자세히</button></div>
          </article>)}
        </>}
      </section>;
    })}
    {open?.kind === "rule" && openRule && <RuleDetail project={open.project} rule={openRule} canControl={canControl} covered={approval !== null} onChanged={load} onApprove={() => setApproval({ project: open.project, rule: openRule })} onClose={() => setOpen(null)} />}
    {open?.kind === "rule" && !openRule && !loading && <ConfirmDialog title="자동화가 없습니다" body={`${open.project}/${open.name}은 지워졌거나 이름이 바뀌었습니다.`} confirmLabel="닫기" onClose={() => setOpen(null)} onConfirm={async () => setOpen(null)} />}
    {open?.kind === "mission" && <MissionDetail project={open.project} name={open.name} onClose={() => setOpen(null)} />}
    {open?.kind === "convert" && <ConvertSheet project={open.project} canControl={canControl} onClose={() => setOpen(null)} onDone={load} />}
    {approval && <ConfirmDialog title={`${approval.rule.title || approval.rule.name} 자동 시작 범위`} body={`${approval.rule.start_scope || ""}\n\n처음·넓힌 조건과 대상에만 승인합니다. 작업 본문과 선택 예산은 승인 범위에 포함하지 않습니다.`} confirmLabel="승인" onClose={() => setApproval(null)} onConfirm={async () => {
      if (!approval.rule.start_approval_sha256) throw new Error("승인할 범위 hash가 없습니다.");
      await call({ op: "automation_approve", project: approval.project, name: approval.rule.name, approval_sha256: approval.rule.start_approval_sha256 });
      setApproval(null); await load();
    }} />}
  </section>;
}
