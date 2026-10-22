import { useEffect, useRef, useState, type ReactNode } from "react";
import { BarChart3, Clock3, Code2, FlaskConical, Hammer, Search, X } from "lucide-react";
import type { CodeLines, MetricPoint, MetricRanking, MetricRun, RepoWorkspace } from "./gen/proto";
import { DrawerToggle, HeadMore } from "./drawer";
import { workspaceParams, workspaceRoute, type WorkspaceData } from "./workspaceData";
import "./WorkspaceView.css";

const time = (ms: number | null | undefined) => ms == null ? "미수집" : ms < 60000 ? `${(ms / 1000).toLocaleString("ko-KR", { maximumFractionDigits: 2 })}초` : `${Math.floor(Math.round(ms / 1000) / 60)}분 ${Math.round(ms / 1000) % 60}초`;
const num = (n: number | null | undefined) => n == null ? "미수집" : n.toLocaleString("ko-KR");
const date = (ms: number, utc: boolean, short = false) => new Intl.DateTimeFormat("ko-KR", { timeZone: utc ? "UTC" : "Asia/Seoul", month: "2-digit", day: "2-digit", ...(short ? {} : { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }) }).format(ms);
const SERIES_COLORS = ["var(--status-working)", "var(--status-done)", "var(--notice)", "var(--syntax-meta)"];
const outcome = (s: string) => ({ succeeded: "성공", failed: "실패", partial: "부분 기록" }[s] ?? s);

export function WorkspaceList({ state, selected, onSelect }: { state: WorkspaceData; selected: string | null; onSelect: (id: string) => void }) {
  const [search, setSearch] = useState("");
  const workspaces = state.data?.workspaces ?? [];
  return <div className="ws-list">
    <label className="ws-search"><Search size={16} aria-hidden /><input aria-label="workspace 찾기" placeholder="workspace 찾기" value={search} onChange={(e) => setSearch(e.target.value)} /></label>
    {state.loading && !state.data && <p role="status">불러오는 중</p>}
    {state.error && <p role="alert">{state.error}</p>}
    {workspaces.filter((w) => `${w.project} ${w.name}`.toLowerCase().includes(search.toLowerCase())).map((w) => <button key={w.id} className="ws-repo" aria-pressed={w.id === selected} onClick={() => onSelect(w.id)} data-testid="workspace-repo">
      <span className="ws-mark" aria-hidden>{w.name.slice(0, 3)}</span><span><strong>{w.name}</strong><small>{w.project} · {w.machines.map((m) => m.name).join(" · ")}</small></span>
    </button>)}
    {!state.loading && !workspaces.length && <p>등록된 저장소가 없습니다.</p>}
    <p className="ws-list-note">저장소별 검증 기록을 봅니다.<br />이름이 같은 저장소를 자동으로 합치지 않습니다.</p>
  </div>;
}
function Empty({ children }: { children: ReactNode }) { return <p className="ws-empty">{children}</p>; }
function Metric({ title, value, note, icon }: { title: string; value: string; note: ReactNode; icon: ReactNode }) {
  return <article className="ws-metric"><div>{title}{icon}</div><strong>{value}</strong><small>{note}</small></article>;
}
function Ranking({ title, ranking }: { title: string; ranking: MetricRanking | undefined }) {
  return <section className="ws-card ws-rank"><h2>{title}<small>{ranking?.complete ? "기록 기준" : "부분 기록"}</small></h2>
    <p>{ranking?.unit === "spec-work-sum" ? "E2E 파일별 · 독립 작업 시간 합계" : ranking?.unit === "scenario-step-sum" ? "시나리오별 · 단계 시간 합계" : "개별 테스트 시간"}</p>
    {!ranking?.items.length ? <Empty>{ranking?.missing_reason ?? "기존 기록에 시간이 없어 미수집"}</Empty> : <ol>{ranking.items.map((item) => <li key={item.name}><span title={item.name}>{item.name}</span><strong>{time(item.duration_ms)}</strong><i style={{ width: `${Math.max(0.5, item.duration_ms / Math.max(1, ranking.items[0].duration_ms) * 100)}%` }} /></li>)}</ol>}
    <small>{ranking?.items.length ? `${ranking.items.length}개 확보 · 선택한 실행 안의 순위` : "순위를 채우기 위한 추가 실행은 하지 않습니다."}</small>
  </section>;
}
function Chart({ points, unit, utc, onRun }: { points: MetricPoint[]; unit: "time" | "size"; utc: boolean; onRun: (id: string) => void }) {
  if (!points.length) return <Empty>이 조건에서 수집한 기록이 없습니다.</Empty>;
  const series = [...new Set(points.map((p) => `${p.machine} / ${p.profile ?? "profile 미기록"}`))].sort();
  const color = (p: MetricPoint) => SERIES_COLORS[series.indexOf(`${p.machine} / ${p.profile ?? "profile 미기록"}`) % SERIES_COLORS.length];
  const values = points.map((p) => unit === "size" ? p.value / 1048576 : p.value / 1000);
  const top = Math.max(1, ...values) * 1.15;
  const first = Math.min(...points.map((p) => p.at_ms)), last = Math.max(...points.map((p) => p.at_ms));
  const x = (at: number) => 56 + (first === last ? 0.5 : (at - first) / (last - first)) * 580;
  const y = (v: number) => 180 - v / top * 156;
  return <><div className="ws-legend" aria-label="머신·profile 범례">{series.map((s, i) => <span key={s}><i style={{ background: SERIES_COLORS[i % SERIES_COLORS.length] }} />{s}</span>)}</div><svg className="ws-chart" viewBox="0 0 680 220" role="group" aria-label={`${unit === "size" ? "바이너리 MiB" : "경과 초"} 추이. 점을 선택하면 원본 실행을 봅니다.`}>
    {[0, 0.5, 1].map((v) => <g key={v}><line x1={56} y1={y(v * top)} x2={646} y2={y(v * top)} /><text x={47} y={y(v * top) + 4} textAnchor="end">{(v * top).toFixed(1)}</text></g>)}
    <text x={56} y={12}>{unit === "size" ? "MiB" : "초"} · 0 기준</text>
    {points.map((p, i) => <circle key={`${p.at_ms}:${p.machine}:${p.profile}:${p.run_id}`} cx={x(p.at_ms)} cy={y(values[i])} r={4.5} style={{ fill: color(p) }} className={p.outcome === "failed" ? "ws-failed" : ""} role="button" tabIndex={0} aria-label={`${date(p.at_ms, utc)} · ${p.machine} / ${p.profile ?? "profile 미기록"} · ${unit === "size" ? `${values[i].toFixed(2)} MiB` : time(p.value)} · ${p.samples > 1 ? `p50 n=${p.samples}` : outcome(p.outcome)}`} onClick={() => onRun(p.run_id)} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onRun(p.run_id); } }}><title>{p.machine} · {values[i].toFixed(2)} · n={p.samples}</title></circle>)}
    <text x={56} y={210}>{date(first, utc, true)}</text>{last !== first && <text x={636} y={210} textAnchor="end">{date(last, utc, true)}</text>}
  </svg></>;
}
function Lines({ rows, branches, branch, axis, total, change, utc }: { rows: CodeLines[]; branches: string[]; branch: string | null; axis: string; total: boolean; change: (c: Record<string, string | null>) => void; utc: boolean }) {
  const latest = rows.at(-1);
  const firstAt = rows[0]?.observed_at_ms ?? 0, lastAt = latest?.observed_at_ms ?? 0;
  const split = !total && latest?.breakdown;
  // 일별 점 선택·중복 제거는 host가 담당한다. 날짜별은 관측 시각 간격, 커밋별은 관측 순서다.
  const keys = split ? ["product", "test", "e2e"] : ["total"];
  const max = Math.max(1, ...rows.flatMap((r) => keys.map((k) => k === "total" ? r.total_lines : r.breakdown?.[k] ?? 0)));
  return <section className="ws-card ws-lines"><h2>코드 줄 수 <Code2 size={16} aria-hidden /></h2><div className="ws-inline">
    <select aria-label="코드 줄 수 branch" value={branch ?? ""} onChange={(e) => change({ branch: e.target.value })}>{!branches.length && <option value="">branch 미수집</option>}{branches.map((b) => <option key={b} value={b}>{b}</option>)}</select>
    <select aria-label="코드 줄 수 표시" value={total ? "total" : "split"} onChange={(e) => change({ lines: e.target.value })}><option value="split">제품 / 시험 / E2E</option><option value="total">합계</option></select>
    <select aria-label="코드 줄 수 축" value={axis} onChange={(e) => change({ axis: e.target.value })}><option value="date">날짜별</option><option value="commit">커밋별</option></select></div>
    {latest ? <><strong className="ws-lines-total">{num(latest.total_lines)}<small>줄 · 빈 줄 제외, 주석 포함</small></strong><p>{split ? `제품 ${num(split.product ?? 0)} / 시험 ${num(split.test ?? 0)} / E2E ${num(split.e2e ?? 0)}` : "혼합 시험 코드의 경계가 불명확하면 합계만 표시합니다."}</p>
      <svg className="ws-chart" viewBox="0 0 680 170" role="img" aria-label={`코드 줄 수 추이 ${rows.length}개 관측`}>
        {[0, 0.5, 1].map((v) => <g key={v}><line x1={65} y1={135 - v * 110} x2={645} y2={135 - v * 110} /><text x={57} y={139 - v * 110} textAnchor="end">{Math.round(v * max).toLocaleString("ko-KR")}</text></g>)}
        {rows.map((r, i) => { const x = 65 + (rows.length === 1 || (axis !== "commit" && firstAt === lastAt) ? 0.5 : axis === "commit" ? i / (rows.length - 1) : (r.observed_at_ms - firstAt) / (lastAt - firstAt)) * 570; return <g key={`${r.commit}:${r.rules_version}`}>{keys.map((key) => { const value = key === "total" ? r.total_lines : r.breakdown?.[key]; return value == null ? null : <circle key={key} data-series={key} cx={x} cy={135 - value / max * 110} r={4}><title>{key} · {r.commit.slice(0, 8)} · {num(value)}줄 · 규칙 {r.rules_version}</title></circle>; })}{(i === 0 || i === rows.length - 1) && <text x={x} y={162} textAnchor={rows.length === 1 ? "middle" : i === 0 ? "start" : "end"}>{axis === "commit" ? r.commit.slice(0, 8) : date(r.observed_at_ms, utc, true)}</text>}</g>; })}
      </svg><small>규칙 {latest.rules_version} · {latest.scanned_files}파일 · 이번에 {latest.changed_files}파일 읽음 · {latest.source}</small></> : <Empty>코드 줄 수 수집 전입니다. 커밋별로 한 번 집계하고 변경 파일만 갱신합니다.</Empty>}
  </section>;
}
function Detail({ run, utc, onClose }: { run: MetricRun; utc: boolean; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { const el = dialog.current!; const focus = document.activeElement as HTMLElement | null; el.showModal(); return () => { el.close(); focus?.focus(); }; }, []);
  return <dialog ref={dialog} className="ws-detail" aria-labelledby="ws-detail-title" onCancel={onClose} onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}><header><h2 id="ws-detail-title">실행 조건과 단계</h2><button type="button" className="icon-button" aria-label="상세 닫기" onClick={onClose}><X size={18} /></button></header>
    <div className="ws-detail-body"><p>{date(run.observed_at_ms, utc)} · {run.machine} / {run.profile ?? "profile 미기록"} · {outcome(run.outcome)}</p><p>출처: {run.source}</p><p>검증 전체 {time(run.verification_ms)} · 빌드 {time(run.build_ms)}</p><h3>단계별 경과 · 병렬 구간 포함</h3><p>단계 시간의 합계는 전체 경과가 아닙니다. 시작 offset이 없는 기록은 겹침을 추정하지 않습니다.</p>
      <ul className="ws-stages">{run.stages.map((s) => <li key={s.name}><span>{s.name}</span><meter min={0} max={Math.max(1, ...run.stages.map((s) => s.duration_ms))} value={s.duration_ms} aria-label={`${s.name} 경과`} /><strong>{time(s.duration_ms)}</strong><small>{s.outcome}</small></li>)}</ul>{run.notes.map((n) => <p key={n}>{n}</p>)}
    </div></dialog>;
}
export function WorkspaceView({ state, route, onRoute, up }: { state: WorkspaceData; route: string | null; onRoute: (s: string) => void; up: boolean }) {
  const params = workspaceParams(route), data = state.data;
  const workspace: RepoWorkspace | undefined = data?.workspaces.find((w) => w.id === data.workspace_id);
  const utc = params.get("tz") === "utc", tab = params.get("tab") ?? "overview";
  const runs = data?.runs ?? [], selected = runs.find((r) => r.run_id === params.get("run")) ?? runs.at(-1);
  const [detail, setDetail] = useState<string | null>(null);
  const shownDetail = runs.find((r) => r.run_id === detail);
  const change = (c: Record<string, string | null>) => onRoute(workspaceRoute(route, c));
  const chart = params.get("metric") === "build" ? data?.build_trend : data?.verification_trend;
  useEffect(() => { if (data?.workspace_id && !workspaceParams(route).has("ws")) onRoute(workspaceRoute(route, { ws: data.workspace_id })); }, [data?.workspace_id, route, onRoute]);
  const stale = workspace?.last_collected_ms != null && (data?.queried_at_ms ?? 0) - workspace.last_collected_ms > 86400000;
  return <section className="ws-view" data-testid="workspace-view"><header className="ws-top"><DrawerToggle /><span>{workspace?.project ?? "workspace"} <span className="muted">/</span> {workspace?.name ?? "저장소 지표"}</span><span className="spacer" /><button type="button" onClick={state.reload} disabled={state.loading || !up}>다시 읽기</button><HeadMore /></header>
    <div className="ws-content">
      <div className="ws-heading"><small>WORKSPACE</small><h1>{workspace?.name ?? "저장소 지표"}<span>검증의 흐름을 한눈에</span></h1><p>검증 시간·테스트 수·E2E·TOP 5를 기존 실행 결과에서 확인합니다.</p></div>
      {!up && <p className="banner warn" role="status">host 연결이 끊겼습니다. 마지막으로 읽은 기록입니다.</p>}
      {state.error && <p className="banner err" role="alert">조회 오류: {state.error} · 마지막 성공 {state.lastSuccess ? date(state.lastSuccess, utc) : "없음"}</p>}
      {state.loading && !data && <div className="ws-skeleton" role="status">지표 불러오는 중</div>}
      {data && <>
        {!workspace ? <Empty>등록된 workspace가 없거나 선택한 저장소가 목록에 없습니다.</Empty> : <>
          <div className="ws-tabs" role="tablist" aria-label="workspace 화면" onKeyDown={(e) => { const tabs = [...e.currentTarget.querySelectorAll<HTMLButtonElement>("[role=tab]")]; const i = tabs.indexOf(document.activeElement as HTMLButtonElement); const next = e.key === "ArrowRight" ? (i + 1) % tabs.length : e.key === "ArrowLeft" ? (i + tabs.length - 1) % tabs.length : e.key === "Home" ? 0 : e.key === "End" ? tabs.length - 1 : -1; if (next >= 0) { e.preventDefault(); tabs[next].click(); tabs[next].focus(); } }}>
            {[["overview", "개요"], ["runs", "실행 기록"], ["collection", "수집 상태"]].map(([id, label]) => <button key={id} type="button" role="tab" id={`ws-tab-${id}`} aria-controls={`ws-panel-${id}`} tabIndex={tab === id ? 0 : -1} aria-selected={tab === id} onClick={() => change({ tab: id })}>{label}</button>)}
          </div>
          <div className="ws-filters"><div className="ws-period" role="group" aria-label="기간">{[["1", "오늘"], ["7", "7일"], ["30", "30일"]].map(([value, label]) => <button key={value} type="button" aria-pressed={(params.get("days") ?? "7") === value} onClick={() => change({ days: value, run: null })}>{label}</button>)}</div>
            <label>머신<select aria-label="머신" value={params.get("machine") ?? ""} onChange={(e) => change({ machine: e.target.value, run: null })}><option value="">모든 머신</option>{workspace.machines.map((m) => <option key={m.name} value={m.name}>{m.name}</option>)}</select></label>
            <label>profile<select aria-label="profile" value={params.get("profile") ?? ""} onChange={(e) => change({ profile: e.target.value, run: null })}><option value="">전체</option><option value="unknown">미기록</option>{data.profiles.map((p) => <option key={p} value={p}>{p}</option>)}</select></label>
            <label>시간대<select aria-label="시간대" value={utc ? "utc" : "kst"} onChange={(e) => change({ tz: e.target.value })}><option value="kst">KST (UTC+9)</option><option value="utc">UTC</option></select></label>
          </div>
          {stale && <p className="ws-status" role="status">마지막 수집이 하루 이상 지났습니다.</p>}
          {data.truncated && <p className="ws-status">최근 요약 400개까지 표시합니다. 기간을 좁히세요.</p>}
          {tab === "collection" ? <div role="tabpanel" id="ws-panel-collection" aria-labelledby="ws-tab-collection" className="ws-collection ws-card"><h2>기존 결과를 보존합니다 · 수집 전용 실행 없음</h2><p>마지막 수집 {workspace.last_collected_ms ? date(workspace.last_collected_ms, utc) : "수집 전"}</p>{workspace.machines.map((m) => <p key={m.name}>{m.name} · {m.connected ? "연결됨" : "agent 단절 · 전송 대기"} · {m.supported ? "수집 지원" : "agent 지표 기능 미지원"}</p>)}{workspace.collection_notes.map((n) => <p key={n}>{n}</p>)}<p>보고 회수 때 기존 종료 결과를 한 번 읽고 host 원장에 보존합니다. 검증·빌드·프로파일링을 추가 실행하지 않으며, 화면 조회 때 로그나 코드를 읽지 않습니다.</p><p>코드 줄 수는 커밋·규칙 판별로 중복을 빼고 Git 객체의 변경 파일만 읽습니다. 비용 상한을 넘으면 수집 대기로 둡니다.</p><p>등록한 저장소의 workspace ID를 사용합니다. 같은 이름·remote만으로 fork와 미러를 합치지 않습니다.</p></div> : tab === "runs" ? <div role="tabpanel" id="ws-panel-runs" aria-labelledby="ws-tab-runs"><RunTable runs={runs} utc={utc} onRun={setDetail} /></div> : <div role="tabpanel" id="ws-panel-overview" aria-labelledby="ws-tab-overview">
            <div className="ws-run-select"><label>기준 실행<select aria-label="기준 실행" value={selected?.run_id ?? ""} onChange={(e) => change({ run: e.target.value })}>{!runs.length && <option value="">실행 기록 없음</option>}{[...runs].reverse().map((r) => <option key={r.run_id} value={r.run_id}>{date(r.observed_at_ms, utc)} · {r.machine} · {outcome(r.outcome)}</option>)}</select></label><small>개수·TOP 5는 선택한 실행 기준입니다.</small></div>
            <div className="ws-metrics"><Metric title="검증 전체 경과" value={time(selected?.verification_ms)} note={selected ? outcome(selected.outcome) : "기존 실행 결과 수집 전"} icon={<Clock3 size={16} />} /><Metric title="테스트 수" value={selected?.tests ? num(selected.tests.passed + selected.tests.failed) : "미수집"} note={selected?.tests ? `${selected.tests.unit} · 성공 ${selected.tests.passed} · 실패 ${selected.tests.failed} · 건너뜀 ${selected.tests.skipped}` : "기존 결과에 개수 없음"} icon={<FlaskConical size={16} />} /><Metric title="E2E 수" value={selected?.e2e_files == null ? "미수집" : `${num(selected.e2e_files)}${selected.e2e_unit === "scenario" ? "시나리오" : "파일"}`} note={selected?.e2e_unit === "scenario" ? "관측 시나리오 수 · 단계 시간 합계" : selected?.e2e_jobs == null ? "기존 결과에 개수 없음" : `${num(selected.e2e_jobs)} 독립 작업 · 파일 수와 더하지 않음`} icon={<BarChart3 size={16} />} /><Metric title="빌드 경과" value={time(selected?.build_ms)} note="선택 실행 · 보조 지표" icon={<Hammer size={16} />} /></div>
            <div className="ws-grid"><Ranking title="테스트 TOP 5" ranking={selected?.slow_tests} /><Ranking title="E2E TOP 5" ranking={selected?.slow_e2e} /></div>
            <section className="ws-card"><h2>검증·빌드 시간 추이</h2><div className="ws-inline"><select aria-label="시간 지표" value={params.get("metric") ?? "verify"} onChange={(e) => change({ metric: e.target.value })}><option value="verify">검증</option><option value="build">빌드</option></select><select aria-label="시간 집계" value={params.get("by") ?? "run"} onChange={(e) => change({ by: e.target.value })}><option value="run">시각별</option><option value="day">날짜별 p50 · 성공만</option></select><small>{utc ? "UTC" : "KST"} · {runs.length}개 실행</small></div><Chart points={chart ?? []} unit="time" utc={utc} onRun={setDetail} /><p>시각별은 실패도 보존합니다. 날짜별은 같은 머신·profile의 성공 표본 p50입니다. 조건이 다른 점과 비어 있는 날짜를 잇지 않습니다.</p></section>
            <Lines rows={data.code_lines} branches={data.code_branches} branch={data.code_branch} axis={params.get("axis") ?? "date"} total={params.get("lines") === "total"} change={change} utc={utc} />
            <section className="ws-card"><h2>바이너리 용량 추이</h2><Chart points={data.artifact_trend} unit="size" utc={utc} onRun={setDetail} /><p>기존 산출물의 byte 크기를 MiB로 표시합니다. 크기를 얻기 위한 빌드·strip·해시 계산은 하지 않습니다.</p></section>
            <section className="ws-card"><h2>실행 기록<button type="button" onClick={() => change({ tab: "runs" })}>모두 보기</button></h2><RunTable runs={runs.slice(-5)} utc={utc} onRun={setDetail} /></section>
          </div>}
        </>}
      </>}
    </div>{shownDetail && <Detail key={shownDetail.run_id} run={shownDetail} utc={utc} onClose={() => setDetail(null)} />}
  </section>;
}
function RunTable({ runs, utc, onRun }: { runs: MetricRun[]; utc: boolean; onRun: (id: string) => void }) {
  if (!runs.length) return <Empty>선택한 조건의 실행 기록이 없습니다. 기존 결과 수집 후 표시합니다.</Empty>;
  return <div className="ws-table-scroll" tabIndex={0} role="region" aria-label="실행 기록 표"><table><thead><tr><th>기록 시각 · {utc ? "UTC" : "KST"}</th><th>머신 / profile</th><th>빌드</th><th>검증</th><th>용량</th><th>결과</th></tr></thead><tbody>{[...runs].reverse().map((r) => <tr key={r.run_id}><td><button type="button" onClick={() => onRun(r.run_id)}>{date(r.observed_at_ms, utc)} ↗</button></td><td>{r.machine} / {r.profile ?? "미기록"}</td><td>{time(r.build_ms)}</td><td>{time(r.verification_ms)}</td><td>{r.artifact_bytes == null ? "미수집" : `${(r.artifact_bytes / 1048576).toFixed(2)} MiB`}</td><td data-outcome={r.outcome}>{outcome(r.outcome)}</td></tr>)}</tbody></table></div>;
}
