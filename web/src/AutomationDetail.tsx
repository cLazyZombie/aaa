// 자동화 한 줄의 상세(관리 GUI W1, plan §8.303): 요약·막힌 입력·설정·대상·승인 범위·적용 판·마지막 건너뜀·기록 50건·파일 원문.
// 읽기만 한다. 쓰는 것은 막힌 `do.message` 묶음 풀기(`automation_unblock`)와 자동 시작 범위 승인(목록과 같은 창)뿐이다.
// 절 순서와 기록 줄 모양은 t3code ScheduledTasksSettings(상태 배지 · 시각 · 요약)와 herdr-projects `routine list`의 kind 낱말을 따른다.
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { call } from "./api";
import { AutomationSheet } from "./AutomationSheet";
import { ConfirmDialog } from "./ConfirmDialog";
import type { MissionView, RoutineRun, RoutineSource, RoutineView } from "./gen/proto";
import { lastSkip, legacyMission, legacyRule, ruleTargets, ruleTone, runTone, runWord, stuckBatch, type RuleTone } from "./automationModel";
import { missionWord } from "./specialModel";

const time = (at: number) => new Date(at).toLocaleString();
const toneWord: Record<RuleTone, string> = { off: "꺼짐", failed: "파일 오류", stuck: "막힘 · 입력 전달 모름", done: "처리됨", skipped: "건너뜀", waiting: "대기", ready: "기록 없음" };

function Section({ title, children, testid }: { title: string; children: ReactNode; testid?: string }) {
  return <section className="automation-section" aria-label={title} data-testid={testid}><h3>{title}</h3>{children}</section>;
}
function Facts({ rows }: { rows: Array<[string, ReactNode] | null | false> }) {
  return <dl className="automation-facts">{rows.filter((r): r is [string, ReactNode] => !!r).map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}</dl>;
}
export function RunLine({ run }: { run: RoutineRun }) {
  return <>
    <span className={`automation-badge is-${runTone(run)}`}>{runWord[run.status] ?? run.status}</span>
    <time dateTime={new Date(run.at_ms).toISOString()}>{time(run.at_ms)}</time>
    <span className="automation-run-detail">{run.detail}{run.thread && ` · ${run.thread}`}</span>
  </>;
}
function Runs({ runs, loading }: { runs: RoutineRun[]; loading: boolean }) {
  if (loading && runs.length === 0) return <p role="status">읽는 중…</p>;
  if (runs.length === 0) return <p className="automation-dim">기록 없음</p>;
  return <ol className="automation-runs">{runs.map((run) => <li key={`${run.id}:${run.at_ms}`}><RunLine run={run} /></li>)}</ol>;
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

export function RuleDetail({ project, rule, canControl, covered = false, onChanged, onApprove, onClose }: {
  project: string;
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
  const tone = ruleTone(rule);
  const stuck = stuckBatch(rule);
  const skip = lastSkip(rule, runs);
  const legacy = legacyRule(rule);
  const queue = rule.queue;
  // 확인창은 고른 그 묶음이 아직 막혀 있을 때만 뜬다(다른 기기·CLI가 먼저 풀면 닫힌다)
  const asked = unblock && stuck?.id === unblock.batch ? unblock : null;
  return <AutomationSheet title={rule.title || rule.name} subtitle={<><span className={`automation-badge is-${tone}`}>{toneWord[tone]}</span>{legacy && <span className="automation-badge is-legacy">이전 형식</span>}<span className="automation-dim">{project} · {rule.name}</span></>} onClose={onClose} covered={covered || asked !== null} testid="automation-detail">
    {error && <p role="alert">{error}</p>}
    <Section title="요약">
      <Facts rows={[
        ["상태", rule.enabled ? toneWord[tone] : "꺼짐"],
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
    <Section title="설정">
      <Facts rows={[
        ["조건", rule.trigger || rule.schedule || "—"],
        ["동작", rule.action || "동작 정보 없음"],
        !!rule.schedule && ["일정", rule.schedule],
        !!rule.command && ["명령", <code key="c">{rule.command}</code>],
        ["하루 상한", rule.max_per_day ? `${rule.max_per_day}번` : "없음"],
      ]} />
      <p className="automation-dim">고치기는 다음 단계(W2)다. 지금은 파일 원문을 본다.</p>
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
      <Runs runs={runs} loading={loading} />
    </Section>
    <Section title="파일 원문" testid="automation-source">
      {source ? <>
        <Source file={source.file} text={source.text} truncated={source.truncated} />
        {source.body_file && <Source file={`본문 · ${source.body_file}`} text={source.body} truncated={source.body_truncated} />}
      </> : <p className="automation-dim">{loading ? "읽는 중…" : "원문 없음"}</p>}
    </Section>
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

export function MissionDetail({ project, name, onClose }: { project: string; name: string; onClose: () => void }) {
  const [m, setM] = useState<MissionView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { runs, loading, load } = useRuns(project, `mission:${name}`, m?.wakes_today);
  const read = useCallback(async () => {
    try {
      const r = await call({ op: "mission_show", project, name });
      if (r.kind !== "mission") throw new Error("임무 응답이 아니다");
      setM(r.mission);
      setError(null);
    } catch (e) { setError(String(e)); }
  }, [project, name]);
  useEffect(() => { void read(); }, [read]);
  const skip = lastSkip({ last_skip: null }, runs);
  const t = m?.thread;
  return <AutomationSheet title={m?.title || name} subtitle={m && <><span className="automation-badge is-mission">임무</span>{legacyMission(m) && <span className="automation-badge is-legacy">이전 형식</span>}<span className="automation-dim">{project} · {name}</span></>} onClose={onClose} testid="mission-detail">
    {error && <p role="alert">{error}</p>}
    {!m ? !error && <p role="status">읽는 중…</p> : <>
      <Section title="요약">
        <Facts rows={[
          ["상태", missionWord(m)],
          ["형식", legacyMission(m) ? "이전 형식(.md) · 목록 머리의 「새 형식으로 바꾸기」" : "TOML"],
          ["thread", t ? `${t.id} · ${t.status}` : "시작 안 함"],
          ["깨움", `오늘 ${m.wakes_today}${m.max_wakes > 0 ? `/${m.max_wakes}` : " · 상한 없음"} · 대기 입력 ${m.pending}`],
          ["post 처리", `${m.handled_posts}/${m.posts}`],
          !!m.usage_wait_until_ms && ["사용량 대기", time(m.usage_wait_until_ms)],
          !!m.next_reset_ms && ["다음 reset", time(m.next_reset_ms)],
        ]} />
        {m.error && <p role="alert">{m.error}</p>}
        {m.blocked_reason && m.blocked_reason !== "question" && <p className="automation-dim">막힘은 특수 패널의 「다시 시작」이 푼다(다음 단계 W3에서 이 창에도 둔다).</p>}
      </Section>
      <Section title="대상">
        {t ? <Facts rows={[["머신", t.machine], ["저장소", t.repo ?? "저장소 없는 폴더"], ["profile", t.profile]]} /> : <p className="automation-dim">시작하면 그 thread의 머신·저장소·profile이 보인다. 범위는 파일 원문에 있다.</p>}
      </Section>
      <Section title="승인 범위">
        <Facts rows={[["임무", m.approved ? "승인됨" : "승인 필요(첫 시작·repo 추가/변경)"], ["승인 hash", <code key="h">{m.approval_sha256}</code>]]} />
      </Section>
      <Section title="적용 판">
        <Facts rows={[
          ["적용 판", m.applied_sha ?? "없음"],
          ["파일 판", m.file_sha ?? "없음"],
          ["예약 판", m.pending_sha ?? "없음"],
          ["적용", !m.applied_sha ? "시작 전 · 적용 사본 없음" : m.apply_wait ?? (m.stale ? "파일이 적용 사본과 다르다" : "후보 판과 적용 판이 같다")],
        ]} />
        {m.applied_sha && m.diff && <Source file="차이(적용 → 파일)" text={m.diff} />}
      </Section>
      <Section title="마지막 건너뜀">
        {skip ? <p className="automation-run"><RunLine run={skip} /></p> : <p className="automation-dim">없음</p>}
      </Section>
      <Section title={`기록 · 최근 ${runs.length}건(최대 50)`} testid="automation-runs">
        <div className="automation-tools"><button type="button" className="btn" onClick={() => { void read(); void load(); }}>다시 읽기</button></div>
        <Runs runs={runs} loading={loading} />
      </Section>
      <Section title="파일 원문" testid="automation-source">
        <Source file="후보 파일" text={m.file_text} />
        {m.brief_text && <Source file="brief_file" text={m.brief_text} />}
        {m.applied_text && m.applied_text !== m.file_text && <Source file="적용 사본" text={m.applied_text} />}
      </Section>
    </>}
  </AutomationSheet>;
}
