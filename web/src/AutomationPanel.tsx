// 사이드바 자동화 카드 목록(t-0628 시안 1A·2A, 관리 GUI W1 plan §8.303의 목록을 바꿈). 카드는 「답할 것」·작업 목록 줄과 같은 밀도의
// 두 줄이다: 점 · 이름 · 표지(켜짐이면 없음) · 열린 thread가 있으면 「실행 중」 / 조건 한 줄. 누르면 본문(AutomationView)에 그 규칙
// 설정이 열린다. 켜기·승인·기록·원문·대상은 본문에 있다. 프로젝트 머리의 「새 자동화」, 옛 형식 변환 띠, 「지운 자동화」는 목록에 남는다.
import { useState } from "react";
import { Plus } from "lucide-react";
import { call } from "./api";
import type { AutomationTrashEntry, ThreadView } from "./gen/proto";
import { ConvertSheet } from "./AutomationConvert";
import { canEditAutomations } from "./automationForm";
import { openThreads, type Automations } from "./automationData";
import { canPreviewConvert, cardTone, cardWord, legacyFiles, legacyRule, shortTrigger } from "./automationModel";
import "./AutomationPanel.css";

const time = (at: number) => new Date(at).toLocaleString();

/**
 * `selected`: 본문에 연 규칙(`<project>/<name>`). `onOpen`: 카드를 누르면 그 규칙 설정을, name 없이 부르면 그 프로젝트의 새 자동화를 연다.
 */
export function AutomationPanel({ data, canControl, threads = [], selected = null, onOpen }: {
  data: Automations;
  canControl: boolean;
  threads?: ThreadView[];
  selected?: string | null;
  onOpen: (project: string, name: string | null) => void;
}) {
  const { rows, error, loading, frontApi, load, setError } = data;
  const [busy, setBusy] = useState<string | null>(null);
  const [convert, setConvert] = useState<string | null>(null);
  const restore = async (project: string, t: AutomationTrashEntry) => {
    setBusy(`${project}/${t.id}`);
    try {
      await call({ op: "automation_restore", project, id: t.id });
      await load();
    } catch (e) { setError(String(e)); }
    finally { setBusy(null); }
  };
  const editable = canControl && canEditAutomations(frontApi);
  return <section className="automations" aria-label="자동화 목록" data-testid="automations">
    <div className="automation-tools"><button type="button" className="ghost small" onClick={() => void load()}>다시 읽기</button></div>
    {error && <p role="alert">{error}</p>}
    {loading && <p role="status">읽는 중…</p>}
    {!loading && rows.every((p) => p.rules.length === 0) && <p className="automation-dim">자동화가 없습니다.</p>}
    {rows.filter((p) => p.rules.length > 0 || p.trash.length > 0 || editable).map(({ project, rules, trash }) => {
      const legacy = legacyFiles(rules).length;
      return <section key={project} className="automation-group" aria-label={`${project} 자동화`}>
        <h3 className="automation-group-head">{project} <span>{rules.length}</span>
          {editable && <button type="button" className="ghost small automation-new" data-testid={`automation-new-${project}`} onClick={() => onOpen(project, null)}><Plus aria-hidden="true" />새 자동화</button>}
        </h3>
        {rules.map((r) => {
          const tone = cardTone(r);
          const title = r.title || r.name;
          const running = openThreads(threads, project, r.name);
          return <button type="button" className={`automation-card${selected === `${project}/${r.name}` ? " is-selected" : ""}`} key={r.name}
            data-testid={`automation-${project}-${r.name}`} data-status={tone} aria-current={selected === `${project}/${r.name}` ? "true" : undefined}
            onClick={() => onOpen(project, r.name)}>
            <span className="automation-indicator" role="img" aria-label={cardWord[tone]} />
            <span className="automation-card-text">
              <span className="automation-card-line">
                <span className="automation-card-title">{title}</span>
                {tone !== "on" && <span className={`automation-badge is-${tone}`}>{cardWord[tone]}</span>}
                {legacyRule(r) && <span className="automation-badge is-legacy">이전 형식</span>}
                {running.length > 0 && <span className="automation-running" data-testid={`automation-running-${project}-${r.name}`} title={running.map((t) => t.id).join(", ")}>실행 중</span>}
              </span>
              <span className="automation-card-meta">{shortTrigger(r)}</span>
            </span>
          </button>;
        })}
        {legacy > 0 && <div className="automation-legacy" data-testid={`automation-legacy-${project}`}>
          <span><span className="automation-badge is-legacy">이전 형식</span> {legacy}개 · 웹은 새 형식(TOML)만 고친다</span>
          {canPreviewConvert(frontApi)
            ? <button type="button" className="ghost small" onClick={() => setConvert(project)}>새 형식으로 바꾸기</button>
            : frontApi !== undefined && <span className="automation-dim">host를 1.58 이상으로 올리면 여기서 바꿀 수 있다</span>}
        </div>}
        {trash.length > 0 && <details className="automation-trash" data-testid={`automation-trash-${project}`}>
          <summary>지운 자동화 {trash.length}개</summary>
          <ul className="automation-list">{trash.map((t) => <li key={t.id}><span>{t.title} · {t.name} · {time(t.deleted_at_ms)}</span>
            {canControl && <button type="button" className="btn btn-ghost" disabled={busy !== null} onClick={() => void restore(project, t)}>꺼진 채로 되살리기</button>}</li>)}</ul>
        </details>}
      </section>;
    })}
    {convert && <ConvertSheet project={convert} canControl={canControl} onClose={() => setConvert(null)} onDone={load} />}
  </section>;
}
