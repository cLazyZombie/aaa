// DOM을 거둔 뒤에도 leaveSheet의 timer와 native history.back은 아직 남을 수 있다.
// reload는 같은 문서 기록 복귀까지 끝난 다음 시작한다.
export function waitForSheetExit(ms = 20_000) {
  return new Promise((resolve, reject) => {
    const outside = () => !document.querySelector(".row-menu, .row-sheet") && !history.state?.["aaa:sheet"];
    if (outside()) return resolve();
    const cleanup = () => {
      clearTimeout(timer);
      observer.disconnect();
      removeEventListener("popstate", check);
    };
    const check = () => { if (outside()) { cleanup(); resolve(); } };
    const observer = new MutationObserver(check);
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`${ms}ms 안에 메뉴의 기록 복귀가 끝나지 않았다`));
    }, ms);
    addEventListener("popstate", check);
    observer.observe(document, { subtree: true, childList: true });
  });
}
