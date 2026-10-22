// 자동화 한 규칙의 설정 화면(t-0628 시안 1A·2A: 본문 thread 화면 자리, 떠 있는 창이 아니다. 관리 GUI W1 plan §8.303·W2 §8.342의 상세).
// 머리: 이름 · 표지 · 켜기 스위치 · 닫기. 그 아래 고치기 · 지금 실행 · 지우기, 사람이 할 것(파일 오류·실행 실패·막힌 입력·자동 시작 승인·
// 명령 승인)은 맨 위 색 상자에 단추와 함께, 숫자 칸(지난 · 다음 · 오늘 · 열린 thread), 설정(조건·동작·대상·상한·큐·파일) → 승인 범위 →
// 기록 50건 → 기억(접힘) → 파일 원문. 쓰는 것: 켜기(`routine_toggle`), 막힌 `do.message` 묶음 풀기(`automation_unblock`), 자동 시작 범위
// 승인(`automation_approve`), 지금 실행(`automation_run`)·지우기(`automation_delete`)·명령 승인(`routine_approve`, 확인 창에 명령·머신·hash).
// 기록 줄 모양은 t3code ScheduledTasksSettings(상태 배지 · 시각 · 요약)와 herdr-projects `routine list`의 kind 낱말을 따른다.
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { call } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { Toggle } from "./herdr/SettingsControls";
import type { RoutineRun, RoutineSource, RoutineView, ThreadView } from "./gen/proto";
import { cardTone, cardWord, lastSkip, legacyRule, ruleTargets, runTone, runWord, shortTrigger, stuckBatch } from "./automationModel";
import { runKey } from "./automationForm";

const time = (at: number) => new Date(at).toLocaleString();
const short = (at: number) => new Date(at).toLocaleString(undefined, { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });

/**
 * 본문 설정·편집 화면의 틀(thread 화면과 같은 머리 + 가운데 칸). 휴대폰은 머리의 목록 단추(≡)가 drawer의 카드 목록을 연다.
 * `tools`: 머리 오른쪽(켜기 스위치). `footer`: 칸 아래 늘 보이는 단추 줄(편집의 닫기·저장).
 */
export function AutomationPage({ title, badge, subtitle, tools, onClose, busy = false, testid, footer, children }: {
  title: string;
  badge?: ReactNode;
  subtitle?: string;
  tools?: ReactNode;
  onClose: () => void;
  /** 일하는 중: 닫지 않는다 */
  busy?: boolean;
  testid?: string;
  footer?: ReactNode;
  children: ReactNode;
}) {
  return <div className="thread-view automation-view" data-testid={testid}>
    <header className="term-head">
      <DrawerToggle />
      <HeadNote />
      <div className="term-title automation-view-title">
        <span className="automation-view-name"><strong>{title}</strong>{badge}</span>
        {subtitle && <span className="muted">{subtitle}</span>}
      </div>
      {tools}
      <button type="button" className="icon-button" aria-label="닫기" title="닫기" data-testid="automation-close" disabled={busy} onClick={onClose}><X aria-hidden="true" /></button>
      <HeadMore />
    </header>
    <div className="thread-body automation-view-body"><div className="thread-col automation-col">{children}</div></div>
    {footer && <footer className="automation-view-foot">{footer}</footer>}
  </div>;
}

function Section({ title, children, testid, tools }: { title: string; children: ReactNode; testid?: string; tools?: ReactNode }) {
  return <section className="automation-section" aria-label={title} data-testid={testid}><h3>{title}{tools}</h3>{children}</section>;
}
function Facts({ rows }: { rows: Array<[string, ReactNode] | null | false> }) {
  return <dl className="automation-facts">{rows.filter((r): r is [string, ReactNode] => !!r).map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}</dl>;
}
function Stat({ label, value, sub }: { label: string; value: ReactNode; sub?: ReactNode }) {
  return <div className="automation-stat"><dt>{label}</dt><dd>{value}</dd>{sub && <dd className="automation-stat-sub">{sub}</dd>}</div>;
}
function Callout({ tone, title, testid, children }: { tone: "red" | "notice"; title: string; testid?: string; children: ReactNode }) {
  return <div className={`automation-callout is-${tone}`} role={tone === "red" ? "alert" : undefined} data-testid={testid}><h4>{title}</h4>{children}</div>;
}
/** 기록의 thread 번호: 누르면 그 thread(열려 있으면 pane, 끝났으면 보고, plan t-0475 A1) */
export function ThreadLink({ id, onThread }: { id: string; onThread: (id: string) => void }) {
  return <button type="button" className="automation-thread-link" data-testid={`automation-thread-${id}`} onClick={() => onThread(id)}>{id}</button>;
}
export function RunLine({ run, onThread }: { run: RoutineRun; onThread?: (id: string) => void }) {
  return <>
    <span className={`automation-badge is-${runTone(run)}`}>{runWord[run.status] ?? run.status}</span>
    <time dateTime={new Date(run.at_ms).toISOString()}>{time(run.at_ms)}</time>
    <span className="automation-run-detail">{run.detail}{run.thread && (onThread ? <> · <ThreadLink id={run.thread} onThread={onThread} /></> : ` · ${run.thread}`)}</span>
  </>;
}
function Runs({ runs, loading, onThread }: { runs: RoutineRun[]; loading: boolean; onThread?: (id: string) => void }) {
  if (loading && runs.length === 0) return <p role="status">읽는 중…</p>;
  if (runs.length === 0) return <p className="automation-dim">기록 없음</p>;
  return <ol className="automation-runs">{runs.map((run) => <li key={`${run.id}:${run.at_ms}`}><RunLine run={run} onThread={onThread} /></li>)}</ol>;
}
function Source({ file, text, truncated }: { file: string; text: string; truncated?: boolean }) {
  return <figure className="automation-source"><figcaption>{file}{truncated && " · 64KiB에서 자름"}</figcaption><pre>{text || "(비었다)"}</pre></figure>;
}
function Closing({ at }: { at: number }) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => { const timer = window.setInterval(() => setNow(Date.now()), 30_000); return () => window.clearInterval(timer); }, [at]);
  const minutes = Math.ceil(Math.max(0, at - now) / 60_000);
  return <span className="automation-dim" data-testid="automation-closing">{minutes > 0 ? `${minutes}분 뒤 끝남` : "곧 끝남"}</span>;
}

/** routine_runs 50건과 파일 원문. 줄이 바뀌면(host 기록이 늘면) 다시 읽는다 */
function useRuns(project: string, name: string, stamp: unknown) {
  const [runs, setRuns] = useState<RoutineRun[]>([]);
  const [source, setSource] = useState<RoutineSource | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const ticket = useRef(0);
  const load = useCallback(async () => {
    const mine = ++ticket.current;
    setLoading(true);
    try {
      const r = await call({ op: "routine_runs", project, name });
      if (r.kind !== "routine_runs") throw new Error("기록 응답이 아니다");
      if (mine === ticket.current) { setRuns(r.runs); setSource(r.source ?? null); setError(null); }
    } catch (e) {
      if (mine === ticket.current) setError(String(e));
    } finally {
      if (mine === ticket.current) setLoading(false);
    }
  }, [project, name]);
  useEffect(() => { void load(); }, [load, stamp]);
  useEffect(() => () => { ++ticket.current; }, []);
  return { runs, source, error, loading, load };
}

export function RuleDetail({ project, rule, canControl, canEdit = false, running = [], onEdit = () => {}, onChanged, onThread, onClose, onDeleted }: {
  project: string;
  rule: RoutineView;
  canControl: boolean;
  /** host가 편집·지금 실행을 안다(FRONT_API 1.76) */
  canEdit?: boolean;
  /** 이 규칙이 띄워 아직 열린 thread */
  running?: ThreadView[];
  onEdit?: () => void;
  onChanged: () => Promise<void>;
  /** 기록·열린 thread 번호를 누르면 그 thread를 연다 */
  onThread?: (id: string) => void;
  onClose: () => void;
  /** 지운 뒤(본문을 닫는다) */
  onDeleted: () => void;
}) {
  canControl = canControl && !rule.web_read_only_reason;
  const { runs, source, error, loading, load } = useRuns(project, rule.name, rule.last_result?.at_ms);
  const [unblock, setUnblock] = useState<{ batch: string; discard: boolean } | null>(null);
  // 지금 실행·지우기·명령 승인·자동 시작 승인 확인창. 지금 실행 key는 창을 열 때 한 번 정한다(같은 누름은 host가 한 번만 받는다)
  const [ask, setAsk] = useState<{ kind: "run"; key: string } | { kind: "delete" } | { kind: "command" } | { kind: "start" } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [toggling, setToggling] = useState(false);
  const [toggleError, setToggleError] = useState<string | null>(null);
  // 다른 규칙으로 옮기면 그 규칙의 알림은 지운다
  useEffect(() => { setNotice(null); setToggleError(null); }, [project, rule.name]);
  const tone = cardTone(rule);
  const stuck = stuckBatch(rule);
  const skip = lastSkip(rule, runs);
  const legacy = legacyRule(rule);
  const queue = rule.queue;
  const failed = rule.last_result?.status === "failed" ? rule.last_result : null;
  // 확인창은 고른 그 묶음이 아직 막혀 있을 때만 뜬다(다른 기기·CLI가 먼저 풀면 닫힌다)
  const asked = unblock && stuck?.id === unblock.batch ? unblock : null;
  const editable = canControl && canEdit && !legacy && !rule.file.startsWith("missions/");
  const runnable = editable && rule.enabled && !rule.error && rule.action !== "배포 제안";
  const toggle = async () => {
    setToggling(true); setToggleError(null);
    try {
      await call({ op: "routine_toggle", project, name: rule.name, on: !rule.enabled });
      await onChanged();
    } catch (e) { setToggleError(String(e)); }
    finally { setToggling(false); }
  };
  const title = rule.title || rule.name;
  return <AutomationPage title={title} subtitle={`${project} · ${rule.name}`} testid="automation-detail" onClose={onClose}
    badge={<>{tone !== "on" && <span className={`automation-badge is-${tone}`} data-testid="automation-detail-tone">{cardWord[tone]}</span>}{legacy && <span className="automation-badge is-legacy">이전 형식</span>}{rule.web_read_only_reason && <span className="automation-badge is-legacy">시스템 · 읽기 전용</span>}</>}
    tools={<label className="automation-switch"><span>{rule.enabled ? "켜짐" : "꺼짐"}</span>
      <Toggle checked={rule.enabled} label={`${project}/${rule.name} 켜기`} disabled={!canControl || toggling || rule.error != null} onChange={() => void toggle()} /></label>}>
    {rule.web_read_only_reason && <Callout tone="notice" title="시스템 자동화 · 읽기 전용" testid="automation-read-only"><p>{rule.web_read_only_reason}</p></Callout>}
    {error && <p role="alert">{error}</p>}
    {toggleError && <p role="alert">{toggleError}</p>}
    {editable && <div className="automation-actions" data-testid="automation-detail-actions">
      <button type="button" className="btn" onClick={onEdit}>고치기</button>
      <button type="button" className="btn" disabled={!runnable} title={runnable ? undefined : "켜진·오류 없는 규칙만(배포 제안 제외)"} onClick={() => setAsk({ kind: "run", key: runKey() })}>지금 실행</button>
      <button type="button" className="btn btn-danger automation-actions-end" disabled={!source} onClick={() => setAsk({ kind: "delete" })}>지우기</button>
    </div>}
    {notice && <p role="status" data-testid="automation-notice">{notice}</p>}

    {rule.error && <Callout tone="red" title="파일 오류" testid="automation-file-error">
      <p>이 파일로는 돌지 않습니다. 파일을 고치면 다시 읽습니다.</p>
      <pre className="automation-scope">{rule.error}</pre>
    </Callout>}
    {!rule.error && failed && <Callout tone="red" title="마지막 실행 실패" testid="automation-failed">
      <p>{time(failed.at_ms)} · {failed.detail}{failed.thread && <> · {onThread ? <ThreadLink id={failed.thread} onThread={onThread} /> : failed.thread}</>}</p>
    </Callout>}
    {stuck && <Callout tone="red" title="막힘 · 입력 전달 모름 · 확인 필요" testid="automation-stuck">
      <p>{stuck.thread ? `${stuck.thread}에` : "대상 thread에"} 보내던 글 묶음({stuck.events}건, {time(stuck.at_ms)}부터)이 닿았는지 모릅니다. 자동으로 다시 보내지 않습니다. 대상 thread 화면에서 글이 왔는지 확인한 뒤 고릅니다. 이 묶음은 어느 쪽이든 다시 보내지 않습니다.</p>
      <p>뒤에 쌓인 입력 {queue?.pending ?? 0}건</p>
      {canControl ? <div className="automation-actions">
        <button type="button" className="btn" onClick={() => setUnblock({ batch: stuck.id, discard: false })}>확인함 · 남은 입력 계속 보내기</button>
        <button type="button" className="btn btn-danger" onClick={() => setUnblock({ batch: stuck.id, discard: true })}>확인함 · 남은 입력 버리기</button>
      </div> : <p className="automation-dim">보기 권한에서는 풀 수 없습니다.</p>}
    </Callout>}
    {rule.start_approved === false && <Callout tone="notice" title="자동 시작 승인 필요" testid="automation-start-approval">
      <p>처음·넓힌 조건과 대상에만 승인합니다. 작업 본문과 선택 예산은 승인 범위에 포함하지 않습니다.</p>
      {rule.start_scope && <pre className="automation-scope">{rule.start_scope}</pre>}
      {canControl && <div className="automation-actions"><button type="button" className="btn btn-primary" onClick={() => setAsk({ kind: "start" })}>승인 검토</button></div>}
    </Callout>}
    {!!rule.command && !rule.approved && <Callout tone="notice" title="명령 승인 필요" testid="automation-command-approval">
      <p>명령 <code>{rule.command}</code>을 {rule.machine ?? "host 머신"}에서 돌리려면 승인이 필요합니다. 명령이나 머신이 바뀌면 다시 승인합니다.</p>
      {canControl && canEdit && <div className="automation-actions"><button type="button" className="btn btn-primary" onClick={() => setAsk({ kind: "command" })}>명령 승인 검토</button></div>}
    </Callout>}

    <dl className="automation-stats" aria-label="요약" data-testid="automation-summary">
      <Stat label="지난" value={rule.last_result ? short(rule.last_result.at_ms) : "기록 없음"}
        sub={rule.last_result && `${runWord[rule.last_result.status] ?? rule.last_result.status} · ${rule.last_result.detail}`} />
      <Stat label="다음" value={rule.enabled ? rule.next_run_ms === null ? "사건 오면" : short(rule.next_run_ms) : "꺼짐"} sub={shortTrigger(rule)} />
      <Stat label="오늘" value={`${rule.today}번`} sub={rule.max_per_day ? `하루 상한 ${rule.max_per_day}번` : "하루 상한 없음"} />
      <Stat label="열린 thread" value={running.length === 0 ? "없음" : <span className="automation-open">{running.map((t) => <button key={t.id} type="button" className="automation-open-chip"
        data-testid={`automation-open-${project}-${t.id}`} title={`${t.id} · ${t.title} 열기`} onClick={() => onThread?.(t.id)}>{t.id}</button>)}</span>}
        sub={rule.closing?.filter((c) => running.some((t) => t.id === c.thread)).map((c) => <Closing key={c.thread} at={c.close_at_ms} />)} />
    </dl>

    <Section title="설정">
      <Facts rows={[
        ["조건", <>{shortTrigger(rule)}{(rule.trigger || rule.schedule) !== shortTrigger(rule) && <> <code>{rule.trigger || rule.schedule}</code></>}</>],
        ["동작", rule.action || "동작 정보 없음"],
        !!rule.command && ["명령", <code key="c">{rule.command}</code>],
        ["대상", <ul key="t" className="automation-list">{ruleTargets(rule).map((t) => <li key={t}>{t}</li>)}</ul>],
        ["하루 상한", rule.max_per_day ? `${rule.max_per_day}번` : "없음"],
        ["대기 입력", queue ? `${queue.pending}건 · 처리 중 묶음 ${queue.inflight ? `${queue.inflight.events}건${queue.inflight.unknown ? "(보낸 결과 모름)" : ""}` : "없음"}` : "이 host는 큐 상태를 알리지 않는다(옛 판)"],
        ["파일", <>{legacy ? "이전 형식(.md) · 웹은 고치지 않는다. 목록의 「새 형식으로 바꾸기」" : "TOML"} · <code>{rule.file || "—"}</code></>],
      ]} />
      {!canEdit && <p className="automation-dim">이 host는 웹 편집을 모른다(1.76 이전). 파일 원문을 본다.</p>}
    </Section>
    <Section title="승인 범위">
      <Facts rows={[
        !!rule.command && ["명령", !rule.commands_allowed ? "routine_commands 꺼짐(safety)" : rule.approved ? "승인됨" : "승인 필요(명령·머신이 바뀌면 다시)"],
        !!rule.command && ["명령 hash", <code key="h">{rule.command_sha256}</code>],
        rule.start_approved != null && ["자동 시작", rule.start_approved ? "승인됨" : "승인 필요"],
        !rule.command && rule.start_approved == null && ["필요한 승인", "없음(안내·글·깨움은 범위 승인이 없다)"],
      ]} />
      {rule.start_approved === true && rule.start_scope && <pre className="automation-scope">{rule.start_scope}</pre>}
    </Section>
    <Section title={`기록 · 최근 ${runs.length}건(최대 50)`} testid="automation-runs" tools={<button type="button" className="ghost small" onClick={() => void load()}>기록 다시 읽기</button>}>
      {skip && <p className="automation-run automation-skip" aria-label="마지막 건너뜀"><span className="automation-dim">마지막 건너뜀</span><RunLine run={skip} /></p>}
      <Runs runs={runs} loading={loading} onThread={onThread} />
    </Section>
    {rule.memory && <details className="automation-fold" data-testid="automation-memory">
      <summary>기억 <span>{rule.memory.bytes.toLocaleString()}바이트 · {rule.memory.thread
        ? <>{onThread ? <ThreadLink id={rule.memory.thread} onThread={onThread} /> : rule.memory.thread}{rule.memory.updated_at_ms != null && ` · ${time(rule.memory.updated_at_ms)}`}</>
        : "아직 사본 없음"}</span></summary>
      <Source file="memory.md" text={rule.memory.text} truncated={rule.memory.truncated} />
    </details>}
    <details className="automation-fold" open data-testid="automation-source">
      <summary>파일 원문 <span>{source?.file ?? rule.file}</span></summary>
      {source ? <>
        <Source file={source.file} text={source.text} truncated={source.truncated} />
        {source.body_file && <Source file={`본문 · ${source.body_file}`} text={source.body} truncated={source.body_truncated} />}
      </> : <p className="automation-dim">{loading ? "읽는 중…" : "원문 없음"}</p>}
    </details>

    {ask?.kind === "start" && <ConfirmDialog title={`${title} 자동 시작 범위`} body={`${rule.start_scope || ""}\n\n처음·넓힌 조건과 대상에만 승인합니다. 작업 본문과 선택 예산은 승인 범위에 포함하지 않습니다.`} confirmLabel="승인" onClose={() => setAsk(null)} onConfirm={async () => {
      if (!rule.start_approval_sha256) throw new Error("승인할 범위 hash가 없습니다.");
      await call({ op: "automation_approve", project, name: rule.name, approval_sha256: rule.start_approval_sha256 });
      setAsk(null);
      await onChanged();
    }} />}
    {ask?.kind === "run" && <ConfirmDialog
      title={`${title} 지금 실행`}
      body={`조건을 기다리지 않고 동작을 한 번 돌립니다: ${rule.action}.${rule.command ? "\n명령 조건은 명령을 지금 돌리고, 출력이 바뀌었을 때만 동작이 돕니다." : ""}\n사용량 문턱·하루 상한·최소 간격은 그대로 지킵니다.`}
      confirmLabel="지금 실행"
      onClose={() => setAsk(null)}
      onConfirm={async () => {
        const r = await call({ op: "automation_run", project, name: rule.name, key: ask.key });
        if (r.kind !== "automation_ran") throw new Error("실행 응답이 아니다");
        setNotice(r.accepted ? r.detail : "이미 받은 실행입니다");
        setAsk(null);
        await onChanged();
        await load();
      }}
    />}
    {ask?.kind === "delete" && <ConfirmDialog
      title={`${title} 지우기`}
      body={`끈 뒤 지운 자동화(automations/.trash/)로 옮깁니다. 기다리던 입력 ${queue?.pending ?? 0}건은 보내지 않습니다.\n처리 중인 묶음·기록·본문 파일은 그대로 두고, 목록의 「지운 자동화」에서 꺼진 채로 되살릴 수 있습니다.`}
      confirmLabel="지우기"
      onClose={() => setAsk(null)}
      onConfirm={async () => {
        if (!source) throw new Error("파일 판을 아직 읽지 못했습니다");
        await call({ op: "automation_delete", project, name: rule.name, expected_sha256: source.sha256 });
        setAsk(null);
        onDeleted();
        await onChanged();
      }}
    />}
    {ask?.kind === "command" && <ConfirmDialog
      title={`${title} 명령 승인`}
      body={`명령: ${rule.command}\n머신: ${rule.machine ?? "host 머신"}\nhash: ${rule.command_sha256}\nsafety routine_commands: ${rule.commands_allowed ? "켜짐" : "꺼짐(승인해도 돌지 않습니다)"}\n\n이 명령을 이 머신에서 일정마다 돌립니다. 명령이나 머신이 바뀌면 다시 승인합니다.`}
      confirmLabel="승인"
      onClose={() => setAsk(null)}
      onConfirm={async () => {
        await call({ op: "routine_approve", project, name: rule.name, command_sha256: rule.command_sha256 });
        setAsk(null);
        await onChanged();
      }}
    />}
    {asked && <ConfirmDialog
      title={asked.discard ? "남은 입력 버리기" : "남은 입력 계속 보내기"}
      body={`${stuck?.thread ?? "대상 thread"}에 보내던 묶음 ${stuck?.events}건은 다시 보내지 않습니다.\n${asked.discard ? `뒤에 쌓인 입력 ${queue?.pending ?? 0}건도 버립니다. 되돌릴 수 없습니다.` : `뒤에 쌓인 입력 ${queue?.pending ?? 0}건은 다음 차례에 보냅니다.`}`}
      confirmLabel={asked.discard ? "버리기" : "계속 보내기"}
      onClose={() => setUnblock(null)}
      onConfirm={async () => {
        await call({ op: "automation_unblock", project, name: rule.name, batch: asked.batch, discard: asked.discard });
        setUnblock(null);
        await onChanged();
        await load();
      }}
    />}
  </AutomationPage>;
}
