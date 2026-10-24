// 설정 DOM 제거와 실제 기록 복귀를 분리한다. 고정 대기 없이 native go/popstate를 쓴다.
export async function holdSettingsExit(page) {
  await page.evaluate(() => {
    const go = history.go.bind(history);
    window.__aaaSettingsExit = { pending: null, released: false };
    history.go = (by) => {
      if (by >= 0) return go(by);
      history.go = go;
      window.__aaaSettingsExit.pending = by;
    };
    window.__aaaReleaseSettingsExit = () => {
      const state = window.__aaaSettingsExit;
      if (state.pending === null) throw new Error('설정 기록 복귀 요청이 없다');
      state.released = true;
      go(state.pending);
    };
  });
}

export async function releaseSettingsExit(page, waiting) {
  let exited = false;
  const completed = waiting.then(() => { exited = true; });
  completed.catch(() => {}); // 앞선 상태 단언이 실패해도 원래 waiter의 늦은 거부를 놓치지 않는다.
  // DOM 제거 commit과 recordSettings의 effect도 따로 도착할 수 있다.
  await page.waitForFunction(() => window.__aaaSettingsExit.pending !== null, null, { polling: 'raf', timeout: 20_000 });
  // 앞서 보낸 evaluate(waitForSettingsExit)가 시작한 뒤 같은 페이지에서 다음 task를 마친다.
  // DOM만 보는 구판 waiter라면 이미 끝나므로 여기서 실패한다.
  const state = await page.evaluate(() => ({
    pending: window.__aaaSettingsExit.pending,
    shown: Boolean(document.querySelector('[role="region"][aria-label="설정"]')),
    entry: Boolean(history.state?.['aaa:settings']),
    query: new URLSearchParams(location.search).has('settings'),
    viewer: Boolean(document.querySelector('[data-testid="file-viewer"]')),
  }));
  if (state.pending === null || state.shown || !state.entry || !state.query || state.viewer || exited)
    throw new Error(`설정 복귀 전에 소스 준비가 끝났다 ${JSON.stringify({ ...state, exited })}`);
  await page.evaluate(() => window.__aaaReleaseSettingsExit());
  await completed;
}
