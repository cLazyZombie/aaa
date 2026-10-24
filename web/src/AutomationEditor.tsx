import { ignoredFailure } from "./errorDiagnostic.ts";
// 자동화 만들기·고치기(관리 GUI W2, plan §8.342). 폼과 TOML 고급 편집이 한 초안을 함께 쓴다: 폼을 고치면 host가 바뀐 칸만
// 원문에 합쳐 돌려주고(`automation_validate`), TOML을 고치면 host가 읽은 값으로 폼을 다시 그린다. 저장은 본 판(sha256)일 때만
// 쓰고, 다르면 host가 `conflict`로 거부한다: 그때 지금 파일과 내 초안의 차이를 보이고 사람이 고른다(자동 병합 없음).
// 틀은 설정 화면과 같은 본문 AutomationPage(t-0628 3A: 떠 있는 창이 아니라 본문에서 고친다), 검토 절은 계획 §2.2 「사건마다 새 thread」.
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ApiError, call } from "./api";
import { AutomationPage } from "./AutomationDetail";
import { Markdown } from "./Markdown";
import type { AutomationCheck, AutomationDoc } from "./gen/proto";
import {
  actionKind, actionKinds, action as actionOf, bodyOf, CI_EVENTS, condition as conditionOf, conditionKind, conditionKinds, GIT_EVENTS, lineDiff,
  listText, newForm, parseList, parsePercent, PR_EVENTS, setAction, setCondition, setField, validName, type ActionKind, type ConditionKind, type Form,
} from "./automationForm";

type Mode = "form" | "toml" | "review";
type Conflict = { reason: string; latest: AutomationDoc | null; mine: string; mineBody: string | null };

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return <label className="automation-field"><span>{label}</span>{children}{hint && <small className="automation-dim">{hint}</small>}</label>;
}
function Text({ label, value, onChange, hint, placeholder, testid, mono }: { label: string; value: unknown; onChange: (v: string) => void; hint?: string; placeholder?: string; testid?: string; mono?: boolean }) {
  return <Field label={label} hint={hint}><input className={`input${mono ? " automation-mono" : ""}`} value={typeof value === "string" || typeof value === "number" ? String(value) : ""} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} data-testid={testid} spellCheck={false} /></Field>;
}
function List({ label, value, onChange, hint, testid }: { label: string; value: unknown; onChange: (v: string[]) => void; hint?: string; testid?: string }) {
  // 쓰는 중의 쉼표·빈칸을 지우지 않게 글을 따로 쥔다. 밖에서 값이 바뀌면(TOML에서 고침) 그 값으로 다시 쓴다
  const [draft, setDraft] = useState(listText(value));
  const outside = listText(value);
  useEffect(() => { setDraft((d) => (parseList(d).join(", ") === outside ? d : outside)); }, [outside]);
  return <Text label={label} hint={hint ?? "쉼표로 나눈다"} value={draft} testid={testid} onChange={(v) => { setDraft(v); onChange(parseList(v)); }} />;
}
function Checks({ label, options, value, onChange }: { label: string; options: string[]; value: unknown; onChange: (v: string[]) => void }) {
  const on = new Set(Array.isArray(value) ? (value as string[]) : []);
  return <fieldset className="automation-checks"><legend>{label}</legend>{options.map((o) => <label key={o}><input type="checkbox" checked={on.has(o)} onChange={(e) => onChange(options.filter((x) => (x === o ? e.target.checked : on.has(x))))} />{o}</label>)}</fieldset>;
}
function Toggle({ label, value, onChange }: { label: string; value: unknown; onChange: (v: boolean) => void }) {
  return <label className="automation-toggle"><input type="checkbox" checked={value === true} onChange={(e) => onChange(e.target.checked)} />{label}</label>;
}

/** 일정: 매일 HH:MM 또는 간격 */
function Timing({ form, update }: { form: Form; update: (f: Form) => void }) {
  const c = conditionOf(form);
  const daily = typeof c.daily === "string";
  const set = (key: string, value: unknown) => update(setField(form, "on", key, value));
  return <>
    <fieldset className="automation-checks"><legend>언제</legend>
      <label><input type="radio" name="timing" checked={daily} onChange={() => update(setField(setField(form, "on", "every", null), "on", "daily", "09:00"))} />매일 정한 시각</label>
      <label><input type="radio" name="timing" checked={!daily} onChange={() => update(setField(setField(form, "on", "daily", null), "on", "every", "1h"))} />간격마다</label>
    </fieldset>
    {daily ? <Text label="시각(host 지역 시간)" value={c.daily} onChange={(v) => set("daily", v)} placeholder="09:00" testid="automation-form-daily" />
      : <Text label="간격" value={c.every} onChange={(v) => set("every", v)} placeholder="30m · 2h · 1d" testid="automation-form-every" />}
    <Text label="열린 일반 thread 최소 개수" value={c.min_open_threads} onChange={(v) => set("min_open_threads", v === "" ? null : Number(v))} placeholder="비우면 제한 없음" testid="automation-form-min-open-threads" hint="자동화·완료·보류 thread는 제외한다. 부족한 차례는 기록하고 넘긴다" />
    <Field label="놓친 차례"><select className="input" value={c.catch_up === "skip" ? "skip" : "once"} onChange={(e) => set("catch_up", e.target.value)}><option value="once">한 번으로 모아 돈다</option><option value="skip">기록하고 넘긴다</option></select></Field>
  </>;
}

function ConditionFields({ form, update }: { form: Form; update: (f: Form) => void }) {
  const kind = conditionKind(form);
  const c = conditionOf(form);
  const set = (key: string, value: unknown) => update(setField(form, "on", key, value));
  switch (kind) {
    case "schedule": return <Timing form={form} update={update} />;
    case "command": return <>
      <Timing form={form} update={update} />
      <Text label="명령" value={c.command} onChange={(v) => set("command", v)} mono testid="automation-form-command" hint="출력이 바뀌면 동작이 돈다. 명령·머신은 승인한 것만 돈다" />
      <Text label="명령 머신" value={c.machine} onChange={(v) => set("machine", v)} placeholder="비우면 host 머신" />
    </>;
    case "thread": return <>
      <List label="사건" value={c.events} onChange={(v) => set("events", v)} hint="예: thread_failed, thread_reported, git_pushed" testid="automation-form-events" />
      <List label="저장소" value={c.repos} onChange={(v) => set("repos", v)} hint="비우면 모든 저장소" />
      <Toggle label="다른 자동화가 띄운 thread의 사건도 받는다" value={c.include_automation} onChange={(v) => set("include_automation", v || null)} />
    </>;
    case "pr": return <Checks label="PR 사건" options={PR_EVENTS} value={c.events} onChange={(v) => set("events", v)} />;
    case "ci": case "git": return <>
      <Text label="저장소" value={c.repo} onChange={(v) => set("repo", v)} testid="automation-form-repo" />
      <List label="branch" value={c.branches} onChange={(v) => set("branches", v)} />
      {kind === "ci" && <List label="workflow" value={c.workflows} onChange={(v) => set("workflows", v)} hint="비우면 모든 workflow" />}
      <Checks label="사건" options={kind === "ci" ? CI_EVENTS : GIT_EVENTS} value={c.events} onChange={(v) => set("events", v)} />
      <Text label="조회 머신" value={c.machine} onChange={(v) => set("machine", v)} placeholder="비우면 저장소가 있는 머신" />
    </>;
    default: return <p role="alert">이 조건은 폼으로 고치지 못한다. TOML에서 고친다.</p>;
  }
}

function ActionFields({ form, update }: { form: Form; update: (f: Form) => void }) {
  const kind = actionKind(form);
  const a = actionOf(form);
  const set = (key: string, value: unknown) => update(setField(form, "do", key, value));
  switch (kind) {
    case "command": return <><Text label="실행 명령" value={a.command} onChange={(v) => set("command", v)} mono testid="automation-action-command" /><Text label="실행 머신" value={a.machine} onChange={(v) => set("machine", v)} placeholder="비우면 host 머신" /></>;
    case "inbox": return <Toggle label="휴대폰에도 알린다(앱 안 알림)" value={a.notify} onChange={(v) => set("notify", v)} />;
    case "message": return <Text label="받는 thread" value={a.thread} onChange={(v) => set("thread", v)} placeholder="owner 또는 t-0123" hint="owner는 사건의 thread다(일정·명령에는 없다)" testid="automation-form-thread" />;
    case "start": return <>
      <Text label="thread 제목" value={a.title} onChange={(v) => set("title", v)} hint="{date}·{machine}·{repo_path}를 채운다" testid="automation-form-start-title" />
      <List label="profile 후보" value={a.profiles} onChange={(v) => set("profiles", v)} testid="automation-form-profiles" />
      <List label="머신 후보" value={a.machines} onChange={(v) => set("machines", v)} testid="automation-form-machines" />
      <Text label="저장소" value={a.repo} onChange={(v) => set("repo", v)} placeholder="비우면 저장소 없는 작업 폴더" />
      <Field label="열린 thread"><select className="input" value={a.reuse === "open" ? "open" : "new"} onChange={(e) => update(e.target.value === "open" ? setField(form, "do", "reuse", "open") : setField(setField(setField(form, "do", "reuse", "new"), "do", "memory", null), "do", "close_idle", null))}><option value="new">사건마다 새 thread</option><option value="open">같은 규칙의 열린 thread에 이어 보낸다</option></select></Field>
      {a.reuse === "open" && <>
        <Toggle label="규칙 기억(memory) 쓰기" value={a.memory} onChange={(v) => set("memory", v)} />
        <Text label="조용하면 끝내기" value={a.close_idle} onChange={(v) => set("close_idle", v)} placeholder="예: 30m" />
      </>}
      <Toggle label="사건 thread의 보고를 붙인다" value={a.reports} onChange={(v) => set("reports", v)} />
    </>;
    case "deploy": return <p className="automation-dim">main push가 쌓이면 coordinator inbox에 배포 제안을 남긴다. 빌드·배포는 하지 않는다.</p>;
    default: return <p role="alert">이 동작은 폼으로 고치지 못한다. TOML에서 고친다.</p>;
  }
}

function Limits({ form, update }: { form: Form; update: (f: Form) => void }) {
  const l = form.limits ?? {};
  const set = (key: string, value: unknown) => update(setField(form, "limits", key, value));
  const percent = (key: string) => (v: string) => { const n = parsePercent(v); if (n !== "bad") set(key, n); };
  const number = (key: string) => (v: string) => { const n = Number(v); set(key, v.trim() === "" || !Number.isFinite(n) ? null : Math.max(0, Math.floor(n))); };
  return <>
    <div className="automation-grid">
      <Text label="5시간 창 남은 % 아래면 쉼" value={l.usage_session_below} onChange={percent("usage_session_below")} testid="automation-form-usage-session" />
      <Text label="주간 남은 % 아래면 쉼" value={l.usage_week_below} onChange={percent("usage_week_below")} testid="automation-form-usage-week" />
      <Text label="입력 묶는 시간" value={l.batch} onChange={(v) => set("batch", v)} placeholder="없음" />
      <Text label="실행 사이 최소 간격" value={l.min_gap} onChange={(v) => set("min_gap", v)} placeholder="없음" />
      {actionKind(form) === "start" && <Text label="자동화 thread 동시 상한" value={l.max_running} onChange={number("max_running")} placeholder="1" />}
      <Text label="하루 상한" value={l.max_per_day} onChange={number("max_per_day")} placeholder="없음" />
      {actionKind(form) === "deploy" && <Text label="새 커밋 수" value={l.min_commits} onChange={number("min_commits")} placeholder="1" />}
    </div>
    <p className="automation-dim">자동화 thread는 규칙당 기본 1개이며 동시 상한의 빈칸·0도 1개다. 횟수·간격에는 기본 제한이 없다. 사용량 문턱만 기본으로 채운다(5시간 5%·주간 10%).</p>
  </>;
}

function Review({ check, form }: { check: AutomationCheck | null; form: Form }) {
  const r = check?.review;
  if (!r) return <p className="automation-dim">{check?.errors.length ? "오류를 고치면 검토를 보인다." : "검사 중…"}</p>;
  const l = form.limits ?? {};
  const usage = [l.usage_session_below != null && `5시간 창 ${l.usage_session_below}%`, l.usage_week_below != null && `주간 ${l.usage_week_below}%`].filter(Boolean);
  const start = actionKind(form) === "start";
  return <dl className="automation-facts" data-testid="automation-review">
    <div><dt>저장하면</dt><dd>{r.enabled ? "켜진 채 저장한다. 저장만으로는 돌지 않고 다음 일정·사건부터 돈다" : "꺼진 채 저장한다"}</dd></div>
    <div><dt>조건 → 동작</dt><dd>{r.trigger} → {r.action}</dd></div>
    {start && <div><dt>지금 설정이면</dt><dd data-testid="automation-review-start">{r.start_threads === "auto" ? "바로 시작(safety start_threads = auto)" : "제안만 남긴다(safety start_threads = propose)"}</dd></div>}
    {start && <div><dt>자동 시작 범위</dt><dd>{r.start_approved ? "승인한 범위 안" : "승인 필요: 저장한 뒤 「승인 검토」에서 승인해야 시작한다"}{r.start_scope && <pre className="automation-scope">{r.start_scope}</pre>}</dd></div>}
    {r.command && <div><dt>명령</dt><dd><code>{r.command}</code> · {r.machine ?? "host 머신"} · {!r.commands_allowed ? "safety routine_commands 꺼짐" : r.command_approved ? "승인됨" : "승인 필요(저장한 뒤 「명령 승인 검토」)"}</dd></div>}
    <div><dt>사용량 문턱</dt><dd>{usage.length ? `${usage.join("·")} 아래면 쉼` : "없음"}</dd></div>
    {check!.warnings.map((w) => <div key={w}><dt>주의</dt><dd role="note">{w}</dd></div>)}
  </dl>;
}

function DiffBlock({ from, to, testid }: { from: string; to: string; testid?: string }) {
  return <pre className="automation-diff" data-testid={testid}>{lineDiff(from, to).map((d, i) => <span key={i} className={`is-${d.kind}`}>{d.kind === "add" ? "+ " : d.kind === "del" ? "− " : "  "}{d.text}{"\n"}</span>)}</pre>;
}
function ConflictPanel({ conflict, onReopen, onOverwrite }: { conflict: Conflict; onReopen: () => void; onOverwrite: (() => void) | null }) {
  const latest = conflict.latest;
  // 본문 파일이 바뀐 충돌은 본문 차이도 보인다
  const bodyChanged = !!latest?.body_file && conflict.mineBody !== null && (latest.body ?? "") !== conflict.mineBody;
  return <section className="automation-conflict" role="alert" data-testid="automation-conflict">
    <p><strong>그사이 바뀜</strong> · {conflict.reason}</p>
    {latest ? <>
      <p className="automation-dim">− 내 초안에만 있는 줄 · + 지금 파일에만 있는 줄</p>
      <DiffBlock from={conflict.mine} to={latest.text} testid="automation-conflict-diff" />
      {bodyChanged && <><p className="automation-dim">본문 · {latest.body_file}</p><DiffBlock from={conflict.mineBody ?? ""} to={latest.body ?? ""} testid="automation-conflict-body-diff" /></>}
      <div className="automation-actions">
        <button type="button" className="btn" onClick={onReopen}>지금 파일로 다시 열기(내 초안 버림)</button>
        {onOverwrite && <button type="button" className="btn btn-danger" onClick={onOverwrite}>차이를 봤다 · 내 초안으로 저장</button>}
      </div>
    </> : <p>이 자동화는 지워졌다. 되살리려면 목록의 「지운 자동화」에서 되살린다.</p>}
  </section>;
}

export function AutomationEditor({ canV2 = false, project, name: openName, onClose, onSaved }: {
  canV2?: boolean;
  project: string;
  /** 없으면 새 자동화 */
  name?: string;
  onClose: () => void;
  onSaved: (name: string) => Promise<void>;
}) {
  const creating = !openName;
  const [name, setName] = useState(openName ?? "");
  const [mode, setMode] = useState<Mode>("form");
  const [text, setText] = useState("");
  const [form, setForm] = useState<Form>(creating ? newForm() : {});
  const [base, setBase] = useState<{ sha: string | null; bodySha: string | null; bodyFile: string | null }>({ sha: null, bodySha: null, bodyFile: null });
  const [body, setBody] = useState<string | null>(null);
  const [check, setCheck] = useState<AutomationCheck | null>(null);
  const [preview, setPreview] = useState(false);
  // 새 초안도 첫 host 검사가 끝난 뒤 연다. 잠깐 보인 입력칸을 load가 제거하면
  // focus와 실제 글 입력 사이에 대상이 사라져 이름 입력을 잃을 수 있다.
  const [loading, setLoading] = useState(true);
  const [readOnlyReason, setReadOnlyReason] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<Conflict | null>(null);
  // 검사 차례: 고칠 때마다·다시 읽을 때마다 올린다. 늦게 온 검사 응답은 그 뒤 고침을 덮지 않는다
  const ticket = useRef(0);
  const draft = useRef({ text, form, body, mode });
  draft.current = { text, form, body, mode };
  // 아직 검사하지 않은 고침이 어느 쪽(폼·TOML)에서 왔는가. 탭을 바꾸거나 저장하기 전에 먼저 검사한다
  const pending = useRef<"form" | "toml" | null>(null);
  const last = useRef<"form" | "toml">("form");
  const timer = useRef<number | null>(null);
  const [flushing, setFlushing] = useState(false);
  const checkName = creating ? (validName(name) ? name : "new") : openName!;

  /** host 검사. 폼에서 온 것이면 합친 원문을, TOML에서 온 것이면 읽은 폼을 받는다 */
  const validate = useCallback(async (from: "form" | "toml"): Promise<{ text: string; check: AutomationCheck } | null> => {
    const mine = ticket.current;
    const { text: t, form: f, body: b } = draft.current;
    const r = await call({ op: "automation_validate", project, name: checkName, text: t, ...(from === "form" ? { form: f } : {}), ...(b !== null ? { body: b } : {}) });
    if (r.kind !== "automation_checked") throw new Error("검사 응답이 아니다");
    if (mine !== ticket.current) return null;
    setCheck(r.check);
    if (from === "form") setText(r.text);
    else if (r.check.form) setForm(r.check.form as Form);
    return { text: r.text, check: r.check };
  }, [project, checkName]);

  const load = useCallback(async () => {
    ++ticket.current;
    pending.current = null;
    if (timer.current !== null) { window.clearTimeout(timer.current); timer.current = null; }
    setLoading(true);
    try {
      if (creating) { await validate("form"); return; }
      const r = await call({ op: "automation_read", project, name: openName! });
      if (r.kind !== "automation_doc") throw new Error("자동화 응답이 아니다");
      const d = r.doc;
      setText(d.text); setCheck(d.check); setBody(d.body_file ? d.body ?? "" : null);
      if (d.check.form) setForm(d.check.form as Form);
      setBase({ sha: d.sha256, bodySha: d.body_sha256 ?? null, bodyFile: d.body_file ?? null });
      if (d.web_read_only_reason) setReadOnlyReason(d.web_read_only_reason);
      else if (d.legacy) setError("이전 형식(.md)은 웹에서 고치지 않는다. 목록 머리의 「새 형식으로 바꾸기」를 먼저 한다");
      else if (!d.check.form) setMode("toml");
      setConflict(null);
    } catch (e) { setError(String(e instanceof Error ? e.message : e)); }
    finally { setLoading(false); }
  }, [creating, openName, project, validate]);
  useEffect(() => { void load(); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  /** 기다리던 고침을 지금 검사한다(없으면 마지막 쪽으로 다시 검사). 그사이 더 고쳤으면 null이다 */
  const flush = useCallback(async () => {
    if (timer.current !== null) { window.clearTimeout(timer.current); timer.current = null; }
    const from = pending.current ?? last.current;
    const mine = ticket.current;
    const r = await validate(from);
    if (r && mine === ticket.current) pending.current = null;
    return r;
  }, [validate]);
  // 고친 뒤 잠시 멈추면 host가 검사한다(키마다 부르지 않는다)
  const edited = (from: "form" | "toml") => {
    ++ticket.current;
    // 다른 쪽 고침이 아직 검사 전이면 그쪽 글을 덮지 않게 탭 전환에서 먼저 검사했다(아래 switchMode)
    pending.current = from;
    last.current = from;
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => { timer.current = null; flush().catch((e) => setError(String(e instanceof Error ? e.message : e))); }, 350);
  };
  useEffect(() => () => { if (timer.current !== null) window.clearTimeout(timer.current); }, []);
  const updateForm = (f: Form) => { setForm(f); edited("form"); };
  const updateText = (t: string) => { setText(t); edited("toml"); };
  const updateBody = (b: string) => {
    if (bodyOf(form).file) { setBody(b); edited(mode === "toml" ? "toml" : "form"); }
    else updateForm(setField(form, "do", "text", b));
  };
  /** 탭 바꾸기: 다른 쪽 고침을 먼저 검사해 폼과 TOML이 같은 초안을 보게 한다 */
  const switchMode = async (m: Mode) => {
    if (pending.current) {
      setFlushing(true);
      try { if (!(await flush())) return; }
      catch (e) { setError(String(e instanceof Error ? e.message : e)); return; }
      finally { setFlushing(false); }
    }
    setMode(m);
  };

  const save = async (over?: { sha: string; bodySha: string | null }) => {
    setBusy(true); setError(null);
    try {
      const checked = await flush();
      if (!checked) { setError("저장하는 동안 초안이 바뀌었다. 다시 누른다"); return; }
      if (checked.check.errors.length) { setMode((m) => (m === "review" ? "form" : m)); return; }
      const file = bodyOf(form).file;
      const sendBody = file !== null && body !== null;
      const r = await call({
        op: "automation_save", project, name: creating ? name : openName!, text: checked.text,
        ...((over?.sha ?? base.sha) ? { expected_sha256: (over?.sha ?? base.sha)! } : {}),
        ...(sendBody ? { body: body!, ...((over ? over.bodySha : file === base.bodyFile ? base.bodySha : null) ? { body_expected_sha256: (over ? over.bodySha : base.bodySha)! } : {}) } : {}),
      });
      if (r.kind !== "automation_doc") throw new Error("저장 응답이 아니다");
      // 저장한 뒤 갈 곳(그 규칙 설정)은 부르는 쪽이 정한다
      await onSaved(r.doc.name);
    } catch (e) {
      if (e instanceof ApiError && (e.body.code === "conflict" || e.body.code === "not_found") && !creating) {
        const latest = await call({ op: "automation_read", project, name: openName! }).then((r) => (r.kind === "automation_doc" ? r.doc : null)).catch(ignoredFailure("AutomationEditor.tsx:288", () => null));
        setConflict({ reason: e.body.message, latest, mine: draft.current.text, mineBody: draft.current.body });
      } else setError(e instanceof Error ? e.message : String(e));
    } finally { setBusy(false); }
  };

  const errors = check?.errors ?? [];
  const legacy = !!error && error.startsWith("이전 형식");
  const body_ = bodyOf(form);
  const bodyText = body_.file ? body ?? "" : body_.inline ?? "";
  const ready = !loading && !legacy && !readOnlyReason && (!creating || validName(name));
  const title = creating ? "새 자동화" : `${(form.title as string) || openName} 고치기`;
  return <AutomationPage title={title} subtitle={`${project} · automations/${creating ? name || "…" : openName}.toml`} onClose={onClose} busy={busy} testid="automation-editor"
    footer={<>
      <button type="button" className="btn" onClick={onClose} disabled={busy}>닫기</button>
      <button type="button" className="btn btn-primary" data-testid="automation-save" disabled={!ready || busy || !!conflict} onClick={() => void save()}>{busy ? "저장 중…" : "저장"}</button>
    </>}>
    {error && <p role="alert">{error}</p>}
    {readOnlyReason && <p role="status">{readOnlyReason}</p>}
    {conflict && <ConflictPanel conflict={conflict} onReopen={() => void load()} onOverwrite={conflict.latest && !conflict.latest.legacy && !conflict.latest.web_read_only_reason ? () => {
      // 본 지금 판(규칙과, 같은 본문 파일이면 그 본문)을 기대 판으로 삼아 내 초안을 쓴다
      const latest = conflict.latest!;
      const over = { sha: latest.sha256, bodySha: latest.body_file && latest.body_file === bodyOf(form).file ? latest.body_sha256 ?? null : null };
      setConflict(null);
      setBase((b) => ({ ...b, sha: over.sha, bodySha: over.bodySha, bodyFile: latest.body_file ?? b.bodyFile }));
      void save(over);
    } : null} />}
    {loading ? <p role="status">읽는 중…</p> : !legacy && !readOnlyReason && <>
      {creating && <Field label="이름(파일 이름)" hint="영문 소문자·숫자·-·_. 만든 뒤에는 바꾸지 않는다"><input className="input" value={name} onChange={(e) => setName(e.target.value.trim())} data-testid="automation-form-name" spellCheck={false} aria-invalid={name !== "" && !validName(name)} /></Field>}
      <div className="automation-tabs" role="tablist">
        {(["form", "toml", "review"] as Mode[]).map((m) => <button key={m} type="button" role="tab" aria-selected={mode === m} data-testid={`automation-tab-${m}`} disabled={flushing || (m === "form" && !check?.form && !creating && mode === "toml" && errors.length > 0)} onClick={() => void switchMode(m)}>{m === "form" ? "폼" : m === "toml" ? "TOML" : "검토"}</button>)}
      </div>
      <fieldset className="automation-draft" disabled={busy || flushing}>
      {errors.length > 0 && <ul className="automation-errors" role="alert" data-testid="automation-errors">{errors.map((e) => <li key={e}>{e}</li>)}</ul>}
      {mode === "form" && <>
        <section className="automation-section" aria-label="일반">
          <Text label="제목" value={form.title} onChange={(v) => updateForm({ ...form, title: v })} testid="automation-form-title" />
          <Toggle label="켜기" value={form.enabled !== false} onChange={(v) => updateForm({ ...form, enabled: v })} />
        </section>
        <section className="automation-section" aria-label="조건">
          <h3>조건</h3>
          <Field label="종류"><select className="input" data-testid="automation-form-condition" value={conditionKind(form) ?? ""} onChange={(e) => updateForm(setCondition(form, e.target.value as ConditionKind))}>{conditionKinds.map((c) => <option key={c.kind} value={c.kind}>{c.label}</option>)}</select></Field>
          <ConditionFields form={form} update={updateForm} />
        </section>
        <section className="automation-section" aria-label="동작">
          <h3>동작</h3>
          <Field label="종류"><select className="input" data-testid="automation-form-action" value={actionKind(form) ?? ""} onChange={(e) => updateForm(setAction(form, e.target.value as ActionKind))}>{actionKinds.filter((a) => a.kind !== "command" || canV2).map((a) => <option key={a.kind} value={a.kind}>{a.label}</option>)}</select></Field>
          <ActionFields form={form} update={updateForm} />
          {actionKind(form) !== "deploy" && actionKind(form) !== "command" && <div className="automation-body">
            <div className="automation-tools"><span>본문{body_.file ? ` · ${body_.file}` : ""} (Markdown)</span><button type="button" className="btn btn-ghost" aria-pressed={preview} onClick={() => setPreview((p) => !p)}>{preview ? "고치기" : "미리보기"}</button></div>
            {preview ? <div className="automation-preview" data-testid="automation-body-preview"><Markdown>{bodyText || "(비었다)"}</Markdown></div>
              : <textarea className="input automation-textarea" value={bodyText} onChange={(e) => updateBody(e.target.value)} aria-label="본문" data-testid="automation-form-body" spellCheck={false} />}
          </div>}
        </section>
        <section className="automation-section" aria-label="한도">
          <h3>한도</h3>
          <Limits form={form} update={updateForm} />
        </section>
      </>}
      {mode === "toml" && <section className="automation-section" aria-label="TOML">
        <textarea className="input automation-textarea automation-mono" value={text} onChange={(e) => updateText(e.target.value)} aria-label="TOML 원문" data-testid="automation-toml" spellCheck={false} rows={18} />
        <p className="automation-dim">주석·칸 순서는 그대로 저장한다. 폼으로 돌아가면 host가 읽은 값으로 폼을 다시 그린다.</p>
      </section>}
      {mode === "review" && <section className="automation-section" aria-label="검토"><Review check={check} form={form} /></section>}
      </fieldset>
    </>}
  </AutomationPage>;
}
