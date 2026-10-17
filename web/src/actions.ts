// 앱 동작(herdr-web-ui src/lib/actions.ts `AppActions`·`PaneControls` 가운데 단축키가 부르는 것). App이 하나를 만들어
// 단축키(herdr/shortcuts.ts `useShortcuts`)에 준다. aaa에 그 화면이 없는 동작(명령 팔레트·찾기·사이드바 접기·새 워크스페이스·
// 탭·나누기)은 단축키를 끈 채로 두므로(settings.ts `AAA_SHORTCUTS`) 불리지 않는다.

/** 고른 pane의 입력과 기록(참고 PaneControls). 보이는 TermView가 단다 */
export interface PaneControls {
  focusInput: () => void;
  scrollPage: (direction: -1 | 1) => void;
  /** 대화 / 터미널 전환(참고는 App의 view 상태, aaa는 TermView가 보기를 갖는다) */
  toggleView: () => void;
}

export interface AppActions {
  /** 목록 차례의 이전·다음 pane(같은 머신 안, 끝에서 처음으로) */
  selectAdjacentPane: (direction: -1 | 1) => void;
  toggleView: () => void;
  focusInput: () => void;
  scrollPage: (direction: -1 | 1) => void;
  openSettings: () => void;
  openPalette: () => void;
  openFind: () => void;
  toggleSidebar: () => void;
  openNewSession: () => void;
  openNewTab: () => void;
  splitPane: ((direction: "right" | "down") => void) | null;
}
