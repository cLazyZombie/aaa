// 자동화 한 줄의 상세(관리 GUI W1, plan §8.303): 요약·막힌 입력·설정·대상·승인 범위·적용 판·마지막 건너뜀·기록 50건·파일 원문.
// 쓰는 것: 막힌 `do.message` 묶음 풀기(`automation_unblock`), 자동 시작 범위 승인(목록과 같은 창), 그리고 W2(plan §8.342)의
// 고치기(편집 창)·지금 실행(`automation_run`)·지우기(`automation_delete`)·명령 승인(`routine_approve`, 확인 창에 명령·머신·hash).
// 절 순서와 기록 줄 모양은 t3code ScheduledTasksSettings(상태 배지 · 시각 · 요약)와 herdr-projects `routine list`의 kind 낱말을 따른다.
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { call } from "./api";
import { AutomationSheet } from "./AutomationSheet";
import { ConfirmDialog } from "./ConfirmDialog";
import type { RoutineRun, RoutineSource, RoutineView } from "./gen/proto";
import { lastSkip, legacyRule, ruleTargets, ruleTone, ruleWord, runTone, runWord, stuckBatch } from "./automationModel";
import { runKey } from "./automationForm";

const time = (at: number) => new Date(at).toLocaleString();

function Section({ title, children, testid }: { title: string; children: ReactNode; testid?: string }) {
  return <section className="automation-section" aria-label={title} data-testid={testid}><h3>{title}</h3>{children}</section>;
}
function Facts({ rows }: { rows: Array<[string, ReactNode] | null | false> }) {
  return <dl className="automation-facts">{rows.filter((r): r is [string, ReactNode] => !!r).map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}</dl>;
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

export function RuleDetail({ project, rule, canControl, canEdit = false, onEdit = () => {}, covered = false, onChanged, onApprove, onThread, onClose }: {
  project: string;
  /** host가 편집·지금 실행을 안다(FRONT_API 1.76) */
  canEdit?: boolean;
  onEdit?: () => void;
  /** 기록의 thread 번호를 누르면(창을 닫고 그 thread를 연다) */
  onThread?: (id: string) => void;
  /** 목록의 승인 확인창이 위에 떠 있다 */
  covered?: boolean;
  rule: RoutineView;
  canControl: boolean;
  onChanged: () => Promise<void>;
  onApprove: () => void;
  onClose: () => void;
}) {
  const { runs, source, error, loading, load } = useRuns(project, rule.name, rule.last_result?.at_ms);
  const [unblock, setUnblock] = useState<{ batch: string; discard: boolean } | null>(null);
  // 지금 실행·지우기·명령 승인 확인창. 지금 실행 key는 창을 열 때 한 번 정한다(같은 누름은 host가 한 번만 받는다)
  const [ask, setAsk] = useState<{ kind: "run"; key: string } | { kind: "delete" } | { kind: "command" } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const tone = ruleTone(rule);
  const stuck = stuckBatch(rule);
  const skip = lastSkip(rule, runs);
  const legacy = legacyRule(rule);
  const queue = rule.queue;
  // 확인창은 고른 그 묶음이 아직 막혀 있을 때만 뜬다(다른 기기·CLI가 먼저 풀면 닫힌다)
  const asked = unblock && stuck?.id === unblock.batch ? unblock : null;
  const editable = canControl && canEdit && !legacy && !rule.file.startsWith("missions/");
  const runnable = editable && rule.enabled && !rule.error && rule.action !== "배포 제안";
  return <AutomationSheet title={rule.title || rule.name} subtitle={<><span className={`automation-badge is-${tone}`}>{ruleWord[tone]}</span>{legacy && <span className="automation-badge is-legacy">이전 형식</span>}<span className="automation-dim">{project} · {rule.name}</span></>} onClose={onClose} covered={covered || asked !== null || ask !== null} testid="automation-detail">
    {error && <p role="alert">{error}</p>}
    {editable && <div className="automation-actions" data-testid="automation-detail-actions">
      <button type="button" className="btn" onClick={onEdit}>고치기</button>
      <button type="button" className="btn" disabled={!runnable} title={runnable ? undefined : "켜진·오류 없는 규칙만(배포 제안 제외)"} onClick={() => setAsk({ kind: "run", key: runKey() })}>지금 실행</button>
      <button type="button" className="btn btn-danger" disabled={!source} onClick={() => setAsk({ kind: "delete" })}>지우기</button>
    </div>}
    {notice && <p role="status" data-testid="automation-notice">{notice}</p>}
    <Section title="요약">
      <Facts rows={[
        ["상태", ruleWord[tone]],
        ["형식", legacy ? "이전 형식(.md) · 웹은 고치지 않는다. 목록 머리의 「새 형식으로 바꾸기」" : "TOML"],
        ["파일", rule.file || "—"],
        ["지난", rule.last_result ? <RunLineText run={rule.last_result} /> : "기록 없음"],
        ["다음", rule.enabled ? rule.next_run_ms === null ? "사건 오면" : time(rule.next_run_ms) : "꺼짐"],
        ["오늘", `${rule.today}${rule.max_per_day ? `/${rule.max_per_day}` : " · 상한 없음"}`],
      ]} />
      {rule.error && <p role="alert">{rule.error}</p>}
    </Section>
    {stuck && <Section title="막힌 입력" testid="automation-stuck">
      <div className="automation-stuck" role="alert">
        <p><strong>입력 전달 모름 · 확인 필요</strong></p>
        <p>{stuck.thread ? `${stuck.thread}에` : "대상 thread에"} 보내던 글 묶음({stuck.events}건, {time(stuck.at_ms)}부터)이 닿았는지 모릅니다. 자동으로 다시 보내지 않습니다. 대상 thread 화면에서 글이 왔는지 확인한 뒤 고릅니다. 이 묶음은 어느 쪽이든 다시 보내지 않습니다.</p>
        <p>뒤에 쌓인 입력 {queue?.pending ?? 0}건</p>
        {canControl ? <div className="automation-actions">
          <button type="button" className="btn" onClick={() => setUnblock({ batch: stuck.id, discard: false })}>확인함 · 남은 입력 계속 보내기</button>
          <button type="button" className="btn btn-danger" onClick={() => setUnblock({ batch: stuck.id, discard: true })}>확인함 · 남은 입력 버리기</button>
        </div> : <p className="automation-dim">보기 권한에서는 풀 수 없습니다.</p>}
      </div>
    </Section>}
    {rule.memory && <Section title="기억" testid="automation-memory">
      <Facts rows={[
        ["크기", `${rule.memory.bytes.toLocaleString()}바이트`],
        ["마지막 갱신", rule.memory.thread ? <>{onThread ? <ThreadLink id={rule.memory.thread} onThread={onThread} /> : rule.memory.thread}{rule.memory.updated_at_ms != null && ` · ${time(rule.memory.updated_at_ms)}`}</> : "아직 사본 없음"],
      ]} />
      <Source file="memory.md" text={rule.memory.text} truncated={rule.memory.truncated} />
    </Section>}
    <Section title="설정">
      <Facts rows={[
        ["조건", rule.trigger || rule.schedule || "—"],
        ["동작", rule.action || "동작 정보 없음"],
        !!rule.schedule && ["일정", rule.schedule],
        !!rule.command && ["명령", <code key="c">{rule.command}</code>],
        ["하루 상한", rule.max_per_day ? `${rule.max_per_day}번` : "없음"],
      ]} />
      {!canEdit && <p className="automation-dim">이 host는 웹 편집을 모른다(1.76 이전). 파일 원문을 본다.</p>}
    </Section>
    <Section title="대상">
      <ul className="automation-list">{ruleTargets(rule).map((t) => <li key={t}>{t}</li>)}</ul>
    </Section>
    <Section title="승인 범위">
      <Facts rows={[
        !!rule.command && ["명령", !rule.commands_allowed ? "routine_commands 꺼짐(safety)" : rule.approved ? "승인됨" : "승인 필요(명령·머신이 바뀌면 다시)"],
        !!rule.command && ["명령 hash", <code key="h">{rule.command_sha256}</code>],
        rule.start_approved != null && ["자동 시작", rule.start_approved ? "승인됨" : "승인 필요"],
        !rule.command && rule.start_approved == null && ["필요한 승인", "없음(안내·글·깨움은 범위 승인이 없다)"],
      ]} />
      {rule.start_scope && <pre className="automation-scope">{rule.start_scope}</pre>}
      {rule.start_approved === false && canControl && <button type="button" className="btn" onClick={onApprove}>승인 검토</button>}
      {!!rule.command && !rule.approved && canControl && canEdit && <button type="button" className="btn" onClick={() => setAsk({ kind: "command" })}>명령 승인 검토</button>}
    </Section>
    <Section title="적용 판">
      <p>규칙은 따로 적용 판을 고정하지 않습니다. 사건마다 그때의 동작·범위 사본을 큐에 넣고 그 사본대로 처리합니다.</p>
      <Facts rows={queue ? [
        ["대기 입력", `${queue.pending}건`],
        ["처리 중 묶음", queue.inflight ? `${queue.inflight.events}건${queue.inflight.unknown ? " · 보낸 결과 모름" : ""}` : "없음"],
      ] : [["큐", "이 host는 큐 상태를 알리지 않는다(옛 판)"]]} />
    </Section>
    <Section title="마지막 건너뜀">
      {skip ? <p className="automation-run"><RunLine run={skip} /></p> : <p className="automation-dim">없음</p>}
    </Section>
    <Section title={`기록 · 최근 ${runs.length}건(최대 50)`} testid="automation-runs">
      <div className="automation-tools"><button type="button" className="btn" onClick={() => void load()}>기록 다시 읽기</button></div>
      <Runs runs={runs} loading={loading} onThread={onThread} />
    </Section>
    <Section title="파일 원문" testid="automation-source">
      {source ? <>
        <Source file={source.file} text={source.text} truncated={source.truncated} />
        {source.body_file && <Source file={`본문 · ${source.body_file}`} text={source.body} truncated={source.body_truncated} />}
      </> : <p className="automation-dim">{loading ? "읽는 중…" : "원문 없음"}</p>}
    </Section>
    {ask?.kind === "run" && <ConfirmDialog
      title={`${rule.title || rule.name} 지금 실행`}
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
      title={`${rule.title || rule.name} 지우기`}
      body={`끈 뒤 지운 자동화(automations/.trash/)로 옮깁니다. 기다리던 입력 ${queue?.pending ?? 0}건은 보내지 않습니다.\n처리 중인 묶음·기록·본문 파일은 그대로 두고, 목록의 「지운 자동화」에서 꺼진 채로 되살릴 수 있습니다.`}
      confirmLabel="지우기"
      onClose={() => setAsk(null)}
      onConfirm={async () => {
        if (!source) throw new Error("파일 판을 아직 읽지 못했습니다");
        await call({ op: "automation_delete", project, name: rule.name, expected_sha256: source.sha256 });
        setAsk(null);
        onClose();
        await onChanged();
      }}
    />}
    {ask?.kind === "command" && <ConfirmDialog
      title={`${rule.title || rule.name} 명령 승인`}
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
  </AutomationSheet>;
}
function RunLineText({ run }: { run: RoutineRun }) {
  return <>{time(run.at_ms)} · {runWord[run.status] ?? run.status}: {run.detail}{run.thread && ` · ${run.thread}`}</>;
}
