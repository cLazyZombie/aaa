// 실제 /me 응답의 전달만 사건으로 잡아 둔다. 본문·헤더·Service Worker·제품 코드는 바꾸지 않는다.
export function deferTaskCapabilities() {
  if (window !== window.top) return;
  const nativeFetch = window.fetch.bind(window);
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  const state = { held: 0, released: false, release: () => { state.released = true; release(); } };
  window.__taskCapabilityGate = state;
  window.fetch = (input, init) => {
    const url = new URL(input instanceof Request ? input.url : String(input), location.href);
    const pending = !state.released && url.pathname === "/api/v1/me" &&
      document.querySelector('[data-testid="tasks-view"][data-shown="true"][data-can-edit="pending"]') !== null;
    const result = nativeFetch(input, init);
    if (!pending) return result;
    return result.then(async response => {
      if (response.ok) {
        state.held++;
        document.documentElement.dataset.taskCapabilityHeld = String(state.held);
        await gate;
      }
      return response;
    });
  };
}
