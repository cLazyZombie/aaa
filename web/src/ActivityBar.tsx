// 왼쪽 활동 막대(사용자 요청 10-19 「vs code 처럼 … 같은 버튼 한번 더 누르면 숨겨지고」, plan §8.240). 참고 소스(herdr-web-ui·t3code)에
// 이런 막대가 없어 VS Code activity bar를 따른다: 세로 tablist(`aria-orientation="vertical"`), 고른 패널의 단추에만 왼쪽 선과
// aria-selected·aria-expanded, 사이드바를 접으면 고른 단추가 없다. ↑↓·Home·End는 단추 사이 focus만 옮기고(roving tabindex) Enter·Space가
// 고른다. 「답할 것」(plan §8.345)은 맨 위 패널 단추이고 배지는 빨강이다. 패널이 아닌 보고서(본문을 바꾼다)는 패널 단추 바로 밑의
// 보통 단추다(tablist 밖, 사용자 요청 10-19 「보고서는 왼쪽 위로(지금 에이전트 밑으로)」). 보고서 배지는 안 본 보고서 수뿐이다. 그 바로 밑의 「할 일」(TASKS.md, plan §8.306)도 본문을 바꾸는 단추다. 설정(창)만 아래 묶음에 둔다(VS Code 아래 묶음
// Accounts·Manage 자리).
import { useRef, type KeyboardEvent, type ReactNode } from "react";
import { Workflow, Eye, FileText, FolderKanban, Inbox, ListTodo, MessageCircleQuestionMark, Monitor, type LucideIcon } from "lucide-react";
import { PANEL_LABEL, PANELS, type SidebarPanel } from "./sidebarPanel";
import "./ActivityBar.css";

// 작업은 FolderKanban: 작업 공간 폴더 안에서 도는 thread(시안 6개 중 사용자 결정 10-19, plan §8.280·§8.287)
const PANEL_ICON: Record<SidebarPanel, LucideIcon> = { asks: MessageCircleQuestionMark, work: FolderKanban, machines: Monitor, automations: Workflow, inbox: Inbox };

export function ActivityBar({
  panel,
  shown,
  onPanel,
  badges,
  reports,
  onReports,
  unseenReports,
  tasks,
  onTasks,
  taskBadge,
  up,
  reader,
  settings,
}: {
  panel: SidebarPanel;
  /** 사이드바가 보이나(넓은 창에서 접었으면 false): 아니면 고른 단추가 없다 */
  shown: boolean;
  onPanel: (panel: SidebarPanel) => void;
  /** 단추 위 작은 수(VS Code 배지): 볼 일이 있는 thread·agent 수. `ask`면 빨강(답할 것) */
  badges: Partial<Record<SidebarPanel, { count: number; label: string; ask?: boolean }>>;
  /** 본문이 보고서 화면인가 */
  reports: boolean;
  onReports: () => void;
  /** 열어 보지 않은 사용자 보고서 수(host `reports` 사건, plan §8.256) */
  unseenReports: number;
  /** 본문이 할 일 화면인가 */
  tasks: boolean;
  onTasks: () => void;
  /** 사람이 볼 task 수(형식 어긋남, plan §8.306) */
  taskBadge: number;
  /** host 연결 */
  up: boolean;
  reader: boolean;
  /** 맨 아래 설정 톱니(App이 설정 창과 함께 그린다) */
  settings: ReactNode;
}) {
  const tabs = useRef<HTMLDivElement>(null);
  const onTabKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = [...(tabs.current?.querySelectorAll<HTMLButtonElement>("[role=tab]") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    const to = (next: number) => {
      event.preventDefault();
      items[(next + items.length) % items.length]?.focus();
    };
    if (event.key === "ArrowDown") to(at + 1);
    else if (event.key === "ArrowUp") to(at - 1);
    else if (event.key === "Home") to(0);
    else if (event.key === "End") to(items.length - 1);
  };
  const state = `host ${up ? "연결됨" : "연결 끊김"} · ${reader ? "보기 권한" : "제어 권한"}`;
  return (
    <nav className="activity-bar" aria-label="활동 막대" data-testid="activity-bar">
      <span className="activity-brand" role="img" aria-label={`aaa · ${state}`} title={state} data-testid="activity-brand" data-up={up ? "on" : "off"}>
        <span className="activity-logo">
          <img className="logo" src="/icon.svg" alt="" />
          <span className={`activity-conn ${up ? "is-on" : "is-off"}`} aria-hidden="true" />
          {reader && <Eye className="activity-reader" aria-hidden="true" />}
        </span>
      </span>
      <div className="activity-group" role="tablist" aria-orientation="vertical" aria-label="사이드바 패널" ref={tabs} onKeyDown={onTabKey}>
        {PANELS.map((p) => {
          const Icon = PANEL_ICON[p];
          const selected = shown && panel === p;
          const badge = badges[p];
          const label = PANEL_LABEL[p];
          return (
            <button
              key={p}
              type="button"
              role="tab"
              id={`activity-tab-${p}`}
              className={`activity-item${selected ? " is-active" : ""}`}
              aria-selected={selected}
              aria-expanded={selected}
              aria-controls={`sidebar-panel-${p}`}
              aria-label={badge && badge.count > 0 ? `${label}, ${badge.label} ${badge.count}` : label}
              title={label}
              // Tab이 머무는 단추는 하나: 고른 패널, 접혔으면 기억한 패널
              tabIndex={panel === p ? 0 : -1}
              data-testid={`activity-${p}`}
              onClick={() => onPanel(p)}
            >
              <Icon aria-hidden="true" />
              {badge && badge.count > 0 && (
                <span className={`activity-badge${badge.ask ? " is-ask" : ""}`} aria-hidden="true" data-testid={`activity-badge-${p}`}>
                  {badge.count > 99 ? "99+" : badge.count}
                </span>
              )}
            </button>
          );
        })}
      </div>
      <button
        type="button"
        className={`activity-item${reports ? " is-active" : ""}`}
        aria-pressed={reports}
        aria-label={unseenReports > 0 ? `보고서, 안 본 보고서 ${unseenReports}` : "보고서"}
        title="보고서"
        data-testid="activity-reports"
        onClick={onReports}
      >
        <FileText aria-hidden="true" />
        {unseenReports > 0 && (
          <span className="activity-badge" aria-hidden="true" data-testid="activity-badge-reports" data-kind="unseen">
            {unseenReports > 99 ? "99+" : unseenReports}
          </span>
        )}
      </button>
      <button
        type="button"
        className={`activity-item${tasks ? " is-active" : ""}`}
        aria-pressed={tasks}
        aria-label={taskBadge > 0 ? `할 일, 형식 어긋남 ${taskBadge}` : "할 일"}
        title="할 일"
        data-testid="activity-tasks"
        onClick={onTasks}
      >
        <ListTodo aria-hidden="true" />
        {taskBadge > 0 && (
          <span className="activity-badge" aria-hidden="true" data-testid="activity-badge-tasks">
            {taskBadge > 99 ? "99+" : taskBadge}
          </span>
        )}
      </button>
      <div className="activity-group activity-bottom">{settings}</div>
    </nav>
  );
}
