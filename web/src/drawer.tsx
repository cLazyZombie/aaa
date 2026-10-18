// 휴대폰·좁은 창(768px 이하)의 목록 drawer(herdr-web-ui aa624cb App.tsx 290~316·1368~1378·1479~1490줄, styles.css
// 1560~1614줄): 화면 머리의 단추·왼쪽 가장자리에서 쓸기(herdr/edgeSwipe.ts)로 pane 위에 열고, 바깥 누름·쓸어 닫기·Esc로
// 닫는다. 769px부터는 목록이 늘 옆 칸이고 단추는 숨는다(primitives.css `.drawer-toggle`).
// 참고는 단추가 창 전체 머리(app-header)에 있다. aaa는 화면마다 머리가 있어(터미널·보고·프로젝트·빈 화면) 그 머리 맨 앞에
// 같은 단추를 두고, 이 context로 App의 상태에 잇는다.
import { createContext, useContext } from "react";
import { Menu, X } from "lucide-react";

export type Drawer = { open: boolean; toggle: () => void };

export const DrawerContext = createContext<Drawer | null>(null);

/** 목록 drawer 단추(참고 `icon-button drawer-toggle`, 문구는 aaa 말로 「목록 열기·닫기」) */
export function DrawerToggle() {
  const drawer = useContext(DrawerContext);
  if (!drawer) return null;
  return (
    <button
      type="button"
      className="icon-button drawer-toggle"
      data-testid="drawer-toggle"
      aria-label={drawer.open ? "목록 닫기" : "목록 열기"}
      aria-expanded={drawer.open}
      aria-controls="workspace-drawer"
      onClick={drawer.toggle}
    >
      {drawer.open ? <X /> : <Menu />}
    </button>
  );
}
