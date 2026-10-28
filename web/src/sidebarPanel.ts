import { logIgnoredFailure } from "./errorDiagnostic.ts";
// 왼쪽 활동 막대(사용자 요청 10-19 「vs code 처럼」, plan §8.240): 사이드바가 보이는 패널 하나와 넓은 창의 접힘을 기기마다 기억한다.
// VS Code는 workspace마다 기억하고 herdr-web-ui(aa624cb App.tsx 318~319줄 `sidebarCollapsed`)는 접힘을 주소(`sidebar=hidden`)에
// 싣는다. aaa는 기기마다(brief: 마지막 패널·접힘은 기기별) localStorage에 둔다.

// 「작업」은 예전 프로젝트·에이전트 두 패널을 합친 트리다(사용자 결정 10-19 안 A, plan §8.280). 「답할 것」은 보고서 바로 위다: thread 질문·
// coordinator 확인 필요·할 일 확인 필요 목록이고 고른 것을 본문 카드로 연다(사용자 결정 10-21 1B, plan §8.345)
// coordinator 알림함은 CLI에서 다룬다. 사용자 화면에는 「답할 것」·보고서로 필요한 항목을 보인다(t-0590).
export const PANELS = ["work", "workspace", "machines", "automations", "ideas", "asks"] as const;
export type SidebarPanel = (typeof PANELS)[number];

export const PANEL_LABEL: Record<SidebarPanel, string> = { workspace: "workspace", asks: "답할 것", work: "작업", machines: "머신", automations: "자동화", ideas: "아이디어" };

/** 예전 패널 이름이나 숨긴 알림함으로 남긴 기억은 「작업」으로 연다 */
const OLD_PANELS: Record<string, SidebarPanel> = { projects: "work", agents: "work", special: "work", inbox: "work" };

const PANEL_KEY = "aaa:sidebar-panel";
const COLLAPSED_KEY = "aaa:sidebar-collapsed";

export function storedPanel(): SidebarPanel {
  try {
    const v = localStorage.getItem(PANEL_KEY) ?? "";
    return PANELS.find((p) => p === v) ?? OLD_PANELS[v] ?? "work";
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "sidebarPanel.ts:24");
    return "work";
  }
}

export function storedCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "sidebarPanel.ts:32");
    return false;
  }
}

export function storeSidebar(panel: SidebarPanel, collapsed: boolean): void {
  try {
    localStorage.setItem(PANEL_KEY, panel);
    if (collapsed) localStorage.setItem(COLLAPSED_KEY, "1");
    else localStorage.removeItem(COLLAPSED_KEY);
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "sidebarPanel.ts:42");
    // 저장이 막히면 이 화면 동안만
  }
}

/**
 * 단축키·팔레트·알림이 연 줄을 보일 패널(VS Code `explorer.autoReveal`: 보이는 explorer에서 드러낸다). 「작업」 트리에 그 줄이 있으면
 * 「작업」으로, 없는 줄은 지금 패널 그대로.
 */
export function revealPanel(current: SidebarPanel, inWork: boolean): SidebarPanel {
  return inWork ? "work" : current;
}
