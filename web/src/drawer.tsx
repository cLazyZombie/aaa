// 휴대폰·좁은 창(768px 이하)의 목록 drawer(herdr-web-ui aa624cb App.tsx 290~316·1368~1378·1479~1490줄, styles.css
// 1560~1614줄): 화면 머리의 단추·안쪽에서 오른쪽으로 쓸기(herdr/edgeSwipe.ts, 왼쪽 끝은 브라우저 뒤로)로 pane 위에 열고, 바깥 누름·쓸어 닫기·Esc로
// 닫는다. 769px부터는 목록이 늘 옆 칸이고 단추는 숨는다(primitives.css `.drawer-toggle`).
// 참고는 단추가 창 전체 머리(app-header)에 있다. aaa는 화면마다 머리가 있어(터미널·보고·프로젝트·빈 화면) 그 머리 맨 앞에
// 같은 단추를 두고, 이 context로 App의 상태에 잇는다.
// 머리 맨 끝의 「더 보기」(참고 header-more)와 머리 알림(참고 headerNote)도 같은 까닭으로 HeadContext로 잇는다.
import { createContext, useContext, type ReactNode } from "react";
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

/** 화면 머리에 App이 두는 것: 맨 끝 「더 보기」 메뉴와 잠깐 띄우는 알림(참고 App.tsx 1393~1397·1444~1459줄) */
export type Head = { more: ReactNode; note: string | null };

export const HeadContext = createContext<Head>({ more: null, note: null });

/** 머리 맨 끝 「더 보기」(참고 `header-more`: 휴대폰에서도 창 머리에 늘 있다) */
export function HeadMore() {
  return <>{useContext(HeadContext).more}</>;
}

/** 머리 알림(참고 `header-note`): 있는 동안 제목 자리에 선다(style.css `.head-note + .term-title`) */
export function HeadNote() {
  const { note } = useContext(HeadContext);
  if (!note) return null;
  return (
    <span className="head-note" role="status" data-testid="head-note">
      {note}
    </span>
  );
}
