// page.evaluate로 실행한다. 설정 DOM을 거둔 commit 뒤에도 history.go는 아직 도착하지 않을 수 있다.
export function waitForSettingsExit(ms = 20_000) {
  return new Promise((resolve, reject) => {
    const outside = () => !document.querySelector('[role="region"][aria-label="설정"]') &&
      !history.state?.['aaa:settings'] && !new URLSearchParams(location.search).has('settings');
    if (outside()) return resolve();
    const cleanup = () => {
      clearTimeout(timer);
      observer.disconnect();
      window.removeEventListener('popstate', check);
    };
    const check = () => { if (outside()) { cleanup(); resolve(); } };
    const observer = new MutationObserver(check);
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`${ms}ms 안에 설정의 기록 이동이 끝나지 않았다`));
    }, ms);
    window.addEventListener('popstate', check);
    observer.observe(document, { subtree: true, childList: true });
  });
}
