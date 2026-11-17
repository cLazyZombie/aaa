import { domIs } from "./harness.mjs";

export const modalKeyboardReady = page => domIs(page, "새 pane 창으로 키보드 초점 이동", () =>
  document.querySelector(".new-session-modal")?.contains(document.activeElement), null, 20_000, ["focusin"]);

// 첫 목록 전의 빈 화면 머리와 선택 뒤 pane 머리는 서로 다른 DOM이다.
// 주소를 명시하고 실제 목적지의 머리가 붙은 뒤 눌러 down/up 사이 교체를 피한다.
export const initialPaneReady = (page, pane) => domIs(page, "계정 thread의 첫 화면과 목록 반영", pane =>
  Number(document.querySelector(".app")?.dataset.threadsSync) > 0 &&
  !!document.querySelector(`.term:not([hidden])[data-pane="${CSS.escape(pane)}"] .term-head`), pane);
