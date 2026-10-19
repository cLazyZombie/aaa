// 왼쪽 활동 막대(사용자 요청 10-19 「vs code 처럼」, plan §8.240): 사이드바가 보이는 패널 하나와 넓은 창의 접힘을 기기마다 기억한다.
// VS Code는 workspace마다 기억하고 herdr-web-ui(aa624cb App.tsx 318~319줄 `sidebarCollapsed`)는 접힘을 주소(`sidebar=hidden`)에
// 싣는다. aaa는 기기마다(brief: 마지막 패널·접힘은 기기별) localStorage에 둔다.

export const PANELS = ["projects", "machines", "agents", "special", "automations"] as const;
export type SidebarPanel = (typeof PANELS)[number];

// 특수(임무·사후 평가자)는 사용자 결정 10-19로 목록 머리 칩에서 에이전트 아래 단추로 옮겼다(plan §8.258)
export const PANEL_LABEL: Record<SidebarPanel, string> = { projects: "프로젝트", machines: "머신", agents: "에이전트", special: "특수", automations: "자동화" };

const PANEL_KEY = "aaa:sidebar-panel";
const COLLAPSED_KEY = "aaa:sidebar-collapsed";

export function storedPanel(): SidebarPanel {
  try {
    return PANELS.find((p) => p === localStorage.getItem(PANEL_KEY)) ?? "projects";
  } catch {
    return "projects";
  }
}

export function storedCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

export function storeSidebar(panel: SidebarPanel, collapsed: boolean): void {
  try {
    localStorage.setItem(PANEL_KEY, panel);
    if (collapsed) localStorage.setItem(COLLAPSED_KEY, "1");
    else localStorage.removeItem(COLLAPSED_KEY);
  } catch {
    // 저장이 막히면 이 화면 동안만
  }
}

/**
 * 단축키·팔레트·알림이 연 줄을 보일 패널(VS Code `explorer.autoReveal`: 보이는 explorer에서 드러낸다). 지금 패널에 그 줄이 있으면 그대로,
 * 없으면 그 줄이 있는 패널로(프로젝트가 먼저). 어느 패널에도 없는 줄(평가자 pane)은 지금 패널 그대로.
 */
export function revealPanel(current: SidebarPanel, inProjects: boolean, inAgents: boolean): SidebarPanel {
  if ((current === "projects" && inProjects) || (current === "agents" && inAgents)) return current;
  return inProjects ? "projects" : inAgents ? "agents" : current;
}
