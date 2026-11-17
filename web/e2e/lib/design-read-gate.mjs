// 실제 읽기는 host가 처리한다. 응답을 화면에 전달하는 순간만 사건으로 보류한다.
// HTTP interceptor의 등록·제거·continue 수명과 화면의 읽기 중 상태를 엮지 않는다.
export async function designReadGate(context, hold) {
  await context.exposeBinding("__aaaDesignReadGate", (_source, request, armed) => hold(request, armed));
  await context.addInitScript(() => {
    if (window !== window.top) return;
    const nativeFetch = window.fetch.bind(window);
    window.fetch = (input, init) => {
      const url = new URL(input instanceof Request ? input.url : String(input), location.href);
      let request;
      if (url.origin === location.origin && url.pathname === "/api/v1/call" && typeof init?.body === "string") {
        try { request = JSON.parse(init.body); } catch {}
      }
      // 배경 갱신의 응답이 누름 뒤 도착해도, 누름 전에 시작한 읽기는 막지 않는다.
      const armed = document.querySelector('[data-testid="design-reload"]')?.dataset.discardReadGate === "armed";
      const result = nativeFetch(input, init);
      if (request?.op !== "design_read" && request?.op !== "design_tree") return result;
      return result.then(async response => {
        await window.__aaaDesignReadGate(request, armed);
        return response;
      });
    };
  });
}
