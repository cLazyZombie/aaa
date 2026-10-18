// 프로젝트의 모든 thread(끝난 것까지). herdr-projects `popup.rs` `build`(Section::Threads)·`thread_line`·`group_color`(1f05c00)를 웹으로 옮겼다:
// 사이드바는 열린 thread만 보이고(sidebar.rs `recorded_groups`), 프로젝트 하나를 고르면 묶음마다 머리 「묶음 (n)」 아래 줄을 보이며
// 끝난 thread는 맨 아래 「끝남」 묶음에 흐리게 둔다. 「/」 거르기는 popup처럼 이름(id·제목)에 든 글로 고른다. 줄을 고르면 보고(popup detail).
// 프로젝트 고르기(popup 첫 화면, projects_in_scope)도 같은 화면이다: 프로젝트마다 열린 묶음 줄(project_line)과 끝난 수.
import { useEffect, useState, type ReactNode } from "react";
import { ArrowLeft, ChevronRight, Folder, Search } from "lucide-react";
import { call } from "./api";
import type { ProjectView, ThreadView } from "./gen/proto";
import { GROUP_WORD, groupRank, projectLine, stateLine, threadKey } from "./sidebarModel";
import "./ProjectThreads.css";
import { DrawerToggle, HeadMore } from "./drawer";

/** popup `Group::DISPLAY_ORDER` */
const DISPLAY_ORDER = ["waiting_on_you", "ready_for_review", "landing", "working", "idle", "resolved"];

/** popup `thread_line`: 상태 줄, 머신, agent(profile), 남은 Next 수 */
function threadFacts(t: ThreadView, now: number): string {
  const parts = [t.group === "resolved" ? GROUP_WORD.resolved : stateLine(t, true, now)];
  if (t.machine) parts.push(t.machine);
  if (t.profile) parts.push(t.profile);
  if (t.group !== "resolved" && t.next.length > 0) parts.push(`next ${t.next.length}`);
  return parts.join(" · ");
}

const matches = (needle: string, ...texts: string[]) => needle === "" || texts.some((x) => x.toLowerCase().includes(needle));

function Filter({ value, onChange, label }: { value: string; onChange: (v: string) => void; label: string }) {
  return (
    <label className="project-filter">
      <Search aria-hidden="true" />
      <input className="input" type="search" value={value} placeholder="거르기" aria-label={label} onChange={(e) => onChange(e.target.value)} />
    </label>
  );
}

export function ProjectThreads({
  project,
  threads,
  onProject,
  onThread,
  onBack,
}: {
  /** null이면 프로젝트 고르기 */
  project: string | null;
  /** 평가자를 뺀 모든 thread(끝난 것까지) */
  threads: ThreadView[];
  onProject: (name: string | null) => void;
  onThread: (t: ThreadView) => void;
  onBack: () => void;
}) {
  const [projects, setProjects] = useState<ProjectView[] | null>(null);
  const [filter, setFilter] = useState("");
  useEffect(() => setFilter(""), [project]);
  useEffect(() => {
    let live = true;
    // 보관한 프로젝트도 받아 thread 기록에서 나온 이름까지 숨긴다(참고 popup은 보관한 것을 빼고 보인다)
    call({ op: "projects", all: true }).then(
      (r) => {
        if (live && r.kind === "projects") setProjects(r.projects);
      },
      () => {
        if (live) setProjects([]);
      },
    );
    return () => {
      live = false;
    };
  }, []);
  const needle = filter.trim().toLowerCase();
  const now = Date.now();

  if (project === null) {
    // 프로젝트 이름: host가 아는 프로젝트와 thread가 있는 프로젝트(옮겨 온 기록만 있는 것도)
    const archived = new Set((projects ?? []).filter((p) => p.status === "archived").map((p) => p.name));
    const paused = new Set([...(projects ?? []).filter((p) => p.status === "paused").map((p) => p.name), ...threads.filter((t) => t.project_status === "paused").map((t) => t.project)]);
    const names = [...new Set([...(projects ?? []).map((p) => p.name), ...threads.map((t) => t.project)])].filter((n) => !archived.has(n)).sort();
    const shown = names.filter((n) => matches(needle, n));
    return (
      <section className="project-threads" aria-label="프로젝트" data-testid="projects">
        <header className="project-threads-head">
          <DrawerToggle />
        <button type="button" className="icon-button" aria-label="닫기" title="닫기" onClick={onBack}>
            <ArrowLeft aria-hidden="true" />
          </button>
          <h2 className="project-threads-title">프로젝트</h2>
          <HeadMore />
        </header>
        <Filter value={filter} onChange={setFilter} label="프로젝트 거르기" />
        <ul className="project-threads-list">
          {shown.map((name) => {
            const mine = threads.filter((t) => t.project === name);
            const open = mine.filter((t) => t.group !== "resolved");
            const done = mine.length - open.length;
            return (
              <li key={name}>
                <button type="button" className="project-threads-row" data-testid={`project-open-${name}`} onClick={() => onProject(name)}>
                  <Folder className="project-threads-mark" aria-hidden="true" />
                  <span className="project-threads-copy">
                    <span className="project-threads-name">{name}</span>
                    <span className="project-threads-facts">{[projectLine(open.map((t) => t.group), paused.has(name)), done > 0 ? `끝남 ${done}` : null].filter(Boolean).join(" · ")}</span>
                  </span>
                  <ChevronRight className="project-threads-go" aria-hidden="true" />
                </button>
              </li>
            );
          })}
          {projects !== null && shown.length === 0 && (
            <li className="project-threads-empty" role="status">
              {names.length === 0 ? "프로젝트 없음" : "맞는 프로젝트 없음"}
            </li>
          )}
        </ul>
      </section>
    );
  }

  const mine = threads.filter((t) => t.project === project && matches(needle, t.id, t.title));
  const all = threads.filter((t) => t.project === project);
  const groups = DISPLAY_ORDER.map((g) => ({
    g,
    rows: mine.filter((t) => t.group === g).sort((a, b) => b.updated_at_ms - a.updated_at_ms),
  })).filter((x) => x.rows.length > 0);
  // popup에 없는 묶음(앞으로 생길 것)은 rank 자리에
  const known = new Set(DISPLAY_ORDER);
  const other = mine.filter((t) => !known.has(t.group)).sort((a, b) => groupRank(a.group) - groupRank(b.group));
  return (
    <ProjectBody
      project={project}
      line={projectLine(
        all.filter((t) => t.group !== "resolved").map((t) => t.group),
        all.some((t) => t.project_status === "paused") || (projects ?? []).some((p) => p.name === project && p.status === "paused"),
      )}
      groups={other.length > 0 ? [...groups, { g: "other", rows: other }] : groups}
      empty={all.length === 0 ? "thread 없음" : mine.length === 0 ? "맞는 thread 없음" : null}
      filter={<Filter value={filter} onChange={setFilter} label={`${project} thread 거르기`} />}
      now={now}
      onThread={onThread}
      onBack={() => onProject(null)}
    />
  );
}

function ProjectBody({
  project,
  line,
  groups,
  empty,
  filter,
  now,
  onThread,
  onBack,
}: {
  project: string;
  line: string;
  groups: { g: string; rows: ThreadView[] }[];
  empty: string | null;
  filter: ReactNode;
  now: number;
  onThread: (t: ThreadView) => void;
  onBack: () => void;
}) {
  return (
    <section className="project-threads" aria-label={`${project} thread`} data-testid={`project-threads-${project}`}>
      <header className="project-threads-head">
        <DrawerToggle />
        <button type="button" className="icon-button" aria-label="프로젝트 목록" title="프로젝트 목록" onClick={onBack}>
          <ArrowLeft aria-hidden="true" />
        </button>
        <span className="project-threads-heading">
          <h2 className="project-threads-title">{project}</h2>
          <span className="project-threads-facts">{line}</span>
        </span>
        <HeadMore />
      </header>
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
                      {t.id} · {t.title}
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
