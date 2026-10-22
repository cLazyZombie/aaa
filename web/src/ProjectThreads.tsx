import { ThreadLimitBadge } from "./ThreadLimitBadge";
import { ThreadStageBadge } from "./ThreadStageBadge";
// 모든 thread(끝난 것까지, `#threads`). herdr-projects `popup.rs` `build`(Section::Threads)·`thread_line`·`group_color`(1f05c00)를 웹으로
// 옮겼다: 사이드바는 열린 thread만 보이고(sidebar.rs `recorded_groups`), 이 화면은 묶음마다 머리 「묶음 (n)」 아래 줄을 보이며 끝난 thread는
// 맨 아래 「끝남」 묶음에 흐리게 둔다. 「/」 거르기는 popup처럼 이름(id·제목)에 든 글로 고른다. 줄을 고르면 보고(popup detail).
// host는 프로젝트 하나만 맡아(plan §8.291) popup 첫 화면의 프로젝트 고르기는 없다. 머리의 「coordinator」는 popup Threads의 `c`
// (CoordinatorOpen.tsx).
import { useEffect, useState, type ReactNode } from "react";
import { ArrowLeft, ChevronRight, Search } from "lucide-react";
import { call } from "./api";
import type { ProjectView, ThreadView } from "./gen/proto";
import { GROUP_WORD, groupRank, projectLine, stateLine, threadKey } from "./sidebarModel";
import "./ProjectThreads.css";
import { DrawerToggle, HeadMore } from "./drawer";
import { CoordinatorOpen } from "./CoordinatorOpen";

/** popup `Group::DISPLAY_ORDER` */
const DISPLAY_ORDER = ["waiting_on_you", "ready_for_review", "landing", "working", "idle", "parked", "resolved"];

/** popup `thread_line`: 상태 줄, 머신, agent(profile), 남은 Next 수. 자동화가 띄운 thread는 앞에 「자동화 thread」(plan t-0475 A1) */
function threadFacts(t: ThreadView, now: number): string {
  const ended = t.group === "resolved" || t.group === "parked";
  const parts = [ended ? GROUP_WORD[t.group] : stateLine(t, true, now)];
  if (t.automation) parts.unshift("자동화 thread");
  if (t.machine) parts.push(t.machine);
  if (t.profile) parts.push(t.profile);
  // 보류(plan §8.426): 보류한 까닭
  if (t.group === "parked" && t.park?.note) parts.push(t.park.note);
  if (!ended && t.next.length > 0) parts.push(`next ${t.next.length}`);
  return parts.join(" · ");
}

const matches = (needle: string, ...texts: string[]) => needle === "" || texts.some((x) => x.toLowerCase().includes(needle));

function Filter({ value, onChange, label, automationOnly, onAutomationOnly }: { value: string; onChange: (v: string) => void; label: string; automationOnly: boolean; onAutomationOnly: (on: boolean) => void }) {
  return (
    <div className="project-filter-row">
      <label className="project-filter">
        <Search aria-hidden="true" />
        <input className="input" type="search" value={value} placeholder="거르기" aria-label={label} onChange={(e) => onChange(e.target.value)} />
      </label>
      {/* 자동화가 띄운 thread만(끝난 것까지, plan t-0475 A1) */}
      <button type="button" className="btn project-filter-toggle" aria-pressed={automationOnly} data-testid="threads-automation-only" onClick={() => onAutomationOnly(!automationOnly)}>
        자동화만
      </button>
    </div>
  );
}

export function ProjectThreads({
  threads,
  onThread,
  onBack,
  onPane,
}: {
  /** 평가자를 뺀 모든 thread(끝난 것까지) */
  threads: ThreadView[];
  onThread: (t: ThreadView) => void;
  onBack: () => void;
  /** 「coordinator」가 연 pane으로 간다. 없으면(보기 권한) 단추가 없다 */
  onPane?: (id: string) => void;
}) {
  // host가 맡은 프로젝트 하나(`instance`, FRONT_API 1.50): 멈춤 표시와 coordinator 열기에 쓴다
  const [instance, setInstance] = useState<ProjectView | null>(null);
  const [filter, setFilter] = useState("");
  const [automationOnly, setAutomationOnly] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    call({ op: "instance" }).then(
      (r) => {
        if (live && r.kind === "instance") setInstance(r.project);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, []);
  const needle = filter.trim().toLowerCase();
  const now = Date.now();
  // 프로젝트는 하나다(plan §8.291): 옮겨 온 기록만 있는 다른 이름의 thread가 남았으면 그것도 보인다
  const mine = threads.filter((t) => matches(needle, t.id, t.title) && (!automationOnly || t.automation != null));
  const groups = DISPLAY_ORDER.map((g) => ({
    g,
    rows: mine.filter((t) => t.group === g).sort((a, b) => b.updated_at_ms - a.updated_at_ms),
  })).filter((x) => x.rows.length > 0);
  // popup에 없는 묶음(앞으로 생길 것)은 rank 자리에
  const known = new Set(DISPLAY_ORDER);
  const other = mine.filter((t) => !known.has(t.group)).sort((a, b) => groupRank(a.group) - groupRank(b.group));
  return (
    <ProjectBody
      line={projectLine(
        threads.filter((t) => t.group !== "resolved" && t.group !== "parked").map((t) => t.group),
        threads.some((t) => t.project_status === "paused") || instance?.status === "paused",
      )}
      groups={other.length > 0 ? [...groups, { g: "other", rows: other }] : groups}
      empty={threads.length === 0 ? "thread 없음" : mine.length === 0 ? "맞는 thread 없음" : null}
      filter={<Filter value={filter} onChange={setFilter} label="thread 거르기" automationOnly={automationOnly} onAutomationOnly={setAutomationOnly} />}
      now={now}
      onThread={onThread}
      onBack={onBack}
      coordinator={onPane && <CoordinatorOpen onPane={onPane} onError={setError} />}
      error={error}
    />
  );
}

function ProjectBody({
  line,
  groups,
  empty,
  filter,
  now,
  onThread,
  onBack,
  coordinator,
  error,
}: {
  line: string;
  groups: { g: string; rows: ThreadView[] }[];
  empty: string | null;
  filter: ReactNode;
  now: number;
  onThread: (t: ThreadView) => void;
  onBack: () => void;
  coordinator: ReactNode;
  error: string | null;
}) {
  return (
    <section className="project-threads" aria-label="모든 thread" data-testid="all-threads">
      <header className="project-threads-head">
        <DrawerToggle />
        <button type="button" className="icon-button" aria-label="닫기" title="닫기" onClick={onBack}>
          <ArrowLeft aria-hidden="true" />
        </button>
        <span className="project-threads-heading">
          <h2 className="project-threads-title">모든 thread</h2>
          <span className="project-threads-facts">{line}</span>
        </span>
        {coordinator}
        <HeadMore />
      </header>
      {error && (
        <p className="project-threads-error" role="alert" data-testid="project-coordinator-error">
          {error}
        </p>
      )}
      {filter}
      {empty && (
        <p className="project-threads-empty" role="status">
          {empty}
        </p>
      )}
      {groups.map(({ g, rows }) => (
        <section key={g} className={`project-threads-group is-${g}`} aria-label={GROUP_WORD[g] ?? "그 밖"} data-testid={`project-group-${g}`}>
          <h3 className="project-threads-group-head">
            {GROUP_WORD[g] ?? "그 밖"} ({rows.length})
          </h3>
          <ul className="project-threads-list">
            {rows.map((t) => (
              <li key={threadKey(t)}>
                <button type="button" className={`project-threads-row is-${t.group}`} data-testid={`project-thread-${t.id}`} onClick={() => onThread(t)}>
                  <span className="project-threads-copy">
                    <span className="project-threads-name">
                      <ThreadStageBadge stage={t.stage} /><ThreadLimitBadge start={t.over_limit} />{t.id} · {t.title}
                    </span>
                    <span className="project-threads-facts">{threadFacts(t, now)}</span>
                  </span>
                  <ChevronRight className="project-threads-go" aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </section>
  );
}
